mod gpu;
mod ui;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Instant;

use export::Progress as ExportProgress;
use gpu::Gpu;
use project::{KeyboardCalibration, Project};
use render::Compositor;
use ui::UiState;
use video_pipeline::VideoPipeline;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

/// Format of the offscreen preview texture the compositor renders into. Fixed at
/// `Rgba8Unorm` (not the swapchain's own sRGB format) because `egui_wgpu::Renderer::
/// register_native_texture` requires exactly that format for a texture displayed via
/// `egui::Image` — see `AppState::set_canvas_size`.
const PREVIEW_TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Preview canvas size before any video is loaded, so the offscreen texture and its egui
/// registration exist from the very first redraw.
const DEFAULT_CANVAS_SIZE: (u32, u32) = (1280, 720);

/// Bundles the raw handles `render::Compositor` needs from our interactive-window `Gpu`.
///
/// `texture_format` is always `PREVIEW_TEXTURE_FORMAT`, not the swapchain's own format — the
/// compositor now always renders into the offscreen preview texture (see `AppState::redraw`),
/// never directly onto the swapchain, so it needs to be built against the offscreen texture's
/// format regardless of what the window surface happens to use.
fn gpu_handles(gpu: &Gpu) -> render::GpuHandles<'_> {
    render::GpuHandles {
        instance: &gpu.instance,
        adapter: &gpu.adapter,
        device: &gpu.device,
        queue: &gpu.queue,
        texture_format: PREVIEW_TEXTURE_FORMAT,
    }
}

/// Rounds down to an even number (yuv420p/some encoders need it; harmless for the interactive
/// preview too, and keeps this consistent with `crates/export`'s own `even` helper).
fn even(value: u32) -> u32 {
    (value & !1).max(2)
}

