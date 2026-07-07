//! Renders barrier-hit transitions: a fixed-pool CPU particle simulation plus decaying flashes,
//! spawned when note arrivals cross the transport position. The simulation is stateful and uses
//! separate additive and premultiplied-alpha pipelines; see `docs/implementation-notes.md`.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use project::{EmissionMode, FlashMode, FlashSpec, ParticleSpec, TransitionKind, TransitionLayer};

use super::notes::NoteInterval;

/// A `time_seconds` jump larger than this (in either direction) between two `update` calls is
/// treated as a scrub rather than ordinary playback advancing by a redraw's `dt` — the pool is
/// cleared instead of spawning every event the jump skipped over or trying to run particles
/// backward.
const MAX_ORDINARY_STEP_SECONDS: f32 = 0.35;

/// Cutoff distance past which a `GlowLayer`'s `exp(-d / sigma_px)` contribution is treated as
/// invisible — see `barrier.rs`'s identical constant for the full rationale.
const GLOW_CUTOFF_SIGMAS: f32 = 5.0;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ViewUniform {
    transform: [f32; 16],
}

fn orthographic_projection(width: f32, height: f32) -> [f32; 16] {
    #[rustfmt::skip]
    let out = [
        2.0 / width, 0.0, 0.0, 0.0,
        0.0, -2.0 / height, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        -1.0, 1.0, 0.0, 1.0,
    ];
    out
}

impl Default for ViewUniform {
    fn default() -> Self {
        Self {
            transform: orthographic_projection(1.0, 1.0),
        }
    }
}

/// Phase M: `radius`/`softness` replaced by a `core_radius`/`quad_radius` split (same idea as
/// `notes::NoteInstance`'s `true_size`/`draw_size`) plus per-instance additive corona layers,
/// baked in at spawn time (`spawn_one_particle`/`spawn_flash`) rather than read from a shared
/// uniform, since particles/flashes already bake their final linear color into the instance this
/// way. `core_radius` is the configured half-extent (ellipse-aware); `quad_radius` is
/// `core_radius + margin` for glow instances (flashes, additive particles) or exactly
/// `core_radius` for non-additive "puff" particles (`fs_puff` never reads `layer_amp`/
/// `layer_sigma` at all, so leaving them zeroed for puffs is not a footgun).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct EffectInstance {
    center: [f32; 2],
    core_radius: [f32; 2],
    quad_radius: [f32; 2],
    alpha: f32,
    color: [f32; 3],
    /// x/y/z = layer[0..3].amplitude, pre-multiplied by the spec's `brightness` at spawn time
    /// (a plain multiply, not a `hot_color` mix — additive saturation whitens for free).
    layer_amp: [f32; 3],
    layer_sigma: [f32; 3],
}

impl EffectInstance {
    fn attributes() -> [wgpu::VertexAttribute; 7] {
        wgpu::vertex_attr_array![
            1 => Float32x2, // center
            2 => Float32x2, // core_radius
            3 => Float32x2, // quad_radius
            4 => Float32,   // alpha
            5 => Float32x3, // color
            6 => Float32x3, // layer_amp
            7 => Float32x3, // layer_sigma
        ]
    }

    fn layout(attributes: &[wgpu::VertexAttribute]) -> wgpu::VertexBufferLayout<'_> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<EffectInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct QuadVertex {
    position: [f32; 2],
}

/// sRGB u8 -> linear f32, the same small conversion this codebase keeps a private copy of in
/// every module that needs it (`notes::color_to_linear`, `barrier::srgb_to_linear`) rather than
/// sharing, since none of them are `pub`.
fn srgb_to_linear([r, g, b]: [u8; 3]) -> [f32; 3] {
    fn component(u: u8) -> f32 {
        let u = u as f32 / 255.0;
        if u < 0.04045 {
            u / 12.92
        } else {
            ((u + 0.055) / 1.055).powf(2.4)
        }
    }
    [component(r), component(g), component(b)]
}

