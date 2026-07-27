//! Faint vertical reference lines at each octave's C boundary in the note highway — a purely
//! visual grid, independent of any specific note. Reuses `notes::octave_boundary_fractions` (the
//! same computation the note-lane layout itself keys off) so the lines always align exactly with
//! where each octave's keys actually sit, camera-stretch calibration included.
//!
//! Self-contained quad pass, structured like `barrier.rs`: no vertex buffer (six hardcoded
//! unit-quad corners per line, positioned/sized in the vertex shader from a uniform), one bind
//! group, one instanced draw call (one instance per interior octave boundary) instead of a loop of
//! draw calls.

use bytemuck::{Pod, Zeroable};

use project::{KeyboardCalibration, OctaveLineSpec};

use crate::notes::octave_boundary_fractions;

/// Interior octave (C-note) boundaries on a standard 88-key keyboard — see
/// `notes::OCTAVE_BOUNDARY_NOTES`, the same count `octave_boundary_fractions` returns bounds for
/// (10 fractions total: the two calibrated edges plus these 8 interior ones).
const LINE_COUNT: usize = 8;

/// All-vec4 layout, same reasoning as `barrier::Uniforms`/`notes::pipeline::StyleUniform` — every
/// field is already vec4-aligned so there's no std140 column-padding mismatch to get wrong.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    /// x = canvas width, y = canvas height, z = barrier y (px, lines stop here — same span as the
    /// note highway itself), w = line width (px).
    geometry: [f32; 4],
    /// xyz = line color (linear), w = alpha (straight, not premultiplied).
    color: [f32; 4],
    /// The 8 interior octave-boundary x positions (canvas px), packed as two vec4s since WGSL's
    /// uniform-buffer layout has no plain `array<f32, 8>` shortcut without per-element padding.
    x_positions: [[f32; 4]; 2],
}

impl Default for Uniforms {
    fn default() -> Self {
        Self {
            geometry: [1.0, 1.0, 0.0, 0.0],
            color: [1.0, 1.0, 1.0, 0.0],
            x_positions: [[0.0; 4]; 2],
        }
    }
}

/// sRGB u8 -> linear f32, matching `barrier::srgb_to_linear`/`notes::color_to_linear` — kept as
/// its own small copy rather than shared (both are private to their own modules), same call this
/// codebase already makes more than once for the identical conversion.
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

pub struct OctaveLinesRenderer {
    pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    data: Uniforms,
    /// Whether `set_style` was last called with `Some(..)` — `None` draws nothing at all, rather
    /// than a zero-alpha (and so invisible but still GPU-costed) draw call.
    visible: bool,
}

impl OctaveLinesRenderer {
    pub fn new(device: &wgpu::Device, texture_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("octave_lines_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("octave_lines.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("octave_lines_bind_group_layout"),
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
            label: Some("octave_lines_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("octave_lines_pipeline"),
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
                    // Straight (non-additive) alpha blending — an RGBA line should read as a plain
                    // translucent overlay over the video/notes beneath it, not stack brighter the
                    // way the glow passes' additive blending does.
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
            label: Some("octave_lines_uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("octave_lines_bind_group"),
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
            visible: false,
        }
    }

    /// Recomputes line positions/color/width from the current canvas size, calibrated barrier
    /// fraction, and `spec`, and uploads them. Cheap (one small uniform write, no instance
    /// rebuild) — called unconditionally every redraw, like `barrier::BarrierRenderer::set_style`.
    /// `spec: None` (no `octave_lines` configured) marks the renderer as invisible; `render` skips
    /// the draw call entirely rather than issuing a zero-alpha one.
    pub fn set_style(
        &mut self,
        queue: &wgpu::Queue,
        canvas_size: (f32, f32),
        barrier_fraction: f32,
        calibration: &KeyboardCalibration,
        spec: Option<&OctaveLineSpec>,
    ) {
        let (width, height) = (canvas_size.0.max(1.0), canvas_size.1.max(1.0));
        self.visible = spec.is_some();
        let Some(spec) = spec else {
            queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[self.data]));
            return;
        };

        let bounds = octave_boundary_fractions(calibration);
        let mut positions = [0.0f32; LINE_COUNT];
        for (i, position) in positions.iter_mut().enumerate() {
            // `bounds[0]` is the calibrated left edge, `bounds[9]` the right edge — the 8 interior
            // entries in between (`bounds[1..9]`) are the octave (C-note) boundaries themselves.
            *position = bounds[i + 1] * width;
        }
        self.data.x_positions = [
            [positions[0], positions[1], positions[2], positions[3]],
            [positions[4], positions[5], positions[6], positions[7]],
        ];

        let [r, g, b, a] = spec.color;
        let [lr, lg, lb] = srgb_to_linear([r, g, b]);
        self.data.color = [lr, lg, lb, a as f32 / 255.0];
        self.data.geometry = [
            width,
            height,
            height * barrier_fraction,
            spec.width_px.max(0.0),
        ];

        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[self.data]));
    }

    pub fn render(&self, render_pass: &mut wgpu::RenderPass<'_>) {
        if !self.visible {
            return;
        }
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.draw(0..6, 0..LINE_COUNT as u32);
    }
}
