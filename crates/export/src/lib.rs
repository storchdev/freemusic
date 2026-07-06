//! Headless-GPU offline render loop: re-runs the exact same `render::Compositor` the
//! interactive preview uses, but against an offscreen texture and a frame-accurate (`exact:
//! true`) decode of every output frame, muxing in the source video's own audio track. Meant to
//! be driven from a background thread — `run` blocks for the whole export and reports progress
//! over `mpsc`.

mod audio;
mod gpu;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

use gpu::HeadlessGpu;
use project::Project;
use render::{Compositor, GpuHandles};
use video_pipeline::VideoPipeline;

pub struct ExportSettings {
    pub output_path: PathBuf,
    pub fps: u32,
}

pub enum Progress {
    Frame { index: u64, total: u64 },
    Done,
    Cancelled,
    Error(String),
}

/// `wgpu` requires `bytes_per_row` in a texture<->buffer copy to be a multiple of this
/// (`COPY_BYTES_PER_ROW_ALIGNMENT` = 256). Rows aren't naturally aligned for arbitrary widths —
/// `neothesia-cli`'s own example code copies with unpadded `width * 4`, which only happens to
/// validate for widths that are multiples of 64. Fixed here by padding the readback buffer's
/// stride and trimming it back off per row before handing tightly-packed BGRA to the encoder.
fn align_up(value: u32, align: u32) -> u32 {
    value.div_ceil(align) * align
}

/// yuv420p (the encoder's pixel format) requires even dimensions.
fn even(value: u32) -> u32 {
    (value & !1).max(2)
}

/// sRGB u8 -> linear f32, matching `render::barrier::srgb_to_linear`/`render::effects::srgb_to_linear`
/// (and `app`'s own copy) — kept as its own small copy rather than shared (those are private to
/// `render`), same call this codebase already makes more than once for the identical conversion.
/// Used for the export scene pass's clear color so it composites correctly against the
/// compositor's linear-space blending, and matches the interactive preview's own clear color
/// exactly (both go through this identical formula).
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

/// Runs the whole export synchronously — call this from a `std::thread::spawn` closure.
/// `project` is a snapshot (paths, sync offset, calibration, transform); `cancel` is polled once
/// per output frame.
pub fn run(
    project: Project,
    settings: ExportSettings,
    progress: Sender<Progress>,
    cancel: Arc<AtomicBool>,
) {
    if let Err(err) = run_inner(&project, &settings, &progress, &cancel) {
        let _ = progress.send(Progress::Error(err));
    }
}

