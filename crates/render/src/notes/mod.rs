//! Loads a MIDI file and renders its falling-notes "note highway" above the video quad.
//!
//! Vendored in-tree with its own shader, pipeline, and instance-building loop. Owning the shader
//! keeps `barrier_fraction` as a real uniform instead of a viewport/scissor remap.
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
use project::{
    AddedNote, BlackKeyFill, Fill, KeyboardCalibration, NoteDurationEdit, NoteLayer, SkippedNote,
};

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

/// One note's full window at the barrier: arrival/departure time (sync-offset-subtracted
/// transport seconds, matching `note_start_times`'s convention) and the x-span of its key —
/// everything `render::effects::EffectsRenderer` needs to spawn a one-shot burst/flash at
/// arrival *or* to sample "is a note sustained right now" for continuous particles/sustained
/// flash. Recomputed alongside the note instances themselves in `rebuild_instances`, since the x
/// position depends on the same calibrated keyboard layout the instances are built from.
#[derive(Debug, Clone, Copy)]
pub struct NoteInterval {
    pub start_seconds: f32,
    pub end_seconds: f32,
    pub x_left: f32,
    pub x_right: f32,
}

impl NoteInterval {
    pub fn x_center(&self) -> f32 {
        (self.x_left + self.x_right) * 0.5
    }
}

/// One note's full identity (track/channel/number) plus its start/end time in seconds —
/// everything the keyboard tab's note editor (`ui::draw_note_editor`) needs to list "what's
/// playing right now" and to build a `project::SkippedNote`/`project::NoteDurationEdit` key to
/// delete, restore, or re-time it. Unlike `NoteInterval` (x-position, no note number — used by
/// particle/flash effects), this carries the identifying fields and drops the x-span, since the
/// editor has no use for layout. Built for every in-range/non-drum note (both MIDI-derived and
/// `project::AddedNote`s) regardless of `skipped` status (unlike `NoteInterval`/the rendered
/// `NoteInstance`s, which only exist for non-skipped notes) so the editor can still list a skipped
/// note with a restore option.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActiveNote {
    pub track_id: usize,
    pub channel: u8,
    pub note: u8,
    pub start_seconds: f64,
    /// Effective end time — `start_seconds + duration_edits`'s override if one applies, otherwise
    /// the MIDI-parsed duration (both subject to the same 0.1s floor `NoteInstance`'s on-screen
    /// size uses, so this always matches what's actually visible on the highway).
    pub end_seconds: f64,
    /// Whether this note is currently in the project's `skipped_notes` list (excluded from the
    /// note highway and playback). Always `false` for an added note — there's no MIDI original to
    /// exclude in favor of; deleting an added note removes it from `added_notes` outright instead.
    pub skipped: bool,
    /// Whether a `project::NoteDurationEdit` currently overrides this note's duration. Always
    /// `false` for an added note (its duration lives directly on `project::AddedNote`, no separate
    /// override record needed).
    pub edited: bool,
    /// The duration this note would have with no `NoteDurationEdit` applied (same 0.1s floor as
    /// `end_seconds - start_seconds`) — lets the note editor tell whether a value the user just
    /// typed/dragged actually differs from the un-overridden original, regardless of whether an
    /// override is *currently* active (so dragging an already-edited note back to its original
    /// value cleanly drops the override instead of storing a redundant, if harmless, one). Equal
    /// to `end_seconds - start_seconds` when `edited` is `false`. Meaningless (mirrors
    /// `end_seconds - start_seconds`) for an added note, which has no "original" to speak of.
    pub original_duration_seconds: f64,
    /// `Some(id)` if this is a `project::AddedNote` rather than a note parsed from the loaded
    /// `.mid` file — carries its `AddedNote::id` so the editor can mutate/delete the right entry.
    pub added_note_id: Option<u64>,
}