fn create_preview_texture(
    device: &wgpu::Device,
    size: (u32, u32),
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("preview_offscreen_texture"),
        size: wgpu::Extent3d {
            width: size.0.max(1),
            height: size.1.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: PREVIEW_TEXTURE_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

struct AppState {
    window: Arc<Window>,
    gpu: Gpu,
    compositor: Compositor,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
    /// Pixel size the compositor renders into — decoupled from the window/surface size (see
    /// CLAUDE.md's milestone 6c notes). Starts at `DEFAULT_CANVAS_SIZE`, then tracks the loaded
    /// video's own decoded resolution (rounded even) once one loads.
    canvas_size: (u32, u32),
    preview_texture: wgpu::Texture,
    preview_view: wgpu::TextureView,
    preview_texture_id: egui::TextureId,
    pipeline: Option<VideoPipeline>,
    video_path: Option<PathBuf>,
    midi_path: Option<PathBuf>,
    /// Calibration last used to build the waterfall layout; compared against `ui_state.calibration`
    /// each redraw so a drag only triggers the (fairly heavy, full-rebuild) `compositor.resize`
    /// when it actually changed, not every frame.
    applied_calibration: KeyboardCalibration,
    ui_state: UiState,
    last_instant: Instant,
    /// Transport position `pipeline.seek_and_decode` was last called with, so a redraw that
    /// fires for an unrelated reason while paused (cursor blink in a text field, hover
    /// animations, mouse movement) doesn't re-invoke video decode at all — previously it called
    /// `seek_and_decode` unconditionally every redraw, which is usually a cheap no-op but isn't
    /// always: if the frozen paused position sits even slightly ahead of the last decoded
    /// frame's own timestamp (routine — the displayed frame's pts is always <= the transport
    /// position it was shown for), `seek_and_decode`'s "not yet caught up" branch decodes one
    /// more frame forward and keeps doing so each time it's re-entered, which is exactly what a
    /// repaint storm while "paused" turns into: visible frame-by-frame stutter with the video
    /// never actually settling. `None` until the first frame is decoded.
    last_decoded_position: Option<f64>,
    /// `Some` while a background export thread is running; polled each redraw and cleared once
    /// it reports `Done`/`Cancelled`/`Error`.
    export_run: Option<ExportRun>,
}

struct ExportRun {
    rx: mpsc::Receiver<ExportProgress>,
    cancel: Arc<AtomicBool>,
}

impl AppState {
    fn new(
        event_loop: &ActiveEventLoop,
        video_path: Option<&Path>,
        midi_path: Option<&Path>,
    ) -> Self {
        let window_attributes = Window::default_attributes()
            .with_title("freemusic")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));
        let window = Arc::new(
            event_loop
                .create_window(window_attributes)
                .expect("failed to create window"),
        );

        let gpu = Gpu::new(window.clone());
        let canvas_size = DEFAULT_CANVAS_SIZE;
        let compositor = Compositor::new(
            &gpu_handles(&gpu),
            (canvas_size.0 as f32, canvas_size.1 as f32),
            &KeyboardCalibration::default(),
        );

        let egui_ctx = egui::Context::default();
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        let mut egui_renderer = egui_wgpu::Renderer::new(
            &gpu.device,
            gpu.config.format,
            egui_wgpu::RendererOptions::default(),
        );

        let (preview_texture, preview_view) = create_preview_texture(&gpu.device, canvas_size);
        let preview_texture_id = egui_renderer.register_native_texture(
            &gpu.device,
            &preview_view,
            wgpu::FilterMode::Linear,
        );

        let mut state = Self {
            window,
            gpu,
            compositor,
            egui_ctx,
            egui_state,
            egui_renderer,
            canvas_size,
            preview_texture,
            preview_view,
            preview_texture_id,
            pipeline: None,
            video_path: None,
            midi_path: None,
            applied_calibration: KeyboardCalibration::default(),
            ui_state: UiState {
                playing: false,
                position_seconds: 0.0,
                duration_seconds: 0.0,
                seek_request: None,
                dropping: false,
                midi_name: None,
                midi_note_times: Vec::new(),
                active_tab: ui::Tab::default(),
                preview_texture_id,
                canvas_size,
                sync_offset_seconds: 0.0,
                calibration: KeyboardCalibration::default(),
                transform: project::VideoTransform::default(),
                project_path_text: String::new(),
                save_requested: false,
                load_requested: false,
                open_video_requested: false,
                open_midi_requested: false,
                status_message: None,
                export_path_text: String::new(),
                export_fps: 30,
                export_requested: false,
                export_cancel_requested: false,
                export_progress: None,
                export_message: None,
            },
            last_instant: Instant::now(),
            last_decoded_position: None,
            export_run: None,
        };

        if let Some(path) = video_path {
            state.load_video(path);
        }
        if let Some(path) = midi_path {
            state.load_midi(path);
        }

        state
    }

    /// Switches the preview canvas to a new pixel size, recreating the offscreen texture and
    /// its egui registration (a bind group tied to the old texture's view can't just be resized
    /// in place) and rebuilding the waterfall layout for it. No-ops if `size` is unchanged — the
    /// common case, since this is called on every video load even when the resolution matches
    /// the previous one.
    fn set_canvas_size(&mut self, size: (u32, u32)) {
        if size == self.canvas_size {
            return;
        }
        let (texture, view) = create_preview_texture(&self.gpu.device, size);
        self.egui_renderer.free_texture(&self.preview_texture_id);
        self.preview_texture_id = self.egui_renderer.register_native_texture(
            &self.gpu.device,
            &view,
            wgpu::FilterMode::Linear,
        );
        self.preview_texture = texture;
        self.preview_view = view;
        self.canvas_size = size;
        self.ui_state.canvas_size = size;
        self.ui_state.preview_texture_id = self.preview_texture_id;
        self.compositor.resize(
            &gpu_handles(&self.gpu),
            (size.0 as f32, size.1 as f32),
            &self.ui_state.calibration,
        );
        self.applied_calibration = self.ui_state.calibration;
    }

    /// Opens `path` as the active video, replacing whatever was loaded before (CLI arg at
    /// startup, or a previous drag-drop). Decodes and uploads the first frame immediately so
    /// the window isn't blank before the next redraw. Resizes the preview canvas to the video's
    /// own decoded resolution (rounded even) — see CLAUDE.md's milestone 6c notes on why the
    /// canvas tracks the video rather than the window.
    fn load_video(&mut self, path: &Path) {
        let mut pipeline = match VideoPipeline::open(path) {
            Ok(pipeline) => pipeline,
            Err(err) => {
                eprintln!("failed to open video file {path:?}: {err}");
                return;
            }
        };

        self.ui_state.duration_seconds = pipeline.duration_seconds();
        self.ui_state.position_seconds = 0.0;
        self.ui_state.playing = false;

        if let Ok(frame) = pipeline.seek_and_decode(0.0, false) {
            self.set_canvas_size((even(frame.width), even(frame.height)));
            self.compositor.upload_frame(
                &self.gpu.device,
                &self.gpu.queue,
                frame.width,
                frame.height,
                &frame.bgra,
            );
            self.compositor.update_viewport(
                &self.gpu.queue,
                self.canvas_size,
                &self.ui_state.transform,
            );
            self.last_decoded_position = Some(0.0);
        }

        self.pipeline = Some(pipeline);
        self.video_path = Some(path.to_path_buf());
        if self.ui_state.project_path_text.is_empty() {
            self.ui_state.project_path_text = default_project_path(path).display().to_string();
        }
        if self.ui_state.export_path_text.is_empty() {
            self.ui_state.export_path_text = default_export_path(path).display().to_string();
        }
    }

    /// Parses `path` as a MIDI file and (re)builds the waterfall overlay for it.
    fn load_midi(&mut self, path: &Path) {
        let size = self.canvas_size;
        match self.compositor.load_midi(
            &gpu_handles(&self.gpu),
            (size.0 as f32, size.1 as f32),
            &self.ui_state.calibration,
            path,
        ) {
            Ok(()) => {
                self.midi_path = Some(path.to_path_buf());
                self.ui_state.midi_name = self.compositor.loaded_midi_name().map(str::to_owned);
                self.ui_state.midi_note_times = self.compositor.note_start_times().to_vec();
            }
            Err(err) => eprintln!("failed to open midi file {path:?}: {err}"),
        }
    }

    /// Serializes the current video/MIDI paths, sync offset, and calibration to the path in
    /// the project text field.
    /// Bundles the current video/MIDI paths, sync offset, calibration, and transform into a
    /// `Project` snapshot — used by both `save_project` and `start_export` (export's background
    /// thread runs entirely off one of these, since `project::Project` is already UI-agnostic).
    fn snapshot_project(&self) -> Project {
        Project {
            video_path: self.video_path.clone(),
            midi_path: self.midi_path.clone(),
            sync_offset_seconds: self.ui_state.sync_offset_seconds,
            calibration: self.ui_state.calibration,
            transform: self.ui_state.transform,
        }
    }

    fn save_project(&mut self) {
        let project = self.snapshot_project();
        let path = PathBuf::from(&self.ui_state.project_path_text);
        self.ui_state.status_message = Some(match project.save(&path) {
            Ok(()) => format!("Saved to {}", path.display()),
            Err(err) => err,
        });
    }

    /// Spawns a background thread running the whole MP4 export; no-ops if one is already
    /// running. Progress/completion is polled off `ExportRun::rx` each redraw (see `redraw`).
    fn start_export(&mut self) {
        if self.export_run.is_some() {
            return;
        }
        if self.video_path.is_none() {
            self.ui_state.export_message = Some("Load a video before exporting".to_string());
            return;
        }

        let project = self.snapshot_project();
        let settings = export::ExportSettings {
            output_path: PathBuf::from(&self.ui_state.export_path_text),
            fps: self.ui_state.export_fps,
        };
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let thread_cancel = cancel.clone();
        std::thread::spawn(move || export::run(project, settings, tx, thread_cancel));

        self.ui_state.export_progress = Some((0, 1));
        self.ui_state.export_message = None;
        self.export_run = Some(ExportRun { rx, cancel });
    }

    /// Loads a project from the path in the project text field, replacing the current video,
    /// MIDI, sync offset, and calibration with whatever it contains.
    fn load_project(&mut self) {
        let path = PathBuf::from(&self.ui_state.project_path_text);
        match Project::load(&path) {
            Ok(project) => {
                self.ui_state.sync_offset_seconds = project.sync_offset_seconds;
                self.ui_state.calibration = project.calibration;
                self.ui_state.transform = project.transform;
                if let Some(video_path) = project.video_path.clone() {
                    self.load_video(&video_path);
                }
                if let Some(midi_path) = project.midi_path.clone() {
                    self.load_midi(&midi_path);
                }
                self.ui_state.project_path_text = path.display().to_string();
                self.ui_state.status_message = Some(format!("Loaded {}", path.display()));
            }
            Err(err) => self.ui_state.status_message = Some(err),
        }
    }

    fn redraw(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_instant).as_secs_f64();
        self.last_instant = now;

        // Whether this redraw's position change (if any) came from an explicit scrub (the
        // timeline being dragged/clicked, or in future a keyboard seek) rather than ordinary
        // playback advancing by `dt`. Passed through to `seek_and_decode` as `!is_scrub` below —
        // see that function's doc comment for why the two cases need different `exact` behavior
        // after a reseek: conflating them was a real bug (video snapping back to a stale frame
        // and stuttering in place whenever a redraw-cadence stall — not an actual scrub —
        // tripped the same big-forward-jump reseek path a real scrub does).
        let is_scrub = self.ui_state.seek_request.is_some();
        if let Some(target) = self.ui_state.seek_request.take() {
            self.ui_state.position_seconds = target.clamp(0.0, self.ui_state.duration_seconds);
        } else if self.ui_state.playing {
            self.ui_state.position_seconds =
                (self.ui_state.position_seconds + dt).min(self.ui_state.duration_seconds);
            if self.ui_state.position_seconds >= self.ui_state.duration_seconds {
                self.ui_state.playing = false;
            }
        }

        // Skip decode entirely if the transport position hasn't actually moved since the last
        // decoded frame — see `last_decoded_position`'s doc comment for why this guard matters
        // beyond just avoiding wasted work: without it, a redraw firing for an unrelated reason
        // while paused (cursor blink, hover, mouse movement) can re-enter `seek_and_decode`'s
        // "not caught up yet" branch and decode one more frame forward each time, which looks
        // like the paused video stuttering/looping instead of holding still.
        if self.last_decoded_position != Some(self.ui_state.position_seconds) {
            if let Some(pipeline) = self.pipeline.as_mut() {
                if let Ok(frame) =
                    pipeline.seek_and_decode(self.ui_state.position_seconds, !is_scrub)
                {
                    self.compositor.upload_frame(
                        &self.gpu.device,
                        &self.gpu.queue,
                        frame.width,
                        frame.height,
                        &frame.bgra,
                    );
                    self.last_decoded_position = Some(self.ui_state.position_seconds);
                }
            }
        }
        let midi_time = self.ui_state.position_seconds - self.ui_state.sync_offset_seconds;
        self.compositor.update_midi(midi_time as f32);

        let raw_input = self.egui_state.take_egui_input(&self.window);
        let full_output = {
            let ui_state = &mut self.ui_state;
            self.egui_ctx.run_ui(raw_input, |ui| {
                ui::draw(ui, ui_state);
            })
        };

        if self.ui_state.calibration != self.applied_calibration {
            self.compositor.resize(
                &gpu_handles(&self.gpu),
                (self.canvas_size.0 as f32, self.canvas_size.1 as f32),
                &self.ui_state.calibration,
            );
            self.applied_calibration = self.ui_state.calibration;
        }

        // Cheap (one small uniform write), so applied unconditionally every redraw rather than
        // dirty-checked — unlike `compositor.resize`'s full note-instance rebuild above, this
        // needs no guard, and running it after the egui pass means a slider drag this frame is
        // reflected in this same frame's render rather than lagging by one redraw.
        self.compositor.update_viewport(
            &self.gpu.queue,
            self.canvas_size,
            &self.ui_state.transform,
        );

        if self.ui_state.save_requested {
            self.ui_state.save_requested = false;
            self.save_project();
        }
        if self.ui_state.load_requested {
            self.ui_state.load_requested = false;
            self.load_project();
        }
        if self.ui_state.open_video_requested {
            self.ui_state.open_video_requested = false;
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Video", &["mp4", "mov", "mkv", "avi", "webm"])
                .pick_file()
            {
                self.load_video(&path);
            }
        }
        if self.ui_state.open_midi_requested {
            self.ui_state.open_midi_requested = false;
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("MIDI", &["mid", "midi"])
                .pick_file()
            {
                self.load_midi(&path);
            }
        }
        if self.ui_state.export_requested {
            self.ui_state.export_requested = false;
            self.start_export();
        }
        if self.ui_state.export_cancel_requested {
            self.ui_state.export_cancel_requested = false;
            if let Some(run) = self.export_run.as_ref() {
                run.cancel.store(true, Ordering::Relaxed);
            }
        }
        if let Some(run) = self.export_run.as_ref() {
            let mut finished = false;
            while let Ok(progress) = run.rx.try_recv() {
                match progress {
                    ExportProgress::Frame { index, total } => {
                        self.ui_state.export_progress = Some((index, total));
                    }
                    ExportProgress::Done => {
                        self.ui_state.export_message = Some("Export finished".to_string());
                        finished = true;
                    }
                    ExportProgress::Cancelled => {
                        self.ui_state.export_message = Some("Export cancelled".to_string());
                        finished = true;
                    }
                    ExportProgress::Error(err) => {
                        self.ui_state.export_message = Some(format!("Export failed: {err}"));
                        finished = true;
                    }
                }
            }
            if finished {
                self.ui_state.export_progress = None;
                self.export_run = None;
            }
        }

        let paint_jobs = self
            .egui_ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);
        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.gpu.config.width, self.gpu.config.height],
            pixels_per_point: full_output.pixels_per_point,
        };

        for (id, delta) in &full_output.textures_delta.set {
            self.egui_renderer
                .update_texture(&self.gpu.device, &self.gpu.queue, *id, delta);
        }

        let surface_texture = match self.gpu.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => texture,
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
            _ => {
                self.window.request_redraw();
                return;
            }
        };
        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame_encoder"),
            });

        let egui_cmd_buffers = self.egui_renderer.update_buffers(
            &self.gpu.device,
            &self.gpu.queue,
            &mut encoder,
            &paint_jobs,
            &screen_descriptor,
        );

        // The compositor now renders into the offscreen preview texture, not the swapchain
        // directly — the egui pass below displays it via `egui::Image` in the central panel
        // (see CLAUDE.md's milestone 6c notes). This also sidesteps the old two-pass split that
        // was needed only because `WaterfallRenderer::render` ties its `&mut self` borrow to the
        // render pass's invariant lifetime parameter: this pass has a real (non-`'static`)
        // lifetime, same as the old "scene_pass" did, so `WaterfallRenderer` is still fine with
        // it even though it's a different render target now.
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("preview_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.preview_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            self.compositor.render(&mut render_pass);
        }

        {
            // `Clear`, not `Load` — the swapchain no longer has a prior pass drawn onto it (the
            // compositor now renders into the offscreen preview texture above instead), so this
            // is the only pass touching it this frame.
            let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            let mut render_pass = render_pass.forget_lifetime();
            self.egui_renderer
                .render(&mut render_pass, &paint_jobs, &screen_descriptor);
        }

        for id in &full_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }

        self.gpu.queue.submit(
            egui_cmd_buffers
                .into_iter()
                .chain(std::iter::once(encoder.finish())),
        );
        surface_texture.present();

        self.egui_state
            .handle_platform_output(&self.window, full_output.platform_output);

        // Also keep redrawing while an export is running (even if playback is paused) so its
        // progress bar advances instead of sitting frozen until some other input wakes the loop.
        if self.ui_state.playing || self.export_run.is_some() {
            self.window.request_redraw();
        }
    }
}

