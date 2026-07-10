//! Renders the barrier where falling notes stop: a full-canvas-width horizontal bar with optional
//! glow, wavy edges, and note-arrival pulse. `ui::draw_barrier_handle` keeps only the drag
//! hit-region for editing.
//!
//! Self-contained quad pass, structured like `video_quad.rs`: no vertex buffer (six hardcoded
//! unit-quad corners, positioned/sized in the vertex shader from a uniform), one bind group.
//! Historical glow/pulse rationale lives in `docs/implementation-notes.md`.

use bytemuck::{Pod, Zeroable};

use project::{BarrierLayer, Pulse, StrandSpec, WavyMode, WavySpec};

/// Cutoff distance past which a `GlowLayer`'s `exp(-d / sigma_px)` contribution is treated as
/// invisible (`exp(-5) ≈ 0.0067`, well below 8-bit display precision) — used to size how far past
/// the core each layer's rasterized quad needs to extend. Shared verbatim by `notes/pipeline.rs`.
const GLOW_CUTOFF_SIGMAS: f32 = 5.0;

/// All-vec4 layout, same reasoning as `notes::pipeline::StyleUniform` — every field is already
/// vec4-aligned so there's no std140 column-padding mismatch (the `mat3x3<f32>` uniform CLAUDE.md
/// documents elsewhere in this codebase) to get wrong.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    /// x = canvas width, y = canvas height, z = barrier center y (pixels), w = thickness (pixels).
    geometry: [f32; 4],
    /// xyz = bar color (linear), w unused.
    bar_color: [f32; 4],
    /// x = glow enabled (0/1), y = pulse curve (0..1, decaying), z = wavy enabled (0/1),
    /// w = wavy mode (0=TopOnly, 1=Mirrored, 2=BothEdges; only meaningful when z is set).
    flags: [f32; 4],
    /// x = wave amplitude (px), y = wavelength (px), z = speed, w = transport time (seconds).
    wave: [f32; 4],
    /// xyz = halo color (linear, independent of the bar's own `bar_color`), w = wavy ripple slide
    /// speed in canvas px/second (`WavySpec::slide_speed`; `0.0` = no lateral translation) —
    /// parked in this vec4's spare slot, unrelated to the halo color itself.
    glow_style: [f32; 4],
    /// x = resting brightness (the bar's `Glow::brightness` when glow is set, else `1.0`), y =
    /// peak brightness at `pulse = 1.0` (the bar's `Pulse::brightness`, or the resting value when
    /// no pulse is configured so a no-pulse mix is an exact no-op), zw unused.
    glow_brightness_pulse: [f32; 4],
    /// Additive corona layers: x = layer[0].amplitude, y = layer[0].sigma_px,
    /// z = layer[1].amplitude, w = layer[1].sigma_px.
    glow_layers_ab: [f32; 4],
    /// x = layer[2].amplitude, y = layer[2].sigma_px, z = precomputed glow margin in pixels
    /// (`max(sigma) * GLOW_CUTOFF_SIGMAS`, computed once on the CPU rather than per-vertex), w =
    /// `show_bar` as 1.0/0.0 — `fs_glow` reads this to decide whether to zero its output under the
    /// opaque core's footprint (only correct when that core is actually drawn) or shine straight
    /// through instead (`show_bar: false`, see `fs_glow`'s doc comment).
    glow_layers_c: [f32; 4],
    /// Strand bundle (`project::StrandSpec`), part a: x = strand count (0 = disabled,
    /// otherwise capped at 8 by `barrier.wgsl`'s loop), y = `spread_px`, z = `jitter` (0..1),
    /// w = `thickness_px`. Uploaded unconditionally whenever `WavySpec::strands` is `Some(..)`,
    /// regardless of `mode` — `barrier.wgsl`'s `fs_glow` is the sole place that gates actual
    /// rendering to `WavyMode::Edge`, matching `strand_params_a.x`/`strand_params_b`'s own doc
    /// comment in `barrier.wgsl`.
    strand_params_a: [f32; 4],
    /// Strand bundle, part b: x = `halo_amplitude`, y = `halo_sigma_px`, z = `glow_intensity`,
    /// w = `flicker_speed`.
    strand_params_b: [f32; 4],
}

