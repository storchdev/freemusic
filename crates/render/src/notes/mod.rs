//! Loads a MIDI file and renders its falling-notes "note highway" above the video quad.
//!
//! Vendored in-tree (own shader, own pipeline, own instance-building loop) rather than reusing
//! Neothesia's `WaterfallRenderer` — see CLAUDE.md's "Vendor note pipeline" section for the
//! rationale (mirrors how `mp4-encoder` was forked from `ffmpeg-encoder`) and for why this
//! deletes the fragile `virtual_canvas_height` viewport/scissor trick the previous
//! `neothesia_core`-backed version needed: owning the shader means `barrier_fraction` can be a
//! real uniform instead of an indirect remap of a hardcoded 80% constant.
//!
//! Note lanes are aligned to the real keyboard visible in the footage via `KeyboardCalibration`
//! (left/right fractions of window width, set by the user dragging guide handles — see
//! `ui::draw_calibration_handles`). `update`'s `time` argument is expected to already have the
//! manual sync offset applied by the caller (`midi_time = transport_time - sync_offset_seconds`).

mod instance;
mod pipeline;

use std::path::Path;

use midi_file::MidiFile;
use piano_layout::{KeyboardLayout, KeyboardRange, Sizing};
use project::{BlackKeyFill, Fill, KeyboardCalibration, NoteLayer};

use instance::NoteInstance;
use pipeline::NotesPipeline;

/// Raw wgpu handles needed to build the notes pipeline, taken instead of a single app-specific
/// `Gpu` struct so this works identically for the interactive window's GPU and export's headless
/// one. `instance`/`adapter` are unused now that we no longer construct a `neothesia_core::Gpu`
/// wrapper, but kept so `Compositor`'s callers (`app::gpu_handles`, `export::run_inner`) don't
/// need to change what they build.
pub struct GpuHandles<'a> {
    pub instance: &'a wgpu::Instance,
    pub adapter: &'a wgpu::Adapter,
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub texture_format: wgpu::TextureFormat,
}

pub struct NotesRenderer {
    pipeline: NotesPipeline,
    loaded: Option<Loaded>,
    canvas_size: (f32, f32),
    barrier_fraction: f32,
}

/// One note's arrival at the barrier: when (sync-offset-subtracted transport seconds, matching
/// `note_start_times`'s convention) and where (pixel x, center of the note's lane) — everything
/// `render::effects::EffectsRenderer` needs to spawn a particle burst/flash at the right place and
/// time. Recomputed alongside the note instances themselves in `rebuild_instances`, since the x
/// position depends on the same calibrated keyboard layout the instances are built from.
#[derive(Debug, Clone, Copy)]
pub struct HitEvent {
    pub time_seconds: f32,
    pub x_px: f32,
}

struct Loaded {
    midi: MidiFile,
    /// Sorted note start times in seconds, cached at load time — used by the bottom timeline's
    /// note-density strip (`ui::draw_timeline`), which just needs raw onset times, not the full
    /// per-track structure.
    note_starts: Vec<f32>,
    /// Sorted (ascending time, matching `note_starts`) barrier-arrival events for the notes
    /// actually drawn (in-range, non-drum) — see `HitEvent`.
    hit_events: Vec<HitEvent>,
}

impl NotesRenderer {
    pub fn new(gpu: &GpuHandles) -> Self {
        Self {
            pipeline: NotesPipeline::new(gpu.device, gpu.texture_format),
            loaded: None,
            canvas_size: (1.0, 1.0),
            barrier_fraction: 0.8,
        }
    }

    pub fn loaded_name(&self) -> Option<&str> {
        self.loaded.as_ref().map(|l| l.midi.name.as_str())
    }

    /// Sorted note onset times in seconds, or an empty slice if nothing is loaded.
    pub fn note_start_times(&self) -> &[f32] {
        self.loaded
            .as_ref()
            .map(|l| l.note_starts.as_slice())
            .unwrap_or(&[])
    }

    /// Sorted barrier-arrival events (time + x position), or an empty slice if nothing is loaded
    /// — used by `render::effects::EffectsRenderer` to spawn particles/flashes as the transport
    /// crosses each note's arrival time.
    pub fn hit_events(&self) -> &[HitEvent] {
        self.loaded
            .as_ref()
            .map(|l| l.hit_events.as_slice())
            .unwrap_or(&[])
    }