/// Default project save path for a freshly loaded video: alongside it, same stem, `.fmproj.ron`
/// extension. Only used to prefill the project text field, never overwrites a path the user
/// already typed in.
fn default_project_path(video_path: &Path) -> PathBuf {
    let mut path = video_path.to_path_buf();
    path.set_extension("fmproj.ron");
    path
}

/// Default export path for a freshly loaded video: alongside it, `_export.mp4` suffix on the
/// stem (never the source path itself — exporting would otherwise silently overwrite it). Only
/// used to prefill the export text field, never overwrites a path the user already typed in.
fn default_export_path(video_path: &Path) -> PathBuf {
    let stem = video_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    video_path.with_file_name(format!("{stem}_export.mp4"))
}

struct App {
    video_path: Option<PathBuf>,
    midi_path: Option<PathBuf>,
    state: Option<AppState>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_none() {
            self.state = Some(AppState::new(
                event_loop,
                self.video_path.as_deref(),
                self.midi_path.as_deref(),
            ));
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(state) = self.state.as_mut() else {
            return;
        };

        let response = state.egui_state.on_window_event(&state.window, &event);
        if response.repaint {
            state.window.request_redraw();
        }

        match &event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
                return;
            }
            WindowEvent::Resized(size) => {
                // Only the swapchain follows the window now — the compositor's canvas is
                // decoupled from it (see `AppState::set_canvas_size`), so a window resize no
                // longer touches the compositor at all.
                state.gpu.resize(size.width, size.height);
                return;
            }
            WindowEvent::HoveredFile(_) => {
                state.ui_state.dropping = true;
                state.window.request_redraw();
                return;
            }
            WindowEvent::HoveredFileCancelled => {
                state.ui_state.dropping = false;
                state.window.request_redraw();
                return;
            }
            WindowEvent::DroppedFile(path) => {
                state.ui_state.dropping = false;
                match path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(str::to_ascii_lowercase)
                    .as_deref()
                {
                    Some("mid") | Some("midi") => state.load_midi(path),
                    _ => state.load_video(path),
                }
                state.window.request_redraw();
                return;
            }
            WindowEvent::RedrawRequested => {
                state.redraw();
                return;
            }
            _ => {}
        }

        if response.consumed {
            return;
        }

        if let WindowEvent::KeyboardInput {
            event: key_event, ..
        } = &event
        {
            if key_event.state == ElementState::Pressed
                && key_event.physical_key == PhysicalKey::Code(KeyCode::Space)
            {
                state.ui_state.playing = !state.ui_state.playing;
                state.last_instant = Instant::now();
                state.window.request_redraw();
            }
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1).map(PathBuf::from);
    let video_path = args.next();
    let midi_path = args.next();

    let event_loop = EventLoop::new().expect("failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = App {
        video_path,
        midi_path,
        state: None,
    };
    event_loop.run_app(&mut app).expect("event loop error");
}
