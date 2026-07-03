mod gpu;
mod ui;
mod video_quad;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use gpu::Gpu;
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
    pipeline: VideoPipeline,
    ui_state: UiState,
    last_instant: Instant,
}

impl AppState {
    fn new(event_loop: &ActiveEventLoop, video_path: &std::path::Path) -> Self {
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

        let pipeline = VideoPipeline::open(video_path).expect("failed to open video file");
        let duration_seconds = pipeline.duration_seconds();

        let mut state = Self {
            window,
            gpu,
            video_quad,
            egui_ctx,
            egui_state,
            egui_renderer,
            pipeline,
            ui_state: UiState {
                playing: false,
                position_seconds: 0.0,
                duration_seconds,
                seek_request: None,
            },
            last_instant: Instant::now(),
        };

        // Decode and upload the first frame so the window isn't blank before any redraw.
        if let Ok(frame) = state.pipeline.seek_and_decode(0.0, false) {
            state.video_quad.upload_frame(
                &state.gpu.device,
                &state.gpu.queue,
                frame.width,
                frame.height,
                &frame.bgra,
            );
            let size = state.window.inner_size();
            state
                .video_quad
                .update_viewport(&state.gpu.queue, (size.width, size.height));
        }

        state
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

        if let Ok(frame) = self
            .pipeline
            .seek_and_decode(self.ui_state.position_seconds, false)
        {
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

        let raw_input = self.egui_state.take_egui_input(&self.window);
        let ui_state = &mut self.ui_state;
        let full_output = self.egui_ctx.run_ui(raw_input, |ui| {
            ui::draw(ui, ui_state);
        });

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

        {
            let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main_pass"),
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
            self.video_quad.render(&mut render_pass);
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

struct App {
    video_path: PathBuf,
    state: Option<AppState>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_none() {
            self.state = Some(AppState::new(event_loop, &self.video_path));
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
    let video_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            eprintln!("usage: app <video-file>");
            std::process::exit(1);
        });

    let event_loop = EventLoop::new().expect("failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = App {
        video_path,
        state: None,
    };
    event_loop.run_app(&mut app).expect("event loop error");
}
