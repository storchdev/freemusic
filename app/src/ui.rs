pub struct UiState {
    pub playing: bool,
    pub position_seconds: f64,
    pub duration_seconds: f64,
    /// Set by the UI when the user drags/clicks the timeline; the app loop consumes and clears
    /// it.
    pub seek_request: Option<f64>,
    /// True when the pending seek should decode exactly to the requested timestamp instead of
    /// accepting the first post-seek keyframe. Used for discrete jumps, not live drags.
    pub seek_request_exact: bool,
    /// True while a file is being dragged over the window; drives the drop-zone overlay.
    pub dropping: bool,
    pub midi_name: Option<String>,
    /// Sorted note onset times in seconds, mirrored from `Compositor::note_start_times` each
    /// time a MIDI file loads — used by the bottom timeline's note-density strip.
    pub midi_note_times: Vec<f32>,
    /// Peak-amplitude waveform, mirrored from `AudioPlayback::waveform_peaks` each time a video
    /// loads — used by the bottom timeline to draw the audio waveform. Empty if the loaded video
    /// has no audio track.
    pub waveform_peaks: Vec<f32>,
    /// Width in seconds of each `waveform_peaks` entry, mirrored alongside it.
    pub waveform_bucket_seconds: f64,
    /// `midi_time = position_seconds - sync_offset_seconds`; video is always the master clock,
    /// dragging this only moves where notes render relative to it.
    pub sync_offset_seconds: f64,
    pub calibration: project::KeyboardCalibration,
    pub transform: project::VideoTransform,
    pub barrier_style: project::BarrierStyle,
    pub note_style: project::NoteStyle,
    /// Canvas clear color for the legacy (no-imported-style) path — mirrors
    /// `project::Project::background_color`, edited by the Keyboard tab's "Background" picker.
    pub background_color: [u8; 3],
    /// Skip list — notes excluded from rendering/playback, mirroring
    /// `project::Project::skipped_notes`. Edited directly by the note editor's trash/restore
    /// icons (`draw_note_editor`) with no staging/confirmation step; the app loop dirty-checks
    /// this against `applied_skipped_notes` each redraw (same pattern as `calibration`/the
    /// effective `NoteLayer`) and rebuilds the compositor's note instances when it changes.
    pub skipped_notes: Vec<project::SkippedNote>,
    /// Per-note duration overrides, mirroring `project::Project::duration_edits` — same
    /// dirty-check/rebuild pattern as `skipped_notes` (see its doc comment), edited via the note
    /// editor's duration field (`draw_note_editor`).
    pub duration_edits: Vec<project::NoteDurationEdit>,
    /// Notes created from the note editor's "Add note" form, mirroring
    /// `project::Project::added_notes` — same dirty-check/rebuild pattern as `skipped_notes`.
    pub added_notes: Vec<project::AddedNote>,
    /// "Add note" form fields, purely local UI state (not persisted) — the pitch/velocity/
    /// duration to use the next time "Add at current frame" is clicked.
    pub add_note_pitch: u8,
    pub add_note_velocity: u8,
    pub add_note_duration_seconds: f64,
    /// Notes whose window contains the current playback frame (both skipped and not — see
    /// `render::ActiveNote::skipped`), mirrored from `Compositor::notes_at` every redraw (see
    /// `main.rs`'s `update_midi_position`) regardless of whether the keyboard tab is actually
    /// visible — cheap enough not to bother gating it.
    pub notes_now: Vec<render::ActiveNote>,
    /// Path typed into the Save/Load text field; defaulted from the video path on first load.
    pub project_path_text: String,
    /// Set by the Save/Load buttons; the app loop consumes and clears these each redraw.
    pub save_requested: bool,
    pub load_requested: bool,
    /// Set by the Project tab's "Open Video…"/"Open MIDI…" buttons; the app loop consumes and
    /// clears these each redraw, popping a native `rfd` file picker and loading whatever the
    /// user chose (a no-op if they cancel).
    pub open_video_requested: bool,
    pub open_midi_requested: bool,
    /// Set by the File menu's "New Project"/"Open Project…"/"Save Project As…"/"Exit" items (and
    /// mirrored keyboard shortcuts — see `main.rs`'s `KeyboardInput` handling); the app loop
    /// consumes and clears these each redraw, same one-request-flag-per-action pattern as
    /// `save_requested`/`open_video_requested` etc. above.
    pub new_project_requested: bool,
    pub open_project_requested: bool,
    pub save_project_as_requested: bool,
    pub exit_requested: bool,
    /// A fully imported `.fmstyle.ron` look, set by the Project tab's "Import style…" button.
    /// When `Some`, this is the effective style the renderer should use instead of one
    /// synthesized from `barrier_style`/`note_style` (see `project::Style::from_legacy`) — the
    /// Keyboard tab's sliders still edit those legacy fields, but they're overridden while a
    /// style is imported. `None` means "use the legacy sliders", the only state before this
    /// milestone existed.
    pub style: Option<project::Style>,
    /// Set by the "Import style…" button; the app loop consumes and clears it each redraw,
    /// popping a native `rfd` file picker and loading whatever the user chose into `style`.
    pub import_style_requested: bool,
    /// Filesystem path of the last-imported `.fmstyle.ron`, mirrored alongside `style` whenever
    /// it's loaded from a file (`None` for a style embedded directly in a loaded project, since
    /// there's no external file to reload from in that case). Backs the refresh button next to
    /// "Import style…", which reloads from this path so the user can edit the file externally and
    /// re-apply it without reopening the file picker each time.
    pub style_path: Option<std::path::PathBuf>,
    /// Set by the refresh button next to "Import style…"; the app loop consumes and clears it
    /// each redraw, re-loading `style` from `style_path`.
    pub reload_style_requested: bool,
    pub status_message: Option<String>,
    /// Path typed into the Export text field; defaulted from the video path on first load.
    pub export_path_text: String,
    pub export_fps: u32,
    /// Set by the Export/Cancel buttons; the app loop consumes and clears these each redraw.
    pub export_requested: bool,
    pub export_cancel_requested: bool,
    /// `(frames done, total frames)` while an export is running, driven by the app loop
    /// draining `export::Progress` off the background thread's channel; `None` otherwise.
    pub export_progress: Option<(u64, u64)>,
    pub export_message: Option<String>,
    /// Which side-panel tab is currently showing.
    pub active_tab: Tab,
    /// Egui-registered id of the offscreen preview texture `AppState` renders the compositor
    /// into; displayed via `egui::Image` in the central panel.
    pub preview_texture_id: egui::TextureId,
    /// Pixel size of that texture — the compositor's canvas, decoupled from window size (see
    /// CLAUDE.md's milestone 6c notes). Used here only to compute the aspect-fit display rect.
    pub canvas_size: (u32, u32),
    pub timeline_height: f32,
    pub timeline_zoom: f64,
    pub timeline_view_start_seconds: f64,
    /// Whether the left side panel is expanded (tabs + content) or collapsed to a narrow strip
    /// — dragging its edge past its size limits (see `draw_side_panel`) flips this directly via
    /// `egui::Panel::show_switched`'s own `&mut bool`, so this mirrors that rather than driving
    /// it.
    pub side_panel_expanded: bool,
    /// Set by the panel's own «/» collapse/expand button; consumed and cleared the same redraw.
    /// Not applied directly to `side_panel_expanded` at the click site — that field is on loan
    /// to `show_switched` as a plain local `&mut bool` for the duration of the call (so the
    /// panel's own drag-to-collapse/expand can flip it), and the click happens from inside that
    /// same call's content closure, which borrows `UiState` as a whole to reach the tabs below
    /// it. A flag consumed right after the call is the straightforward way to let both write to
    /// the same logical piece of state without an overlapping-borrow conflict.
    pub side_panel_toggle_requested: bool,
    /// State machine for the "Align notes to camera stretch" guided click flow (see
    /// `draw_camera_stretch_overlay`); `None` when not currently calibrating. Entirely
    /// self-contained within `ui::draw` (unlike the `_requested` flags above, the app loop never
    /// looks at this) — it only ever writes into `calibration` once finished.
    pub camera_stretch_capture: Option<CameraStretchCapture>,
}

