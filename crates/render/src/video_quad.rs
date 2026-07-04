//! Renders a single decoded video frame as an aspect-correct textured quad.

use bytemuck::{Pod, Zeroable};
use project::VideoTransform;

/// Column-major 3x3 matrix (`m[col][row]`), matching WGSL's `mat3x3<f32>` layout convention.
type Mat3 = [[f32; 3]; 3];

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    /// Each column padded to 16 bytes (4 floats) to match WGSL's uniform-buffer layout rules
    /// for `mat3x3<f32>` (column stride 16, not 12) — writing this without the padding would
    /// silently misalign every field after it in the shader's view of the buffer.
    transform: [[f32; 4]; 3],
    crop_uv_min: [f32; 2],
    crop_uv_max: [f32; 2],
    brightness: f32,
    _padding: [f32; 3],
}

fn mat3_mul(a: Mat3, b: Mat3) -> Mat3 {
    let mut out = [[0.0f32; 3]; 3];
    for col in 0..3 {
        for row in 0..3 {
            out[col][row] = a[0][row] * b[col][0] + a[1][row] * b[col][1] + a[2][row] * b[col][2];
        }
    }
    out
}

fn mat3_scale(sx: f32, sy: f32) -> Mat3 {
    [[sx, 0.0, 0.0], [0.0, sy, 0.0], [0.0, 0.0, 1.0]]
}

/// Scales the y axis by `factor`, used to temporarily equalize the physical pixel distance
/// represented by one NDC unit on each axis before rotating — without this, rotating in NDC
/// directly would visibly shear the image on any non-square window.
fn mat3_aspect(factor: f32) -> Mat3 {
    [[1.0, 0.0, 0.0], [0.0, factor, 0.0], [0.0, 0.0, 1.0]]
}

fn mat3_rotate(radians: f32) -> Mat3 {
    let (s, c) = radians.sin_cos();
    [[c, s, 0.0], [-s, c, 0.0], [0.0, 0.0, 1.0]]
}

/// Third row `[tilt_x, tilt_y, 1]` — applied to a homogeneous (x, y, 1) point this produces
/// `w' = 1 + tilt_x*x + tilt_y*y`, which the vertex shader feeds straight into `clip_position.w`
/// so the GPU's own perspective divide does the keystone distortion.
fn mat3_tilt(tilt_x: f32, tilt_y: f32) -> Mat3 {
    [[1.0, 0.0, tilt_x], [0.0, 1.0, tilt_y], [0.0, 0.0, 1.0]]
}

/// Third column `[tx, ty, 1]` — a plain NDC-space pan offset, applied after rotation so it
/// shifts the already-rotated/letterboxed rectangle rather than shifting before rotating around
/// the origin (which would also swing the shifted position around).
fn mat3_translate(tx: f32, ty: f32) -> Mat3 {
    [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [tx, ty, 1.0]]
}

fn pad_columns(m: Mat3) -> [[f32; 4]; 3] {
    [
        [m[0][0], m[0][1], m[0][2], 0.0],
        [m[1][0], m[1][1], m[1][2], 0.0],
        [m[2][0], m[2][1], m[2][2], 0.0],
    ]
}

/// Builds the single homography uploaded to the shader: letterbox scale (`letterbox`, fit
/// video aspect into the window) combined with the user's zoom (`transform.scale`) first, then
/// a rotation done in pixel-aspect-corrected space (see `mat3_aspect`), then the pan/translate
/// offset (shifting the already-rotated rectangle within the window), then the tilt/keystone
/// terms last so they act on the final on-screen rectangle.
fn build_transform(letterbox: [f32; 2], window_aspect: f32, transform: &VideoTransform) -> Mat3 {
    let scale = mat3_scale(
        letterbox[0] * transform.scale,
        letterbox[1] * transform.scale,
    );
    let rotation = mat3_rotate(transform.rotation_degrees.to_radians());
    let translate = mat3_translate(transform.translate_x, transform.translate_y);
    let tilt = mat3_tilt(transform.tilt_x, transform.tilt_y);
    let aspect_corrected_rotation = mat3_mul(
        mat3_aspect(window_aspect),
        mat3_mul(rotation, mat3_aspect(1.0 / window_aspect)),
    );
    mat3_mul(
        tilt,
        mat3_mul(translate, mat3_mul(aspect_corrected_rotation, scale)),
    )
}

