mod gpu;
mod ui;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use audio_playback::AudioPlayback;
use export::Progress as ExportProgress;
use gpu::Gpu;
use project::{KeyboardCalibration, NoteLayer, Project, Style};
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

/// Caps how far `position_seconds` can advance in a single redraw during ordinary playback before
/// audio has been started for that playback run.
/// `dt` is real wall-clock time since the previous redraw. Before audio starts, cap it to one
/// source-video frame so a stale first redraw cannot ask exact decode to close a large gap. Once
/// audio is running, let the transport follow elapsed wall time so a later render stall does not
/// make us repeatedly anchor the audio callback backward to an old video position.
/// The real cap applied in `redraw` is one source-video frame's own duration (tighter than any
/// fixed constant could be for every possible frame rate); this constant is only the fallback
/// used before a video is loaded and `frame_duration_seconds()` isn't known yet.
const MAX_PLAYBACK_DT_SECONDS: f64 = 0.1;
const MAX_AUDIO_SYNC_PLAYBACK_DT_SECONDS: f64 = 1.0;

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

/// The `NoteLayer` the compositor should actually draw: an imported style's notes layer, or one
/// synthesized from the legacy `note_style`/`barrier_style` sliders if none has been imported —
/// mirrors `project::Project::effective_note_layer`, which can't be used directly here since
/// `UiState` isn't a `Project` (no video/MIDI paths to snapshot just to resolve this).
fn effective_note_layer(ui_state: &UiState) -> NoteLayer {
    ui_state
        .style
        .clone()
        .unwrap_or_else(|| {
            Style::from_legacy(
                &ui_state.note_style,
                &ui_state.barrier_style,
                ui_state.background_color,
            )
        })
        .notes
        .resolve(0.0)
        .clone()
}

/// Same idea as `effective_note_layer`, for the barrier axis — mirrors
/// `project::Project::effective_barrier_layer`, which can't be used directly here for the same
/// reason `effective_note_layer` can't (`UiState` isn't a `Project`).
fn effective_barrier_layer(ui_state: &UiState) -> project::BarrierLayer {
    ui_state
        .style
        .clone()
        .unwrap_or_else(|| {
            Style::from_legacy(
                &ui_state.note_style,
                &ui_state.barrier_style,
                ui_state.background_color,
            )
        })
        .barrier
        .resolve(0.0)
        .clone()
}

/// Same idea as `effective_note_layer`/`effective_barrier_layer`, for the barrier-hit transition
/// axis — mirrors `project::Project::effective_transition_layer`.
fn effective_transition_layer(ui_state: &UiState) -> project::TransitionLayer {
    ui_state
        .style
        .clone()
        .unwrap_or_else(|| {
            Style::from_legacy(
                &ui_state.note_style,
                &ui_state.barrier_style,
                ui_state.background_color,
            )
        })
        .transition
        .resolve(0.0)
        .clone()
}

/// Same idea as `effective_note_layer`/`effective_barrier_layer`/`effective_transition_layer`,
/// for the canvas background — mirrors `project::Project::effective_background_color`.
fn effective_background_color(ui_state: &UiState) -> [u8; 3] {
    ui_state
        .style
        .clone()
        .unwrap_or_else(|| {
            Style::from_legacy(
                &ui_state.note_style,
                &ui_state.barrier_style,
                ui_state.background_color,
            )
        })
        .background
        .resolve_constant()
}

/// sRGB u8 -> linear f32, matching `render::barrier::srgb_to_linear`/`render::effects::srgb_to_linear`
/// — kept as its own small copy rather than shared (both of those are private to `render`), same
/// call this codebase already makes twice for the identical conversion. Used for the preview
/// pass's clear color so the canvas background composites correctly against the compositor's
/// linear-space blending (glow, additive particles) instead of clearing to a raw sRGB byte value.
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
    /// Same idea as `applied_calibration`, for the effective `NoteLayer` (see
    /// `effective_note_layer`) — fill/sheen/glow/roundedness/fall_speed are all baked into each
    /// `NoteInstance`/style uniform at build time (see
    /// `render::notes::NotesRenderer::rebuild_instances`), so a change (whether from the
    /// legacy sliders or importing a `.fmstyle.ron`) needs a full `compositor.resize`, not just a
    /// per-frame uniform write.
    applied_note_layer: NoteLayer,
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
    /// Live modifier-key state, tracked via `WindowEvent::ModifiersChanged` since
    /// `WindowEvent::KeyboardInput` itself carries no modifier info — used to distinguish e.g.
    /// Ctrl+S (save) from a bare S, and Left/Right (1-frame seek) from Shift+Left/Right
    /// (1-second seek).
    modifiers: winit::keyboard::ModifiersState,
    /// The loaded video's own audio track (if any), played back in sync with the transport —
    /// see `crates/audio-playback` for why this drives from position rather than its own clock.
    audio: AudioPlayback,
    /// `set_playing` last called with; compared each redraw so play/pause is only pushed to the
    /// `cpal` stream when it actually changes, not every frame (same dirty-check idea as
    /// `applied_calibration`).
    audio_playing: bool,
    /// Set when playback is resumed from the UI. Audio stays paused until the first playback
    /// tick has advanced/decode-synced video, so a slow first decode cannot make audio run ahead
    /// and then snap backward.
    audio_resume_pending: bool,
    next_playback_redraw_at: Option<Instant>,
    /// Deadline for egui's own animations (side-panel collapse/expand slide, menu open/hover,
    /// button flash, etc.) — set from `full_output`'s per-viewport `repaint_delay` each redraw.
    /// Without this, an in-progress animation only advances when some unrelated event (mouse
    /// movement) happens to trigger the next repaint, since this app's redraw loop otherwise
    /// only self-schedules for playback/export — see `about_to_wait`.
    next_ui_redraw_at: Option<Instant>,
    pointer_buttons_down: u8,
    perf: Option<PerfStats>,
    interaction_log_enabled: bool,
    interaction_log_until: Option<Instant>,
}

