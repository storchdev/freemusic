# UI

Describes the app's UI as it currently exists: the preview surface, the tabbed side panel, the
custom timeline, barrier/note-highway styling, camera-stretch keyboard calibration, synced audio
playback, keyboard shortcuts, slider input behavior, and the note editor. For the development
history behind these features (milestones, bugs found and fixed, design decisions), see
`docs/narratives/ui-milestones.md`.

## Preview surface

The compositor (video quad + note-highway overlay) renders into an offscreen
`wgpu::TextureFormat::Rgba8Unorm` texture (`AppState::preview_texture`/`preview_view`), which is
displayed via `egui::Image` in the central panel. Only the egui pass writes to the window's
swapchain; nothing renders to the swapchain directly.

Canvas size is decoupled from window size. `AppState::canvas_size` starts at
`DEFAULT_CANVAS_SIZE` (1280×720) before any video is loaded, and `set_canvas_size` resizes it to
the loaded video's own decoded resolution (rounded to an even number) — this recreates the
offscreen texture, re-registers it with egui, and rebuilds the note-highway layout for the new
pixel dimensions. Resizing the app window does not affect canvas size or the compositor at all.

The preview image's on-screen rect is computed once per frame (`ui::fit_rect`): a contain-fit
(letterboxed) rect matching the canvas's aspect ratio, sized to whatever space remains in the
central panel after the side panel and bottom timeline reserve their own space. Calibration and
crop drag handles (`draw_calibration_handles`/`draw_crop_handles`) hit-test and paint against this
computed rect, not the window as a whole.

## Side panel and tabs

The side panel (`ui::Tab`) is a hand-rolled tab strip with five tabs:

- **Project** — media open (Open Video…/Open MIDI… via native file-picker dialogs), sync offset,
  style file actions (Import/Reload/Save style as…), and project actions (New Project, Open
  Project…, Save Project, Save Project As…, Exit).
- **Keyboard** — keyboard calibration (left/right), camera-stretch calibration, barrier
  *position*, and the note editor. Purely geometry/notes-as-data — not appearance.
- **Style** — the project's full visual look: background, falling notes, barrier appearance
  (color/glow/pulse/wavy edge), barrier-hit transitions (particles/flash), and octave reference
  lines. See "Style tab" below.
- **Transform** — video transform controls: brightness, scale, crop, rotation, tilt, translate.
- **Export** — MP4 export controls and progress.

The panel is collapsible: dragging its edge past its minimum width collapses it to a narrow strip
(28–56px wide); dragging the collapsed strip's edge past its own maximum expands it back to full
width (220–420px). A small `«`/`»` button toggles between collapsed and expanded as a
discoverable alternative to dragging.

Both the tab-selector row and the "add note" row in the note editor use `horizontal_wrapped`
layout so they wrap onto a second line instead of demanding extra width when the panel is narrow.
The panel's outer content area is a two-axis (`ScrollArea::both`) scroll region with
`auto_shrink([false, false])`, so any tab's widest content scrolls horizontally within the panel
rather than growing the panel itself.

## Timeline

The bottom timeline (`ui::draw_timeline_panel`/`draw_timeline_scrubber`) is a custom-painted bar,
not a plain slider:

- Clicking or dragging anywhere on the bar seeks to that time
  (`Sense::click_and_drag()`).
- A time ruler draws tick marks at a duration-adaptive interval (`ruler_tick_interval`, targets
  roughly 10 ticks regardless of clip length), enforcing a minimum 50px (`MIN_RULER_TICK_SPACING`)
  between labeled ticks so labels don't crowd when the panel is narrow or zoomed in.
- The scrubber strip is split into a top half showing the audio waveform and a bottom half
  showing MIDI note density, each centered/aligned within its own half of the strip.
  - The waveform is a downsampled peak-amplitude summary of the whole audio track, computed at
    video-load time (`compute_waveform_peaks`) by bucketing decoded stereo samples into fixed
    10ms (`WAVEFORM_BUCKET_SECONDS`) windows and keeping the louder of L/R per bucket. The UI
    re-buckets this cheap summary into however many on-screen columns the current zoom level
    needs, rather than re-scanning raw audio every redraw.
  - The note-density strip (`draw_note_density`) buckets MIDI note onset times into 240 columns
    sized by relative density.