/// The points recorded so far during a "Align notes to camera stretch" run, in click order: the
/// left edge of A0, then the left edge of each of C1..C8, then the right edge of C8 (see the doc
/// comment on `project::CameraStretch` for why exactly these 10 points). `points.len()` is always
/// less than `CAMERA_STRETCH_POINT_LABELS.len()` while capturing — once the last point lands,
/// `finalize_camera_stretch` consumes this and resets it to `None`.
#[derive(Debug, Clone, Default)]
pub struct CameraStretchCapture {
    pub points: Vec<f32>,
}

/// Labels for each of the 10 points a camera-stretch calibration run asks for, in click order —
/// see `CameraStretchCapture`.
const CAMERA_STRETCH_POINT_LABELS: [&str; 10] = [
    "left edge of A0 (leftmost key)",
    "left edge of C1",
    "left edge of C2",
    "left edge of C3",
    "left edge of C4",
    "left edge of C5",
    "left edge of C6",
    "left edge of C7",
    "left edge of C8",
    "right edge of C8 (rightmost key)",
];

/// Short tags for the same 10 points, used to label each already-clicked point's guide line on
/// the preview during capture (see `draw_camera_stretch_overlay`) — `CAMERA_STRETCH_POINT_LABELS`
/// is too long to fit next to a thin vertical line.
const CAMERA_STRETCH_POINT_TAGS: [&str; 10] =
    ["A0", "C1", "C2", "C3", "C4", "C5", "C6", "C7", "C8", "end"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Project,
    Keyboard,
    Transform,
    Export,
}

pub fn draw(ui: &mut egui::Ui, state: &mut UiState) {
    draw_side_panel(ui, state);
    draw_timeline_panel(ui, state);

    egui::CentralPanel::default().show(ui, |ui| {
        let image_rect = fit_rect(ui.available_rect_before_wrap(), canvas_aspect(state));
        ui.put(
            image_rect,
            egui::Image::new((state.preview_texture_id, image_rect.size()))
                .fit_to_exact_size(image_rect.size()),
        );
        if state.camera_stretch_capture.is_some() {
            draw_camera_stretch_overlay(ui, image_rect, state);
        } else {
            draw_calibration_handles(ui, image_rect, &mut state.calibration);
            draw_barrier_handle(ui, image_rect, &mut state.calibration);
            draw_camera_stretch_handles(ui, image_rect, &mut state.calibration);
        }
    });

    clamp_camera_stretch(&mut state.calibration);

    if state.dropping {
        draw_drop_overlay(ui, ui.max_rect());
    }
}

fn canvas_aspect(state: &UiState) -> f32 {
    let (w, h) = state.canvas_size;
    if h == 0 {
        1.0
    } else {
        w as f32 / h as f32
    }
}

/// Centers a rect of the given aspect ratio inside `container`, contain-fit (letterboxed, never
/// cropped) — this is the rect the preview image is drawn into, and the rect all the
/// calibration/crop drag handles hit-test and draw against (replacing the old assumption that
/// the video filled the whole window, back when it was painted directly onto the swapchain).
fn fit_rect(container: egui::Rect, aspect: f32) -> egui::Rect {
    let container_w = container.width().max(1.0);
    let container_h = container.height().max(1.0);
    let container_aspect = container_w / container_h;
    let size = if aspect > container_aspect {
        egui::vec2(container_w, container_w / aspect)
    } else {
        egui::vec2(container_h * aspect, container_h)
    };
    egui::Rect::from_center_size(container.center(), size)
}

/// Width range (collapsed) of the side panel once collapsed to a narrow strip — just enough to
/// hold the expand button and give something to grab for a drag-to-expand gesture.
const SIDE_PANEL_COLLAPSED_WIDTH: f32 = 28.0;
const SIDE_PANEL_COLLAPSED_MAX_WIDTH: f32 = 56.0;

