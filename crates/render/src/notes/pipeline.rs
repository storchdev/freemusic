//! Owns the note-highway render pipeline: geometry (a single reusable unit quad, instanced),
//! the view/time uniforms, and the instance buffer. Deliberately hand-rolled rather than reusing
//! `wgpu_jumpstart`'s generic `Uniform`/`Instances`/`Shape` helpers (see `video_quad.rs` for the
//! same style already used elsewhere in this crate) — now that we own the shader there's no
//! reason to keep depending on Neothesia's renderer-side crate at all, matching the precedent
//! `mp4-encoder` set for forking rather than reusing upstream code that needed real changes.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use project::{Fill, Glow, NoteLayer, Sheen};

use super::instance::NoteInstance;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ViewUniform {
    /// Column-major 4x4 orthographic projection, flattened (`orthographic_projection`).
    transform: [f32; 16],
    size: [f32; 2],
    scale: f32,
    /// Fraction (0.0-1.0) of `size.y` where the hit line sits — replaces Neothesia's vendored
    /// shader's hardcoded 80% constant, so no viewport-remapping trick is needed to move it.
    barrier_fraction: f32,
}

impl Default for ViewUniform {
    fn default() -> Self {
        Self {
            transform: orthographic_projection(1.0, 1.0),
            size: [1.0, 1.0],
            scale: 1.0,
            barrier_fraction: 0.8,
        }
    }
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

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TimeUniform {
    time: f32,
    speed: f32,
}

impl Default for TimeUniform {
    fn default() -> Self {
        Self {
            time: 0.0,
            speed: 400.0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct QuadVertex {
    position: [f32; 2],
}

/// Mirrors `shader.wgsl`'s `StyleUniform` field-for-field — see that struct's doc comment for why
/// every field is packed into vec4s (sidesteps the std140 padding mismatch CLAUDE.md documents
/// for `mat3x3<f32>` uniforms elsewhere in this codebase).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct StyleUniform {
    fill_and_flags: [f32; 4],
    sheen_params: [f32; 4],
    glow_color_radius: [f32; 4],
    glow_intensity: [f32; 4],
}

impl Default for StyleUniform {
    fn default() -> Self {
        Self {
            fill_and_flags: [0.0; 4],
            sheen_params: [0.0; 4],
            glow_color_radius: [0.0; 4],
            glow_intensity: [0.0; 4],
        }
    }
}

/// sRGB u8 -> linear f32, matching `super::color_to_linear` (kept private to that module; glow
/// color needs the same conversion here since it's uploaded via a separate uniform, not a
/// `NoteInstance` field).
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

impl StyleUniform {
    fn from_note_layer(note_layer: &NoteLayer) -> Self {
        let fill_kind = match note_layer.fill {
            Fill::Solid(_) => 0.0,
            Fill::VerticalGradient { .. } => 1.0,
        };
        let (sheen_enabled, sheen_params) = match note_layer.sheen {
            Some(Sheen {
                intensity,
                width,
                angle_degrees,
            }) => (1.0, [intensity, width, angle_degrees.to_radians(), 0.0]),
            None => (0.0, [0.0; 4]),
        };
        let (glow_enabled, glow_color_radius, glow_intensity) = match &note_layer.glow {
            Some(Glow {
                color,
                radius_px,
                intensity,
            }) => {
                let [r, g, b] = srgb_to_linear(color.resolve_constant());
                (1.0, [r, g, b, *radius_px], [*intensity, 0.0, 0.0, 0.0])
            }
            None => (0.0, [0.0; 4], [0.0; 4]),
        };
        Self {
            fill_and_flags: [fill_kind, sheen_enabled, glow_enabled, 0.0],
            sheen_params,
            glow_color_radius,
            glow_intensity,
        }
    }
}

pub struct NotesPipeline {
    render_pipeline: wgpu::RenderPipeline,

    quad_vertex_buffer: wgpu::Buffer,
    quad_index_buffer: wgpu::Buffer,
    quad_index_count: u32,

    instances: Vec<NoteInstance>,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,

    view_data: ViewUniform,
    view_buffer: wgpu::Buffer,
    view_bind_group: wgpu::BindGroup,

    time_data: TimeUniform,
    time_buffer: wgpu::Buffer,
    time_bind_group: wgpu::BindGroup,

    style_buffer: wgpu::Buffer,
    style_bind_group: wgpu::BindGroup,
}

impl NotesPipeline {
    const INITIAL_INSTANCE_CAPACITY: usize = 1024;

    pub fn new(device: &wgpu::Device, texture_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("notes_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let view_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("notes_view_bind_group_layout"),
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
        let time_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("notes_time_bind_group_layout"),
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
        let style_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("notes_style_bind_group_layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    // Read by both stages: the vertex shader needs `glow_color_radius.w` to
                    // inflate the quad, the fragment shader needs the rest for fill/sheen/glow.
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
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
            label: Some("notes_view_uniform"),
            contents: bytemuck::cast_slice(&[view_data]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let view_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("notes_view_bind_group"),
            layout: &view_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: view_buffer.as_entire_binding(),
            }],
        });

        let time_data = TimeUniform::default();
        let time_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("notes_time_uniform"),
            contents: bytemuck::cast_slice(&[time_data]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let time_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("notes_time_bind_group"),
            layout: &time_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: time_buffer.as_entire_binding(),
            }],
        });