impl Default for Uniforms {
    fn default() -> Self {
        Self {
            geometry: [1.0, 1.0, 0.0, 4.0],
            bar_color: [1.0, 1.0, 1.0, 0.0],
            flags: [0.0, 0.0, 0.0, 0.0],
            wave: [0.0, 0.0, 0.0, 0.0],
            glow_style: [1.0, 1.0, 1.0, 0.0],
            glow_brightness_pulse: [1.0, 1.0, 0.0, 0.0],
            glow_layers_ab: [0.0, 0.0, 0.0, 0.0],
            glow_layers_c: [0.0, 0.0, 0.0, 0.0],
            strand_params_a: [0.0, 0.0, 0.0, 0.0],
            strand_params_b: [0.0, 0.0, 0.0, 0.0],
        }
    }
}

/// sRGB u8 -> linear f32, matching `notes::color_to_linear`/`notes::pipeline::srgb_to_linear` —
/// kept as its own small copy rather than shared (both of those are private to `notes`), same
/// call this codebase already made twice for the identical conversion.
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

pub struct BarrierRenderer {
    /// Opaque flat bar, drawn second so it occludes the glow beneath it (unchanged
    /// `ALPHA_BLENDING`, `fs_core` entry point). Drawn only when `show_bar` is true.
    core_pipeline: wgpu::RenderPipeline,
    /// Additive corona, drawn first (`ONE`/`ONE` blend, `fs_glow` entry point). Drawn only when
    /// `glow` is set.
    glow_pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    data: Uniforms,
    canvas_size: (f32, f32),
    /// Stashed from the last `set_style` call so `update_pulse` (called separately, every frame)
    /// doesn't need the whole `BarrierLayer` threaded through again just for this one field.
    pulse: Option<Pulse>,
    /// Stashed from the last `set_style` call, read directly by `render` — independent of
    /// `data.flags[0]` (glow enabled), see the module doc comment.
    show_bar: bool,
}