/// CPU-side equivalent of `barrier.wgsl`/`notes/shader.wgsl`'s `hot_color`: desaturates `base`
/// toward pure white as `brightness` climbs past `1.0`, rather than just scaling its channels up
/// (which doesn't converge to white unless they already share the same magnitude) — same
/// rationale as those shaders' doc comments. `brightness <= 1.0` is a plain dimmer;
/// `brightness == 1.0` is an exact no-op. Since Phase M this is only used for non-additive "puff"
/// particles (which have no separate opaque core to whiten, unlike barrier/notes, so the fill
/// color itself is whitened) — flashes and additive particles instead pre-multiply `brightness`
/// into their additive corona `layer_amp`, letting additive saturation whiten for free.
fn hot_color([r, g, b]: [f32; 3], brightness: f32) -> [f32; 3] {
    let b_mul = brightness.max(0.0);
    if b_mul <= 1.0 {
        return [r * b_mul, g * b_mul, b * b_mul];
    }
    let whiteness = 1.0 - 1.0 / b_mul;
    [
        r + (1.0 - r) * whiteness,
        g + (1.0 - g) * whiteness,
        b + (1.0 - b) * whiteness,
    ]
}

struct Particle {
    pos: [f32; 2],
    vel: [f32; 2],
    gravity_px: f32,
    life_seconds: f32,
    lifetime_seconds: f32,
    size_px: f32,
    color: [f32; 3],
    /// `0.0` for non-additive "puff" particles (no corona, `quad_radius == core_radius`).
    margin_px: f32,
    layer_amp: [f32; 3],
    layer_sigma: [f32; 3],
}

struct Flash {
    pos: [f32; 2],
    /// Absolute transport-time threshold at which decay begins — `time_seconds` at spawn for
    /// `FlashMode::Instant`, or the note's `end_seconds` (in the future, at spawn time) for
    /// `FlashMode::Sustained`. Alpha is a pure function of `time_seconds - decay_start_seconds`,
    /// so a sustained flash simply stays at full intensity for as long as this stays in the
    /// future — no separate per-frame "is the note still held" bookkeeping needed.
    decay_start_seconds: f32,
    decay_seconds: f32,
    radius_x_px: f32,
    radius_y_px: f32,
    color: [f32; 3],
    margin_px: f32,
    layer_amp: [f32; 3],
    layer_sigma: [f32; 3],
}

/// Tiny deterministic xorshift32 PRNG for particle spawn jitter — not worth pulling in a `rand`
/// dependency (not used anywhere else in this workspace) for what only needs to look plausibly
/// random, not be cryptographically or statistically rigorous.
struct Rng(u32);

impl Rng {
    fn new() -> Self {
        Self(0x9E3779B9)
    }

    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    fn range(&mut self, low: f32, high: f32) -> f32 {
        let unit = self.next_u32() as f32 / u32::MAX as f32;
        low + (high - low) * unit
    }
}

pub struct EffectsRenderer {
    additive_pipeline: wgpu::RenderPipeline,
    alpha_pipeline: wgpu::RenderPipeline,

    quad_vertex_buffer: wgpu::Buffer,
    quad_index_buffer: wgpu::Buffer,
    quad_index_count: u32,

    view_buffer: wgpu::Buffer,
    view_bind_group: wgpu::BindGroup,

    additive_instances: Vec<EffectInstance>,
    additive_buffer: wgpu::Buffer,
    additive_capacity: usize,

    alpha_instances: Vec<EffectInstance>,
    alpha_buffer: wgpu::Buffer,
    alpha_capacity: usize,

    particles: Vec<Particle>,
    flashes: Vec<Flash>,
    rng: Rng,
    last_time_seconds: Option<f32>,
    canvas_size: (f32, f32),
}

impl EffectsRenderer {
    const INITIAL_INSTANCE_CAPACITY: usize = 256;

    pub fn new(device: &wgpu::Device, texture_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("effects_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("effects.wgsl").into()),
        });

        let view_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("effects_view_bind_group_layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let view_data = ViewUniform::default();
        let view_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("effects_view_uniform"),
            contents: bytemuck::cast_slice(&[view_data]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let view_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("effects_view_bind_group"),
            layout: &view_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: view_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("effects_pipeline_layout"),
            bind_group_layouts: &[Some(&view_bind_group_layout)],
            immediate_size: 0,
        });