/// Sentinel `track_id` given to a `NoteSource` built from a `project::AddedNote` rather than a
/// real MIDI track — real track ids are always small (an index into the parsed file's track
/// list), so this can never collide with one. Lets `ActiveNote::track_id`/`NoteInstance::
/// track_index` keep working unchanged for added notes without a separate "is this real" enum.
const ADDED_NOTE_TRACK_ID: usize = usize::MAX;

/// Normalized view of one note to render, built fresh each `rebuild_instances` call from either a
/// MIDI-parsed `midi_file::MidiNote` (with any `NoteDurationEdit` override already resolved) or a
/// `project::AddedNote` — everything downstream (sorting, key lookup, instance/interval/
/// active-note building) operates on this single shape so both sources are treated identically
/// except where `added_id` specifically matters (skip/restore has no meaning for an added note).
struct NoteSource {
    track_id: usize,
    channel: u8,
    note: u8,
    /// Full `f64` precision (unlike the `f32` everything else here uses) so identity comparisons
    /// against `SkippedNote`/`NoteDurationEdit::start_seconds` — themselves built from an
    /// `ActiveNote::start_seconds` that round-trips through this same field — never lose bits to
    /// an f64->f32->f64 narrow/widen, matching the exact-equality matching `SkippedNote` used
    /// before `NoteSource` existed.
    start_seconds: f64,
    duration_seconds: f32,
    /// The MIDI-parsed duration before any `NoteDurationEdit` override (equal to
    /// `duration_seconds` for an added note, which has no override concept). See
    /// `ActiveNote::original_duration_seconds`.
    original_duration_seconds: f32,
    velocity: u8,
    added_id: Option<u64>,
}

struct Loaded {
    midi: MidiFile,
    /// Sorted note start times in seconds (MIDI-derived and added notes both), rebuilt alongside
    /// `note_intervals`/`active_notes` in `rebuild_instances` — used by the bottom timeline's
    /// note-density strip (`ui::draw_timeline`), which just needs raw onset times, not the full
    /// per-track structure. Rebuilding it on every `resize` (not just `load`) is what makes a note
    /// added from the note editor show up in the density strip without reloading the MIDI file.
    note_starts: Vec<f32>,
    /// Sorted (ascending start time, matching `note_starts`) intervals for the notes actually
    /// drawn (in-range, non-drum, non-skipped) — see `NoteInterval`.
    note_intervals: Vec<NoteInterval>,
    /// Same in-range/non-drum filter and ordering as `note_intervals`, built alongside it in
    /// `rebuild_instances` — see `ActiveNote`. Unlike `note_intervals`, this also includes
    /// skipped notes (flagged via `ActiveNote::skipped`), since the note editor needs to offer a
    /// restore option for them. Kept as a separate vec (rather than folding fields into
    /// `NoteInterval`) since `NoteInterval` is on the hot path for every particle/flash frame
    /// update and callers there have no use for the identifying fields.
    active_notes: Vec<ActiveNote>,
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

    /// Sorted note intervals (start/end time + key x-span), or an empty slice if nothing is
    /// loaded — used by `render::effects::EffectsRenderer` to spawn particles/flashes as the
    /// transport crosses each note's arrival time, and to sample which notes are currently held
    /// for continuous particle emission / sustained flash.
    pub fn note_intervals(&self) -> &[NoteInterval] {
        self.loaded
            .as_ref()
            .map(|l| l.note_intervals.as_slice())
            .unwrap_or(&[])
    }

    /// In-range, non-drum notes (both skipped and not — see `ActiveNote::skipped`) whose
    /// `[start, end]` window contains `time_seconds`; empty if nothing is loaded. Used by the
    /// keyboard tab's note editor to list what's playing at the current frame, including already-
    /// skipped notes so they can be offered a restore option. Linear scan over `active_notes`,
    /// same pattern as `effects::EffectsRenderer::update`'s "currently held" check — cheap enough
    /// to call once per UI redraw.
    pub fn notes_at(&self, time_seconds: f32) -> Vec<ActiveNote> {
        let Some(loaded) = self.loaded.as_ref() else {
            return Vec::new();
        };
        loaded
            .active_notes
            .iter()
            .copied()
            .filter(|note| {
                note.start_seconds as f32 <= time_seconds && time_seconds <= note.end_seconds as f32
            })
            .collect()
    }

