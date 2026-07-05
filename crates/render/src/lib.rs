//! UI-agnostic compositor: draws the transformed video quad then the MIDI note-highway overlay
//! into whatever render pass the caller provides. Used by both the interactive `app` (against
//! the window's swapchain-backed GPU) and `export` (against a headless, offscreen GPU) — see
//! CLAUDE.md for why this was split out of `app` when milestone 5 (MP4 export) needed a second
//! GPU context to run the exact same compositing logic against.

mod barrier;
mod notes;
mod video_quad;

use project::{BarrierLayer, KeyboardCalibration, NoteLayer, VideoTransform};

pub use notes::GpuHandles;

pub struct Compositor {
    video_quad: video_quad::VideoQuad,
    notes: notes::NotesRenderer,
    barrier: barrier::BarrierRenderer,
}

impl Compositor {
    pub fn new(
        gpu: &GpuHandles,
        viewport: (f32, f32),
        calibration: &KeyboardCalibration,
        note_layer: &NoteLayer,
    ) -> Self {
        let video_quad = video_quad::VideoQuad::new(gpu.device, gpu.texture_format);
        let mut notes = notes::NotesRenderer::new(gpu);
        notes.resize(gpu, viewport, calibration, note_layer);
        let barrier = barrier::BarrierRenderer::new(gpu.device, gpu.texture_format);
        Self {
            video_quad,
            notes,
            barrier,
        }
    }

    pub fn loaded_midi_name(&self) -> Option<&str> {
        self.notes.loaded_name()
    }

    /// Sorted note onset times in seconds; empty if no MIDI is loaded. Used by the bottom
    /// timeline's note-density strip.
    pub fn note_start_times(&self) -> &[f32] {
        self.notes.note_start_times()
    }

    pub fn load_midi(
        &mut self,
        gpu: &GpuHandles,
        viewport: (f32, f32),
        calibration: &KeyboardCalibration,
        note_layer: &NoteLayer,
        path: &std::path::Path,
    ) -> Result<(), String> {
        self.notes
            .load(gpu, viewport, calibration, note_layer, path)
    }

    /// Recomputes note-lane layout for a new viewport size, calibration, or note layer. Not
    /// needed for the video quad itself — its viewport-dependent state is the cheap per-frame
    /// uniform written by `update_viewport` instead.
    pub fn resize(
        &mut self,
        gpu: &GpuHandles,
        viewport: (f32, f32),
        calibration: &KeyboardCalibration,
        note_layer: &NoteLayer,
    ) {
        self.notes.resize(gpu, viewport, calibration, note_layer);
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
    pub fn update_midi(&mut self, queue: &wgpu::Queue, time_seconds: f32) {
        self.notes.update(queue, time_seconds);
    }

    /// Recomputes the barrier's geometry/color/glow and its note-arrival pulse intensity for
    /// `midi_time_seconds` (same sync-offset-subtracted convention as `update_midi`). Cheap
    /// uniform writes only — called unconditionally every redraw, like `update_viewport`, rather
    /// than dirty-checked like `resize`/`load_midi`'s note-instance rebuild.
    pub fn update_barrier(
        &mut self,
        queue: &wgpu::Queue,
        canvas_size: (f32, f32),
        calibration: &KeyboardCalibration,
        barrier_layer: &BarrierLayer,
        midi_time_seconds: f32,
    ) {
        let barrier_fraction = calibration.barrier_fraction.clamp(0.05, 1.0);
        self.barrier
            .set_style(queue, canvas_size, barrier_fraction, barrier_layer);
        self.barrier
            .update_pulse(queue, midi_time_seconds, self.notes.note_start_times());
    }

    pub fn render<'rpass>(&'rpass mut self, render_pass: &mut wgpu::RenderPass<'rpass>) {
        self.video_quad.render(render_pass);
        self.notes.render(render_pass);
        self.barrier.render(render_pass);
    }
}