impl BarrierRenderer {
    pub fn new(device: &wgpu::Device, texture_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("barrier_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("barrier.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("barrier_bind_group_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("barrier_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        // Additive: color/alpha both `src + dst` (`ONE`/`ONE`) — light stacking on light, same
        // convention `effects.rs`'s `additive_blend` already uses for flashes/additive particles.
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

        let make_pipeline = |label: &str, entry_point: &'static str, blend: wgpu::BlendState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
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
        let core_pipeline = make_pipeline(
            "barrier_core_pipeline",
            "fs_core",
            wgpu::BlendState::ALPHA_BLENDING,
        );
        let glow_pipeline = make_pipeline("barrier_glow_pipeline", "fs_glow", additive_blend);

        let data = Uniforms::default();
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("barrier_uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("barrier_bind_group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        Self {
            core_pipeline,
            glow_pipeline,
            uniform_buffer,
            bind_group,
            data,
            canvas_size: (1.0, 1.0),
            pulse: None,
            show_bar: true,
        }
    }

    /// Recomputes geometry/color/glow from the current canvas size, calibrated barrier fraction,
    /// and `BarrierLayer`, and uploads them. Cheap (one small uniform write, no instance rebuild
    /// like `notes::NotesRenderer::resize`'s fill/sheen/glow changes need) — called
    /// unconditionally every redraw, like `video_quad::update_viewport`.
    pub fn set_style(
        &mut self,
        queue: &wgpu::Queue,
        canvas_size: (f32, f32),
        barrier_fraction: f32,
        barrier_layer: &BarrierLayer,
    ) {
        let (width, height) = (canvas_size.0.max(1.0), canvas_size.1.max(1.0));
        let barrier_y = height * barrier_fraction;
        // `resolve_constant`, not `resolve_for_note`: the barrier is one canvas-wide bar, not tied
        // to any single note, so there's no per-note velocity/pitch/track to resolve against here.
        let [r, g, b] = srgb_to_linear(barrier_layer.color.resolve_constant());
        self.canvas_size = (width, height);
        self.pulse = barrier_layer.pulse;
        self.show_bar = barrier_layer.show_bar;
        self.data.geometry = [width, height, barrier_y, barrier_layer.thickness.max(0.0)];
        self.data.bar_color[0] = r;
        self.data.bar_color[1] = g;
        self.data.bar_color[2] = b;
        match &barrier_layer.glow {
            Some(glow) => {
                // Same reasoning as `barrier_layer.color` above — one glow for the whole bar.
                let [gr, gg, gb] = srgb_to_linear(glow.color.resolve_constant());
                let layers = glow.layers;
                let margin = layers
                    .iter()
                    .fold(0.0f32, |acc, layer| acc.max(layer.sigma_px))
                    * GLOW_CUTOFF_SIGMAS;
                self.data.flags[0] = 1.0;
                self.data.glow_style = [gr, gg, gb, 0.0];
                self.data.glow_brightness_pulse[0] = glow.brightness;
                self.data.glow_layers_ab = [
                    layers[0].amplitude,
                    layers[0].sigma_px,
                    layers[1].amplitude,
                    layers[1].sigma_px,
                ];
                self.data.glow_layers_c = [layers[2].amplitude, layers[2].sigma_px, margin, 0.0];
            }
            None => {
                self.data.flags[0] = 0.0;
                self.data.glow_style = [0.0; 4];
                self.data.glow_brightness_pulse[0] = 1.0;
                self.data.glow_layers_ab = [0.0; 4];
                self.data.glow_layers_c = [0.0; 4];
            }
        }
        self.data.glow_layers_c[3] = if barrier_layer.show_bar { 1.0 } else { 0.0 };
        // No pulse configured: peak == resting, so `mix(resting, peak, pulse_curve)` in the
        // shader is an exact no-op regardless of what `pulse_curve` computes to.
        self.data.glow_brightness_pulse[1] = barrier_layer
            .pulse
            .map(|pulse| pulse.brightness)
            .unwrap_or(self.data.glow_brightness_pulse[0]);
        match &barrier_layer.wavy {
            Some(WavySpec {
                amplitude_px,
                wavelength_px,
                speed,
                mode,
                slide_speed,
                strands,
            }) => {
                self.data.flags[2] = 1.0;
                self.data.flags[3] = match mode {
                    WavyMode::TopWave => 0.0,
                    WavyMode::Edge => 1.0,
                    WavyMode::FullWave => 2.0,
                };
                self.data.wave[0] = amplitude_px.max(0.0);
                self.data.wave[1] = *wavelength_px;
                self.data.wave[2] = *speed;
                // Parked in `glow_style`'s spare `w` slot (see that field's own doc comment) —
                // affects the whole wavy edge, every `WavyMode`, not just the strand bundle, since
                // strands re-sample the same `wavy_offset_seeded` field the base edge itself uses.
                self.data.glow_style[3] = *slide_speed;
                // Uploaded unconditionally regardless of `mode` — `barrier.wgsl`'s `fs_glow` is
                // the single place that gates strand rendering to `WavyMode::Edge` (see its own
                // comment); no matching gate here, so this stays in lockstep with the shader
                // rather than encoding the same restriction twice and risking drift.
                match strands {
                    Some(StrandSpec {
                        count,
                        spread_px,
                        jitter,
                        thickness_px,
                        halo_amplitude,
                        halo_sigma_px,
                        glow_intensity,
                        flicker_speed,
                    }) => {
                        self.data.strand_params_a = [
                            *count as f32,
                            spread_px.max(0.0),
                            jitter.clamp(0.0, 1.0),
                            thickness_px.max(0.01),
                        ];
                        self.data.strand_params_b = [
                            halo_amplitude.max(0.0),
                            halo_sigma_px.max(0.01),
                            glow_intensity.max(0.0),
                            flicker_speed.max(0.0),
                        ];
                    }
                    None => {
                        self.data.strand_params_a = [0.0; 4];
                        self.data.strand_params_b = [0.0; 4];
                    }
                }
            }
            None => {
                self.data.flags[2] = 0.0;
                self.data.flags[3] = 0.0;
                self.data.wave[0] = 0.0;
                self.data.wave[1] = 0.0;
                self.data.wave[2] = 0.0;
                self.data.glow_style[3] = 0.0;
                self.data.strand_params_a = [0.0; 4];
                self.data.strand_params_b = [0.0; 4];
            }
        }
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[self.data]));
    }

    /// The most recent note at or before `time_seconds` (expected to already have the sync offset
    /// subtracted, same convention as `notes::NotesRenderer::update`) triggers a decaying `1.0 ->
    /// 0.0` curve over `pulse.decay_seconds`, which the shader mixes between the resting and peak
    /// brightness (see `barrier.wgsl`). Stateless by design: scrubbing anywhere just recomputes
    /// from whatever note last landed at or before the new position — no separate "clear on
    /// scrub" bookkeeping needed (unlike the transition pass's particle pool, which is inherently
    /// stateful and won't get this luxury).
    fn pulse_curve(&self, time_seconds: f32, note_starts: &[f32]) -> f32 {
        let Some(pulse) = self.pulse else {
            return 0.0;
        };
        if pulse.decay_seconds <= 0.0 || note_starts.is_empty() {
            return 0.0;
        }
        let idx = note_starts.partition_point(|&t| t <= time_seconds);
        if idx == 0 {
            return 0.0;
        }
        let elapsed = time_seconds - note_starts[idx - 1];
        if !(0.0..pulse.decay_seconds).contains(&elapsed) {
            return 0.0;
        }
        1.0 - elapsed / pulse.decay_seconds
    }

    /// Recomputes and uploads the pulse curve for `time_seconds` against `note_starts` (the
    /// same sorted onset list `notes::NotesRenderer::note_start_times` already caches for the
    /// timeline's note-density strip). Split from `set_style` since this needs to run every frame
    /// as the transport advances, while `set_style`'s inputs only change on a slider drag or style
    /// import — both are cheap uniform writes regardless, so `Compositor::update_barrier` calls
    /// them together.
    pub fn update_pulse(&mut self, queue: &wgpu::Queue, time_seconds: f32, note_starts: &[f32]) {
        self.data.flags[1] = self.pulse_curve(time_seconds, note_starts);
        self.data.wave[3] = time_seconds;
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[self.data]));
    }

    pub fn render(&self, render_pass: &mut wgpu::RenderPass<'_>) {
        // `notes::NotesRenderer::render` (drawn just before this in `Compositor::render`) leaves a
        // scissor rect clipping to everything *above* the barrier line — reset to the full canvas
        // first, or the barrier bar (which sits at/below that edge, and extends further below it
        // when glow is enabled) would be clipped away instead of drawn.
        render_pass.set_scissor_rect(
            0,
            0,
            self.canvas_size.0.round().max(1.0) as u32,
            self.canvas_size.1.round().max(1.0) as u32,
        );
        // Glow first (additive), core second (alpha-blended) — the opaque core drawn on top
        // correctly occludes the glow directly beneath it. Independent conditions: a barrier can
        // be pure glow with no bar, a bar with no glow, both, or neither.
        if self.data.flags[0] > 0.5 {
            render_pass.set_pipeline(&self.glow_pipeline);
            render_pass.set_bind_group(0, &self.bind_group, &[]);
            render_pass.draw(0..6, 0..1);
        }
        if self.show_bar {
            render_pass.set_pipeline(&self.core_pipeline);
            render_pass.set_bind_group(0, &self.bind_group, &[]);
            render_pass.draw(0..6, 0..1);
        }
    }
}