/// Hand-rolled tab strip + content for the persistent side panel — replaces the four floating
/// windows milestones 3-5 used (Sync & Project / Video Transform / Export, plus a keyboard
/// calibration readout folded into the first). A real `egui::SidePanel`-equivalent (`egui::
/// Panel::left`) only became viable once the video moved to a shrinkable `egui::Image` in the
/// central panel instead of being painted directly under floating windows — see CLAUDE.md.
///
/// Collapsible via `egui::Panel::show_switched`, so the video/timeline can reclaim its width:
/// dragging the expanded panel's edge past its `min_size` collapses it to a narrow strip, and
/// dragging that strip's edge past its own `max_size` expands it back — one shared resize-handle
/// widget under the hood handles both directions of that drag. The «/» buttons are a
/// discoverable alternative to the drag, not a replacement for it.
fn draw_side_panel(ui: &mut egui::Ui, state: &mut UiState) {
    let mut expanded = state.side_panel_expanded;

    let collapsed_panel = egui::Panel::left("side_panel_collapsed")
        .resizable(true)
        .min_size(SIDE_PANEL_COLLAPSED_WIDTH)
        .max_size(SIDE_PANEL_COLLAPSED_MAX_WIDTH)
        .default_size(SIDE_PANEL_COLLAPSED_WIDTH);
    let expanded_panel = egui::Panel::left("side_panel")
        .resizable(true)
        .default_size(280.0)
        .min_size(220.0)
        .max_size(420.0);

    egui::Panel::show_switched(
        ui,
        &mut expanded,
        collapsed_panel,
        expanded_panel,
        |ui, is_expanded| {
            ui.add_space(4.0);
            if is_expanded {
                ui.horizontal(|ui| {
                    tab_button(ui, state, Tab::Project, "Project");
                    tab_button(ui, state, Tab::Keyboard, "Keyboard");
                    tab_button(ui, state, Tab::Transform, "Transform");
                    tab_button(ui, state, Tab::Export, "Export");
                    if ui.button("«").on_hover_text("Collapse panel").clicked() {
                        state.side_panel_toggle_requested = true;
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match state.active_tab {
                        Tab::Project => draw_project_tab(ui, state),
                        Tab::Keyboard => draw_keyboard_tab(ui, state),
                        Tab::Transform => draw_transform_tab(ui, &mut state.transform),
                        Tab::Export => draw_export_tab(ui, state),
                    });
            } else if ui.button("»").on_hover_text("Expand panel").clicked() {
                state.side_panel_toggle_requested = true;
            }
        },
    );

    if state.side_panel_toggle_requested {
        expanded = !expanded;
        state.side_panel_toggle_requested = false;
    }
    state.side_panel_expanded = expanded;
}

fn tab_button(ui: &mut egui::Ui, state: &mut UiState, tab: Tab, label: &str) {
    if ui
        .selectable_label(state.active_tab == tab, label)
        .clicked()
    {
        state.active_tab = tab;
    }
}

/// Draws a slider whose typed edits are only validated when the edit commits (Enter pressed, or
/// focus leaves the field), rather than on every keystroke. Plain `egui::Slider` defaults to
/// `SliderClamping::Always` with `update_while_editing(true)`, which force-clamps the field to
/// `range` as soon as each keystroke parses to a number — so typing e.g. "15" into a 0.0..=2.0
/// field snaps to "2" after the "1", making it impossible to type past the bound even
/// transiently. Here, `update_while_editing(false)` leaves the underlying value untouched while
/// the field has focus (only the in-progress text buffer changes), and `SliderClamping::Never`
/// means the eventual commit isn't silently clamped either — instead, once the edit commits, an
/// out-of-range result is reverted to whatever the field held before this edit began, rather than
/// snapped to the nearest bound. Dragging the slider handle itself is unaffected (egui always
/// keeps that within `range`).
fn validated_slider(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    decimals: Option<usize>,
) -> egui::Response {
    let previous = *value;
    let mut slider = egui::Slider::new(value, range.clone())
        .clamping(egui::SliderClamping::Never)
        .update_while_editing(false);
    if let Some(decimals) = decimals {
        slider = slider.min_decimals(decimals).max_decimals(decimals);
    }
    let response = ui.add(slider);
    if response.lost_focus() && !range.contains(&*value) {
        *value = previous;
    }
    response
}

/// Darkens an sRGB u8 color by `factor` — matches `render::notes`' own sharp-key darkening, used
/// here only to seed a sensible starting color when switching the black-key mode to `Custom`.
fn darken_color(color: [u8; 3], factor: f32) -> [u8; 3] {
    [
        (color[0] as f32 * factor) as u8,
        (color[1] as f32 * factor) as u8,
        (color[2] as f32 * factor) as u8,
    ]
}

/// Minimum gap kept between the left/right calibration handles, as a fraction of the preview
/// image width — keeps them from being dragged past each other into a zero/negative-width
/// keyboard.
const CALIBRATION_MIN_GAP: f32 = 0.05;

/// MIDI note number to scientific-pitch-notation name (note 60 = "C4", 21 = "A0", 108 = "C8" —
/// matches `piano_layout::KeyboardRange::standard_88_keys`'s own A0..C8 naming).
fn note_name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let octave = note as i32 / 12 - 1;
    format!("{}{octave}", NAMES[note as usize % 12])
}

/// Turns an `render::ActiveNote` (identity + timing, no display concerns) into the
/// `project::SkippedNote` key that names it in the persisted skip list — same fields, just
/// crossing the crate boundary. Only meaningful for a MIDI-derived note (`added_note_id.is_none()`
/// — callers only reach for this after already checking that).
fn skipped_note_key(note: &render::ActiveNote) -> project::SkippedNote {
    project::SkippedNote {
        track_id: note.track_id,
        channel: note.channel,
        note: note.note,
        start_seconds: note.start_seconds,
        end_seconds: note.end_seconds,
    }
}

/// Identity portion shared by `project::SkippedNote`/`project::NoteDurationEdit` — used to match
/// an existing `duration_edits` entry against a given `ActiveNote` regardless of which of the two
/// key structs it's stored as.
fn duration_edit_matches(edit: &project::NoteDurationEdit, note: &render::ActiveNote) -> bool {
    edit.track_id == note.track_id
        && edit.channel == note.channel
        && edit.note == note.note
        && edit.start_seconds == note.start_seconds
}

/// Applies a new duration typed/dragged into the note editor's duration field for `note`: for an
/// added note, mutates `project::AddedNote::duration_seconds` directly (it has no separate
/// override record — the duration *is* its own data); for a MIDI-derived note, replaces (or
/// removes, if the new value matches the original) its `duration_edits` entry. Never touches the
/// loaded `.mid` file either way.
fn apply_duration_edit(state: &mut UiState, note: &render::ActiveNote, new_duration_seconds: f64) {
    let new_duration_seconds = new_duration_seconds.max(0.02);
    if let Some(id) = note.added_note_id {
        if let Some(added) = state.added_notes.iter_mut().find(|added| added.id == id) {
            added.duration_seconds = new_duration_seconds;
        }
        return;
    }
    state
        .duration_edits
        .retain(|edit| !duration_edit_matches(edit, note));
    if (new_duration_seconds - note.original_duration_seconds).abs() > 1e-6 {
        state.duration_edits.push(project::NoteDurationEdit {
            track_id: note.track_id,
            channel: note.channel,
            note: note.note,
            start_seconds: note.start_seconds,
            new_duration_seconds,
        });
    }
}

/// Fixed pixel height of the note editor's currently-playing table (see `draw_note_editor`) —
/// kept constant regardless of how many notes are playing so the section doesn't grow/shrink
/// every frame as notes start and stop; a longer list scrolls within this instead.
const NOTE_EDITOR_TABLE_HEIGHT: f32 = 160.0;

/// Lists every note (both skipped and not — see `render::ActiveNote::skipped`) whose window
/// contains the current playback frame — both MIDI-derived notes and notes created via the "Add
/// note" form below (`render::ActiveNote::added_note_id`) — with an editable duration field and an
/// action icon: 🗑 immediately excludes a playing MIDI-derived note (straight into
/// `state.skipped_notes`, no staging queue or confirmation step — since ♻ is always right there to
/// undo it, a confirmation step would just be a second click for no added safety), ♻ restores an
/// already-skipped one, ↺ resets an edited MIDI-derived note's duration back to its original, and
/// 🗑 on an added note removes it from `state.added_notes` outright (there's no "original" to
/// restore to, so no ♻ for these). No row ever disappears when its action button is clicked —
/// only its icon (and dimmed text, for a skipped note) flips — so every note at the current frame
/// always shows as exactly one clear state. Nothing here ever touches the loaded `.mid` file on
/// disk; every edit lives only in the project's own save file (see
/// `render::notes::rebuild_instances`).
fn draw_note_editor(ui: &mut egui::Ui, state: &mut UiState) {
    ui.heading("Note editor");
    ui.label(
        "Notes at this frame. Drag the duration to change how long a note lasts, 🗑 excludes/\
        removes a note from the note highway and playback wherever this project is opened or \
        exported, ♻ restores an excluded note, ↺ resets an edited duration. Nothing here ever \
        modifies your original MIDI file.",
    );
    ui.add_space(4.0);

    // Fixed height regardless of how many notes are currently playing — `auto_shrink` off keeps
    // this from collapsing to fit shorter content, which is what made the section constantly jump
    // as notes started/stopped during playback. Once there are more rows than fit, this scrolls
    // instead of growing.
    egui::ScrollArea::vertical()
        .max_height(NOTE_EDITOR_TABLE_HEIGHT)
        .auto_shrink([false, false])
        .id_salt("note_editor_scroll")
        .show(ui, |ui| {
            if state.notes_now.is_empty() {
                ui.label("No notes at the current frame.");
                return;
            }
            egui::Grid::new("note_editor_grid")
                .num_columns(4)
                .striped(true)
                .show(ui, |ui| {
                    ui.label("Note");
                    ui.label("Duration");
                    ui.label("");
                    ui.label("");
                    ui.end_row();

                    // `state.notes_now` comes back ordered by start time (see
                    // `render::notes::NotesRenderer::notes_at`); re-sort by pitch so the table
                    // reads like a keyboard, lowest note first.
                    let mut notes_now = state.notes_now.clone();
                    notes_now.sort_by_key(|note| note.note);
                    for note in &notes_now {
                        let dim = ui.visuals().weak_text_color();
                        if note.skipped {
                            ui.colored_label(dim, note_name(note.note));
                        } else {
                            ui.label(note_name(note.note));
                        }

                        let mut duration_seconds = note.end_seconds - note.start_seconds;
                        let duration_response = ui.add(
                            egui::DragValue::new(&mut duration_seconds)
                                .speed(0.01)
                                .range(0.02..=60.0)
                                .suffix("s"),
                        );
                        if duration_response.changed() {
                            apply_duration_edit(state, note, duration_seconds);
                        }

                        if note.edited {
                            if ui
                                .button("↺")
                                .on_hover_text("Reset to original duration")
                                .clicked()
                            {
                                state
                                    .duration_edits
                                    .retain(|edit| !duration_edit_matches(edit, note));
                            }
                        } else {
                            ui.label("");
                        }

                        if let Some(id) = note.added_note_id {
                            if ui.button("🗑").on_hover_text("Remove added note").clicked() {
                                state.added_notes.retain(|added| added.id != id);
                            }
                        } else if note.skipped {
                            if ui.button("♻").on_hover_text("Restore").clicked() {
                                let key = skipped_note_key(note);
                                state.skipped_notes.retain(|skip| *skip != key);
                            }
                        } else if ui.button("🗑").on_hover_text("Delete").clicked() {
                            state.skipped_notes.push(skipped_note_key(note));
                        }
                        ui.end_row();
                    }
                });
        });

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label("Add note:");
        ui.add(
            egui::DragValue::new(&mut state.add_note_pitch)
                .range(21..=108)
                .custom_formatter(|value, _| note_name(value as u8)),
        );
        ui.label("Velocity:");
        ui.add(egui::DragValue::new(&mut state.add_note_velocity).range(1..=127));
        ui.label("Duration:");
        ui.add(
            egui::DragValue::new(&mut state.add_note_duration_seconds)
                .speed(0.01)
                .range(0.02..=60.0)
                .suffix("s"),
        );
        if ui
            .button("Add at current frame")
            .on_hover_text(
                "Adds a new note at the current playhead position — stored in the project file, \
                never written to the MIDI file.",
            )
            .clicked()
        {
            let start_seconds = (state.position_seconds - state.sync_offset_seconds).max(0.0);
            let next_id = state
                .added_notes
                .iter()
                .map(|added| added.id)
                .max()
                .map_or(0, |id| id + 1);
            state.added_notes.push(project::AddedNote {
                id: next_id,
                channel: 0,
                note: state.add_note_pitch,
                start_seconds,
                duration_seconds: state.add_note_duration_seconds,
                velocity: state.add_note_velocity,
            });
        }
    });
}

