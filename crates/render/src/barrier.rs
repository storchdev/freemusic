//! Renders the barrier where falling notes stop: a full-canvas-width horizontal bar, optionally
//! with a soft glow falloff (`BarrierKind::Glow`) and a decaying brightness pulse when a note
//! arrives (Phase D of the `.fmstyle.ron` milestone). A real wgpu pass rather than the plain
//! `egui` overlay milestone 6a used — see CLAUDE.md — so the barrier now shows up in exported
//! video too; `ui::draw_barrier_handle` keeps only the drag hit-region for editing.
//!
//! Self-contained quad pass, structured like `video_quad.rs`: no vertex buffer (six hardcoded
//! unit-quad corners, positioned/sized in the vertex shader from a uniform), one bind group.

use bytemuck::{Pod, Zeroable};

use project::{BarrierKind, BarrierLayer, Pulse, WavySpec};

/// All-vec4 layout, same reasoning as `notes::pipeline::StyleUniform` — every field is already
/// vec4-aligned so there's no std140 column-padding mismatch (the `mat3x3<f32>` uniform CLAUDE.md
/// documents elsewhere in this codebase) to get wrong.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    /// x = canvas width, y = canvas height, z = barrier center y (pixels), w = thickness (pixels).
    geometry: [f32; 4],
    /// xyz = barrier color (linear), w = glow radius (pixels).
    color_glow_radius: [f32; 4],
    /// x = glow enabled (0/1), y = pulse intensity (0..1, decaying), z = wavy enabled (0/1),
    /// w = wavy both_edges (0/1, only meaningful when z is set).
    flags: [f32; 4],
    /// x = wave amplitude (px), y = wavelength (px), z = speed, w = transport time (seconds).
    wave: [f32; 4],
}

impl Default for Uniforms {
    fn default() -> Self {
        Self {
            geometry: [1.0, 1.0, 0.0, 4.0],
            color_glow_radius: [1.0, 1.0, 1.0, 0.0],
            flags: [0.0, 0.0, 0.0, 0.0],
            wave: [0.0, 0.0, 0.0, 0.0],
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
    pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    data: Uniforms,
    canvas_size: (f32, f32),
    /// Stashed from the last `set_style` call so `update_pulse` (called separately, every frame)
    /// doesn't need the whole `BarrierLayer` threaded through again just for this one field.
    pulse: Option<Pulse>,
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

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("barrier_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: texture_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

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
            pipeline,
            uniform_buffer,
            bind_group,
            data,
            canvas_size: (1.0, 1.0),
            pulse: None,
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
        let glow_enabled = matches!(barrier_layer.kind, BarrierKind::Glow) as u8 as f32;
        let [r, g, b] = srgb_to_linear(barrier_layer.color.resolve_constant());
        self.canvas_size = (width, height);
        self.pulse = barrier_layer.pulse;
        self.data.geometry = [width, height, barrier_y, barrier_layer.thickness.max(0.0)];
        self.data.color_glow_radius = [r, g, b, barrier_layer.glow_radius_px.max(0.0)];
        self.data.flags[0] = glow_enabled;
        match &barrier_layer.wavy {
            Some(WavySpec {
                amplitude_px,
                wavelength_px,
                speed,
                both_edges,
            }) => {
                self.data.flags[2] = 1.0;
                self.data.flags[3] = *both_edges as u8 as f32;
                self.data.wave[0] = amplitude_px.max(0.0);
                self.data.wave[1] = *wavelength_px;
                self.data.wave[2] = *speed;
            }
            None => {
                self.data.flags[2] = 0.0;
                self.data.flags[3] = 0.0;
                self.data.wave[0] = 0.0;
                self.data.wave[1] = 0.0;
                self.data.wave[2] = 0.0;
            }
        }
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[self.data]));
    }

    /// The most recent note at or before `time_seconds` (expected to already have the sync offset
    /// subtracted, same convention as `notes::NotesRenderer::update`) brightens the bar, decaying
    /// linearly to 0 over `pulse.decay_seconds`. Stateless by design: scrubbing anywhere just
    /// recomputes from whatever note last landed at or before the new position — no separate
    /// "clear on scrub" bookkeeping needed (unlike the transition pass's particle pool, which is
    /// inherently stateful and won't get this luxury).
    fn pulse_intensity(&self, time_seconds: f32, note_starts: &[f32]) -> f32 {
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
        pulse.intensity * (1.0 - elapsed / pulse.decay_seconds)
    }

    /// Recomputes and uploads the pulse intensity for `time_seconds` against `note_starts` (the
    /// same sorted onset list `notes::NotesRenderer::note_start_times` already caches for the
    /// timeline's note-density strip). Split from `set_style` since this needs to run every frame
    /// as the transport advances, while `set_style`'s inputs only change on a slider drag or style
    /// import — both are cheap uniform writes regardless, so `Compositor::update_barrier` calls
    /// them together.
    pub fn update_pulse(&mut self, queue: &wgpu::Queue, time_seconds: f32, note_starts: &[f32]) {
        self.data.flags[1] = self.pulse_intensity(time_seconds, note_starts);
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
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.draw(0..6, 0..1);
    }
}
