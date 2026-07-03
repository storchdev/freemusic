//! UI-agnostic compositor: draws the transformed video quad then the MIDI note-highway overlay
//! into whatever render pass the caller provides. Used by both the interactive `app` (against
//! the window's swapchain-backed GPU) and `export` (against a headless, offscreen GPU) — see
//! CLAUDE.md for why this was split out of `app` when milestone 5 (MP4 export) needed a second
//! GPU context to run the exact same compositing logic against.

mod midi_overlay;
mod video_quad;

use project::{KeyboardCalibration, VideoTransform};

pub use midi_overlay::GpuHandles;

pub struct Compositor {
    video_quad: video_quad::VideoQuad,
    midi_overlay: midi_overlay::MidiOverlay,
}

impl Compositor {
    pub fn new(gpu: &GpuHandles, viewport: (f32, f32), calibration: &KeyboardCalibration) -> Self {
        let video_quad = video_quad::VideoQuad::new(gpu.device, gpu.texture_format);
        let mut midi_overlay = midi_overlay::MidiOverlay::new(gpu);
        midi_overlay.resize(gpu, viewport, calibration);
        Self {
            video_quad,
            midi_overlay,
        }
    }

    pub fn loaded_midi_name(&self) -> Option<&str> {
        self.midi_overlay.loaded_name()
    }

    /// Sorted note onset times in seconds; empty if no MIDI is loaded. Used by the bottom
    /// timeline's note-density strip.
    pub fn note_start_times(&self) -> &[f32] {
        self.midi_overlay.note_start_times()
    }

    pub fn load_midi(
        &mut self,
        gpu: &GpuHandles,
        viewport: (f32, f32),
        calibration: &KeyboardCalibration,
        path: &std::path::Path,
    ) -> Result<(), String> {
        self.midi_overlay.load(gpu, viewport, calibration, path)
    }

    /// Recomputes note-lane layout for a new viewport size or calibration. Not needed for the
    /// video quad itself — its viewport-dependent state is the cheap per-frame uniform written by
    /// `update_viewport` instead.
    pub fn resize(
        &mut self,
        gpu: &GpuHandles,
        viewport: (f32, f32),
        calibration: &KeyboardCalibration,
    ) {
        self.midi_overlay.resize(gpu, viewport, calibration);
    }

    pub fn upload_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        bgra: &[u8],
    ) {
        self.video_quad
            .upload_frame(device, queue, width, height, bgra);
    }

    pub fn update_viewport(
        &self,
        queue: &wgpu::Queue,
        window_size: (u32, u32),
        transform: &VideoTransform,
    ) {
        self.video_quad
            .update_viewport(queue, window_size, transform);
    }

    /// Advances the waterfall to `time_seconds`, expected to already have the sync offset
    /// subtracted by the caller (`midi_time = transport_time - sync_offset_seconds`).
    pub fn update_midi(&mut self, time_seconds: f32) {
        self.midi_overlay.update(time_seconds);
    }

    pub fn render<'rpass>(&'rpass mut self, render_pass: &mut wgpu::RenderPass<'rpass>) {
        self.video_quad.render(render_pass);
        self.midi_overlay.render(render_pass);
    }
}