fn draw_keyboard_tab(ui: &mut egui::Ui, state: &mut UiState) {
    draw_note_editor(ui, state);
    ui.separator();

    ui.heading("Keyboard calibration");
    ui.label("Drag the yellow guides on the preview, or use the sliders below.");
    ui.horizontal(|ui| {
        ui.label("Left:");
        validated_slider(ui, &mut state.calibration.left_fraction, 0.0..=1.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Right:");
        validated_slider(ui, &mut state.calibration.right_fraction, 0.0..=1.0, None);
    });
    // Same min-gap clamp as the drag handles, so a slider can't collapse the keyboard span to
    // zero width either.
    state.calibration.right_fraction = state
        .calibration
        .right_fraction
        .clamp(state.calibration.left_fraction + CALIBRATION_MIN_GAP, 1.0);
    state.calibration.left_fraction = state
        .calibration
        .left_fraction
        .clamp(0.0, state.calibration.right_fraction - CALIBRATION_MIN_GAP);
    ui.label(format!(
        "Keyboard: {:.0}%\u{2013}{:.0}% of width",
        state.calibration.left_fraction * 100.0,
        state.calibration.right_fraction * 100.0,
    ));
    if ui.button("Reset calibration").clicked() {
        state.calibration = project::KeyboardCalibration::default();
    }

    ui.add_space(4.0);
    ui.label(
        "Camera stretch: corrects perspective distortion in filmed footage, where octaves \
        further from the camera's center appear narrower or wider than a head-on shot would.",
    );
    if state.calibration.stretch.is_some() {
        ui.label("Calibrated — drag the green anchors on the preview to fine-tune.");
        ui.horizontal(|ui| {
            if ui.button("Redo camera stretch…").clicked() {
                state.camera_stretch_capture = Some(CameraStretchCapture::default());
            }
            if ui.button("Clear camera stretch").clicked() {
                state.calibration.stretch = None;
            }
        });
    } else if ui.button("Align notes to camera stretch…").clicked() {
        state.camera_stretch_capture = Some(CameraStretchCapture::default());
    }

    ui.separator();
    ui.heading("Barrier");
    ui.label("Drag the guide on the preview, or use the slider below.");
    ui.horizontal(|ui| {
        ui.label("Position:");
        validated_slider(
            ui,
            &mut state.calibration.barrier_fraction,
            BARRIER_MIN_FRACTION..=BARRIER_MAX_FRACTION,
            None,
        );
    });
    ui.horizontal(|ui| {
        ui.label("Color:");
        ui.color_edit_button_srgb(&mut state.barrier_style.color);
    });
    ui.horizontal(|ui| {
        ui.label("Thickness:");
        validated_slider(ui, &mut state.barrier_style.thickness, 1.0..=12.0, None);
    });
    if ui.button("Reset barrier").clicked() {
        state.calibration.barrier_fraction =
            project::KeyboardCalibration::default().barrier_fraction;
        state.barrier_style = project::BarrierStyle::default();
    }

    ui.separator();
    ui.heading("Note style");
    ui.horizontal(|ui| {
        ui.label("Color:");
        ui.color_edit_button_srgb(&mut state.note_style.color);
    });
    ui.horizontal(|ui| {
        ui.label("Black keys:");
        let mode_label = match state.note_style.black_key_color {
            project::BlackKeyColorMode::Auto => "Auto",
            project::BlackKeyColorMode::Same => "Same",
            project::BlackKeyColorMode::Custom(_) => "Custom",
        };
        egui::ComboBox::from_id_salt("black_key_color_mode")
            .selected_text(mode_label)
            .show_ui(ui, |ui| {
                if ui.selectable_label(mode_label == "Auto", "Auto").clicked() {
                    state.note_style.black_key_color = project::BlackKeyColorMode::Auto;
                }
                if ui.selectable_label(mode_label == "Same", "Same").clicked() {
                    state.note_style.black_key_color = project::BlackKeyColorMode::Same;
                }
                if ui
                    .selectable_label(mode_label == "Custom", "Custom")
                    .clicked()
                    && mode_label != "Custom"
                {
                    // Seed with the same darkening Auto already applies, so switching modes
                    // doesn't jump to an arbitrary color.
                    state.note_style.black_key_color = project::BlackKeyColorMode::Custom(
                        darken_color(state.note_style.color, 0.6),
                    );
                }
            });
    });
    if let project::BlackKeyColorMode::Custom(color) = &mut state.note_style.black_key_color {
        ui.horizontal(|ui| {
            ui.label("Black key color:");
            ui.color_edit_button_srgb(color);
        });
    }
    ui.horizontal(|ui| {
        ui.label("Roundedness:");
        validated_slider(ui, &mut state.note_style.roundedness, 0.0..=3.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Fall speed:");
        validated_slider(ui, &mut state.note_style.fall_speed, 50.0..=2000.0, Some(0));
    })
    .response
    .on_hover_text("Also changes how long each note looks, since a note's on-screen length is its duration times this speed.");
    if ui.button("Reset note style").clicked() {
        state.note_style = project::NoteStyle::default();
    }

    ui.separator();
    ui.heading("Background");
    ui.label("Canvas color behind the video and note highway.");
    ui.horizontal(|ui| {
        ui.label("Color:");
        ui.color_edit_button_srgb(&mut state.background_color);
    });
    if ui.button("Reset background").clicked() {
        state.background_color = [0, 0, 0];
    }
}

fn draw_project_tab(ui: &mut egui::Ui, state: &mut UiState) {
    ui.heading("Media");
    ui.horizontal(|ui| {
        if ui.button("Open Video…").clicked() {
            state.open_video_requested = true;
        }
        if ui.button("Open MIDI…").clicked() {
            state.open_midi_requested = true;
        }
    });

    ui.separator();
    ui.heading("Sync");
    ui.horizontal(|ui| {
        ui.label("Sync offset (s):");
        ui.add(egui::DragValue::new(&mut state.sync_offset_seconds).speed(0.01));
    });

    ui.separator();
    ui.heading("Style");
    ui.horizontal(|ui| {
        if ui.button("Import style…").clicked() {
            state.import_style_requested = true;
        }
        if ui
            .add_enabled(state.style_path.is_some(), egui::Button::new("⟳"))
            .on_hover_text("Reload the imported style from its file")
            .clicked()
        {
            state.reload_style_requested = true;
        }
    });
    ui.label(if state.style.is_some() {
        "Custom style imported (overrides note/barrier sliders)"
    } else {
        "Using note/barrier sliders (no style imported)"
    });

    ui.separator();
    ui.heading("Project file");
    ui.label("Project file:");
    ui.text_edit_singleline(&mut state.project_path_text);
    ui.horizontal(|ui| {
        if ui.button("Save").clicked() {
            state.save_requested = true;
        }
        if ui.button("Load").clicked() {
            state.load_requested = true;
        }
        if ui.button("Save As…").clicked() {
            state.save_project_as_requested = true;
        }
        if ui.button("Open…").clicked() {
            state.open_project_requested = true;
        }
    });
    if ui.button("New Project").clicked() {
        state.new_project_requested = true;
    }
    if let Some(message) = &state.status_message {
        ui.label(message);
    }

    ui.separator();
    if ui.button("Exit").clicked() {
        state.exit_requested = true;
    }
}

/// Minimum gap kept between opposing crop edges, as a fraction of frame width/height — same
/// purpose as `CALIBRATION_MIN_GAP`, keeps a drag from collapsing the crop to zero size.
const CROP_MIN_GAP: f32 = 0.1;

fn draw_transform_tab(ui: &mut egui::Ui, transform: &mut project::VideoTransform) {
    ui.heading("Video Transform");
    ui.horizontal(|ui| {
        ui.label("Brightness:");
        validated_slider(ui, &mut transform.brightness, 0.0..=2.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Scale (zoom):");
        validated_slider(ui, &mut transform.scale, 0.2..=3.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Rotation (deg):");
        validated_slider(ui, &mut transform.rotation_degrees, -180.0..=180.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Tilt X:");
        validated_slider(ui, &mut transform.tilt_x, -0.3..=0.3, None);
    });
    ui.horizontal(|ui| {
        ui.label("Tilt Y:");
        validated_slider(ui, &mut transform.tilt_y, -0.3..=0.3, None);
    });
    ui.horizontal(|ui| {
        ui.label("Translate X:");
        validated_slider(ui, &mut transform.translate_x, -1.0..=1.0, Some(3));
    });
    ui.horizontal(|ui| {
        ui.label("Translate Y:");
        validated_slider(ui, &mut transform.translate_y, -1.0..=1.0, Some(3));
    });

    ui.separator();
    ui.label("Crop (also draggable on the preview):");
    ui.horizontal(|ui| {
        ui.label("Left:");
        validated_slider(ui, &mut transform.crop_left, 0.0..=1.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Right:");
        validated_slider(ui, &mut transform.crop_right, 0.0..=1.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Top:");
        validated_slider(ui, &mut transform.crop_top, 0.0..=1.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Bottom:");
        validated_slider(ui, &mut transform.crop_bottom, 0.0..=1.0, None);
    });
    // Same min-gap clamp as the drag handles, so a slider can't collapse the crop to zero size
    // either.
    transform.crop_right = transform
        .crop_right
        .clamp(transform.crop_left + CROP_MIN_GAP, 1.0);
    transform.crop_left = transform
        .crop_left
        .clamp(0.0, transform.crop_right - CROP_MIN_GAP);
    transform.crop_bottom = transform
        .crop_bottom
        .clamp(transform.crop_top + CROP_MIN_GAP, 1.0);
    transform.crop_top = transform
        .crop_top
        .clamp(0.0, transform.crop_bottom - CROP_MIN_GAP);

    if ui.button("Reset transform").clicked() {
        *transform = project::VideoTransform::default();
    }
}

fn draw_export_tab(ui: &mut egui::Ui, state: &mut UiState) {
    ui.heading("Export");
    ui.horizontal(|ui| {
        ui.label("Output file:");
        ui.text_edit_singleline(&mut state.export_path_text);
    });
    ui.horizontal(|ui| {
        ui.label("FPS:");
        ui.add(egui::DragValue::new(&mut state.export_fps).range(1..=120));
    });

    if let Some((done, total)) = state.export_progress {
        let fraction = if total > 0 {
            done as f32 / total as f32
        } else {
            0.0
        };
        ui.add(
            egui::ProgressBar::new(fraction)
                .show_percentage()
                .text(format!("{done} / {total} frames")),
        );
        if ui.button("Cancel").clicked() {
            state.export_cancel_requested = true;
        }
    } else if ui.button("Export").clicked() {
        state.export_requested = true;
    }

    if let Some(message) = &state.export_message {
        ui.label(message);
    }
}

/// Height of the custom-painted ruler/playhead/density strip, not counting the transport
/// controls row above it.
pub const DEFAULT_TIMELINE_HEIGHT: f32 = 40.0;
const MIN_TIMELINE_HEIGHT: f32 = 24.0;
const MAX_TIMELINE_HEIGHT: f32 = 180.0;
const TIMELINE_RESIZE_HANDLE_HEIGHT: f32 = 6.0;
const TIMELINE_PANEL_CHROME_HEIGHT: f32 = 62.0;
pub const DEFAULT_TIMELINE_ZOOM: f64 = 1.0;
const MIN_TIMELINE_ZOOM: f64 = 1.0;
const MAX_TIMELINE_ZOOM: f64 = 128.0;
const TIMELINE_ZOOM_SCROLL_SENSITIVITY: f64 = 0.005;
const MIN_RULER_TICK_SPACING: f32 = 50.0;

/// Bottom transport bar: play/pause, timecode, and a custom-painted timeline (time ruler,
/// draggable/clickable playhead, note-density strip) — replaces the plain `egui::Slider`
/// scrubber milestones 1-5 used, which had no room to show song structure while scrubbing.
fn draw_timeline_panel(ui: &mut egui::Ui, state: &mut UiState) {
    egui::Panel::bottom("timeline")
        .default_size(state.timeline_height + TIMELINE_PANEL_CHROME_HEIGHT)
        .min_size(MIN_TIMELINE_HEIGHT + TIMELINE_PANEL_CHROME_HEIGHT)
        .max_size(MAX_TIMELINE_HEIGHT + TIMELINE_PANEL_CHROME_HEIGHT)
        .resizable(false)
        .show_separator_line(false)
        .show(ui, |ui| {
            draw_timeline_resize_handle(ui, state);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let play_label = if state.playing { "Pause" } else { "Play" };
                if ui.button(play_label).clicked() {
                    state.playing = !state.playing;
                }
                ui.label(format!(
                    "{} / {}",
                    format_timecode(state.position_seconds),
                    format_timecode(state.duration_seconds)
                ));
                if let Some(name) = &state.midi_name {
                    ui.separator();
                    ui.label(format!("MIDI: {name}"));
                }
            });
            ui.add_space(4.0);
            draw_timeline_scrubber(ui, state);
            ui.add_space(6.0);
        });
}

fn draw_timeline_resize_handle(ui: &mut egui::Ui, state: &mut UiState) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), TIMELINE_RESIZE_HANDLE_HEIGHT),
        egui::Sense::drag(),
    );
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if response.dragged() {
        state.timeline_height = (state.timeline_height - response.drag_delta().y)
            .clamp(MIN_TIMELINE_HEIGHT, MAX_TIMELINE_HEIGHT);
    }

    let color = if response.dragged() {
        egui::Color32::from_gray(170)
    } else if response.hovered() {
        egui::Color32::from_gray(130)
    } else {
        egui::Color32::from_gray(70)
    };
    let full_width = ui.ctx().viewport_rect().x_range();
    ui.painter()
        .hline(full_width, rect.center().y, egui::Stroke::new(1.0, color));
}

fn draw_timeline_scrubber(ui: &mut egui::Ui, state: &mut UiState) {
    let desired_size = egui::vec2(ui.available_width(), state.timeline_height);
    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click_and_drag());
    let duration = state.duration_seconds.max(0.001);
    let (mut view_start, mut view_end) = timeline_view_range(state);

    if response.hovered() {
        let scroll_y = ui.input(|input| input.smooth_scroll_delta.y);
        if scroll_y.abs() > f32::EPSILON {
            let anchor_fraction = response
                .hover_pos()
                .map(|pos| ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0))
                .unwrap_or(0.5) as f64;
            let anchor_time = view_start + anchor_fraction * (view_end - view_start);
            let zoom_factor = (scroll_y as f64 * TIMELINE_ZOOM_SCROLL_SENSITIVITY).exp();
            state.timeline_zoom =
                (state.timeline_zoom * zoom_factor).clamp(MIN_TIMELINE_ZOOM, MAX_TIMELINE_ZOOM);
            let view_duration = timeline_view_duration(duration, state.timeline_zoom);
            state.timeline_view_start_seconds = clamp_timeline_view_start(
                anchor_time - anchor_fraction * view_duration,
                duration,
                view_duration,
            );
            (view_start, view_end) = timeline_view_range(state);
        }
    }

    // Keep the playhead visible: if it's outside the current view (an arrow-key seek, Home/End,
    // or ordinary playback advancing past a zoomed-in view), shift the view just enough to bring
    // it back in, landing right at the edge it crossed rather than re-centering — a small nudge
    // stays a small nudge instead of jumping the view around.
    let view_duration = (view_end - view_start).max(0.001);
    if state.position_seconds < view_start {
        state.timeline_view_start_seconds =
            clamp_timeline_view_start(state.position_seconds, duration, view_duration);
        (view_start, view_end) = timeline_view_range(state);
    } else if state.position_seconds > view_end {
        state.timeline_view_start_seconds = clamp_timeline_view_start(
            state.position_seconds - view_duration,
            duration,
            view_duration,
        );
        (view_start, view_end) = timeline_view_range(state);
    }

    if response.clicked() || response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            let fraction = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            state.seek_request = Some(view_start + fraction as f64 * (view_end - view_start));
            state.seek_request_exact = response.clicked() && !response.dragged();

            if response.dragged() {
                edge_auto_scroll(ui, state, rect, pos, view_start, view_end, duration);
            }
        }
    }

    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, egui::Color32::from_gray(35));

    // Audio waveform and MIDI note density each get their own half of the strip rather than
    // sharing the full height — drawn on top of each other they were hard to tell apart (both
    // reaching for the same vertical space) even though one is center-aligned and the other
    // bottom-aligned.
    let half_y = rect.top() + rect.height() * 0.5;
    let top_half = egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), half_y));
    let bottom_half = egui::Rect::from_min_max(egui::pos2(rect.left(), half_y), rect.max);
    draw_waveform(
        painter,
        top_half,
        &state.waveform_peaks,
        state.waveform_bucket_seconds,
        view_start,
        view_end,
    );
    draw_note_density(
        painter,
        bottom_half,
        &state.midi_note_times,
        view_start,
        view_end,
    );
    draw_time_ruler(painter, rect, view_start, view_end);

    if (view_start..=view_end).contains(&state.position_seconds) {
        let playhead_fraction =
            ((state.position_seconds - view_start) / (view_end - view_start)).clamp(0.0, 1.0);
        let playhead_x = rect.left() + playhead_fraction as f32 * rect.width();
        painter.line_segment(
            [
                egui::pos2(playhead_x, rect.top()),
                egui::pos2(playhead_x, rect.bottom()),
            ],
            egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 90, 90)),
        );
    }
}