struct ExportRun {
    rx: mpsc::Receiver<ExportProgress>,
    cancel: Arc<AtomicBool>,
}

struct PerfStats {
    started: Instant,
    frames: u32,
    uploads: u32,
    cache_hits: u32,
    decode: Duration,
    // Purely-CPU sub-stages of `decode` (from `VideoPipeline::last_timings`), broken out so the
    // log can show *which* CPU stage balloons under mouse-move load rather than one lumped number.
    // None of these issue a GPU call, so `decode` (and thus these) ballooning while `acquire`/
    // `render_submit` stay low points at CPU/IO contention, not GPU/compositor contention. An
    // earlier round already showed the old `h264` bucket (= demux+send+receive) balloons ~100×
    // while scale/copy stay flat; these split that bucket to pinpoint I/O vs the decode workers.
    demux: Duration,
    send: Duration,
    receive: Duration,
    scale: Duration,
    copy: Duration,
    upload: Duration,
    midi: Duration,
    egui: Duration,
    acquire: Duration,
    render_submit: Duration,
    total: Duration,
    max_total: Duration,
    max_dt: Duration,
}

impl PerfStats {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            frames: 0,
            uploads: 0,
            cache_hits: 0,
            decode: Duration::ZERO,
            demux: Duration::ZERO,
            send: Duration::ZERO,
            receive: Duration::ZERO,
            scale: Duration::ZERO,
            copy: Duration::ZERO,
            upload: Duration::ZERO,
            midi: Duration::ZERO,
            egui: Duration::ZERO,
            acquire: Duration::ZERO,
            render_submit: Duration::ZERO,
            total: Duration::ZERO,
            max_total: Duration::ZERO,
            max_dt: Duration::ZERO,
        }
    }

    fn record_frame(&mut self, total: Duration, dt: Duration) {
        self.frames += 1;
        self.total += total;
        self.max_total = self.max_total.max(total);
        self.max_dt = self.max_dt.max(dt);
    }

    fn maybe_print(&mut self) {
        let elapsed = self.started.elapsed();
        if elapsed < Duration::from_secs(1) || self.frames == 0 {
            return;
        }

        let frames = self.frames as f64;
        eprintln!(
            "[perf] fps={:.1} uploads/s={} cache_hits/s={} avg_us decode={:.0} (demux={:.0} send={:.0} receive={:.0} scale={:.0} copy={:.0}) upload={:.0} midi={:.0} egui={:.0} acquire={:.0} render_submit={:.0} total={:.0} max_total={:.0} max_dt_ms={:.1}",
            frames / elapsed.as_secs_f64(),
            self.uploads,
            self.cache_hits,
            self.decode.as_secs_f64() * 1_000_000.0 / frames,
            self.demux.as_secs_f64() * 1_000_000.0 / frames,
            self.send.as_secs_f64() * 1_000_000.0 / frames,
            self.receive.as_secs_f64() * 1_000_000.0 / frames,
            self.scale.as_secs_f64() * 1_000_000.0 / frames,
            self.copy.as_secs_f64() * 1_000_000.0 / frames,
            self.upload.as_secs_f64() * 1_000_000.0 / frames,
            self.midi.as_secs_f64() * 1_000_000.0 / frames,
            self.egui.as_secs_f64() * 1_000_000.0 / frames,
            self.acquire.as_secs_f64() * 1_000_000.0 / frames,
            self.render_submit.as_secs_f64() * 1_000_000.0 / frames,
            self.total.as_secs_f64() * 1_000_000.0 / frames,
            self.max_total.as_secs_f64() * 1_000_000.0,
            self.max_dt.as_secs_f64() * 1_000.0,
        );
        *self = Self::new();
    }
}

impl AppState {
    fn new(
        event_loop: &ActiveEventLoop,
        video_path: Option<&Path>,
        midi_path: Option<&Path>,
        project_path: Option<&Path>,
        style_path: Option<&Path>,
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
            &NoteLayer::default(),
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
            applied_note_layer: NoteLayer::default(),
            ui_state: UiState {
                playing: false,
                position_seconds: 0.0,
                duration_seconds: 0.0,
                seek_request: None,
                seek_request_exact: false,
                dropping: false,
                midi_name: None,
                midi_note_times: Vec::new(),
                waveform_peaks: Vec::new(),
                waveform_bucket_seconds: 0.0,
                active_tab: ui::Tab::default(),
                preview_texture_id,
                canvas_size,
                timeline_height: ui::DEFAULT_TIMELINE_HEIGHT,
                timeline_zoom: ui::DEFAULT_TIMELINE_ZOOM,
                timeline_view_start_seconds: 0.0,
                side_panel_expanded: true,
                side_panel_toggle_requested: false,
                sync_offset_seconds: 0.0,
                calibration: KeyboardCalibration::default(),
                transform: project::VideoTransform::default(),
                barrier_style: project::BarrierStyle::default(),
                note_style: project::NoteStyle::default(),
                background_color: [0, 0, 0],
                project_path_text: String::new(),
                save_requested: false,
                load_requested: false,
                open_video_requested: false,
                open_midi_requested: false,
                new_project_requested: false,
                open_project_requested: false,
                save_project_as_requested: false,
                exit_requested: false,
                style: None,
                import_style_requested: false,
                style_path: None,
                reload_style_requested: false,
                status_message: None,
                export_path_text: String::new(),
                export_fps: 30,
                export_requested: false,
                export_cancel_requested: false,
                export_progress: None,
                export_message: None,
                camera_stretch_capture: None,
            },
            last_instant: Instant::now(),
            last_decoded_position: None,
            export_run: None,
            modifiers: winit::keyboard::ModifiersState::empty(),
            audio: AudioPlayback::new(),
            audio_playing: false,
            audio_resume_pending: false,
            next_playback_redraw_at: None,
            next_ui_redraw_at: None,
            pointer_buttons_down: 0,
            perf: std::env::var_os("FREEMUSIC_PROFILE").map(|_| PerfStats::new()),
            interaction_log_enabled: std::env::var_os("FREEMUSIC_INTERACTION_LOG").is_some(),
            interaction_log_until: None,
        };

