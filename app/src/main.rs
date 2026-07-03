mod gpu;
mod midi_overlay;
mod ui;
mod video_quad;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use gpu::Gpu;
use midi_overlay::MidiOverlay;
use project::{KeyboardCalibration, Project};
use ui::UiState;
use video_pipeline::VideoPipeline;
use video_quad::VideoQuad;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

struct AppState {
    window: Arc<Window>,
    gpu: Gpu,
    video_quad: VideoQuad,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
    pipeline: Option<VideoPipeline>,
    video_path: Option<PathBuf>,
    midi_path: Option<PathBuf>,
    midi_overlay: MidiOverlay,
    /// Calibration last used to build the waterfall layout; compared against `ui_state.calibration`
    /// each redraw so a drag only triggers the (fairly heavy, full-rebuild) `midi_overlay.resize`
    /// when it actually changed, not every frame.
    applied_calibration: KeyboardCalibration,
    ui_state: UiState,
    last_instant: Instant,
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
        let video_quad = VideoQuad::new(&gpu.device, gpu.config.format);
        let midi_overlay = MidiOverlay::new(&gpu);

        let egui_ctx = egui::Context::default();
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        let egui_renderer = egui_wgpu::Renderer::new(
            &gpu.device,
            gpu.config.format,
            egui_wgpu::RendererOptions::default(),
        );

        let mut state = Self {
            window,
            gpu,
            video_quad,
            egui_ctx,
            egui_state,
            egui_renderer,
            pipeline: None,
            video_path: None,
            midi_path: None,
            midi_overlay,
            applied_calibration: KeyboardCalibration::default(),
            ui_state: UiState {
                playing: false,
                position_seconds: 0.0,
                duration_seconds: 0.0,
                seek_request: None,
                dropping: false,
                midi_name: None,
                sync_offset_seconds: 0.0,
                calibration: KeyboardCalibration::default(),
                project_path_text: String::new(),
                save_requested: false,
                load_requested: false,
                status_message: None,
            },
            last_instant: Instant::now(),
        };

        let size = state.window.inner_size();
        state.midi_overlay.resize(
            &state.gpu,
            (size.width as f32, size.height as f32),
            &state.ui_state.calibration,
        );

        if let Some(path) = video_path {
            state.load_video(path);
        }
        if let Some(path) = midi_path {
            state.load_midi(path);
        }

        state
    }

    /// Opens `path` as the active video, replacing whatever was loaded before (CLI arg at
    /// startup, or a previous drag-drop). Decodes and uploads the first frame immediately so
    /// the window isn't blank before the next redraw.
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
            self.video_quad.upload_frame(
                &self.gpu.device,
                &self.gpu.queue,
                frame.width,
                frame.height,
                &frame.bgra,
            );
            let size = self.window.inner_size();
            self.video_quad
                .update_viewport(&self.gpu.queue, (size.width, size.height));
        }

        self.pipeline = Some(pipeline);
        self.video_path = Some(path.to_path_buf());
        if self.ui_state.project_path_text.is_empty() {
            self.ui_state.project_path_text = default_project_path(path).display().to_string();
        }
    }

    /// Parses `path` as a MIDI file and (re)builds the waterfall overlay for it.
    fn load_midi(&mut self, path: &Path) {
        let size = self.window.inner_size();
        match self.midi_overlay.load(
            &self.gpu,
            (size.width as f32, size.height as f32),
            &self.ui_state.calibration,
            path,
        ) {
            Ok(()) => {
                self.midi_path = Some(path.to_path_buf());
                self.ui_state.midi_name = self.midi_overlay.loaded_name().map(str::to_owned);
            }
            Err(err) => eprintln!("failed to open midi file {path:?}: {err}"),
        }
    }

    /// Serializes the current video/MIDI paths, sync offset, and calibration to the path in
    /// the project text field.
    fn save_project(&mut self) {
        let project = Project {
            video_path: self.video_path.clone(),
            midi_path: self.midi_path.clone(),
            sync_offset_seconds: self.ui_state.sync_offset_seconds,
            calibration: self.ui_state.calibration,
        };
        let path = PathBuf::from(&self.ui_state.project_path_text);
        self.ui_state.status_message = Some(match project.save(&path) {
            Ok(()) => format!("Saved to {}", path.display()),
            Err(err) => err,
        });
    }

    /// Loads a project from the path in the project text field, replacing the current video,
    /// MIDI, sync offset, and calibration with whatever it contains.
    fn load_project(&mut self) {
        let path = PathBuf::from(&self.ui_state.project_path_text);
        match Project::load(&path) {
            Ok(project) => {
                self.ui_state.sync_offset_seconds = project.sync_offset_seconds;
                self.ui_state.calibration = project.calibration;
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

        if let Some(target) = self.ui_state.seek_request.take() {
            self.ui_state.position_seconds = target.clamp(0.0, self.ui_state.duration_seconds);
        } else if self.ui_state.playing {
            self.ui_state.position_seconds =
                (self.ui_state.position_seconds + dt).min(self.ui_state.duration_seconds);
            if self.ui_state.position_seconds >= self.ui_state.duration_seconds {
                self.ui_state.playing = false;
            }
        }

        if let Some(pipeline) = self.pipeline.as_mut() {
            if let Ok(frame) = pipeline.seek_and_decode(self.ui_state.position_seconds, false) {
                self.video_quad.upload_frame(
                    &self.gpu.device,
                    &self.gpu.queue,
                    frame.width,
                    frame.height,
                    &frame.bgra,
                );
                let size = self.window.inner_size();
                self.video_quad
                    .update_viewport(&self.gpu.queue, (size.width, size.height));
            }
        }
        let midi_time = self.ui_state.position_seconds - self.ui_state.sync_offset_seconds;
        self.midi_overlay.update(midi_time as f32);

        let raw_input = self.egui_state.take_egui_input(&self.window);
        let full_output = {
            let ui_state = &mut self.ui_state;
            self.egui_ctx.run_ui(raw_input, |ui| {
                ui::draw(ui, ui_state);
            })
        };

        if self.ui_state.calibration != self.applied_calibration {
            let size = self.window.inner_size();
            self.midi_overlay.resize(
                &self.gpu,
                (size.width as f32, size.height as f32),
                &self.ui_state.calibration,
            );
            self.applied_calibration = self.ui_state.calibration;
        }

        if self.ui_state.save_requested {
            self.ui_state.save_requested = false;
            self.save_project();
        }
        if self.ui_state.load_requested {
            self.ui_state.load_requested = false;
            self.load_project();
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

        // Two passes rather than one: `WaterfallRenderer::render` ties its `&mut self` borrow
        // to the render pass's lifetime parameter, and `wgpu::RenderPass` is invariant over
        // it, so it can't share a pass that's been `forget_lifetime()`'d to `'static` for
        // egui-wgpu below. Scoping the scene pass normally (real, non-'static lifetime) keeps
        // that borrow checker-legal; loading (not clearing) the second pass composites egui on
        // top of what the first pass drew.
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene_pass"),
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

            self.video_quad.render(&mut render_pass);
            self.midi_overlay.render(&mut render_pass);
        }

        {
            let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
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

        if self.ui_state.playing {
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
                state.gpu.resize(size.width, size.height);
                state
                    .video_quad
                    .update_viewport(&state.gpu.queue, (size.width, size.height));
                state.midi_overlay.resize(
                    &state.gpu,
                    (size.width as f32, size.height as f32),
                    &state.ui_state.calibration,
                );
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