const EDGE_SCROLL_ZONE_PX: f32 = 28.0;
const EDGE_SCROLL_MAX_FRACTION_PER_SEC: f64 = 1.5;

/// While dragging the playhead, scrolls the visible timeline range when the pointer nears
/// either edge of the widget, so a single drag can reach times currently off-screen instead of
/// getting stuck clamped to the edge. Scroll speed ramps up the closer the pointer gets to the
/// very edge (0 right at the dead zone boundary, max at the widget's physical edge), and keeps
/// animating even if the pointer stops moving — `request_repaint` is needed for that since
/// nothing else would otherwise wake up a frame while the pointer is held still.
fn edge_auto_scroll(
    ui: &egui::Ui,
    state: &mut UiState,
    rect: egui::Rect,
    pointer: egui::Pos2,
    view_start: f64,
    view_end: f64,
    duration: f64,
) {
    let view_duration = (view_end - view_start).max(0.001);
    let left_overshoot =
        ((rect.left() + EDGE_SCROLL_ZONE_PX - pointer.x) / EDGE_SCROLL_ZONE_PX).clamp(0.0, 1.0);
    let right_overshoot =
        ((pointer.x - (rect.right() - EDGE_SCROLL_ZONE_PX)) / EDGE_SCROLL_ZONE_PX).clamp(0.0, 1.0);
    let overshoot = left_overshoot.max(right_overshoot);
    if overshoot <= 0.0 {
        return;
    }

    let dt = ui.input(|input| input.stable_dt).min(0.1) as f64;
    let scroll = view_duration * EDGE_SCROLL_MAX_FRACTION_PER_SEC * overshoot as f64 * dt;
    let delta = if left_overshoot > right_overshoot {
        -scroll
    } else {
        scroll
    };
    state.timeline_view_start_seconds = clamp_timeline_view_start(
        state.timeline_view_start_seconds + delta,
        duration,
        view_duration,
    );
    ui.ctx().request_repaint();
}