        if let Some(path) = project_path {
            state.load_project_from_path(path);
        } else {
            if let Some(path) = video_path {
                state.load_video(path);
            }
            if let Some(path) = midi_path {
                state.load_midi(path);
            }
        }
        // Applied after the project branch above (not folded into it) so a CLI-passed style
        // always wins over whatever `style` field a loaded project itself carries — same
        // "more specific/later wins" precedent as passing a project path alongside a separate
        // video/midi path (see `main`'s doc comment).
        if let Some(path) = style_path {
            state.load_style(path);
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
        let note_layer = effective_note_layer(&self.ui_state);
        self.compositor.resize(
            &gpu_handles(&self.gpu),
            (size.0 as f32, size.1 as f32),
            &self.ui_state.calibration,
            &note_layer,
        );
        self.applied_calibration = self.ui_state.calibration;
        self.applied_note_layer = note_layer;
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
        self.ui_state.seek_request = None;
        self.ui_state.seek_request_exact = false;
        self.next_playback_redraw_at = None;

        match pipeline.seek_and_decode(0.0, false) {
            Err(err) => eprintln!("initial video frame decode failed: {err:?}"),
            Ok(frame) => {
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
        }

        if let Err(err) = self.audio.load(path) {
            eprintln!("failed to load audio track for {path:?}: {err}");
        }
        self.audio_playing = false;
        self.audio_resume_pending = false;
        self.ui_state.waveform_peaks = self.audio.waveform_peaks().to_vec();
        self.ui_state.waveform_bucket_seconds = self.audio.waveform_bucket_seconds();

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
        let note_layer = effective_note_layer(&self.ui_state);
        match self.compositor.load_midi(
            &gpu_handles(&self.gpu),
            (size.0 as f32, size.1 as f32),
            &self.ui_state.calibration,
            &note_layer,
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

    /// Imports a `.fmstyle.ron` file into `ui_state.style`, same effect as the Project tab's
    /// "Import style…" button (`rfd::FileDialog` picker in `apply_post_ui_updates`) — shared so
    /// both the button and a CLI-passed style path go through one code path. Note this only sets
    /// `ui_state.style`; the caller (here, or the button's picker) is responsible for triggering
    /// whatever compositor rebuild picks the new effective note/barrier/transition layers up
    /// (the existing `applied_note_layer`/`applied_calibration` dirty-check in `redraw` already
    /// does this on the next frame, so no extra call is needed from either call site).
    fn load_style(&mut self, path: &Path) {
        match Style::load(path) {
            Ok(style) => {
                self.ui_state.style = Some(style);
                self.ui_state.style_path = Some(path.to_path_buf());
                self.ui_state.status_message =
                    Some(format!("Imported style from {}", path.display()));
            }
            Err(err) => self.ui_state.status_message = Some(err),
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
            barrier_style: self.ui_state.barrier_style,
            note_style: self.ui_state.note_style,
            background_color: self.ui_state.background_color,
            style: self.ui_state.style.clone(),
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

    /// Clears the loaded video/MIDI and resets sync/calibration/transform to defaults — recreates
    /// the compositor from scratch (same construction `AppState::new` uses) rather than trying to
    /// incrementally clear `video_quad`'s uploaded texture and `midi_overlay`'s loaded track,
    /// since neither exposes an "unload" of its own.
    fn new_project(&mut self) {
        self.pipeline = None;
        self.video_path = None;
        self.midi_path = None;

        self.canvas_size = DEFAULT_CANVAS_SIZE;
        let (texture, view) = create_preview_texture(&self.gpu.device, self.canvas_size);
        self.egui_renderer.free_texture(&self.preview_texture_id);
        self.preview_texture_id = self.egui_renderer.register_native_texture(
            &self.gpu.device,
            &view,
            wgpu::FilterMode::Linear,
        );
        self.preview_texture = texture;
        self.preview_view = view;
        self.compositor = Compositor::new(
            &gpu_handles(&self.gpu),
            (self.canvas_size.0 as f32, self.canvas_size.1 as f32),
            &KeyboardCalibration::default(),
            &NoteLayer::default(),
        );
        self.applied_calibration = KeyboardCalibration::default();
        self.applied_note_layer = NoteLayer::default();
        self.last_decoded_position = None;
        self.audio = AudioPlayback::new();
        self.audio_playing = false;
        self.audio_resume_pending = false;
        self.next_playback_redraw_at = None;

        self.ui_state.playing = false;
        self.ui_state.position_seconds = 0.0;
        self.ui_state.duration_seconds = 0.0;
        self.ui_state.seek_request = None;
        self.ui_state.seek_request_exact = false;
        self.ui_state.midi_name = None;
        self.ui_state.midi_note_times = Vec::new();
        self.ui_state.waveform_peaks = Vec::new();
        self.ui_state.waveform_bucket_seconds = 0.0;
        self.ui_state.sync_offset_seconds = 0.0;
        self.ui_state.calibration = KeyboardCalibration::default();
        self.ui_state.transform = project::VideoTransform::default();
        self.ui_state.barrier_style = project::BarrierStyle::default();
        self.ui_state.note_style = project::NoteStyle::default();
        self.ui_state.background_color = [0, 0, 0];
        self.ui_state.style = None;
        self.ui_state.style_path = None;
        self.ui_state.project_path_text = String::new();
        self.ui_state.export_path_text = String::new();
        self.ui_state.canvas_size = self.canvas_size;
        self.ui_state.preview_texture_id = self.preview_texture_id;
        self.ui_state.status_message = Some("New project".to_string());
    }

    /// Loads a project from the path in the project text field, replacing the current video,
    /// MIDI, sync offset, and calibration with whatever it contains.
    fn load_project(&mut self) {
        let path = PathBuf::from(&self.ui_state.project_path_text);
        self.load_project_from_path(&path);
    }

    /// Shared by `load_project` (path comes from the UI's text field) and the CLI's `--project`/
    /// bare `.ron` argument (path comes from `std::env::args`) — replaces the current video,
    /// MIDI, sync offset, calibration, transform, barrier, and note style with whatever the
    /// project file contains.
    fn load_project_from_path(&mut self, path: &Path) {
        match Project::load(path) {
            Ok(project) => {
                self.ui_state.sync_offset_seconds = project.sync_offset_seconds;
                self.ui_state.calibration = project.calibration;
                self.ui_state.transform = project.transform;
                self.ui_state.barrier_style = project.barrier_style;
                self.ui_state.note_style = project.note_style;
                self.ui_state.background_color = project.background_color;
                self.ui_state.style = project.style.clone();
                self.ui_state.style_path = None;
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

    fn trace_interaction_for(&mut self, label: &str) {
        if !self.interaction_log_enabled {
            return;
        }
        if label != "play" {
            if label == "pause" {
                self.interaction_log_until = None;
            }
            return;
        }
        let now = Instant::now();
        self.interaction_log_until = Some(now + Duration::from_secs(3));
        eprintln!(
            "[interaction:{label}] start pos={:.3} playing={} audio_playing={} audio_pending={} last_decoded={:?} next_playback_due_ms={:?}",
            self.ui_state.position_seconds,
            self.ui_state.playing,
            self.audio_playing,
            self.audio_resume_pending,
            self.last_decoded_position,
            self.next_playback_redraw_at
                .map(|deadline| deadline.saturating_duration_since(now).as_secs_f64() * 1000.0),
        );
    }

    fn interaction_trace_active(&self) -> bool {
        self.interaction_log_enabled
            && self
                .interaction_log_until
                .is_some_and(|deadline| Instant::now() <= deadline)
    }

    fn redraw(&mut self) {
        let frame_start = Instant::now();
        let now = Instant::now();
        let dt_duration = now.duration_since(self.last_instant);
        let dt = dt_duration.as_secs_f64();
        self.last_instant = now;

        if self.interaction_trace_active() {
            eprintln!(
                "[interaction:frame-start] dt_ms={:.2} pos={:.3} playing={} audio_playing={} audio_pending={} seek_request={:?} seek_exact={} last_decoded={:?}",
                dt_duration.as_secs_f64() * 1000.0,
                self.ui_state.position_seconds,
                self.ui_state.playing,
                self.audio_playing,
                self.audio_resume_pending,
                self.ui_state.seek_request,
                self.ui_state.seek_request_exact,
                self.last_decoded_position,
            );
        }

        let mut explicit_seek = false;
        let advanced_playback = self.advance_transport_and_decode(dt, &mut explicit_seek);
        if advanced_playback && self.audio_resume_pending {
            self.audio_resume_pending = false;
            explicit_seek = true;
        }

        let start = Instant::now();
        self.update_midi_position();
        if let Some(perf) = self.perf.as_mut() {
            perf.midi += start.elapsed();
        }

        let start = Instant::now();
        let raw_input = self.egui_state.take_egui_input(&self.window);
        let playing_before_ui = self.ui_state.playing;
        let full_output = {
            let ui_state = &mut self.ui_state;
            self.egui_ctx.run_ui(raw_input, |ui| {
                ui::draw(ui, ui_state);
            })
        };
        let playing_changed_by_ui = self.ui_state.playing != playing_before_ui;
        if playing_changed_by_ui {
            self.last_instant = Instant::now();
            if self.ui_state.playing {
                self.audio_resume_pending = true;
                if self.audio_playing {
                    self.audio.set_playing(false);
                    self.audio_playing = false;
                }
            } else {
                self.audio_resume_pending = false;
            }
            self.trace_interaction_for(if self.ui_state.playing {
                "play"
            } else {
                "pause"
            });
        }
        if let Some(perf) = self.perf.as_mut() {
            perf.egui += start.elapsed();
        }

        if self.ui_state.seek_request.is_some() {
            if self.ui_state.playing && self.audio_playing {
                self.audio.set_playing(false);
                self.audio_playing = false;
            }
            if let Some(target) = self.ui_state.seek_request {
                self.audio.seek_to_position_seconds(target);
            }

            self.advance_transport_and_decode(0.0, &mut explicit_seek);
            if self.ui_state.playing {
                self.audio_resume_pending = false;
            }
            self.last_instant = Instant::now();

            let start = Instant::now();
            self.update_midi_position();
            if let Some(perf) = self.perf.as_mut() {
                perf.midi += start.elapsed();
            }
        }

        self.apply_post_ui_updates();

        // Audio is driven by (never drives) the transport position — see `audio_playback`'s doc
        // comment. Run this after egui and queued UI actions. Pause applies immediately; resume
        // waits until a playback tick has advanced/decode-synced video so audio cannot run ahead
        // during a slow first decode and then snap backward. Ordinary playback ticks only update
        // the stream's resync anchor, but explicit seeks and play/pause transitions force the
        // callback cursor onto the current transport position even for tiny moves.
        if explicit_seek || playing_changed_by_ui {
            self.audio
                .seek_to_position_seconds(self.ui_state.position_seconds);
        } else {
            self.audio
                .set_position_seconds(self.ui_state.position_seconds);
        }
        let target_audio_playing = self.ui_state.playing && !self.audio_resume_pending;
        if self.interaction_trace_active() {
            eprintln!(
                "[interaction:audio] explicit_seek={} playing_changed={} target_audio_playing={} audio_playing_before={} pos={:.3}",
                explicit_seek,
                playing_changed_by_ui,
                target_audio_playing,
                self.audio_playing,
                self.ui_state.position_seconds,
            );
        }
        if target_audio_playing != self.audio_playing {
            self.audio.set_playing(target_audio_playing);
            self.audio_playing = target_audio_playing;
        }

        self.render_frame(full_output, frame_start, dt_duration, playing_changed_by_ui);
    }

    fn advance_transport_and_decode(&mut self, dt: f64, explicit_seek: &mut bool) -> bool {
        let mut advanced_playback = false;
        let start_position = self.ui_state.position_seconds;
        // Whether this redraw's position change (if any) came from an explicit scrub (the
        // timeline being dragged/clicked, or in future a keyboard seek) rather than ordinary
        // playback advancing by `dt`. Scrubs can seek approximately; playback uses exact decode,
        // but redraws are scheduled at the video frame interval instead of chained immediately.
        let is_seek = self.ui_state.seek_request.is_some();
        let seek_exact = self.ui_state.seek_request_exact;
        if let Some(target) = self.ui_state.seek_request.take() {
            self.ui_state.seek_request_exact = false;
            self.ui_state.position_seconds = target.clamp(0.0, self.ui_state.duration_seconds);
            *explicit_seek = true;
        } else if self.ui_state.playing {
            advanced_playback = true;
            // Before audio is running, cap the first resumed tick to one source-video frame so
            // play-button handling cannot immediately ask the decoder to close a large stale
            // wall-clock gap. Once audio is active, use elapsed wall time so a later render stall
            // does not make us keep anchoring the audio callback back to an old video position.
            let max_dt = if self.audio_playing {
                MAX_AUDIO_SYNC_PLAYBACK_DT_SECONDS
            } else {
                self.pipeline
                    .as_ref()
                    .map(VideoPipeline::frame_duration_seconds)
                    .unwrap_or(MAX_PLAYBACK_DT_SECONDS)
            };
            self.ui_state.position_seconds = (self.ui_state.position_seconds + dt.min(max_dt))
                .min(self.ui_state.duration_seconds);
            if self.ui_state.position_seconds >= self.ui_state.duration_seconds {
                self.ui_state.playing = false;
            }
        }

        if self.interaction_trace_active() {
            eprintln!(
                "[interaction:transport] dt_ms={:.2} advanced_playback={} is_seek={} seek_exact={} pos {:.3}->{:.3} playing={}",
                dt * 1000.0,
                advanced_playback,
                is_seek,
                seek_exact,
                start_position,
                self.ui_state.position_seconds,
                self.ui_state.playing,
            );
        }

        // Skip decode entirely if the transport position hasn't actually moved since the last
        // decoded frame — see `last_decoded_position`'s doc comment for why this guard matters
        // beyond just avoiding wasted work: without it, a redraw firing for an unrelated reason
        // while paused (cursor blink, hover, mouse movement) can re-enter `seek_and_decode`'s
        // "not caught up yet" branch and decode one more frame forward each time, which looks
        // like the paused video stuttering/looping instead of holding still.
        if self.last_decoded_position != Some(self.ui_state.position_seconds) {
            let trace_active = self.interaction_trace_active();
            if let Some(pipeline) = self.pipeline.as_mut() {
                let start = Instant::now();
                let decoded_result = if is_seek {
                    pipeline.seek_and_decode_ref(self.ui_state.position_seconds, seek_exact)
                } else {
                    pipeline.seek_and_decode_ref(self.ui_state.position_seconds, true)
                };
                let decode_elapsed = start.elapsed();
                if let Ok(decoded) = decoded_result {
                    if let Some(perf) = self.perf.as_mut() {
                        perf.decode += decode_elapsed;
                    }
                    let decoded_changed = decoded.changed;
                    let frame_pts = decoded.frame.pts_seconds;
                    if decoded.changed {
                        let frame = decoded.frame;
                        let start = Instant::now();
                        self.compositor.upload_frame(
                            &self.gpu.device,
                            &self.gpu.queue,
                            frame.width,
                            frame.height,
                            &frame.bgra,
                        );
                        if let Some(perf) = self.perf.as_mut() {
                            perf.upload += start.elapsed();
                            perf.uploads += 1;
                        }
                    } else if let Some(perf) = self.perf.as_mut() {
                        perf.cache_hits += 1;
                    }
                    if trace_active {
                        eprintln!(
                            "[interaction:decode] ok changed={} target={:.3} frame_pts={:.3} elapsed_ms={:.2} exact={}",
                            decoded_changed,
                            self.ui_state.position_seconds,
                            frame_pts,
                            decode_elapsed.as_secs_f64() * 1000.0,
                            if is_seek { seek_exact } else { true },
                        );
                    }
                    self.last_decoded_position = Some(self.ui_state.position_seconds);
                } else {
                    if let Some(perf) = self.perf.as_mut() {
                        perf.decode += decode_elapsed;
                    }
                    eprintln!(
                        "video decode error at {:.3}s (elapsed {:.2}ms, exact={})",
                        self.ui_state.position_seconds,
                        decode_elapsed.as_secs_f64() * 1000.0,
                        if is_seek { seek_exact } else { true },
                    );
                }
                // Read the purely-CPU sub-stage split now that `decoded_result` (which borrowed
                // `pipeline`) has been dropped. Cache-hit calls leave these at zero, matching that
                // they do no h264/scale/copy work — so summed over the interval they attribute the
                // aggregate `decode` number to its stages.
                let timings = pipeline.last_timings();
                if let Some(perf) = self.perf.as_mut() {
                    perf.demux += timings.demux;
                    perf.send += timings.send;
                    perf.receive += timings.receive;
                    perf.scale += timings.scale;
                    perf.copy += timings.copy;
                }
                if trace_active {
                    eprintln!(
                        "[interaction:decode-stages] demux_ms={:.2} send_ms={:.2} receive_ms={:.2} scale_ms={:.2} copy_ms={:.2}",
                        timings.demux.as_secs_f64() * 1000.0,
                        timings.send.as_secs_f64() * 1000.0,
                        timings.receive.as_secs_f64() * 1000.0,
                        timings.scale.as_secs_f64() * 1000.0,
                        timings.copy.as_secs_f64() * 1000.0,
                    );
                }
            }
        } else if self.interaction_trace_active() {
            eprintln!(
                "[interaction:decode] skipped unchanged_pos={:.3}",
                self.ui_state.position_seconds,
            );
        }
        advanced_playback
    }

    fn update_midi_position(&mut self) {
        let midi_time = self.ui_state.position_seconds - self.ui_state.sync_offset_seconds;
        self.compositor
            .update_midi(&self.gpu.queue, midi_time as f32);
    }

    fn apply_post_ui_updates(&mut self) {
        let note_layer = effective_note_layer(&self.ui_state);
        if self.ui_state.calibration != self.applied_calibration
            || note_layer != self.applied_note_layer
        {
            self.compositor.resize(
                &gpu_handles(&self.gpu),
                (self.canvas_size.0 as f32, self.canvas_size.1 as f32),
                &self.ui_state.calibration,
                &note_layer,
            );
            self.applied_calibration = self.ui_state.calibration;
            self.applied_note_layer = note_layer;
        }

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
        if self.ui_state.import_style_requested {
            self.ui_state.import_style_requested = false;
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Style", &["ron"])
                .pick_file()
            {
                self.load_style(&path);
            }
        }
        if self.ui_state.reload_style_requested {
            self.ui_state.reload_style_requested = false;
            if let Some(path) = self.ui_state.style_path.clone() {
                self.load_style(&path);
            }
        }
        if self.ui_state.new_project_requested {
            self.ui_state.new_project_requested = false;
            self.new_project();
        }
        if self.ui_state.open_project_requested {
            self.ui_state.open_project_requested = false;
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Project", &["ron"])
                .pick_file()
            {
                self.ui_state.project_path_text = path.display().to_string();
                self.load_project();
            }
        }
        if self.ui_state.save_project_as_requested {
            self.ui_state.save_project_as_requested = false;
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Project", &["ron"])
                .save_file()
            {
                self.ui_state.project_path_text = path.display().to_string();
                self.save_project();
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

        // Cheap uniform writes applied after all file-loading handlers so that canvas_size is
        // final (load_video may have changed it above) before barrier/effects store it for use
        // in set_scissor_rect during render. update_viewport is here too for consistency.
        self.compositor.update_viewport(
            &self.gpu.queue,
            self.canvas_size,
            &self.ui_state.transform,
        );
        let barrier_layer = effective_barrier_layer(&self.ui_state);
        let midi_time = (self.ui_state.position_seconds - self.ui_state.sync_offset_seconds) as f32;
        self.compositor.update_barrier(
            &self.gpu.queue,
            (self.canvas_size.0 as f32, self.canvas_size.1 as f32),
            &self.ui_state.calibration,
            &barrier_layer,
            midi_time,
        );
        let transition_layer = effective_transition_layer(&self.ui_state);
        self.compositor.update_transition(
            &self.gpu.device,
            &self.gpu.queue,
            (self.canvas_size.0 as f32, self.canvas_size.1 as f32),
            &self.ui_state.calibration,
            &transition_layer,
            midi_time,
        );
    }

    fn render_frame(
        &mut self,
        full_output: egui::FullOutput,
        frame_start: Instant,
        dt_duration: Duration,
        playing_changed_by_ui: bool,
    ) {
        let trace_active = self.interaction_trace_active();
        let stage_start = Instant::now();
        let paint_jobs = self
            .egui_ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);
        let tessellate_elapsed = stage_start.elapsed();
        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.gpu.config.width, self.gpu.config.height],
            pixels_per_point: full_output.pixels_per_point,
        };

        let stage_start = Instant::now();
        for (id, delta) in &full_output.textures_delta.set {
            self.egui_renderer
                .update_texture(&self.gpu.device, &self.gpu.queue, *id, delta);
        }
        let texture_elapsed = stage_start.elapsed();

        let start = Instant::now();
        let surface_texture = match self.gpu.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => texture,
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
            _ => {
                self.window.request_redraw();
                return;
            }
        };
        if let Some(perf) = self.perf.as_mut() {
            perf.acquire += start.elapsed();
        }
        let acquire_elapsed = start.elapsed();
        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let render_start = Instant::now();
        let stage_start = Instant::now();
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame_encoder"),
            });
        let encoder_elapsed = stage_start.elapsed();

        let stage_start = Instant::now();
        let egui_cmd_buffers = self.egui_renderer.update_buffers(
            &self.gpu.device,
            &self.gpu.queue,
            &mut encoder,
            &paint_jobs,
            &screen_descriptor,
        );
        let egui_buffers_elapsed = stage_start.elapsed();

        let stage_start = Instant::now();
        // The compositor now renders into the offscreen preview texture, not the swapchain
        // directly — the egui pass below displays it via `egui::Image` in the central panel
        // (see CLAUDE.md's milestone 6c notes). This also sidesteps the old two-pass split that
        // was needed only because `WaterfallRenderer::render` ties its `&mut self` borrow to the
        // render pass's invariant lifetime parameter: this pass has a real (non-`'static`)
        // lifetime, same as the old "scene_pass" did, so `WaterfallRenderer` is still fine with
        // it even though it's a different render target now.
        {
            let background = srgb_to_linear(effective_background_color(&self.ui_state));
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("preview_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.preview_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: background[0] as f64,
                            g: background[1] as f64,
                            b: background[2] as f64,
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

            self.compositor.render(&mut render_pass);
        }
        let compositor_elapsed = stage_start.elapsed();

        let stage_start = Instant::now();
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
        let egui_render_elapsed = stage_start.elapsed();

        for id in &full_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }

        let stage_start = Instant::now();
        self.gpu.queue.submit(
            egui_cmd_buffers
                .into_iter()
                .chain(std::iter::once(encoder.finish())),
        );
        let submit_elapsed = stage_start.elapsed();
        let stage_start = Instant::now();
        surface_texture.present();
        let present_elapsed = stage_start.elapsed();
        if let Some(perf) = self.perf.as_mut() {
            perf.render_submit += render_start.elapsed();
        }

        self.egui_state
            .handle_platform_output(&self.window, full_output.platform_output);

        // egui requests its own repaints (side-panel collapse/expand slide, menu open/hover
        // animations, button flash, etc.) via each viewport's `repaint_delay` —
        // `Duration::MAX` means "nothing to animate", anything else is a real deadline
        // (`ZERO` meaning "now"). `about_to_wait` folds this in alongside the playback
        // schedule; without it, an in-progress animation only advances when some unrelated
        // event (e.g. mouse movement) happens to trigger the next repaint, which is exactly
        // why the collapse/expand slide and the File menu's open animation looked stuck.
        self.next_ui_redraw_at = full_output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .filter(|viewport| viewport.repaint_delay < Duration::MAX)
            .and_then(|viewport| frame_start.checked_add(viewport.repaint_delay));

        // Export progress can redraw as fast as the event loop allows, but playback redraws are
        // scheduled from `about_to_wait` at the video's frame interval. Immediately chaining
        // playback redraws here drives a 30fps file at the monitor refresh rate and can make
        // exact decode fall into a catch-up feedback loop.
        if self.export_run.is_some() {
            self.window.request_redraw();
        }
        if playing_changed_by_ui && !self.ui_state.playing {
            self.window.request_redraw();
        }
        self.next_playback_redraw_at = if self.ui_state.playing {
            let frame_duration = self
                .pipeline
                .as_ref()
                .map(VideoPipeline::frame_duration_seconds)
                .unwrap_or(1.0 / 30.0);
            let frame_duration = Duration::from_secs_f64(frame_duration.max(0.001));
            let now = Instant::now();
            let cadence_deadline = frame_start + frame_duration;
            Some(if cadence_deadline > now {
                cadence_deadline
            } else {
                now + frame_duration
            })
        } else {
            None
        };
        if trace_active {
            eprintln!(
                "[interaction:render] tessellate_ms={:.2} texture_ms={:.2} acquire_ms={:.2} encoder_ms={:.2} egui_buffers_ms={:.2} compositor_ms={:.2} egui_render_ms={:.2} submit_ms={:.2} present_ms={:.2}",
                tessellate_elapsed.as_secs_f64() * 1000.0,
                texture_elapsed.as_secs_f64() * 1000.0,
                acquire_elapsed.as_secs_f64() * 1000.0,
                encoder_elapsed.as_secs_f64() * 1000.0,
                egui_buffers_elapsed.as_secs_f64() * 1000.0,
                compositor_elapsed.as_secs_f64() * 1000.0,
                egui_render_elapsed.as_secs_f64() * 1000.0,
                submit_elapsed.as_secs_f64() * 1000.0,
                present_elapsed.as_secs_f64() * 1000.0,
            );
        }
        if trace_active {
            let now = Instant::now();
            eprintln!(
                "[interaction:schedule] frame_total_ms={:.2} dt_ms={:.2} playing={} next_playback_due_ms={:?} ui_due_ms={:?}",
                frame_start.elapsed().as_secs_f64() * 1000.0,
                dt_duration.as_secs_f64() * 1000.0,
                self.ui_state.playing,
                self.next_playback_redraw_at
                    .map(|deadline| deadline.saturating_duration_since(now).as_secs_f64() * 1000.0),
                self.next_ui_redraw_at
                    .map(|deadline| deadline.saturating_duration_since(now).as_secs_f64() * 1000.0),
            );
        }
        if let Some(perf) = self.perf.as_mut() {
            perf.record_frame(frame_start.elapsed(), dt_duration);
            perf.maybe_print();
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
    project_path: Option<PathBuf>,
    style_path: Option<PathBuf>,
    state: Option<AppState>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_none() {
            self.state = Some(AppState::new(
                event_loop,
                self.video_path.as_deref(),
                self.midi_path.as_deref(),
                self.project_path.as_deref(),
                self.style_path.as_deref(),
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
        // egui-winit sets `repaint: true` for `WindowEvent::RedrawRequested` itself (see
        // egui-winit's `on_window_event`, which treats it as "a repaint just happened, the
        // platform may want another one queued"). Requesting another redraw here unconditionally
        // turns every `RedrawRequested` into its own trigger for the next one — a self-sustaining
        // loop at the display's full vsync rate (measured ~120Hz on this machine) regardless of
        // `ui_state.playing`/export state, completely bypassing `redraw`'s own end-of-frame
        // throttle below. `redraw` (called from the `RedrawRequested` arm further down) already
        // decides for itself whether to keep the loop going, so this generic repaint-on-any-event
        // nudge should only fire for other events that actually need an immediate repaint.
        // Passive pointer input during playback is deliberately excluded: otherwise simply
        // moving or clicking the mouse turns playback into an input-rate redraw loop and bypasses
        // the video-frame scheduler. Cursor movement while a button is held is still treated as
        // active input, so timeline/crop/calibration drags remain responsive.
        let passive_playback_pointer_input = state.ui_state.playing
            && (matches!(event, WindowEvent::MouseInput { .. })
                || (state.pointer_buttons_down == 0
                    && matches!(event, WindowEvent::CursorMoved { .. })));
        if response.repaint
            && !matches!(event, WindowEvent::RedrawRequested)
            && !passive_playback_pointer_input
        {
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
                if state.ui_state.exit_requested {
                    event_loop.exit();
                }
                return;
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                state.modifiers = modifiers.state();
                return;
            }
            WindowEvent::Focused(false) => {
                state.pointer_buttons_down = 0;
            }
            WindowEvent::MouseInput {
                state: button_state,
                ..
            } => match button_state {
                ElementState::Pressed => {
                    state.pointer_buttons_down = state.pointer_buttons_down.saturating_add(1);
                }
                ElementState::Released => {
                    state.pointer_buttons_down = state.pointer_buttons_down.saturating_sub(1);
                }
            },
            _ => {}
        }

        if response.consumed {
            return;
        }

        if let WindowEvent::KeyboardInput {
            event: key_event, ..
        } = &event
        {
            if key_event.state == ElementState::Pressed {
                handle_shortcut(state, key_event.physical_key, state.modifiers);
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(state) = self.state.as_mut() else {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        };

        // Playback's own cadence (only relevant while playing) and egui's own animation
        // deadline (relevant regardless of playback state — a menu can be open, or the side
        // panel mid-collapse, while paused) are two independent reasons to wake up early.
        //
        // While PAUSED, whichever comes first wins (smooth menu/panel animations).
        //
        // While PLAYING, the video cadence governs and egui's deadline is NOT allowed to
        // schedule a redraw *sooner* than the next frame. This matters because passive mouse
        // movement makes egui request a repaint on every hover update, and that request flows
        // through `next_ui_redraw_at` — bypassing the `passive_playback_cursor_move` guard in
        // `window_event`, which only suppresses the *direct* redraw nudge. Left unclamped it
        // drove the redraw rate to 40-54fps on a 30fps clip: extra full redraws that decode no
        // new frame but pile main-thread work + event-loop wakeup churn onto an already
        // oversubscribed core count, descheduling the frame-threaded H.264 decode workers so
        // `send_packet` blocks waiting for a free worker slot (measured ~150x blowup during
        // mouse-move — see the investigation section in CLAUDE.md). Any real egui animation
        // still advances during playback, just at the video's frame cadence, which is
        // imperceptible. A `next_ui_redraw_at` *later* than the frame deadline is irrelevant —
        // the playback redraw fires first and services egui's repaint request anyway.
        let deadline = if state.ui_state.playing {
            state.next_playback_redraw_at
        } else {
            state.next_playback_redraw_at = None;
            state.next_ui_redraw_at
        };

        match deadline {
            Some(next) if next > Instant::now() => {
                event_loop.set_control_flow(ControlFlow::WaitUntil(next));
            }
            Some(_) => {
                state.next_playback_redraw_at = None;
                state.next_ui_redraw_at = None;
                state.window.request_redraw();
                event_loop.set_control_flow(ControlFlow::Wait);
            }
            None => {
                event_loop.set_control_flow(ControlFlow::Wait);
            }
        }
    }
}

/// Familiar keyboard navigation (6d): Space play/pause (pre-existing), Left/Right seek ±1 frame
/// (DaVinci Resolve-style), Shift+Left/Right seek ±1s, Home/End jump to start/end, Ctrl+S save
/// project, Ctrl+O open project, Esc cancel an in-progress export. The project/save actions route
/// through the same `UiState` request flags the Project tab's buttons use — one code path
/// regardless of whether the button or the shortcut triggered it, consumed next redraw.
fn handle_shortcut(
    state: &mut AppState,
    key: PhysicalKey,
    modifiers: winit::keyboard::ModifiersState,
) {
    let PhysicalKey::Code(code) = key else {
        return;
    };
    let ctrl = modifiers.control_key();
    let shift = modifiers.shift_key();

    match code {
        KeyCode::Space => {
            state.ui_state.playing = !state.ui_state.playing;
            state.last_instant = Instant::now();
            if state.ui_state.playing {
                state.audio_resume_pending = true;
                if state.audio_playing {
                    state.audio.set_playing(false);
                    state.audio_playing = false;
                }
                state.trace_interaction_for("play");
            } else {
                state.audio_resume_pending = false;
                state.trace_interaction_for("pause");
            }
        }
        KeyCode::ArrowLeft => {
            let step = seek_step_seconds(state, shift);
            let base = state
                .ui_state
                .seek_request
                .unwrap_or(state.ui_state.position_seconds);
            state.ui_state.seek_request = Some((base - step).max(0.0));
            state.ui_state.seek_request_exact = true;
        }
        KeyCode::ArrowRight => {
            let step = seek_step_seconds(state, shift);
            let base = state
                .ui_state
                .seek_request
                .unwrap_or(state.ui_state.position_seconds);
            state.ui_state.seek_request = Some((base + step).min(state.ui_state.duration_seconds));
            state.ui_state.seek_request_exact = true;
        }
        KeyCode::Home => {
            state.ui_state.seek_request = Some(0.0);
            state.ui_state.seek_request_exact = true;
        }
        KeyCode::End => {
            state.ui_state.seek_request = Some(state.ui_state.duration_seconds);
            state.ui_state.seek_request_exact = true;
        }
        KeyCode::KeyS if ctrl => {
            state.ui_state.save_requested = true;
        }
        KeyCode::KeyO if ctrl => {
            state.ui_state.open_project_requested = true;
        }
        KeyCode::Escape => {
            if state.export_run.is_some() {
                state.ui_state.export_cancel_requested = true;
            }
        }
        _ => return,
    }
    state.window.request_redraw();
}

fn seek_step_seconds(state: &AppState, shift: bool) -> f64 {
    if shift {
        1.0
    } else {
        state
            .pipeline
            .as_ref()
            .map(VideoPipeline::frame_duration_seconds)
            .unwrap_or(1.0 / 30.0)
    }
}

/// Classifies CLI args by extension the same way `WindowEvent::DroppedFile` classifies a
/// drag-drop: `.mid`/`.midi` is MIDI, `.fmstyle.ron` is a visual style (checked against the full
/// file name, not just `Path::extension()`'s last-component view, since both a style and a plain
/// project file end in `.ron`), a remaining `.ron` is a saved `.fmproj.ron` project file, anything
/// else is treated as the video. Order-independent, so `app song.fmproj.ron`, `app look.fmstyle.ron`,
/// and `app video.mp4 song.mid` all work without needing separate flags. Unlike drag-drop
/// (`WindowEvent::DroppedFile` has no `.ron`/`.fmstyle.ron` case at all), a style path passed here
/// is applied *after* a project path, so it always wins over whatever `style` field the project
/// itself carries.
fn main() {
    let mut video_path = None;
    let mut midi_path = None;
    let mut project_path = None;
    let mut style_path = None;
    for arg in std::env::args().skip(1) {
        let path = PathBuf::from(arg);
        let is_style = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_ascii_lowercase().ends_with(".fmstyle.ron"))
            .unwrap_or(false);
        if is_style {
            style_path = Some(path);
            continue;
        }
        match path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("mid") | Some("midi") => midi_path = Some(path),
            Some("ron") => project_path = Some(path),
            _ => video_path = Some(path),
        }
    }

    let event_loop = EventLoop::new().expect("failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = App {
        video_path,
        midi_path,
        project_path,
        style_path,
        state: None,
    };
    event_loop.run_app(&mut app).expect("event loop error");
}