        let style_data = StyleUniform::default();
        let style_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("notes_style_uniform"),
            contents: bytemuck::cast_slice(&[style_data]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let style_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("notes_style_bind_group"),
            layout: &style_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: style_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("notes_pipeline_layout"),
            bind_group_layouts: &[
                Some(&view_bind_group_layout),
                Some(&time_bind_group_layout),
                Some(&style_bind_group_layout),
            ],
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
        let instance_attributes = NoteInstance::attributes();
        let instance_layout = NoteInstance::layout(&instance_attributes);

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("notes_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[quad_vertex_layout, instance_layout],
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

        // Unit quad, matching `piano_layout`'s key-position convention (top-left origin, size in
        // the same units as `NoteInstance::size`) — the vertex shader scales/positions it.
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
            label: Some("notes_quad_vertex_buffer"),
            contents: bytemuck::cast_slice(VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let quad_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("notes_quad_index_buffer"),
            contents: bytemuck::cast_slice(INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });

        let instance_buffer = Self::create_instance_buffer(device, Self::INITIAL_INSTANCE_CAPACITY);

        Self {
            render_pipeline,
            quad_vertex_buffer,
            quad_index_buffer,
            quad_index_count: INDICES.len() as u32,
            instances: Vec::new(),
            instance_buffer,
            instance_capacity: Self::INITIAL_INSTANCE_CAPACITY,
            view_data,
            view_buffer,
            view_bind_group,
            time_data,
            time_buffer,
            time_bind_group,
            style_buffer,
            style_bind_group,
        }
    }

    fn create_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("notes_instance_buffer"),
            size: (std::mem::size_of::<NoteInstance>() * capacity.max(1)) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Recomputes the orthographic projection and barrier fraction for a new canvas size.
    pub fn set_view(
        &mut self,
        queue: &wgpu::Queue,
        width: f32,
        height: f32,
        barrier_fraction: f32,
    ) {
        self.view_data.transform = orthographic_projection(width.max(1.0), height.max(1.0));
        self.view_data.size = [width.max(1.0), height.max(1.0)];
        self.view_data.barrier_fraction = barrier_fraction;
        queue.write_buffer(
            &self.view_buffer,
            0,
            bytemuck::cast_slice(&[self.view_data]),
        );
    }

    pub fn set_speed(&mut self, queue: &wgpu::Queue, speed: f32) {
        self.time_data.speed = speed;
        queue.write_buffer(
            &self.time_buffer,
            0,
            bytemuck::cast_slice(&[self.time_data]),
        );
    }

    pub fn update_time(&mut self, queue: &wgpu::Queue, time: f32) {
        self.time_data.time = time;
        queue.write_buffer(
            &self.time_buffer,
            0,
            bytemuck::cast_slice(&[self.time_data]),
        );
    }

    /// Uploads fill/sheen/glow parameters for the given `NoteLayer`. Per-note colors (including
    /// the resolved solid/gradient endpoints) are baked into `NoteInstance` at build time instead
    /// (see `rebuild_instances`) — this uniform only carries the style-wide knobs a single note
    /// instance can't express: which fill mode to interpret the two colors as, and the sheen/glow
    /// parameters, which apply uniformly to every note rather than varying per-note.
    pub fn set_style(&mut self, queue: &wgpu::Queue, note_layer: &NoteLayer) {
        let style_data = StyleUniform::from_note_layer(note_layer);
        queue.write_buffer(&self.style_buffer, 0, bytemuck::cast_slice(&[style_data]));
    }

    pub fn instances(&mut self) -> &mut Vec<NoteInstance> {
        &mut self.instances
    }

    pub fn clear(&mut self) {
        self.instances.clear();
    }

    /// Uploads the current instance list, growing the GPU buffer first if it no longer fits.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if self.instances.len() > self.instance_capacity {
            self.instance_capacity = self.instances.len();
            self.instance_buffer = Self::create_instance_buffer(device, self.instance_capacity);
        }
        queue.write_buffer(
            &self.instance_buffer,
            0,
            bytemuck::cast_slice(&self.instances),
        );
    }

    pub fn render<'rpass>(&'rpass self, render_pass: &mut wgpu::RenderPass<'rpass>) {
        if self.instances.is_empty() {
            return;
        }
        render_pass.set_pipeline(&self.render_pipeline);
        render_pass.set_bind_group(0, &self.view_bind_group, &[]);
        render_pass.set_bind_group(1, &self.time_bind_group, &[]);
        render_pass.set_bind_group(2, &self.style_bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.quad_vertex_buffer.slice(..));
        render_pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        render_pass.set_index_buffer(self.quad_index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        render_pass.draw_indexed(0..self.quad_index_count, 0, 0..self.instances.len() as u32);
    }
}