fn timeline_view_range(state: &mut UiState) -> (f64, f64) {
    let duration = state.duration_seconds.max(0.001);
    state.timeline_zoom = state
        .timeline_zoom
        .clamp(MIN_TIMELINE_ZOOM, MAX_TIMELINE_ZOOM);
    let view_duration = timeline_view_duration(duration, state.timeline_zoom);
    state.timeline_view_start_seconds =
        clamp_timeline_view_start(state.timeline_view_start_seconds, duration, view_duration);
    let view_start = state.timeline_view_start_seconds;
    (view_start, view_start + view_duration)
}

fn timeline_view_duration(duration: f64, zoom: f64) -> f64 {
    (duration / zoom).clamp(0.001, duration)
}

fn clamp_timeline_view_start(start: f64, duration: f64, view_duration: f64) -> f64 {
    start.clamp(0.0, (duration - view_duration).max(0.0))
}

/// Draws the audio waveform as a vertically-centered bar strip, drawn first so the note-density
/// strip and time ruler layer on top of it. `peaks`/`bucket_seconds` are the whole track's
/// downsampled amplitude summary (`AudioPlayback::waveform_peaks`, mirrored into `UiState` at
/// video-load time) — re-bucketing that (already small) array into the visible column count is
/// cheap at any zoom level, unlike re-scanning raw samples every redraw would be.
fn draw_waveform(
    painter: &egui::Painter,
    rect: egui::Rect,
    peaks: &[f32],
    bucket_seconds: f64,
    view_start: f64,
    view_end: f64,
) {
    if peaks.is_empty() || bucket_seconds <= 0.0 {
        return;
    }
    let view_duration = (view_end - view_start).max(0.001);
    const BUCKETS: usize = 240;
    let bucket_width = rect.width() / BUCKETS as f32;
    let mid_y = rect.center().y;
    let half_height = rect.height() * 0.5 - 2.0;

    for i in 0..BUCKETS {
        let column_start = view_start + (i as f64 / BUCKETS as f64) * view_duration;
        let column_end = view_start + ((i + 1) as f64 / BUCKETS as f64) * view_duration;
        let start_index =
            ((column_start / bucket_seconds).floor().max(0.0) as usize).min(peaks.len());
        let end_index = ((column_end / bucket_seconds).ceil().max(0.0) as usize)
            .clamp(start_index + 1, peaks.len().max(start_index + 1))
            .min(peaks.len());
        if start_index >= end_index {
            continue;
        }
        let amplitude = peaks[start_index..end_index]
            .iter()
            .copied()
            .fold(0.0f32, f32::max);
        if amplitude <= 0.0 {
            continue;
        }
        let half_bar = amplitude.min(1.0) * half_height;
        let x = rect.left() + i as f32 * bucket_width;
        let bar = egui::Rect::from_min_max(
            egui::pos2(x, mid_y - half_bar),
            egui::pos2(x + bucket_width.max(1.0), mid_y + half_bar),
        );
        painter.rect_filled(bar, 0.0, egui::Color32::from_rgb(60, 80, 90));
    }
}