    /// Parses `path` as a MIDI file and (re)builds the note instances for it.
    pub fn load(
        &mut self,
        gpu: &GpuHandles,
        viewport: (f32, f32),
        calibration: &KeyboardCalibration,
        note_layer: &NoteLayer,
        path: &Path,
    ) -> Result<(), String> {
        let midi = MidiFile::new(path)?;
        let mut note_starts: Vec<f32> = midi
            .tracks
            .iter()
            .flat_map(|track| track.notes.iter())
            .map(|note| note.start.as_secs_f32())
            .collect();
        note_starts.sort_by(|a, b| a.partial_cmp(b).unwrap());
        self.loaded = Some(Loaded {
            midi,
            note_starts,
            hit_events: Vec::new(),
        });
        self.apply_view(gpu, viewport, calibration, note_layer);
        Ok(())
    }

    /// Recomputes the projection and note-lane layout for a new viewport size, calibration, or
    /// note layer (fill/sheen/glow/roundedness/fall_speed).
    pub fn resize(
        &mut self,
        gpu: &GpuHandles,
        viewport: (f32, f32),
        calibration: &KeyboardCalibration,
        note_layer: &NoteLayer,
    ) {
        self.apply_view(gpu, viewport, calibration, note_layer);
    }

    fn apply_view(
        &mut self,
        gpu: &GpuHandles,
        viewport: (f32, f32),
        calibration: &KeyboardCalibration,
        note_layer: &NoteLayer,
    ) {
        let barrier_fraction = calibration.barrier_fraction.clamp(0.05, 1.0);
        self.pipeline
            .set_view(gpu.queue, viewport.0, viewport.1, barrier_fraction);
        self.pipeline
            .set_speed(gpu.queue, note_layer.fall_speed.max(1.0));
        self.pipeline.set_style(gpu.queue, note_layer);
        self.canvas_size = viewport;
        self.barrier_fraction = barrier_fraction;
        self.rebuild_instances(gpu, viewport, calibration, note_layer);
    }

    fn rebuild_instances(
        &mut self,
        gpu: &GpuHandles,
        viewport: (f32, f32),
        calibration: &KeyboardCalibration,
        note_layer: &NoteLayer,
    ) {
        self.pipeline.clear();
        let Some(loaded) = self.loaded.as_ref() else {
            return;
        };

        let layout = keyboard_layout(viewport, calibration);
        let range = &layout.range;
        let range_start = range.start() as usize;
        let left_x = viewport.0 * calibration.left_fraction;

        // Resolve the fill to a top/bottom color pair once per rebuild — for `Fill::Solid` both
        // ends are the same color, so the shader's gradient mix is a no-op and every note just
        // renders flat, matching the pre-Phase-C look exactly.
        let (top_base, bottom_base) = resolve_fill_base(&note_layer.fill);
        let top_light = color_to_linear(top_base);
        let bottom_light = color_to_linear(bottom_base);
        // `Auto`'s output is byte-identical to the pre-Phase-F behavior (same `darken(_, 0.6)`
        // call on the same base colors) — the required no-regression guarantee.
        let (top_dark, bottom_dark) = match &note_layer.black_key_fill {
            BlackKeyFill::Auto => (
                color_to_linear(darken(top_base, 0.6)),
                color_to_linear(darken(bottom_base, 0.6)),
            ),
            BlackKeyFill::Same => (top_light, bottom_light),
            BlackKeyFill::Custom(fill) => {
                let (dark_top, dark_bottom) = resolve_fill_base(fill);
                (color_to_linear(dark_top), color_to_linear(dark_bottom))
            }
        };

        let mut notes: Vec<_> = loaded
            .midi
            .tracks
            .iter()
            .flat_map(|track| track.notes.iter())
            .filter(|note| range.contains(note.note) && note.channel != 9)
            .collect();
        // Render newer notes on top of older ones, matching Neothesia's own convention.
        notes.sort_by_key(|note| note.start);

        let mut hit_events = Vec::with_capacity(notes.len());
        let instances = self.pipeline.instances();
        for note in notes {
            let key = &layout.keys[note.note as usize - range_start];
            let (color_top, color_bottom) = if key.kind().is_sharp() {
                (top_dark, bottom_dark)
            } else {
                (top_light, bottom_light)
            };
            let duration = note.duration.as_secs_f32().max(0.1);
            let note_x = key.x() + left_x;
            let start_seconds = note.start.as_secs_f32();

            instances.push(NoteInstance {
                position: [note_x, start_seconds],
                size: [key.width() - 1.0, duration - 0.01],
                color_top,
                color_bottom,
                radius: key.width() * 0.2 * note_layer.roundedness,
                velocity: note.velocity as f32 / 127.0,
                track_index: note.track_id as f32,
            });
            hit_events.push(HitEvent {
                time_seconds: start_seconds,
                x_px: note_x + key.width() * 0.5,
            });
        }

        // `notes` was already sorted ascending by `note.start` above, so `hit_events` (pushed in
        // the same order) is too — `effects::EffectsRenderer::update` relies on that ordering to
        // binary-search the slice of events crossed since the last update.
        if let Some(loaded) = self.loaded.as_mut() {
            loaded.hit_events = hit_events;
        }

        self.pipeline.prepare(gpu.device, gpu.queue);
    }

