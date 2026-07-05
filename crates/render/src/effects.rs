//! Renders barrier-hit transitions: a fixed-pool CPU particle simulation plus decaying flashes,
//! spawned when a note's arrival crosses the transport position (Phase E of the `.fmstyle.ron`
//! milestone — see CLAUDE.md). Structured like `notes/pipeline.rs`: own shader, own instance
//! buffer(s), no vertex-buffer-per-frame reallocation unless the pool outgrows its capacity.
//!
//! **Stateful, unlike the barrier's pulse** (`barrier.rs`'s `pulse_intensity`, which recomputes
//! cleanly from time alone): a particle's position is the integral of its velocity/gravity since
//! spawn, so it cannot be derived from `time_seconds` alone without also knowing every particle's
//! spawn time, initial velocity, and RNG draw. `update` therefore tracks `last_time_seconds` and
//! advances the pool by `time_seconds - last_time_seconds` each call, spawning one burst per
//! `HitEvent` whose time falls in `(last_time_seconds, time_seconds]`. A big jump (forward or
//! backward — a scrub, not ordinary playback) clears the pool instead of trying to catch up or
//! rewind it, since these are transient effects with no "correct" mid-scrub state to reconstruct.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use project::{FlashSpec, ParticleSpec, TransitionKind, TransitionLayer};

use super::notes::HitEvent;

/// A `time_seconds` jump larger than this (in either direction) between two `update` calls is
/// treated as a scrub rather than ordinary playback advancing by a redraw's `dt` — the pool is
/// cleared instead of spawning every event the jump skipped over or trying to run particles
/// backward.
const MAX_ORDINARY_STEP_SECONDS: f32 = 0.35;

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

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct EffectInstance {
    center: [f32; 2],
    radius: f32,
    alpha: f32,
    color: [f32; 3],
}

impl EffectInstance {
    fn attributes() -> [wgpu::VertexAttribute; 4] {
        wgpu::vertex_attr_array![
            1 => Float32x2,
            2 => Float32,
            3 => Float32,
            4 => Float32x3,
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

struct Particle {
    pos: [f32; 2],
    vel: [f32; 2],
    gravity_px: f32,
    life_seconds: f32,
    lifetime_seconds: f32,
    size_px: f32,
    color: [f32; 3],
}

struct Flash {
    pos: [f32; 2],
    age_seconds: f32,
    decay_seconds: f32,
    radius_px: f32,
    intensity: f32,
    color: [f32; 3],
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
        // stacking on light); premultiplied-alpha (`One, OneMinusSrcAlpha`) for particles with
        // `ParticleSpec::additive = false` (soft/smoke-like puffs that should occlude, not just
        // brighten). The fragment shader always emits premultiplied color regardless of which
        // pipeline draws it, so only the target's blend factors differ between the two.
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

        let make_pipeline = |label: &str, blend: wgpu::BlendState| {
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
                    entry_point: Some("fs_main"),
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
        let additive_pipeline = make_pipeline("effects_additive_pipeline", additive_blend);
        let alpha_pipeline = make_pipeline("effects_alpha_pipeline", premultiplied_alpha_blend);

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
    /// update_pulse`), spawning a burst for every `hit_events` entry crossed since the previous
    /// call, then uploads the current pool. `hit_events` must be sorted ascending by
    /// `time_seconds` (guaranteed by `notes::NotesRenderer::rebuild_instances`).
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        canvas_size: (f32, f32),
        barrier_fraction: f32,
        transition_layer: &TransitionLayer,
        time_seconds: f32,
        hit_events: &[HitEvent],
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
            let start = hit_events.partition_point(|e| e.time_seconds <= last);
            let end = hit_events.partition_point(|e| e.time_seconds <= time_seconds);
            let spawn_particles = matches!(
                transition_layer.kind,
                TransitionKind::Particles | TransitionKind::ParticlesAndFlash
            );
            let spawn_flash = matches!(
                transition_layer.kind,
                TransitionKind::Flash | TransitionKind::ParticlesAndFlash
            );
            for event in &hit_events[start..end] {
                if spawn_particles {
                    if let Some(spec) = &transition_layer.particles {
                        self.spawn_particles(event.x_px, barrier_y, spec);
                    }
                }
                if spawn_flash {
                    if let Some(spec) = &transition_layer.flash {
                        self.spawn_flash(event.x_px, barrier_y, spec);
                    }
                }
            }

            let dt = step.max(0.0);
            for particle in &mut self.particles {
                particle.pos[0] += particle.vel[0] * dt;
                particle.pos[1] += particle.vel[1] * dt;
                particle.vel[1] += particle.gravity_px * dt;
                particle.life_seconds -= dt;
            }
            self.particles.retain(|p| p.life_seconds > 0.0);

            for flash in &mut self.flashes {
                flash.age_seconds += dt;
            }
            self.flashes.retain(|f| f.age_seconds < f.decay_seconds);
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
        self.rebuild_instances(particle_additive);

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
        let lifetime = spec.lifetime_seconds.max(0.01);
        for _ in 0..spec.count {
            // Bursts spread around straight up (canvas convention is y-down, so "up" is negative
            // y) — `angle_degrees` measured from that upward axis.
            let angle_deg = -90.0
                + self
                    .rng
                    .range(-spec.spread_degrees * 0.5, spec.spread_degrees * 0.5);
            let angle = angle_deg.to_radians();
            let speed = spec.speed_px * self.rng.range(0.5, 1.0);
            self.particles.push(Particle {
                pos: [x_px, y_px],
                vel: [angle.cos() * speed, angle.sin() * speed],
                gravity_px: spec.gravity_px,
                life_seconds: lifetime,
                lifetime_seconds: lifetime,
                size_px: spec.size_px.max(0.5),
                color,
            });
        }
    }

    fn spawn_flash(&mut self, x_px: f32, y_px: f32, spec: &FlashSpec) {
        let color = srgb_to_linear(spec.color.resolve_constant());
        self.flashes.push(Flash {
            pos: [x_px, y_px],
            age_seconds: 0.0,
            decay_seconds: spec.decay_seconds.max(0.01),
            radius_px: spec.radius_px.max(1.0),
            intensity: spec.intensity,
            color,
        });
    }

    /// Rebuilds the two CPU-side instance lists from the current particle/flash pool state.
    /// Flashes always draw additive (a flash reads as a bright pop regardless of style); particles
    /// draw additive or premultiplied-alpha together, chosen by the *currently resolved*
    /// `ParticleSpec::additive` — a style swap mid-flight is a documented edge case where
    /// already-alive particles from the previous style render under the new blend mode instead of
    /// finishing out their old one, not worth extra per-particle bookkeeping for.
    fn rebuild_instances(&mut self, particle_additive: bool) {
        self.additive_instances.clear();
        self.alpha_instances.clear();

        for flash in &self.flashes {
            let t = 1.0 - (flash.age_seconds / flash.decay_seconds).clamp(0.0, 1.0);
            self.additive_instances.push(EffectInstance {
                center: flash.pos,
                radius: flash.radius_px,
                alpha: flash.intensity * t,
                color: flash.color,
            });
        }

        let target = if particle_additive {
            &mut self.additive_instances
        } else {
            &mut self.alpha_instances
        };
        for particle in &self.particles {
            let t = (particle.life_seconds / particle.lifetime_seconds).clamp(0.0, 1.0);
            target.push(EffectInstance {
                center: particle.pos,
                radius: particle.size_px,
                alpha: t,
                color: particle.color,
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