/// Buckets note onset times into columns spanning the timeline's width and draws each as a
/// short vertical bar sized by relative density — cheap (the MIDI is already parsed once at
/// load time) and gives a rough sense of song structure while scrubbing, without needing to
/// actually render note content in miniature.
fn draw_note_density(
    painter: &egui::Painter,
    rect: egui::Rect,
    note_times: &[f32],
    view_start: f64,
    view_end: f64,
) {
    if note_times.is_empty() {
        return;
    }
    let view_duration = (view_end - view_start).max(0.001);
    const BUCKETS: usize = 240;
    let mut counts = [0u32; BUCKETS];
    for &t in note_times {
        let t = t as f64;
        if t < view_start || t > view_end {
            continue;
        }
        let fraction = ((t - view_start) / view_duration).clamp(0.0, 0.999_999);
        counts[(fraction * BUCKETS as f64) as usize] += 1;
    }
    let max_count = counts.iter().copied().max().unwrap_or(1).max(1);
    let bucket_width = rect.width() / BUCKETS as f32;
    let strip_bottom = rect.bottom() - 3.0;
    let strip_max_height = (rect.height() - 6.0).max(1.0);
    for (i, &count) in counts.iter().enumerate() {
        if count == 0 {
            continue;
        }
        let height = (count as f32 / max_count as f32) * strip_max_height;
        let x = rect.left() + i as f32 * bucket_width;
        let bar = egui::Rect::from_min_max(
            egui::pos2(x, strip_bottom - height),
            egui::pos2(x + bucket_width.max(1.0), strip_bottom),
        );
        painter.rect_filled(bar, 0.0, egui::Color32::from_rgb(80, 150, 210));
    }
}

/// Draws tick marks + timecode labels at a duration-appropriate interval (aiming for roughly
/// 6-12 ticks when there is room, while enforcing a minimum pixel gap so labels do not crowd.
fn draw_time_ruler(painter: &egui::Painter, rect: egui::Rect, view_start: f64, view_end: f64) {
    let view_duration = (view_end - view_start).max(0.001);
    let interval = ruler_tick_interval(view_duration, rect.width());
    let mut t = (view_start / interval).ceil() * interval;
    while t <= view_end {
        let x = rect.left() + ((t - view_start) / view_duration) as f32 * rect.width();
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.top() + 5.0)],
            egui::Stroke::new(1.0, egui::Color32::from_gray(150)),
        );
        painter.text(
            egui::pos2(x + 2.0, rect.top()),
            egui::Align2::LEFT_TOP,
            format_timecode(t),
            egui::FontId::proportional(9.0),
            egui::Color32::from_gray(190),
        );
        t += interval;
    }
}

fn ruler_tick_interval(duration: f64, width: f32) -> f64 {
    const STEPS: [f64; 10] = [1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0];
    let max_ticks = (width / MIN_RULER_TICK_SPACING).floor().max(1.0) as f64;
    let target = (duration / max_ticks).max(duration / 10.0);
    STEPS
        .into_iter()
        .find(|&step| step >= target)
        .unwrap_or(600.0)
}

fn draw_drop_overlay(ui: &egui::Ui, screen: egui::Rect) {
    egui::Area::new(egui::Id::new("drop_overlay"))
        .fixed_pos(egui::Pos2::ZERO)
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            ui.painter()
                .rect_filled(screen, 0.0, egui::Color32::from_black_alpha(170));
            ui.painter().text(
                screen.center(),
                egui::Align2::CENTER_CENTER,
                "Drop a video or MIDI (.mid) file",
                egui::FontId::proportional(28.0),
                egui::Color32::WHITE,
            );
        });
}

/// Draws two draggable vertical guides over the preview image marking the left/right edges of
/// the keyboard visible in the footage, updating `calibration` in place as they're dragged.
/// `screen` is the preview image's actual on-screen rect (aspect-fit inside the central panel,
/// see `fit_rect`) — not the whole window, since milestone 6c moved the video off the raw
/// swapchain and into a shrinkable `egui::Image`.
fn draw_calibration_handles(
    ui: &egui::Ui,
    screen: egui::Rect,
    calibration: &mut project::KeyboardCalibration,
) {
    let stroke = egui::Stroke::new(3.0, egui::Color32::from_rgb(255, 209, 0));

    let left_x = screen.left() + screen.width() * calibration.left_fraction;
    let left_rect = egui::Rect::from_min_max(
        egui::pos2(left_x - 6.0, screen.top()),
        egui::pos2(left_x + 6.0, screen.bottom()),
    );
    let left_response = ui.interact(
        left_rect,
        egui::Id::new("calibration_left_handle"),
        egui::Sense::drag(),
    );
    if left_response.dragged() {
        let delta = left_response.drag_delta().x / screen.width();
        calibration.left_fraction = (calibration.left_fraction + delta)
            .clamp(0.0, calibration.right_fraction - CALIBRATION_MIN_GAP);
    }

    let right_x = screen.left() + screen.width() * calibration.right_fraction;
    let right_rect = egui::Rect::from_min_max(
        egui::pos2(right_x - 6.0, screen.top()),
        egui::pos2(right_x + 6.0, screen.bottom()),
    );
    let right_response = ui.interact(
        right_rect,
        egui::Id::new("calibration_right_handle"),
        egui::Sense::drag(),
    );
    if right_response.dragged() {
        let delta = right_response.drag_delta().x / screen.width();
        calibration.right_fraction = (calibration.right_fraction + delta)
            .clamp(calibration.left_fraction + CALIBRATION_MIN_GAP, 1.0);
    }

    let painter = ui.painter();
    painter.line_segment(
        [
            egui::pos2(left_x, screen.top()),
            egui::pos2(left_x, screen.bottom()),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(right_x, screen.top()),
            egui::pos2(right_x, screen.bottom()),
        ],
        stroke,
    );
    painter.text(
        egui::pos2(left_x, screen.top() + 4.0),
        egui::Align2::CENTER_TOP,
        "key L",
        egui::FontId::proportional(12.0),
        stroke.color,
    );
    painter.text(
        egui::pos2(right_x, screen.top() + 4.0),
        egui::Align2::CENTER_TOP,
        "key R",
        egui::FontId::proportional(12.0),
        stroke.color,
    );
}

/// Minimum gap kept between adjacent camera-stretch anchors, and between an anchor and its
/// neighboring left/right calibration edge, as a fraction of the preview image width — same
/// purpose as `CALIBRATION_MIN_GAP`, keeps a drag (or a shrunk left/right calibration span) from
/// collapsing an octave segment to zero width or reordering two anchors past each other.
const STRETCH_ANCHOR_MIN_GAP: f32 = 0.01;

/// Draws the 8 camera-stretch anchor points (the left edge of C1..C8) as draggable vertical
/// guides, shown once `calibration.stretch` is `Some` — lets the user fine-tune a calibration
/// captured by `draw_camera_stretch_overlay` without redoing the whole click sequence. Each
/// anchor is clamped between its immediate neighbors (the adjacent anchor, or
/// `left_fraction`/`right_fraction` at the two ends) so a drag can't reorder or collapse an
/// octave segment; `clamp_camera_stretch` enforces the same invariant against other mutation
/// paths (sliders, a shrunk left/right span) every frame.
fn draw_camera_stretch_handles(
    ui: &egui::Ui,
    screen: egui::Rect,
    calibration: &mut project::KeyboardCalibration,
) {
    let Some(stretch) = calibration.stretch.as_mut() else {
        return;
    };
    let stroke = egui::Stroke::new(2.0, egui::Color32::from_rgb(0, 220, 160));
    let left_fraction = calibration.left_fraction;
    let right_fraction = calibration.right_fraction;

    for i in 0..8 {
        let lower_bound = (if i == 0 {
            left_fraction
        } else {
            stretch.c_fractions[i - 1]
        }) + STRETCH_ANCHOR_MIN_GAP;
        let upper_bound = (if i == 7 {
            right_fraction
        } else {
            stretch.c_fractions[i + 1]
        }) - STRETCH_ANCHOR_MIN_GAP;

        let x = screen.left() + screen.width() * stretch.c_fractions[i];
        let rect = egui::Rect::from_min_max(
            egui::pos2(x - 5.0, screen.top()),
            egui::pos2(x + 5.0, screen.bottom()),
        );
        let response = ui.interact(
            rect,
            egui::Id::new(("camera_stretch_handle", i)),
            egui::Sense::drag(),
        );
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if response.dragged() {
            let delta = response.drag_delta().x / screen.width();
            stretch.c_fractions[i] =
                (stretch.c_fractions[i] + delta).clamp(lower_bound.min(upper_bound), upper_bound);
        }

        let painter = ui.painter();
        painter.line_segment(
            [egui::pos2(x, screen.top()), egui::pos2(x, screen.bottom())],
            stroke,
        );
        painter.text(
            egui::pos2(x, screen.bottom() - 4.0),
            egui::Align2::CENTER_BOTTOM,
            format!("C{}", i + 1),
            egui::FontId::proportional(11.0),
            stroke.color,
        );
    }
}