pub struct VideoQuad {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    uniform_buffer: wgpu::Buffer,
    texture: wgpu::Texture,
    texture_size: (u32, u32),
}

impl VideoQuad {
    const TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

    pub fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("video_quad_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("video_quad_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    // Also FRAGMENT now: the fragment shader reads `uniforms.brightness`.
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("video_quad_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("video_quad_pipeline"),
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
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
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

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("video_quad_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("video_quad_uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let (texture, view) = create_placeholder_texture(device);
        let bind_group =
            create_bind_group(device, &bind_group_layout, &uniform_buffer, &view, &sampler);

        Self {
            pipeline,
            bind_group_layout,
            bind_group,
            sampler,
            uniform_buffer,
            texture,
            texture_size: (1, 1),
        }
    }

    /// Uploads a freshly decoded BGRA frame, recreating the GPU texture if its size changed.
    pub fn upload_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        bgra: &[u8],
    ) {
        if self.texture_size != (width, height) {
            let (texture, view) = create_texture(device, width, height);
            self.bind_group = create_bind_group(
                device,
                &self.bind_group_layout,
                &self.uniform_buffer,
                &view,
                &self.sampler,
            );
            self.texture = texture;
            self.texture_size = (width, height);
        }

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bgra,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Recomputes the letterbox/pillarbox scale, rotate/tilt homography, crop UV rect, and
    /// brightness for the current window size and transform, and uploads them. Cheap enough
    /// (one small uniform write) to call unconditionally every redraw rather than dirty-checking
    /// like `midi_overlay.resize`'s full instance-buffer rebuild.
    pub fn update_viewport(
        &self,
        queue: &wgpu::Queue,
        window_size: (u32, u32),
        transform: &VideoTransform,
    ) {
        let (video_w, video_h) = self.texture_size;
        let crop_w_frac = (transform.crop_right - transform.crop_left).max(0.01);
        let crop_h_frac = (transform.crop_bottom - transform.crop_top).max(0.01);

        let scale = if video_w == 0 || video_h == 0 || window_size.0 == 0 || window_size.1 == 0 {
            [1.0, 1.0]
        } else {
            let cropped_aspect = (video_w as f32 * crop_w_frac) / (video_h as f32 * crop_h_frac);
            let window_aspect = window_size.0 as f32 / window_size.1 as f32;
            if cropped_aspect > window_aspect {
                [1.0, window_aspect / cropped_aspect]
            } else {
                [cropped_aspect / window_aspect, 1.0]
            }
        };
        let window_aspect = if window_size.1 == 0 {
            1.0
        } else {
            window_size.0 as f32 / window_size.1 as f32
        };

        queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[Uniforms {
                transform: pad_columns(build_transform(scale, window_aspect, transform)),
                crop_uv_min: [transform.crop_left, transform.crop_top],
                crop_uv_max: [transform.crop_right, transform.crop_bottom],
                brightness: transform.brightness,
                _padding: [0.0; 3],
            }]),
        );
    }

    pub fn render(&self, render_pass: &mut wgpu::RenderPass<'_>) {
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.draw(0..6, 0..1);
    }
}

fn create_placeholder_texture(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
    create_texture(device, 1, 1)
}

fn create_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("video_frame_texture"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: VideoQuad::TEXTURE_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform_buffer: &wgpu::Buffer,
    texture_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("video_quad_bind_group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(texture_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}