    /// Advances the waterfall to `time_seconds`, expected to already have the sync offset
    /// subtracted by the caller.
    pub fn update(&mut self, queue: &wgpu::Queue, time_seconds: f32) {
        if self.loaded.is_some() {
            self.pipeline.update_time(queue, time_seconds);
        }
    }

    /// Renders the note highway, clipping notes at the barrier via a real scissor rect (no
    /// viewport remapping needed now that `barrier_fraction` is a shader uniform).
    pub fn render<'rpass>(&'rpass self, render_pass: &mut wgpu::RenderPass<'rpass>) {
        if self.loaded.is_none() {
            return;
        }
        let (canvas_w, canvas_h) = self.canvas_size;
        if canvas_w > 0.0 && canvas_h > 0.0 {
            let scissor_height = (canvas_h * self.barrier_fraction)
                .round()
                .clamp(1.0, canvas_h) as u32;
            render_pass.set_scissor_rect(0, 0, canvas_w.round().max(1.0) as u32, scissor_height);
        }
        self.pipeline.render(render_pass);
    }
}

/// Builds a keyboard layout sized to fit between the calibrated left/right bounds rather than
/// the full window width. `KeyboardLayout::from_range` always starts its keys at local x=0, so
/// the calibrated left bound is applied afterward, added directly into each instance's position.
fn keyboard_layout(
    (width, height): (f32, f32),
    calibration: &KeyboardCalibration,
) -> KeyboardLayout {
    let range = KeyboardRange::standard_88_keys();
    let keyboard_width =
        (width * (calibration.right_fraction - calibration.left_fraction)).max(1.0);
    let neutral_width = keyboard_width / range.white_count() as f32;
    let neutral_height = height * 0.2;
    KeyboardLayout::from_range(Sizing::new(neutral_width, neutral_height), range)
}

/// Resolves a `Fill` to its (top, bottom) base colors — shared by the white-key fill and, since
/// Phase F, `BlackKeyFill::Custom`'s independently resolved fill.
fn resolve_fill_base(fill: &Fill) -> ([u8; 3], [u8; 3]) {
    match fill {
        Fill::Solid(binding) => {
            let color = binding.resolve_constant();
            (color, color)
        }
        Fill::VerticalGradient { top, bottom } => {
            (top.resolve_constant(), bottom.resolve_constant())
        }
    }
}

fn darken(color: [u8; 3], factor: f32) -> [u8; 3] {
    [
        (color[0] as f32 * factor) as u8,
        (color[1] as f32 * factor) as u8,
        (color[2] as f32 * factor) as u8,
    ]
}

/// sRGB u8 -> linear f32, matching the conversion Neothesia's own vendored `Color` type applied
/// before uploading note colors (credit: https://github.com/hecrj, via `wgpu_jumpstart::Color`).
fn color_to_linear([r, g, b]: [u8; 3]) -> [f32; 3] {
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