/// Keeps `calibration.stretch`'s 8 anchors ascending and inside
/// `(left_fraction, right_fraction)` after any edit this frame — dragging the plain left/right
/// calibration handles or typing into their sliders doesn't itself touch `stretch`, so without
/// this an anchor could end up outside the new bounds or out of order, collapsing an octave
/// segment to zero/negative width (`render::notes::keyboard_layout` floors segment width at a
/// tiny minimum so this can't crash, but it would otherwise still look broken).
fn clamp_camera_stretch(calibration: &mut project::KeyboardCalibration) {
    let left = calibration.left_fraction;
    let right = calibration.right_fraction;
    let Some(stretch) = calibration.stretch.as_mut() else {
        return;
    };

    // Forward pass: each anchor at least MIN_GAP above the previous one (or `left`).
    let mut floor = left;
    for c in stretch.c_fractions.iter_mut() {
        floor += STRETCH_ANCHOR_MIN_GAP;
        *c = c.max(floor);
        floor = *c;
    }
    // Backward pass: each anchor at least MIN_GAP below the next one (or `right`) — run after
    // the forward pass so a shrunk `right_fraction` pulls anchors down without breaking the
    // ascending order the forward pass already established.
    let mut ceiling = right;
    for c in stretch.c_fractions.iter_mut().rev() {
        ceiling -= STRETCH_ANCHOR_MIN_GAP;
        *c = c.min(ceiling);
        ceiling = *c;
    }
}

/// Drawn instead of the ordinary calibration/barrier/stretch-anchor handles while
/// `state.camera_stretch_capture` is `Some` — walks the user through clicking
/// `CAMERA_STRETCH_POINT_LABELS.len()` points on the preview image (a crosshair follows the
/// pointer, with an instruction label naming the next point). Every point already clicked this
/// run is also plotted as its own labeled guide line (yellow for the two calibration edges, teal
/// for the 8 interior anchors, matching the colors those points settle into once finalized) so
/// it stays unambiguous which points remain as the run progresses. Records each click's
/// x-fraction of `screen`, and finalizes into `calibration.left_fraction`/`right_fraction`/
/// `stretch` once every point is captured (see `finalize_camera_stretch`); Escape or the Cancel
/// button abort the whole sequence, discarding whatever was clicked so far and leaving
/// `calibration` untouched.
fn draw_camera_stretch_overlay(ui: &mut egui::Ui, screen: egui::Rect, state: &mut UiState) {
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.camera_stretch_capture = None;
        return;
    }

    let cancel_rect = egui::Rect::from_min_size(
        egui::pos2(screen.right() - 96.0, screen.top() + 8.0),
        egui::vec2(88.0, 24.0),
    );
    if ui
        .put(cancel_rect, egui::Button::new("Cancel (Esc)"))
        .clicked()
    {
        state.camera_stretch_capture = None;
        return;
    }

    let captured: Vec<f32> = match &state.camera_stretch_capture {
        Some(capture) => capture.points.clone(),
        None => return,
    };
    let step = captured.len();

    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    let response = ui.interact(
        screen,
        egui::Id::new("camera_stretch_capture"),
        egui::Sense::click(),
    );

    let painter = ui.painter();

    for (i, &fraction) in captured.iter().enumerate() {
        let x = screen.left() + screen.width() * fraction;
        let color = if i == 0 || i == CAMERA_STRETCH_POINT_LABELS.len() - 1 {
            egui::Color32::from_rgb(255, 209, 0)
        } else {
            egui::Color32::from_rgb(0, 220, 160)
        };
        painter.line_segment(
            [egui::pos2(x, screen.top()), egui::pos2(x, screen.bottom())],
            egui::Stroke::new(2.0, color),
        );
        painter.text(
            egui::pos2(x, screen.bottom() - 4.0),
            egui::Align2::CENTER_BOTTOM,
            CAMERA_STRETCH_POINT_TAGS[i],
            egui::FontId::proportional(11.0),
            color,
        );
    }

    let stroke = egui::Stroke::new(1.5, egui::Color32::from_rgb(255, 209, 0));
    if let Some(pos) = response.hover_pos() {
        painter.line_segment(
            [
                egui::pos2(pos.x, screen.top()),
                egui::pos2(pos.x, screen.bottom()),
            ],
            stroke,
        );
        painter.line_segment(
            [
                egui::pos2(screen.left(), pos.y),
                egui::pos2(screen.right(), pos.y),
            ],
            stroke,
        );
    }
    painter.text(
        egui::pos2(screen.center().x, screen.top() + 10.0),
        egui::Align2::CENTER_TOP,
        format!(
            "Click the {} ({}/{})",
            CAMERA_STRETCH_POINT_LABELS[step],
            step + 1,
            CAMERA_STRETCH_POINT_LABELS.len(),
        ),
        egui::FontId::proportional(16.0),
        egui::Color32::WHITE,
    );

    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let fraction = ((pos.x - screen.left()) / screen.width()).clamp(0.0, 1.0);
            if let Some(capture) = state.camera_stretch_capture.as_mut() {
                capture.points.push(fraction);
                if capture.points.len() == CAMERA_STRETCH_POINT_LABELS.len() {
                    finalize_camera_stretch(state);
                }
            }
        }
    }
}

/// Called once `camera_stretch_capture`'s 10 points are all recorded — sorts them (defensive
/// against a click landing slightly out of left-to-right order, which would otherwise fold the
/// keyboard layout back on itself) and splits them into `calibration.left_fraction`/
/// `right_fraction` (the first/last point) and the 8 interior `CameraStretch::c_fractions`
/// anchors.
fn finalize_camera_stretch(state: &mut UiState) {
    let Some(capture) = state.camera_stretch_capture.take() else {
        return;
    };
    let mut points = capture.points;
    points.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut c_fractions = [0.0; 8];
    c_fractions.copy_from_slice(&points[1..9]);
    state.calibration.left_fraction = points[0];
    state.calibration.right_fraction = points[9];
    state.calibration.stretch = Some(project::CameraStretch { c_fractions });
}

/// Valid range for `KeyboardCalibration::barrier_fraction` — keeps the drag handle (and the
/// render-side `barrier_fraction` uniform, see `render::notes::NotesRenderer::render`) away from
/// a degenerate (near-zero-height or far-off-canvas) viewport.
const BARRIER_MIN_FRACTION: f32 = 0.05;
const BARRIER_MAX_FRACTION: f32 = 0.98;

/// Drag hit-region for `calibration.barrier_fraction`, editor-only — same `Sense::drag()` +
/// accumulated `drag_delta()` pattern as `draw_calibration_handles`, rotated 90°. The barrier
/// itself is rendered by `render::barrier::BarrierRenderer`; this only owns the invisible drag
/// target over the rendered bar.
fn draw_barrier_handle(
    ui: &egui::Ui,
    screen: egui::Rect,
    calibration: &mut project::KeyboardCalibration,
) {
    let y = screen.top() + screen.height() * calibration.barrier_fraction;
    let handle_rect = egui::Rect::from_min_max(
        egui::pos2(screen.left(), y - 6.0),
        egui::pos2(screen.right(), y + 6.0),
    );
    let response = ui.interact(
        handle_rect,
        egui::Id::new("barrier_handle"),
        egui::Sense::drag(),
    );
    if response.dragged() {
        let delta = response.drag_delta().y / screen.height();
        calibration.barrier_fraction = (calibration.barrier_fraction + delta)
            .clamp(BARRIER_MIN_FRACTION, BARRIER_MAX_FRACTION);
    }
}

fn format_timecode(seconds: f64) -> String {
    let total_ms = (seconds.max(0.0) * 1000.0).round() as i64;
    let minutes = total_ms / 60_000;
    let secs = (total_ms / 1000) % 60;
    let millis = total_ms % 1000;
    format!("{minutes:02}:{secs:02}.{millis:03}")
}