        let quad_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<QuadVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x2,
            }],
        };
        let instance_attributes = EffectInstance::attributes();
        let instance_layout = EffectInstance::layout(&instance_attributes);

        // Additive (`One, One`) for flashes and additive-mode particles (sparks, glints — light
        // stacking on light, `fs_glow`'s additive-layered-sum formula); premultiplied-alpha
        // (`One, OneMinusSrcAlpha`) for particles with `ParticleSpec::additive = false`
        // (soft/smoke-like puffs that should occlude, not just brighten — `fs_puff`'s unchanged
        // hard-edge shape). Each pipeline uses a different fragment entry point (Phase M) since
        // the two formulas no longer share one `mix(hard_edge, soft_glow, softness)` shader.
        let additive_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let premultiplied_alpha_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        };

        let make_pipeline = |label: &str, entry_point: &'static str, blend: wgpu::BlendState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[quad_vertex_layout.clone(), instance_layout.clone()],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry_point),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: texture_format,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let additive_pipeline =
            make_pipeline("effects_additive_pipeline", "fs_glow", additive_blend);
        let alpha_pipeline = make_pipeline(
            "effects_alpha_pipeline",
            "fs_puff",
            premultiplied_alpha_blend,
        );

        const VERTICES: &[QuadVertex] = &[
            QuadVertex {
                position: [0.0, 0.0],
            },
            QuadVertex {
                position: [1.0, 0.0],
            },
            QuadVertex {
                position: [1.0, 1.0],
            },
            QuadVertex {
                position: [0.0, 1.0],
            },
        ];
        #[rustfmt::skip]
        const INDICES: &[u16] = &[
            0, 1, 2,
            0, 2, 3,
        ];
        let quad_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("effects_quad_vertex_buffer"),
            contents: bytemuck::cast_slice(VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let quad_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("effects_quad_index_buffer"),
            contents: bytemuck::cast_slice(INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });

        let additive_buffer = Self::create_instance_buffer(device, Self::INITIAL_INSTANCE_CAPACITY);
        let alpha_buffer = Self::create_instance_buffer(device, Self::INITIAL_INSTANCE_CAPACITY);

        Self {
            additive_pipeline,
            alpha_pipeline,
            quad_vertex_buffer,
            quad_index_buffer,
            quad_index_count: INDICES.len() as u32,
            view_buffer,
            view_bind_group,
            additive_instances: Vec::new(),
            additive_buffer,
            additive_capacity: Self::INITIAL_INSTANCE_CAPACITY,
            alpha_instances: Vec::new(),
            alpha_buffer,
            alpha_capacity: Self::INITIAL_INSTANCE_CAPACITY,
            particles: Vec::new(),
            flashes: Vec::new(),
            rng: Rng::new(),
            last_time_seconds: None,
            canvas_size: (1.0, 1.0),
        }
    }

    fn create_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("effects_instance_buffer"),
            size: (std::mem::size_of::<EffectInstance>() * capacity.max(1)) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Advances the simulation to `time_seconds` (expected to already have the sync offset
    /// subtracted, same convention as `notes::NotesRenderer::update`/`barrier::BarrierRenderer::
    /// update_pulse`), spawning a one-shot burst/flash for every `note_intervals` entry crossed
    /// since the previous call (plus, for `EmissionMode::Continuous`, streaming particles for
    /// every interval currently held), then uploads the current pool. `note_intervals` must be
    /// sorted ascending by `start_seconds` (guaranteed by `notes::NotesRenderer::
    /// rebuild_instances`).
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        canvas_size: (f32, f32),
        barrier_fraction: f32,
        transition_layer: &TransitionLayer,
        time_seconds: f32,
        note_intervals: &[NoteInterval],
    ) {
        self.canvas_size = (canvas_size.0.max(1.0), canvas_size.1.max(1.0));
        let barrier_y = self.canvas_size.1 * barrier_fraction;

        let step = self
            .last_time_seconds
            .map(|last| time_seconds - last)
            .unwrap_or(0.0);
        let is_ordinary_step =
            self.last_time_seconds.is_some() && (0.0..=MAX_ORDINARY_STEP_SECONDS).contains(&step);

        if is_ordinary_step {
            let last = self.last_time_seconds.unwrap();
            let start = note_intervals.partition_point(|e| e.start_seconds <= last);
            let end = note_intervals.partition_point(|e| e.start_seconds <= time_seconds);
            let spawn_particles = matches!(
                transition_layer.kind,
                TransitionKind::Particles | TransitionKind::ParticlesAndFlash
            );
            let spawn_flash = matches!(
                transition_layer.kind,
                TransitionKind::Flash | TransitionKind::ParticlesAndFlash
            );
            for interval in &note_intervals[start..end] {
                if spawn_particles {
                    if let Some(spec) = &transition_layer.particles {
                        if matches!(spec.emission, EmissionMode::Burst) {
                            self.spawn_particles(interval.x_center(), barrier_y, spec);
                        }
                    }
                }
                if spawn_flash {
                    if let Some(spec) = &transition_layer.flash {
                        let decay_start = match spec.mode {
                            FlashMode::Instant => time_seconds,
                            FlashMode::Sustained => interval.end_seconds,
                        };
                        self.spawn_flash(interval.x_center(), barrier_y, spec, decay_start);
                    }
                }
            }

            let dt = step.max(0.0);

            // Continuous emission: sample every note currently held (a plain "is this note
            // active right now" point check, not a crossing check like the burst spawn above —
            // this is a per-frame density sample, not a one-shot cue, so there's nothing to
            // "miss" the way a burst would if it skipped an arrival).
            if spawn_particles {
                if let Some(spec) = &transition_layer.particles {
                    if let EmissionMode::Continuous { rate_per_second } = spec.emission {
                        let color = srgb_to_linear(spec.color.resolve_constant());
                        let expected = rate_per_second.max(0.0) * dt;
                        for interval in note_intervals {
                            if interval.start_seconds <= time_seconds
                                && time_seconds <= interval.end_seconds
                            {
                                let mut n = expected.floor() as u32;
                                if self.rng.range(0.0, 1.0) < expected.fract() {
                                    n += 1;
                                }
                                for _ in 0..n {
                                    let x = self.rng.range(interval.x_left, interval.x_right);
                                    self.spawn_one_particle(x, barrier_y, spec, color);
                                }
                            }
                        }
                    }
                }
            }

            for particle in &mut self.particles {
                particle.pos[0] += particle.vel[0] * dt;
                particle.pos[1] += particle.vel[1] * dt;
                particle.vel[1] += particle.gravity_px * dt;
                particle.life_seconds -= dt;
            }
            self.particles.retain(|p| p.life_seconds > 0.0);

            self.flashes
                .retain(|f| time_seconds - f.decay_start_seconds < f.decay_seconds);
        } else {
            // First call, or a scrub (forward or backward) large enough that "catch up" doesn't
            // make sense for a transient effect — clear rather than guess.
            self.particles.clear();
            self.flashes.clear();
        }
        self.last_time_seconds = Some(time_seconds);

        let particle_additive = transition_layer
            .particles
            .as_ref()
            .map(|spec| spec.additive)
            .unwrap_or(true);
        self.rebuild_instances(particle_additive, time_seconds);

        queue.write_buffer(
            &self.view_buffer,
            0,
            bytemuck::cast_slice(&[ViewUniform {
                transform: orthographic_projection(self.canvas_size.0, self.canvas_size.1),
            }]),
        );
        self.upload(device, queue);
    }

    fn spawn_particles(&mut self, x_px: f32, y_px: f32, spec: &ParticleSpec) {
        let color = srgb_to_linear(spec.color.resolve_constant());
        for _ in 0..spec.count {
            self.spawn_one_particle(x_px, y_px, spec, color);
        }
    }

    /// Spawns a single particle — shared by burst spawns (`spawn_particles`, called `count`
    /// times per note arrival) and continuous emission (called a variable number of times per
    /// frame, spread across a held note's key width). `color` is the spec's already
    /// srgb-to-linear-converted color, before `ParticleSpec::brightness`/`layers` are baked in
    /// (Phase M): non-additive "puff" particles keep the pre-Phase-M behavior exactly (`hot_color`
    /// whitens the fill itself, no corona); additive particles instead leave `color` unwhitened
    /// and pre-multiply `brightness` into `layer_amp`, since their corona is additive and whitens
    /// via saturation instead.
    fn spawn_one_particle(&mut self, x_px: f32, y_px: f32, spec: &ParticleSpec, color: [f32; 3]) {
        let (color, margin_px, layer_amp, layer_sigma) = if spec.additive {
            let layer_amp = [
                spec.layers[0].amplitude * spec.brightness,
                spec.layers[1].amplitude * spec.brightness,
                spec.layers[2].amplitude * spec.brightness,
            ];
            let layer_sigma = [
                spec.layers[0].sigma_px,
                spec.layers[1].sigma_px,
                spec.layers[2].sigma_px,
            ];
            let margin_px = spec
                .layers
                .iter()
                .fold(0.0f32, |acc, layer| acc.max(layer.sigma_px))
                * GLOW_CUTOFF_SIGMAS;
            (color, margin_px, layer_amp, layer_sigma)
        } else {
            (hot_color(color, spec.brightness), 0.0, [0.0; 3], [1.0; 3])
        };
        // Spread around straight up (canvas convention is y-down, so "up" is negative y) —
        // `angle_degrees` measured from that upward axis.
        let angle_deg = -90.0
            + self
                .rng
                .range(-spec.spread_degrees * 0.5, spec.spread_degrees * 0.5);
        let angle = angle_deg.to_radians();
        let speed = spec.speed_px * self.rng.range(0.5, 1.0);
        let lifetime = spec.lifetime_seconds.max(0.01);
        self.particles.push(Particle {
            pos: [x_px, y_px],
            vel: [angle.cos() * speed, angle.sin() * speed],
            gravity_px: spec.gravity_px,
            life_seconds: lifetime,
            lifetime_seconds: lifetime,
            size_px: spec.size_px.max(0.5),
            color,
            margin_px,
            layer_amp,
            layer_sigma,
        });
    }

    fn spawn_flash(&mut self, x_px: f32, y_px: f32, spec: &FlashSpec, decay_start_seconds: f32) {
        // A flash always renders additively (Phase M): `color` stays unwhitened, `brightness` is
        // pre-multiplied into `layer_amp` instead of baked in via `hot_color` — additive
        // saturation whitens for free. A flash is always fully "on" at spawn, fading to 0 over
        // `decay_seconds` as before.
        let color = srgb_to_linear(spec.color.resolve_constant());
        let layer_amp = [
            spec.layers[0].amplitude * spec.brightness,
            spec.layers[1].amplitude * spec.brightness,
            spec.layers[2].amplitude * spec.brightness,
        ];
        let layer_sigma = [
            spec.layers[0].sigma_px,
            spec.layers[1].sigma_px,
            spec.layers[2].sigma_px,
        ];
        let margin_px = spec
            .layers
            .iter()
            .fold(0.0f32, |acc, layer| acc.max(layer.sigma_px))
            * GLOW_CUTOFF_SIGMAS;
        self.flashes.push(Flash {
            pos: [x_px, y_px],
            decay_start_seconds,
            decay_seconds: spec.decay_seconds.max(0.01),
            radius_x_px: spec.radius_x_px.max(1.0),
            radius_y_px: spec.radius_y_px.max(1.0),
            color,
            margin_px,
            layer_amp,
            layer_sigma,
        });
    }

    /// Rebuilds the two CPU-side instance lists from the current particle/flash pool state.
    /// Flashes always draw additive (a flash reads as a bright pop regardless of style); particles
    /// draw additive or premultiplied-alpha together, chosen by the *currently resolved*
    /// `ParticleSpec::additive` — a style swap mid-flight is a documented edge case where
    /// already-alive particles from the previous style render under the new blend mode instead of
    /// finishing out their old one, not worth extra per-particle bookkeeping for.
    fn rebuild_instances(&mut self, particle_additive: bool, time_seconds: f32) {
        self.additive_instances.clear();
        self.alpha_instances.clear();

        for flash in &self.flashes {
            // Pure function of current transport time vs. the flash's stored decay-start
            // threshold — for `FlashMode::Instant` that threshold is the spawn time, so `elapsed`
            // grows immediately (identical curve to before this phase); for `Sustained` it's the
            // note's `end_seconds` (in the future at spawn time), so `elapsed` stays clamped to
            // 0 (t == 1.0, full intensity) for the note's entire held duration and only starts
            // decaying once the note actually ends.
            let elapsed = (time_seconds - flash.decay_start_seconds).max(0.0);
            let t = 1.0 - (elapsed / flash.decay_seconds).clamp(0.0, 1.0);
            let core_radius = [flash.radius_x_px, flash.radius_y_px];
            self.additive_instances.push(EffectInstance {
                center: flash.pos,
                core_radius,
                quad_radius: [
                    core_radius[0] + flash.margin_px,
                    core_radius[1] + flash.margin_px,
                ],
                alpha: t,
                color: flash.color,
                layer_amp: flash.layer_amp,
                layer_sigma: flash.layer_sigma,
            });
        }

        let target = if particle_additive {
            &mut self.additive_instances
        } else {
            &mut self.alpha_instances
        };
        for particle in &self.particles {
            let t = (particle.life_seconds / particle.lifetime_seconds).clamp(0.0, 1.0);
            let core_radius = [particle.size_px, particle.size_px];
            target.push(EffectInstance {
                center: particle.pos,
                core_radius,
                quad_radius: [
                    core_radius[0] + particle.margin_px,
                    core_radius[1] + particle.margin_px,
                ],
                alpha: t,
                color: particle.color,
                layer_amp: particle.layer_amp,
                layer_sigma: particle.layer_sigma,
            });
        }
    }

    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if self.additive_instances.len() > self.additive_capacity {
            self.additive_capacity = self.additive_instances.len();
            self.additive_buffer = Self::create_instance_buffer(device, self.additive_capacity);
        }
        queue.write_buffer(
            &self.additive_buffer,
            0,
            bytemuck::cast_slice(&self.additive_instances),
        );

        if self.alpha_instances.len() > self.alpha_capacity {
            self.alpha_capacity = self.alpha_instances.len();
            self.alpha_buffer = Self::create_instance_buffer(device, self.alpha_capacity);
        }
        queue.write_buffer(
            &self.alpha_buffer,
            0,
            bytemuck::cast_slice(&self.alpha_instances),
        );
    }

    pub fn render(&self, render_pass: &mut wgpu::RenderPass<'_>) {
        // `barrier::BarrierRenderer::render` (drawn just before this) already resets the scissor
        // rect to the full canvas, but reset again defensively rather than depend on draw order
        // elsewhere in `Compositor::render` never changing.
        render_pass.set_scissor_rect(
            0,
            0,
            self.canvas_size.0.round().max(1.0) as u32,
            self.canvas_size.1.round().max(1.0) as u32,
        );

        if !self.additive_instances.is_empty() {
            render_pass.set_pipeline(&self.additive_pipeline);
            render_pass.set_bind_group(0, &self.view_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.quad_vertex_buffer.slice(..));
            render_pass.set_vertex_buffer(1, self.additive_buffer.slice(..));
            render_pass
                .set_index_buffer(self.quad_index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            render_pass.draw_indexed(
                0..self.quad_index_count,
                0,
                0..self.additive_instances.len() as u32,
            );
        }

        if !self.alpha_instances.is_empty() {
            render_pass.set_pipeline(&self.alpha_pipeline);
            render_pass.set_bind_group(0, &self.view_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.quad_vertex_buffer.slice(..));
            render_pass.set_vertex_buffer(1, self.alpha_buffer.slice(..));
            render_pass
                .set_index_buffer(self.quad_index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            render_pass.draw_indexed(
                0..self.quad_index_count,
                0,
                0..self.alpha_instances.len() as u32,
            );
        }
    }
}