    /// Parses `path` as a MIDI file and (re)builds the note instances for it. `skipped` excludes
    /// specific note occurrences (see `project::SkippedNote`) from both the rendered instances and
    /// `notes_at`, without touching the file itself; `duration_edits` overrides specific notes'
    /// rendered/playback duration (see `project::NoteDurationEdit`); `added_notes` are extra notes
    /// (see `project::AddedNote`) rendered/played back alongside the parsed ones.
    #[allow(clippy::too_many_arguments)]
    pub fn load(
        &mut self,
        gpu: &GpuHandles,
        viewport: (f32, f32),
        calibration: &KeyboardCalibration,
        note_layer: &NoteLayer,
        path: &Path,
        skipped: &[SkippedNote],
        duration_edits: &[NoteDurationEdit],
        added_notes: &[AddedNote],
    ) -> Result<(), String> {
        let midi = MidiFile::new(path)?;
        self.loaded = Some(Loaded {
            midi,
            note_starts: Vec::new(),
            note_intervals: Vec::new(),
            active_notes: Vec::new(),
        });
        self.apply_view(
            gpu,
            viewport,
            calibration,
            note_layer,
            skipped,
            duration_edits,
            added_notes,
        );
        Ok(())
    }

    /// Recomputes the projection and note-lane layout for a new viewport size, calibration, note
    /// layer (fill/sheen/glow/roundedness/fall_speed), skipped-notes set, duration-edit set, or
    /// added-notes set.
    #[allow(clippy::too_many_arguments)]
    pub fn resize(
        &mut self,
        gpu: &GpuHandles,
        viewport: (f32, f32),
        calibration: &KeyboardCalibration,
        note_layer: &NoteLayer,
        skipped: &[SkippedNote],
        duration_edits: &[NoteDurationEdit],
        added_notes: &[AddedNote],
    ) {
        self.apply_view(
            gpu,
            viewport,
            calibration,
            note_layer,
            skipped,
            duration_edits,
            added_notes,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_view(
        &mut self,
        gpu: &GpuHandles,
        viewport: (f32, f32),
        calibration: &KeyboardCalibration,
        note_layer: &NoteLayer,
        skipped: &[SkippedNote],
        duration_edits: &[NoteDurationEdit],
        added_notes: &[AddedNote],
    ) {
        let barrier_fraction = calibration.barrier_fraction.clamp(0.05, 1.0);
        self.pipeline
            .set_view(gpu.queue, viewport.0, viewport.1, barrier_fraction);
        self.pipeline
            .set_speed(gpu.queue, note_layer.fall_speed.max(1.0));
        self.pipeline.set_style(gpu.queue, note_layer);
        self.canvas_size = viewport;
        self.barrier_fraction = barrier_fraction;
        self.rebuild_instances(
            gpu,
            viewport,
            calibration,
            note_layer,
            skipped,
            duration_edits,
            added_notes,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn rebuild_instances(
        &mut self,
        gpu: &GpuHandles,
        viewport: (f32, f32),
        calibration: &KeyboardCalibration,
        note_layer: &NoteLayer,
        skipped: &[SkippedNote],
        duration_edits: &[NoteDurationEdit],
        added_notes: &[AddedNote],
    ) {
        self.pipeline.clear();
        let Some(loaded) = self.loaded.as_ref() else {
            return;
        };

        let range = KeyboardRange::standard_88_keys();
        let range_start = range.start() as usize;
        let keys = keyboard_layout(viewport, calibration);
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

        // Merge MIDI-parsed notes (with any `duration_edits` override already applied) and
        // `added_notes` into one normalized list before the instance-building loop below, so both
        // sources go through identical range/drum filtering, sorting, and rendering — an added
        // note behaves exactly like a real one everywhere except skip/restore (see `NoteSource`'s
        // `added_id`, checked below).
        let mut notes: Vec<NoteSource> = loaded
            .midi
            .tracks
            .iter()
            .flat_map(|track| track.notes.iter())
            .filter(|note| range.contains(note.note) && note.channel != 9)
            .map(|note| {
                let start_seconds = note.start.as_secs_f64();
                let original_duration_seconds = note.duration.as_secs_f32();
                let duration_seconds = duration_edits
                    .iter()
                    .find(|edit| {
                        edit.track_id == note.track_id
                            && edit.channel == note.channel
                            && edit.note == note.note
                            && edit.start_seconds == start_seconds
                    })
                    .map(|edit| edit.new_duration_seconds as f32)
                    .unwrap_or(original_duration_seconds);
                NoteSource {
                    track_id: note.track_id,
                    channel: note.channel,
                    note: note.note,
                    start_seconds,
                    duration_seconds,
                    original_duration_seconds,
                    velocity: note.velocity,
                    added_id: None,
                }
            })
            .chain(
                added_notes
                    .iter()
                    .filter(|added| range.contains(added.note) && added.channel != 9)
                    .map(|added| NoteSource {
                        track_id: ADDED_NOTE_TRACK_ID,
                        channel: added.channel,
                        note: added.note,
                        start_seconds: added.start_seconds,
                        duration_seconds: added.duration_seconds as f32,
                        original_duration_seconds: added.duration_seconds as f32,
                        velocity: added.velocity,
                        added_id: Some(added.id),
                    }),
            )
            .collect();
        // Render newer notes on top of older ones, matching Neothesia's own convention.
        notes.sort_by(|a, b| a.start_seconds.total_cmp(&b.start_seconds));

        let mut note_starts = Vec::with_capacity(notes.len());
        let mut note_intervals = Vec::with_capacity(notes.len());
        let mut active_notes = Vec::with_capacity(notes.len());
        let instances = self.pipeline.instances();
        for note in &notes {
            // An added note has no MIDI original to skip/restore — deleting it removes it from
            // `added_notes` outright instead (see `ActiveNote::added_note_id`'s doc comment).
            let is_skipped = note.added_id.is_none()
                && skipped.iter().any(|skip| {
                    skip.track_id == note.track_id
                        && skip.channel == note.channel
                        && skip.note == note.note
                        && skip.start_seconds == note.start_seconds
                });
            let is_edited = note.added_id.is_none()
                && duration_edits.iter().any(|edit| {
                    edit.track_id == note.track_id
                        && edit.channel == note.channel
                        && edit.note == note.note
                        && edit.start_seconds == note.start_seconds
                });

            let key = &keys[note.note as usize - range_start];
            let (color_top, color_bottom) = if key.is_sharp {
                (top_dark, bottom_dark)
            } else {
                (top_light, bottom_light)
            };
            let duration = note.duration_seconds.max(0.1);
            let original_duration = note.original_duration_seconds.max(0.1);
            let note_x = key.x + left_x;
            let start_seconds = note.start_seconds as f32;

            note_starts.push(start_seconds);

            // A skipped note gets neither a rendered instance nor a `NoteInterval` (so it never
            // draws on the highway and never triggers barrier/particle effects) — but it still
            // gets an `ActiveNote` below, so the note editor can list it (with a restore icon)
            // even while it's excluded from playback.
            if !is_skipped {
                instances.push(NoteInstance {
                    position: [note_x, start_seconds],
                    size: [key.width - 1.0, duration - 0.01],
                    color_top,
                    color_bottom,
                    radius: key.width * 0.2 * note_layer.roundedness,
                    velocity: note.velocity as f32 / 127.0,
                    track_index: note.track_id as f32,
                });
                note_intervals.push(NoteInterval {
                    start_seconds,
                    end_seconds: start_seconds + duration,
                    x_left: note_x,
                    x_right: note_x + key.width,
                });
            }
            active_notes.push(ActiveNote {
                track_id: note.track_id,
                channel: note.channel,
                note: note.note,
                start_seconds: note.start_seconds,
                // Match the rendered note duration floor so the editor's active-note window
                // matches what is visible. See `docs/implementation-notes.md`.
                end_seconds: (start_seconds + duration) as f64,
                skipped: is_skipped,
                edited: is_edited,
                original_duration_seconds: original_duration as f64,
                added_note_id: note.added_id,
            });
        }

        // `notes` was already sorted ascending by start time above, so `note_intervals` (pushed
        // in the same order) is too — `effects::EffectsRenderer::update` relies on that ordering
        // to binary-search the slice of events crossed since the last update.
        note_starts.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if let Some(loaded) = self.loaded.as_mut() {
            loaded.note_starts = note_starts;
            loaded.note_intervals = note_intervals;
            loaded.active_notes = active_notes;
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

/// MIDI note numbers of the interior octave boundaries of a standard 88-key keyboard (A0=21 ..
/// C8=108) — the left edge of C1 through C8, i.e. every multiple of 12 within that range. Along
/// with the calibrated left/right edges, these bound the 9 independent layout segments (a partial
/// A0-B0 segment, seven full octaves C1-B1..C7-B7, and a final C8-alone segment) that
/// `keyboard_layout` lays out one at a time, so a camera-stretch calibration can give each octave
/// a different real width.
const OCTAVE_BOUNDARY_NOTES: [u8; 8] = [24, 36, 48, 60, 72, 84, 96, 108];

/// A laid-out key's absolute position, local to `calibration.left_fraction` (0.0 at the left
/// calibration edge, growing rightward) — replaces `piano_layout::Key` as the per-note lookup
/// table `rebuild_instances` indexes into. `Key`'s fields are private (getter-only), so once each
/// octave segment is laid out by its own `KeyboardLayout::from_range` call there's no way to
/// re-offset a `Key` from outside that crate; this plain struct is built instead.
#[derive(Debug, Clone, Copy)]
struct LayoutKey {
    x: f32,
    width: f32,
    is_sharp: bool,
}

/// The 10 fractions (of canvas width) bounding the 9 octave segments, in order:
/// `calibration.left_fraction`, the 8 interior C-boundaries, then `calibration.right_fraction`.
/// Without a camera-stretch calibration (`calibration.stretch == None`) the interior boundaries
/// are placed proportionally to cumulative white-key count, which reproduces the pre-stretch
/// single-`neutral_width` uniform layout exactly (every octave the same real width) — so
/// `keyboard_layout` only needs one code path for both the calibrated and uncalibrated case.
fn octave_boundary_fractions(calibration: &KeyboardCalibration) -> [f32; 10] {
    let mut bounds = [0.0; 10];
    bounds[0] = calibration.left_fraction;
    bounds[9] = calibration.right_fraction;
    match calibration.stretch {
        Some(stretch) => bounds[1..9].copy_from_slice(&stretch.c_fractions),
        None => {
            let total_white = KeyboardRange::standard_88_keys().white_count() as f32;
            for (i, &boundary_note) in OCTAVE_BOUNDARY_NOTES.iter().enumerate() {
                let cumulative_white = KeyboardRange::new(21..boundary_note).white_count() as f32;
                bounds[i + 1] = calibration.left_fraction
                    + (calibration.right_fraction - calibration.left_fraction) * cumulative_white
                        / total_white;
            }
        }
    }
    bounds
}

/// Lays out all 88 keys as 9 independent segments (see `OCTAVE_BOUNDARY_NOTES`), each scaled to
/// fit exactly between its two `octave_boundary_fractions`, instead of one `KeyboardLayout` with
/// a single global `neutral_width`. This is what lets a camera-stretch calibration give different
/// octaves different real widths while keeping each octave's own internal key proportions (from
/// the vendored `piano_layout::Octave`) intact — the width fed to `Sizing::new` for a segment is
/// derived from its target pixel width divided by its own white-key count, so `Octave`'s existing
/// per-key math (unmodified) produces correctly-scaled keys for that segment alone. Returned keys
/// are ordered by MIDI note number (index 0 = A0/note 21), local to `calibration.left_fraction`
/// like the old single-layout version was.
fn keyboard_layout(
    (width, height): (f32, f32),
    calibration: &KeyboardCalibration,
) -> Vec<LayoutKey> {
    let bounds = octave_boundary_fractions(calibration);
    let neutral_height = height * 0.2;
    let segment_notes: [u8; 10] = [21, 24, 36, 48, 60, 72, 84, 96, 108, 109];

    // One past the real top key (C8 = note 108) — see the `full_range` comment below for why
    // every segment's query always extends out to here rather than stopping at its own end.
    const KEYBOARD_END: u8 = 109;

    let mut keys = Vec::with_capacity(88);
    for i in 0..9 {
        let segment_start = segment_notes[i];
        let segment_len = (segment_notes[i + 1] - segment_start) as usize;
        let segment_left = (bounds[i] - calibration.left_fraction) * width;
        let segment_width = ((bounds[i + 1] - bounds[i]) * width).max(0.1);
        // Counting white keys is a simple range scan (no octave-chunking involved), so the exact,
        // narrow segment range is safe to use here even though the wider `full_range` below has
        // to be used for the actual layout call.
        let white_count = KeyboardRange::new(segment_start..segment_notes[i + 1])
            .white_count()
            .max(1) as f32;
        let neutral_width = segment_width / white_count;
        // Query through the real keyboard end, then discard trailing keys, to avoid a
        // `piano_layout` mid-octave truncation edge case. See `docs/implementation-notes.md`.
        let full_range = KeyboardRange::new(segment_start..KEYBOARD_END);
        let layout =
            KeyboardLayout::from_range(Sizing::new(neutral_width, neutral_height), full_range);
        keys.extend(layout.keys.iter().take(segment_len).map(|key| LayoutKey {
            x: segment_left + key.x(),
            width: key.width(),
            is_sharp: key.kind().is_sharp(),
        }));
    }
    keys
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression test for a real crash: the vendored `piano_layout` crate's octave-chunking
    /// (`split_range_by_octaves`) mis-slices when a queried range starts mid-octave and ends
    /// before that octave completes — exactly the A0-B0 segment (`21..24`) hits this if queried
    /// directly, panicking with "slice index starts at 9 but ends at 3" the moment a MIDI file
    /// was loaded. `keyboard_layout` works around it by always querying through to the real
    /// keyboard end and discarding the extra keys (see its `full_range` comment) — this just
    /// confirms that holds for every segment, with and without a camera-stretch calibration.
    #[test]
    fn keyboard_layout_does_not_panic_and_covers_all_88_keys() {
        let calibration = KeyboardCalibration::default();
        let keys = keyboard_layout((1000.0, 200.0), &calibration);
        assert_eq!(keys.len(), 88);
        for pair in keys.windows(2) {
            assert!(pair[1].x >= pair[0].x, "keys must be left-to-right ordered");
        }
    }

    #[test]
    fn keyboard_layout_with_camera_stretch_does_not_panic() {
        let calibration = KeyboardCalibration {
            stretch: Some(project::CameraStretch {
                c_fractions: [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8],
            }),
            ..KeyboardCalibration::default()
        };
        let keys = keyboard_layout((1000.0, 200.0), &calibration);
        assert_eq!(keys.len(), 88);
        for pair in keys.windows(2) {
            assert!(pair[1].x >= pair[0].x, "keys must be left-to-right ordered");
        }
    }
}