fn run_inner(
    project: &Project,
    settings: &ExportSettings,
    progress: &Sender<Progress>,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let video_path = project
        .video_path
        .as_ref()
        .ok_or_else(|| "no video loaded".to_string())?;

    let mut pipeline =
        VideoPipeline::open(video_path).map_err(|err| format!("failed to open video: {err}"))?;

    let width = even(pipeline.width);
    let height = even(pipeline.height);
    let duration_seconds = pipeline.duration_seconds();
    let fps = settings.fps.max(1);
    let total_frames = ((duration_seconds * fps as f64).ceil() as u64).max(1);

    let gpu = HeadlessGpu::new();
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let handles = GpuHandles {
        instance: &gpu.instance,
        adapter: &gpu.adapter,
        device: &gpu.device,
        queue: &gpu.queue,
        texture_format: format,
    };

    let note_layer = project.effective_note_layer();
    let barrier_layer = project.effective_barrier_layer();
    let transition_layer = project.effective_transition_layer();
    let background_color = srgb_to_linear(project.effective_background_color());
    let mut compositor = Compositor::new(
        &handles,
        (width as f32, height as f32),
        &project.calibration,
        &note_layer,
    );
    if let Some(midi_path) = project.midi_path.as_ref() {
        compositor
            .load_midi(
                &handles,
                (width as f32, height as f32),
                &project.calibration,
                &note_layer,
                midi_path,
                &project.skipped_notes,
            )
            .map_err(|err| format!("failed to load MIDI: {err}"))?;
    }

    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("export_offscreen_texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let unpadded_bytes_per_row = width * 4;
    let padded_bytes_per_row = align_up(unpadded_bytes_per_row, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);

    let with_audio = audio::has_audio_stream(video_path);
    let (encoder_info, mut feed_encoder) =
        mp4_encoder::new(&settings.output_path, width, height, fps, with_audio);

    let decoded_audio = with_audio
        .then(|| audio::decode_all(video_path, encoder_info.sample_rate))
        .transpose()?;
    let frame_size = encoder_info.frame_size;
    let mut audio_cursor = 0usize;

    for frame_index in 0..total_frames {
        if cancel.load(Ordering::Relaxed) {
            let _ = progress.send(Progress::Cancelled);
            return Ok(());
        }

        let t = frame_index as f64 / fps as f64;
        let frame = pipeline
            .seek_and_decode(t, true)
            .map_err(|err| format!("failed to decode frame at {t:.3}s: {err}"))?;

        compositor.upload_frame(
            &gpu.device,
            &gpu.queue,
            frame.width,
            frame.height,
            &frame.bgra,
        );
        compositor.update_viewport(&gpu.queue, (width, height), &project.transform);
        let midi_time = t - project.sync_offset_seconds;
        compositor.update_midi(&gpu.queue, midi_time as f32);
        compositor.update_barrier(
            &gpu.queue,
            (width as f32, height as f32),
            &project.calibration,
            &barrier_layer,
            midi_time as f32,
        );
        // Export renders frames in strictly increasing `t` order (never a scrub), so the
        // particle/flash sim's per-frame `dt` derivation in `update_transition` behaves exactly
        // like ordinary interactive playback — no special-casing needed here.
        compositor.update_transition(
            &gpu.device,
            &gpu.queue,
            (width as f32, height as f32),
            &project.calibration,
            &transition_layer,
            midi_time as f32,
        );

        let readback_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("export_readback_buffer"),
            size: u64::from(padded_bytes_per_row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut cmd_encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("export_frame_encoder"),
            });
        {
            let mut render_pass = cmd_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("export_scene_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: background_color[0] as f64,
                            g: background_color[1] as f64,
                            b: background_color[2] as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            compositor.render(&mut render_pass);
        }
        cmd_encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        gpu.queue.submit(std::iter::once(cmd_encoder.finish()));

        let slice = readback_buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        gpu.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map_err(|err| format!("wgpu poll failed: {err:?}"))?;

        let bgra = {
            let mapped = slice.get_mapped_range();
            let mut bgra = Vec::with_capacity((unpadded_bytes_per_row * height) as usize);
            for row in 0..height as usize {
                let start = row * padded_bytes_per_row as usize;
                let end = start + unpadded_bytes_per_row as usize;
                bgra.extend_from_slice(&mapped[start..end]);
            }
            bgra
        };
        readback_buffer.unmap();

        feed_encoder(mp4_encoder::Frame::Vide(&bgra));

        if let (Some(audio), true) = (decoded_audio.as_ref(), frame_size > 0) {
            while audio_cursor + frame_size <= audio.left.len() {
                feed_encoder(mp4_encoder::Frame::Audio(
                    &audio.left[audio_cursor..audio_cursor + frame_size],
                    &audio.right[audio_cursor..audio_cursor + frame_size],
                ));
                audio_cursor += frame_size;
            }
        }

        let _ = progress.send(Progress::Frame {
            index: frame_index + 1,
            total: total_frames,
        });
    }

    if let Some(audio) = decoded_audio.as_ref() {
        if audio_cursor < audio.left.len() {
            feed_encoder(mp4_encoder::Frame::Audio(
                &audio.left[audio_cursor..],
                &audio.right[audio_cursor..],
            ));
        }
    }
    feed_encoder(mp4_encoder::Frame::Terminator);

    let _ = progress.send(Progress::Done);
    Ok(())
}