- Scrolling while hovering the scrubber zooms the visible time window in/out around the cursor
  (`UiState::timeline_zoom`/`timeline_view_start_seconds`); clicks/drags, note-density buckets,
  ruler ticks, and the playhead all map through the currently visible range rather than always
  compressing the whole song into the panel width.
- Auto-scroll has two behaviors:
  - *Follow-on-seek*: whenever the playhead moves outside the visible range (an arrow-key/Home/End
    seek, or ordinary playback outrunning a zoomed-in view), the view shifts by just enough to
    bring the playhead back to whichever edge it crossed.
  - *Edge auto-scroll while dragging the playhead* (`edge_auto_scroll`): if the pointer enters a
    28px (`EDGE_SCROLL_ZONE_PX`) zone at either edge of the timeline during an active drag, the
    view scrolls in that direction, with speed ramping up toward the edge (capped at 1.5× of the
    visible duration per second, `EDGE_SCROLL_MAX_FRACTION_PER_SEC`).
- Timeline height is adjustable via a drag strip on the panel's top edge
  (`draw_timeline_resize_handle`), clamped to 24–180px, stored in `ui_state.timeline_height`.

## Project actions

The Project tab holds all file/project actions — there is no separate top menu bar:

- **Open Video…**/**Open MIDI…** open a native file-picker dialog (`rfd::FileDialog::pick_file()`)
  and load the chosen file.
- **New Project** clears the loaded video/MIDI and resets sync offset, calibration, transform, and
  style to their defaults, recreating the compositor from scratch.
- **Open Project…**/**Save Project As…** open a native file dialog that populates the project
  path text field, then run the same load/save logic as typing a path directly and pressing
  Load/Save.
- **Save Project** saves to the currently-set project path.
- **Import style…**/**⟳**/style path field/**Load**/**Save style as…** load or write the
  project's `style` as a standalone `.fmstyle.ron` file — see "Style tab" below for the live
  editing model these buttons feed into.
- **Exit** closes the application.

## Keyboard shortcuts

Shortcuts are suppressed whenever a focused text field would otherwise consume the keystroke (the
field's own key handling takes priority).

| Shortcut | Action |
| --- | --- |
| Left / Right | Seek ±1 source-video frame |
| Shift+Left / Shift+Right | Seek ±1 second |
| Home / End | Jump to start / end of the timeline |
| Ctrl+S | Save project |
| Ctrl+O | Open project |
| Esc | Cancel an in-progress export |
| Space | Play / pause |

## Barrier position

`project::KeyboardCalibration` includes `barrier_fraction` (0.0 = top of frame, 1.0 = bottom;
default `0.8`), controlling where the note highway's hit line sits vertically. It's adjustable via
both a slider (Keyboard tab) and an on-canvas drag handle. Barrier *appearance* (color, glow,
pulse, wavy edge) is a Style tab concern — see below.

The barrier line is drawn as an egui overlay on top of the preview and is UI-only — it never
appears in an exported video, though its *position* still gates note clipping (notes stop
rendering once they reach the hit line), a real effect (a GPU scissor rect) shared by both the
interactive preview and export.

## Style tab

`project::Project::style: project::Style` is a full `.fmstyle.ron` look (see
`docs/fmstyle-format.md` for the field-by-field schema) — always present, always live: every
control on the Style tab edits it directly, and the compositor picks up the change on the very
next redraw (the same `note_layer != applied_note_layer`-style dirty-check `app/src/main.rs`
already used for the note editor drives this too, so no separate wiring was needed per field).
There is no more "legacy sliders vs. imported style" distinction — a `.fmstyle.ron` file is purely
an interchange format now (Project tab's Import/Reload/Save style as…), not the only way to set a
look.

The tab is organized into the same sections as the schema itself:

- **Background** — the canvas clear color (`style.background`).
- **Octave lines** — optional faint per-octave reference lines (`style.octave_lines`).
- **Notes** — fill (solid/vertical gradient/canvas gradient), optional sheen/glow, roundedness,
  fall speed, black-key fill mode, alpha (`style.notes`).
- **Barrier** — color, thickness, show/hide the solid bar, optional glow/pulse/wavy edge (with an
  optional strand bundle), (`style.barrier`).
- **Transitions** — particle bursts and/or a barrier-hit flash, including god rays/ring/chromatic
  aberration on the flash (`style.transition`).

All of this is built from a shared widget library in `app/src/style_ui.rs`: one editor function per
schema shape, reused everywhere that shape appears (`edit_color_binding`/`edit_scalar_binding` for
the `ColorBinding`/`ScalarBinding` per-note variants — `Constant`/`By velocity`/`By pitch class`/
`By pitch`/`By track`, each with its own inline controls; `optional_section` for every `Option<T>`
field, a checkbox that inserts/removes the value; `edit_glow`/`edit_glow_layers` shared by note and
barrier glow; `edit_fill` shared by a note's own fill and its black-key override).

**`Timed<T>` scope**: `style.notes`/`style.barrier`/`style.transition` are each wrapped in
`Timed<T>` (static, or time-keyed — see `docs/fmstyle-format.md`). The Style tab only edits the
`Static` case, since v1 only ever resolves a `Timed<T>` once at `t = 0.0` anyway. Importing a
`.fmstyle.ron` whose layer is `Keyed` shows a notice and an "Edit as static" button
(`style_ui::timed_static_mut`) that flattens it to `Static` at its `resolve(0.0)` value before any
editing controls for that layer appear — no keyframes are dropped until that button is clicked.

A "Reset to default look" button restores `style` to `project::default_project_style()` — the
app's out-of-the-box look (blue notes, white visible barrier bar, black background), distinct from
`Style::default()` (the schema-neutral default used when an old `.fmstyle.ron`/project file omits
the field entirely).

Note color/fall-speed/roundedness specifics: the black-key darkening under `BlackKeyFill::Auto` is
the natural-key fill's channels multiplied by `0.6`; roundedness ranges `0.0..=3.0` (`0.0` square
corners, `1.0` the renderer's normal rounding, up to `3.0` fully rounded/pill-shaped); fall speed
(pixels/second, default `400.0`, slider range 50–2000) also scales on-screen note length, since a
note's on-screen length is `duration_seconds * fall_speed` — there is no separate "note length"
control.

## Camera-stretch calibration (per-octave keyboard perspective correction)

By default, the 88 piano keys are spaced uniformly between the calibrated left and right edges of
the keyboard (`left_fraction`/`right_fraction`), which matches a camera shooting the keyboard
head-on from far away. Camera-stretch calibration corrects for perspective from a closer or
angled camera, where octaves farther from the lens's optical center appear stretched or
compressed relative to octaves near it.

`KeyboardCalibration.stretch: Option<CameraStretch>` is `None` by default (uniform spacing).
`CameraStretch { c_fractions: [f32; 8] }` holds the canvas-width fractions of the left edge of C1
through C8 — the 8 interior octave boundaries of a standard 88-key keyboard. Combined with the
existing `left_fraction` (A0's left edge) and `right_fraction` (C8's right edge), this gives 10
boundary points bounding the keyboard's 9 octave segments (a partial A0–B0 segment, seven full
octaves, and a final C8-alone segment); each segment is laid out independently, scaled to fit
between its own two boundaries.

**Capturing calibration**: the "Align notes to camera stretch…" button in the Keyboard tab starts
a guided click sequence. A crosshair follows the pointer over the preview image, an instruction
label names the next of the 10 points to click, in order: A0 left edge, C1..C8 left edges, C8
right edge. Escape or the on-screen "Cancel (Esc)" button aborts the sequence without changing the
existing calibration. Once all 10 points are clicked, they're sorted left-to-right and split into
`left_fraction`/`right_fraction` (first/last point) and the 8 interior `CameraStretch::c_fractions`
values — running this flow overwrites the plain left/right calibration too, not just the 8
interior anchors.

**Editing after capture**: once `calibration.stretch` is set, 8 draggable green anchor guides
(labeled C1..C8) appear alongside the ordinary yellow left/right calibration handles, so the
octave boundaries can be nudged individually without redoing the full capture sequence. The 8
anchors are kept ascending and within `(left_fraction, right_fraction)` automatically, regardless
of whether they were moved by a drag, an edit to the plain left/right calibration fields, or a
shrunk keyboard span.

A "Hide calibration guides on preview" checkbox (`UiState::hide_camera_stretch_handles`) hides the
8 anchor guides without touching the saved calibration — useful since they sit at the same
on-screen positions as an `.fmstyle.ron` octave-lines style's own reference lines and would
otherwise cover them.

## Synced audio playback

`crates/audio-playback` (built on `cpal`) plays back a loaded video's own audio track in sync with
the transport position:

- The audio track is fully decoded at load time into stereo `f32` samples, resampled to whatever
  sample rate the output device reports.
- The output callback keeps its own playback cursor that advances by samples between callbacks
  (since redraws, which drive video decode, happen far less often than audio callbacks). The
  cursor is re-anchored to the app's transport position when playback starts or resumes, and
  whenever it drifts more than 50ms from the transport position or a scrub occurs.
- The stream handles whatever channel count and sample format (`f32`/`i16`/`u16`) the output
  device reports, duplicating left/right across however many channels are actually present.
- If the loaded video has no audio stream, no audio stream is built at all; play/pause/seek calls
  on the audio side become safe no-ops.
- Setting `FREEMUSIC_INTERACTION_LOG=1` enables diagnostic tracing of playback/decode/audio/render
  timing after Play is pressed (stops when paused).

## Slider input behavior

All sliders in the Transform tab (brightness, scale, rotation, tilt, translate, crop), the
Keyboard tab (calibration, barrier position), and the Style tab (roundedness, fall speed,
thickness, and the numeric fields inside `style_ui`'s editors) go through a shared
`validated_slider` helper:

- While a slider's numeric text field has keyboard focus, typing does not write into the bound
  value — only egui's internal text buffer changes as you type.
- The typed value commits, unclamped, only when the edit ends (Enter or losing focus). If the
  committed value falls outside the slider's valid range, it reverts to whatever value the field
  held before the edit began, rather than being clamped to the nearest bound.
- Dragging the slider handle itself is always kept within the slider's range, regardless of the
  above.
- Translate X and Translate Y show 3 decimal places; other sliders use egui's default
  auto-computed precision.
- Rotation ranges `-180.0..=180.0` degrees (a full-range control, distinct from the small-angle
  keystone-only `tilt_x`/`tilt_y` terms). Note roundedness ranges `0.0..=3.0`.

Crop and keyboard-calibration sliders additionally enforce a minimum gap between paired fields
(e.g. crop-left vs. crop-right, calibration-left vs. calibration-right) every frame, independent
of the validation behavior above.

## Note editor

A "Note editor" section at the top of the Keyboard tab (`ui::draw_note_editor`) lists every note
currently playing at the transport's current frame and lets the user exclude specific notes from
the note highway and playback, edit a note's duration, or add new notes — all without ever
rewriting the loaded `.mid` file on disk. The file is parsed once at load time and never read from
or written to again; all edits are stored separately in the project file as override lists.

### Identity and persistence

- `project::SkippedNote { track_id, channel, note, start_seconds, end_seconds }` identifies one
  specific MIDI-parsed note occurrence. This works as a stable key because re-parsing the same
  `.mid` bytes is deterministic — a note's derived start/end seconds are identical across reloads.
- `project::NoteDurationEdit { track_id, channel, note, start_seconds, new_duration_seconds }`
  overrides a MIDI-parsed note's duration, keyed the same way (minus `end_seconds`).
- `project::AddedNote { id, channel, note, start_seconds, duration_seconds, velocity }` represents
  a wholly new note with no MIDI-derived identity; `id` is one past the current maximum id among
  added notes (`0` if none exist yet).
- All three lists (`Project.skipped_notes`, `Project.duration_edits`, `Project.added_notes`) are
  persisted fields on the project (`#[serde(default)]`, so older project files load with empty
  lists) and are read by `export::run` as well, so deletions/duration edits/additions apply
  identically to an exported MP4 and the live preview.
- Loading a *different, unrelated* MIDI file (via the Project tab's Open MIDI… button, or
  drag-drop) clears all three lists, since they're keyed to the previously-loaded file's
  track/note/time structure. Loading a saved project instead sets all three lists from the
  project file, so a project's own edits survive its own reload.

### Filtering and the "currently playing" list

- `render::notes::NotesRenderer::rebuild_instances` filters out any note matching an entry in
  `skipped_notes` before building its renderable instance — a skipped note simply never gets
  drawn, so barrier/particle effects (which key off the same filtered note data) never trigger for
  it either.
- A parallel `active_notes: Vec<ActiveNote>` list includes **every** in-range, non-drum note
  regardless of skip status, each carrying its identifying fields plus an `ActiveNote::skipped`
  flag and (for added notes) `added_note_id`. `notes_at(time)` scans this list for notes whose
  `[start_seconds, end_seconds)` window contains the given time, and the app calls this every
  redraw to populate the note editor's table (`UiState.notes_now`). Because it includes skipped
  notes too, an already-deleted note still shows up in the table with a restore option rather than
  disappearing.
- A note's on-screen duration (and therefore its `end_seconds` for "currently playing" purposes)
  has a 0.1-second floor, matching the minimum visible bar length the highway renders for very
  short/staccato notes — this keeps the note editor's notion of "currently playing" matching what
  is actually visible on screen.

### Delete / restore

Each row in the currently-playing table shows a single icon reflecting its state, with no staging
queue or confirmation step:

- 🗑 on a still-playing MIDI-parsed note pushes a `SkippedNote` key into the skip list.
- ♻ on an already-skipped note removes that key from the skip list, restoring it.
- 🗑 on an added note removes it outright from `Project.added_notes` — there is no restore icon
  for an added note, since un-deleting it would mean re-creating it from scratch.

A row's icon and text color (dimmed for a skipped note) are the only visual change; the row itself
never disappears, so every note at the current frame is always exactly one click from being
toggled back. A change takes effect on the next redraw after the compositor rebuilds
(`AppState::applied_skipped_notes`/`applied_duration_edits`/`applied_added_notes` dirty-checks
trigger a `compositor.resize`), the same one-frame latency as any other calibration/style edit.

The currently-playing table has a fixed maximum height (`NOTE_EDITOR_TABLE_HEIGHT = 160.0`) and
scrolls internally rather than growing/shrinking the panel as notes start and stop during
playback; an empty state shows "No notes playing…" inside the same scroll area.

### Duration editing

Every row's duration is an editable `egui::DragValue` (drag or type a value, range
`0.02..=60.0` seconds) instead of a plain label:

- For a MIDI-parsed note, committing a new value writes a `NoteDurationEdit` into
  `Project.duration_edits`. If the typed value matches the note's original parsed duration, any
  existing override is removed instead of stored as a no-op.
- An edited row shows a ↺ button that removes its `NoteDurationEdit`, snapping the field back to
  the original parsed duration on the next redraw.

### Adding notes

A small form below the table (pitch, velocity, and duration fields, laid out with
`horizontal_wrapped` so it folds instead of widening the panel) has a "➕" button that adds a new
note at the current transport position (minus the sync offset) with the entered pitch/velocity/
duration, pushing an `AddedNote` into `Project.added_notes`. An added note is otherwise
indistinguishable from a real MIDI note everywhere downstream — it renders on the highway using a
sentinel `track_id` (`ADDED_NOTE_TRACK_ID = usize::MAX`, guaranteed disjoint from any real track
index), triggers barrier/particle effects, and appears in the timeline's note-density strip.
