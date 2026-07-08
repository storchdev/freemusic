# UI milestones (6a–6e and polish)

Narrative history of the UI restructure and subsequent milestones/fixes, split out of CLAUDE.md.

### UI restructure: offscreen-texture preview, tabbed side panel, timeline (milestone 6c)

- **Why now**: milestone 3's rationale for floating windows over a side panel — "the video quad
  still renders directly to the full swapchain, so a side panel would just paint over a strip of
  it with no way to shrink the video to match" — no longer applies. `app/src/main.rs`'s `redraw`
  now renders `Compositor` (video quad + MIDI waterfall) into an offscreen
  `wgpu::TextureFormat::Rgba8Unorm` texture (`AppState::preview_texture`/`preview_view`, sized to
  `canvas_size`) instead of the swapchain directly, then displays it via `egui::Image` in the
  central panel. Only the egui pass touches the swapchain now (`LoadOp::Clear`, not `Load` — no
  prior pass draws onto it first). `Rgba8Unorm` specifically, not an `Srgb` variant:
  `egui_wgpu::Renderer::register_native_texture` requires exactly that format for a texture shown
  via `egui::Image`.
- **Canvas size is decoupled from window size**, same principle as export's own offscreen
  texture: `AppState::canvas_size` starts at `DEFAULT_CANVAS_SIZE` (1280×720, so the texture and
  its egui registration exist before any video loads) and is resized to the loaded video's own
  decoded resolution (rounded even) by `set_canvas_size` — which recreates the texture, frees the
  old egui `TextureId` and registers a new one (a bind group tied to an old texture's view can't
  just be resized in place), and rebuilds the waterfall layout for the new pixel dimensions. A
  window resize no longer touches the compositor at all (`WindowEvent::Resized` just calls
  `gpu.resize`) — previously it drove `compositor.update_viewport`/`resize` directly since the
  video filled the window.
- **The preview image's on-screen rect is computed once per frame, not assumed to be the whole
  window** (`ui::fit_rect` — contain-fit/letterboxed against the canvas's aspect ratio, inside
  whatever the central panel's `available_rect_before_wrap()` turns out to be after the side
  panel and bottom timeline reserve their own space). This was flagged in the plan as 6c's
  biggest risk: `draw_calibration_handles`/`draw_crop_handles` used to hit-test/paint against
  `ui.max_rect()` (the whole window — valid back when the video was painted directly onto the
  swapchain); they now take that computed `image_rect` instead, otherwise unchanged.
- **Side panel is a hand-rolled tab strip** (`ui::Tab`: `Project` (first — media open + sync
  offset + project save/load), `Keyboard` (calibration; barrier/note-style land here too once 6a
  is implemented), `Transform`, `Export`), replacing the four floating windows milestones 3-5
  used. Built with `egui::Panel::left` — egui 0.35 unified `SidePanel`/`TopBottomPanel` into one
  `Panel` type with `.left`/`.right`/`.top`/`.bottom` constructors and a single `.show(ui, ..)` —
  not a floating `egui::Window`, viable now that the video is a shrinkable `egui::Image` instead
  of something painted under floating windows.
- **Bottom timeline** (`ui::draw_timeline_panel`/`draw_timeline_scrubber`) replaces the plain
  `egui::Slider` scrubber: a custom-painted bar with click/drag-to-seek
  (`ui.allocate_exact_size(.., Sense::click_and_drag())`), a time ruler with a duration-adaptive
  tick interval (`ruler_tick_interval`, aims for ~10 ticks regardless of clip length), and a
  note-density strip (`draw_note_density`) bucketing note onset times into 240 columns sized by
  relative density. That data comes from a new `MidiOverlay::note_start_times`/`Compositor::
  note_start_times` accessor (sorted onset seconds, cached at MIDI-load time in
  `midi_overlay::Loaded`) — `main.rs`'s `load_midi` copies it into `ui_state.midi_note_times`
  each time a MIDI file loads, the same mirror-into-`UiState` pattern already used for
  `midi_name`. Timeline zoom state also lives in `UiState`: scrolling while hovered over the
  scrubber grows/shrinks the visible time window around the cursor (`timeline_zoom` +
  `timeline_view_start_seconds`), so clicks/drags, note-density buckets, ruler ticks, and the
  playhead all map through the visible range instead of always compressing the full song into the
  panel width. Ruler ticks enforce `MIN_RULER_TICK_SPACING` (50px) in addition to the old
  duration-based target, so timecode labels don't crowd when the panel is narrow or zoomed.
  Timeline height is controlled by a custom top-edge drag strip inside the bottom
  panel (`draw_timeline_resize_handle`), clamped to 24-180px for the scrubber strip. Keep the
  panel itself fixed from `ui_state.timeline_height` rather than using egui's built-in
  `Panel::resizable(true)` here: egui intentionally skips storing panel size while dragging and
  commits on release, which fought our live strip-height state and caused black space/snap-back or
  max-height expansion on later hover redraws. The bottom panel also disables egui's own separator
  line (`show_separator_line(false)`) so the custom drag strip is the only visible border/handle;
  otherwise two horizontal gray lines appear, with only the lower custom one being draggable. Draw
  that custom line across `ctx.viewport_rect().x_range()`, not the panel/content `Ui` rect, because
  the latter includes frame margins and leaves visible gaps at the left/right window edges.
- **Native Open Video/Open MIDI dialogs**: `rfd = "0.17"` (added to `app/Cargo.toml`, the version
  the plan identified as already proven at this exact wgpu 29 stack via Neothesia's own
  workspace) backs two buttons in the Project tab. Same request/consume-next-redraw pattern as
  save/load: `ui_state.open_video_requested`/`open_midi_requested` are set by the buttons, and
  `AppState::redraw` pops the corresponding `rfd::FileDialog::pick_file()` and calls the existing
  `load_video`/`load_midi` if the user didn't cancel. **Not yet manually verified that a native
  picker actually appears on this Hyprland/Wayland setup** (`rfd` shells out to a GTK or XDG
  desktop-portal backend depending on what's installed) — worth confirming, and noting here what
  backend it actually used, the first time it's tested. The full File menu bar (New/Open/Save/
  Save As/Exit, Ctrl+S/Ctrl+O) from the original plan's 6b is still outstanding.
- **Sequencing**: per the plan's explicitly-flagged open decision, this shell was built *before*
  6a/6b/6d/6e's remaining controls rather than building them as more floating windows first and
  migrating later — those land directly into these tabs as they're implemented.

### Two playback-timing bugs found testing 6c (pre-existing/newly-triggered, not caused by the UI restructure itself)

- **Paused playback stuttering**: `AppState::redraw` used to call `pipeline.seek_and_decode`
  unconditionally every redraw. A redraw can still fire while "paused" for unrelated reasons
  (cursor blink in a focused text field, hover animations, mouse movement) — and repeatedly
  calling `seek_and_decode` with the *same* frozen position isn't always a no-op: the displayed
  frame's own timestamp is always slightly `<=` the transport position it was shown for, so each
  re-entry could trip the "not caught up yet" branch and decode one more frame forward, forever.
  Fixed by `AppState::last_decoded_position`: decode is now skipped entirely unless the transport
  position actually changed since the last decode.
- **Playback jumping back and looping ~20 frames**: a real bug in `video_pipeline::
  VideoPipeline::seek_and_decode` itself, unrelated to the UI work but only surfaced once actual
  redraw-cadence hiccups (system load, a slow GPU call — anything that makes real wall-clock time
  between two redraws exceed a second) started happening on real hardware. The function already
  distinguished "ordinary playback" from "a scrub" using `MAX_FORWARD_STEP_SECONDS`, but that
  heuristic is about the *size* of a forward jump, not the *reason* for it — so a redraw-cadence
  stall (position legitimately advancing via elapsed `dt`, just by a lot in one step) got treated
  identically to a real scrub: reseek, then show only the first frame decoded afterward, landing
  on a stale keyframe up to ~2 seconds old. Every following redraw saw the same
  still-not-caught-up gap, reseeked to nearly the same spot again, and repeated — visually
  indistinguishable from the video jumping back and stuttering through the same handful of
  frames. Fixed by having the caller distinguish the two cases explicitly instead of leaving
  `seek_and_decode` to infer it from jump size alone: `main.rs` now tracks whether a redraw's
  position change came from consuming `ui_state.seek_request` (an explicit scrub) versus
  ordinary `dt` advancement, and passes that through as `seek_and_decode`'s `exact` parameter —
  `exact = true` for ordinary playback (and export, as before) now means "after a reseek, keep
  decoding forward until we actually reach the target," while `exact = false` stays reserved for
  real scrubs (cheap, approximate, fine since the user is actively dragging and will settle on a
  final position themselves). Moving the also-affected "already covered by the held frame"
  shortcut out from behind `!exact` (it's unconditionally safe — it only returns early when the
  held frame already satisfies the target) was needed alongside this so ordinary playback ticks
  didn't lose that optimization just by switching to `exact = true`.

### Barrier + note-highway styling (milestone 6a)

- `project::KeyboardCalibration` gained a third fraction, `barrier_fraction` (0.0 = top of frame,
  1.0 = bottom; default `0.8`), plus two new structs: `BarrierStyle { color, thickness }` and
  `NoteStyle { color, roundedness }`. All three round-trip through `Project` save/load exactly
  like the existing fields.
- **The plan's original "virtual viewport height" trick (see `m6.md`) does not work, and was not
  used** — worth reading if revisiting this code, since the plan explicitly recommends it.
  Neothesia's vendored shader computes `keyboard_y = view_uniform.size.y * 0.8` and then maps
  that through the ortho projection matrix built from that *same* `size` value
  (`TransformUniform::update(width, height, scale)` sets both fields from the same call). Those
  two uses of `size.y` cancel out exactly: the resulting NDC position is always `-0.6` (80% down
  whatever the render pass's viewport maps NDC onto) no matter what "virtual" height is fed into
  `TransformUniform`. Verified algebraically before implementing, not just by trial and error.
  **What actually moves the hit line**: `wgpu::RenderPass::set_viewport` is a separate, real
  mapping from NDC to physical pixels, independent of anything `TransformUniform` feeds the
  shader. `render::midi_overlay::MidiOverlay::render` calls `set_viewport(0, 0, canvas_w,
  virtual_height, 0, 1)` (solving `0.8 * virtual_height = canvas_height * barrier_fraction` for
  `virtual_height`) immediately before delegating to `WaterfallRenderer::render` — this rescales
  where the shader's fixed -0.6 NDC point lands in real canvas pixels, with no shader fork.
  `TransformUniform` itself is left alone, always built from the real canvas size, since (per the
  above) its own width/height has no effect on the hit line's position anyway.
- **Clipping notes at the barrier** is a `render_pass.set_scissor_rect(0, 0, canvas_w,
  canvas_h * barrier_fraction)` call right alongside the viewport one — a real, ordinary wgpu
  feature, still no shader change. Both calls are scoped to only the waterfall's draw call
  (`video_quad` renders first, at the render pass's default full-canvas viewport, before either is
  set).
- **The barrier itself is a plain `egui` overlay**, not a wgpu pass: `ui::draw_barrier_handle`
  draws a `rect_filled` bar at the calibrated fraction (styled per `BarrierStyle`) and doubles as
  the drag handle (same `Sense::drag()` + accumulated `drag_delta()` pattern as the calibration/
  crop handles, rotated 90°). It's UI-only — the exported MP4 never shows it, only the actual note
  clipping (which export's `Compositor::render` shares with the interactive preview).
- **Note color**: `MidiOverlay` no longer builds `Config::default()` and leaves it alone —
  `set_note_style` calls `Config::set_color_schema` with a single `ColorSchemaV1` derived from
  `NoteStyle::color` (dark/sharp-key variant = base channels darkened by `0.6`), called at the
  top of both `load` and `resize` so a style change picks up on the next rebuild. Since
  `WaterfallRenderer::resize`/`new` both read the color schema fresh from whatever `Config` they're
  given, no upstream change was needed.
- **Note roundedness**: `apply_note_adjustments` (renamed from milestone 3's `apply_left_offset`,
  now doing both jobs in one pass instead of two separate instance loops + two `prepare()` calls)
  multiplies each built `NoteInstance.radius` by `NoteStyle::roundedness` (0.0 = square corners,
  1.0 = Neothesia's own default `key.width() * 0.2`) alongside the existing left-offset shift,
  then re-uploads once.
- `AppState::applied_note_style` mirrors the existing `applied_calibration` dirty-check pattern
  (color/roundedness are baked into instances at build time, so a change needs a full
  `compositor.resize`, not just a per-frame uniform write) — checked in the same `if` alongside
  calibration in `redraw`. `barrier_fraction` needs no separate tracking since it already lives on
  `KeyboardCalibration`, so a barrier drag is caught by the existing calibration comparison.
- **6a's item 3 ("style of the transition into the barrier") was dropped from scope entirely**,
  per user decision during planning — not even the scoped-down "barrier reacts on arrival"
  version. Only barrier position/color/thickness and note color/roundedness (items 1, 2, 4) were
  built.
- Manually verified with a synthetic test video/MIDI (`scripts/gen-test-video.sh` + a throwaway
  ascending-scale SMF): dragging the barrier slider from 0.8 down to 0.05 visibly moves the hit
  line, notes visibly clip exactly at it at both positions (confirmed via screenshot at a
  scrubbed-forward timeline position where a note is mid-fall), and setting roundedness to 0
  visibly squares off the note corners.

### Note fall speed slider (post-6a addition)

- `project::NoteStyle` gained `fall_speed: f32` (pixels/second, default `400.0`), a "Fall speed"
  slider (50–2000) in the Keyboard tab's "Note style" section (`app/src/ui.rs::
  draw_keyboard_tab`), reset by the existing "Reset note style" button since it's just another
  `NoteStyle` field.
- **No separate "note length" control exists, or is needed**: Neothesia's vendored waterfall
  shader (`neothesia-core/src/render/waterfall/pipeline/shader.wgsl`) sizes each note quad as
  `size.y = note.size.y * abs(speed)`, i.e. `duration_seconds * speed` — the same `speed` uniform
  that scales how many pixels the note travels per second of playback. Turning the slider up
  therefore makes notes both fall faster *and* look proportionally longer, matching what was
  asked for.
- **Wiring required touching both `MidiOverlay::load` and `::resize`, asymmetrically**, because
  `WaterfallRenderer::new` and `::resize` don't treat speed the same way upstream:
  `WaterfallRenderer::new` reads `config.animation_speed()` once at construction time, but
  `WaterfallRenderer::resize` never touches speed at all (it only rebuilds note instances/color
  from a fresh `Config`). So `self.config.set_animation_speed(note_style.fall_speed.max(1.0))`
  before `WaterfallRenderer::new` is sufficient in `load`, but `resize` additionally needs an
  explicit `loaded.renderer.pipeline().set_speed(&ngpu.queue, note_style.fall_speed.max(1.0))`
  call after `loaded.renderer.resize(...)` — both `WaterfallRenderer::pipeline()` and
  `WaterfallPipeline::set_speed` are already plain public methods (the latter added upstream for
  exactly this purpose), so no forking was needed.
- `.max(1.0)` guards against `Config::set_animation_speed(0.0)`, which upstream special-cases as
  "invalid, negate the existing speed instead of setting it" rather than erroring — the slider's
  own 50–2000 range keeps the UI from reaching 0 anyway, this is just a defensive floor at the
  `MidiOverlay` boundary.
- No new dirty-check plumbing was needed in `AppState::redraw`: `fall_speed` lives on the same
  `NoteStyle` struct already compared whole (`ui_state.note_style != applied_note_style`) to
  decide whether to call `compositor.resize`, so a fall-speed-only drag already goes through the
  exact same path color/roundedness changes do.
- Not yet manually verified in a running instance (per the "never run the app yourself" rule) —
  worth confirming visually that dragging the slider both speeds up the fall and visibly
  lengthens/shortens notes together, not just one or the other.

### Fixed: notes fading out ("cutting off") before reaching the barrier when it's dragged far from 0.8

- **Symptom**: moving `barrier_fraction` far from its default 0.8 (either direction) made falling
  notes visibly vanish before they reached the barrier line, worse the further the barrier was
  dragged — reported as "notes cut off early". The barrier line itself still landed in the right
  place; it was the notes approaching it that disappeared prematurely.
- **Root cause**: `midi_overlay.rs`'s barrier trick (see `render`'s doc comment above) repositions
  the vendored shader's hardcoded 80%-down hit line by giving *only* `wgpu::RenderPass::
  set_viewport` a `virtual_height` different from the real canvas height — `self.transform.data`
  (fed to the vendored `TransformUniform`, which the vertex shader uses to compute each note's
  `note_pos`/`size` varyings) was still being built from the *real* canvas height in both `load`
  and `resize`. The vendored fragment shader's rounded-rect distance field (`dist()` in
  `shader.wgsl`) compares `@builtin(position)` (real, post-viewport framebuffer pixels) against
  those `note_pos`/`size` varyings (computed in a coordinate system sized by whatever
  `TransformUniform` was given) — the two only agreed when `virtual_height` happened to equal the
  real canvas height (i.e. right at the default `barrier_fraction = 0.8`, where the trick is a
  no-op). Away from that default, the two coordinate systems diverge linearly with a note's
  distance from the top of the canvas — negligible for a freshly-spawned note near y=0, worst for
  a note about to reach the hit line — so `dist()` computes a distance far outside `radius`, the
  `smoothstep` collapses to 0, and the note's alpha goes to zero: it fades to fully transparent
  the closer it gets to the barrier, and the effect scales with how far `barrier_fraction` sits
  from 0.8. (Also found along the way: `load` never called `self.transform.data.update`/`.update`
  at all — it relied entirely on the georeference already set by a prior `resize`, which happens
  to run once at `Compositor::new` time but left the transform stale for any calibration/barrier
  change made between construction and the first MIDI load.)
- **Fix**: extracted the viewport-height formula into `virtual_canvas_height(canvas_h,
  barrier_fraction)` and feed *that* (not the real canvas height) into `self.transform.data.
  update` in both `load` and `resize`, so the same `virtual_height` value drives both the
  physical `set_viewport` call in `render` and the coordinate system `note_pos`/`size` are
  computed in — `builtin(position)` and `note_pos`/`size` are back in the same units regardless of
  `barrier_fraction`, with the barrier's on-screen position unaffected (still solved by the same
  `0.8 * virtual_height = canvas_height * barrier_fraction` relationship). Confirmed by re-deriving
  the shader math by hand (`crates/render/src/midi_overlay.rs`'s new doc comment on `render` walks
  through it) rather than by trial and error; `cargo build`/`clippy` obviously can't catch a
  shader-side coordinate mismatch like this one, so a full manual re-verification of the barrier at
  a few extreme `barrier_fraction` values (not just 0.05, which is what 6a originally tested) is
  still worth doing next time someone has hands on the running app.

### File menu bar and remaining native dialogs (milestone 6b)

- **Post-6b removal**: the top `File` menu bar (`ui::draw_menu_bar`) was removed entirely — there
  is no longer any top bar at all — and its actions (New Project, Open Project…, Save Project
  As…, Exit) folded directly into the Project tab as buttons alongside the pre-existing Open
  Video…/Open MIDI… and Save/Load buttons, so everything project-related lives in one place. Same
  `UiState` request flags as before (`new_project_requested`/`open_project_requested`/
  `save_project_as_requested`/`exit_requested`), just triggered from the tab instead of a menu —
  6d's keyboard shortcuts still route through the same flags, so there are now two trigger paths
  (tab button, shortcut) instead of three.
- Originally (as first built): `ui::draw_menu_bar` (`egui::Panel::top("menu_bar")` + a single
  `ui.menu_button("File", ..)`) added New Project, Open Project…, Save Project (Ctrl+S), Save
  Project As…, and Exit alongside the Open Video…/Open MIDI… buttons 6c already added to the
  Project tab — all of it routed through the same `UiState` request-flag-consumed-next-redraw
  pattern the Project tab's buttons already used, so the menu, the tab buttons, and 6d's keyboard
  shortcuts were three ways to trigger the exact same `AppState` methods, never three separate
  code paths. (Kept here for context on the request-flag plumbing, which is unchanged.)
- **New Project** (`AppState::new_project`) clears the loaded video/MIDI and resets sync/
  calibration/transform/barrier/note style to defaults. It recreates the compositor from scratch
  (same construction `AppState::new` uses) rather than trying to incrementally clear
  `video_quad`'s uploaded texture and `midi_overlay`'s loaded track, since neither exposes an
  "unload" of its own.
- **Open Project…**/**Save Project As…** are `rfd::FileDialog` pick/save calls that just set
  `ui_state.project_path_text` and then call the existing `load_project`/`save_project` — no new
  persistence logic, only a second way to populate the path that the typed text field already
  drove.
- **Exit** needed `event_loop.exit()`, which `AppState::redraw` has no access to (it's called from
  `App::window_event`, which does) — so `WindowEvent::RedrawRequested`'s handler calls
  `state.redraw()` and then checks `state.ui_state.exit_requested` before returning, rather than
  `redraw` triggering the exit itself.

### Familiar keyboard navigation (milestone 6d)

- `AppState` gained a `modifiers: winit::keyboard::ModifiersState` field, updated via
  `WindowEvent::ModifiersChanged` — `WindowEvent::KeyboardInput` itself carries no modifier state
  in winit 0.30, so this is the only way to tell Ctrl+S from a bare S.
- `main.rs::handle_shortcut` (called from the existing `KeyboardInput` match arm, after the
  pre-existing `response.consumed` guard so a focused text field's own key handling still wins)
  adds: Left/Right seek ±1 source-video frame (DaVinci Resolve-style, using
  `VideoPipeline::frame_duration_seconds()` with a 30fps fallback before a video is loaded),
  Shift+Left/Right seek ±1s (relative to `seek_request.unwrap_or(position)`, not the raw
  position, so two quick presses before a redraw consumes the first compound rather than clobber
  each other), Home/End jump to start/end, Ctrl+S save project, Ctrl+O open project, Esc cancel an
  in-progress export. Space (play/pause) now mirrors the UI button's audio-resume gating and
  diagnostic tracing instead of only flipping `ui_state.playing`.
  Every action other than Space sets the same `UiState` request flag the menu bar/tab buttons use
  (see 6b above) rather than calling `AppState` methods directly — one code path.
- J/K/L (the "nice-to-have, skip unless there's time" item from the plan) was skipped.

### Synced audio playback (milestone 6e)

- New crate `crates/audio-playback`, built on `cpal = "0.18"` (matches Neothesia's own workspace
  pin). Decodes a video's audio track fully upfront (`decode_all`, duplicated from — not shared
  with — `crates/export/src/audio.rs`'s near-identical function, since that crate's `audio`
  module is private and this crate has no other reason to depend on `export`) into stereo `f32`
  at whatever sample rate the *output device* reports via `default_output_config()` (not a fixed
  rate — mirrors export's own "ask the encoder what rate it actually chose" approach).
- **Sync design**: `AudioPlayback` stores the app transport position (`position_bits:
  Arc<AtomicU64>`, an `f64::to_bits` seconds value) plus the `cpal::Stream`. `AppState::redraw`
  calls `audio.set_position_seconds(ui_state.position_seconds)` unconditionally every redraw (the
  same position value already driving video decode and `midi_time`), but the output callback
  cannot map that atomic straight to the start of *every* output buffer: redraws are throttled to
  video cadence (e.g. ~30fps) while CPAL callbacks are smaller/more frequent, so doing so replays
  the same short audio chunk several times until the next video redraw and sounds harsh/jarring.
  The callback now keeps a `PlaybackCursor` that advances by samples between callbacks and uses
  the atomic only as a resync anchor: the first callback after play/resume starts from the current
  transport position, and later callbacks snap to the transport only after a scrub or >50ms drift.
  Scrubs also increment `resync_generation`, so even a tiny one-frame seek (smaller than the
  drift threshold) snaps the cursor. This keeps normal playback continuous without letting a real
  seek stay on the old audio cursor.
- **Unpause/seek stutter fixes**: play/resume now sets `audio_resume_pending`, pauses any already
  running stream, and only starts audio after the first playback tick has advanced/decode-synced
  the transport. That first tick is capped to one source-video frame; after audio is active,
  playback uses elapsed wall time (bounded at 1s) instead of the one-frame cap so a long render
  stall does not make the app repeatedly push CPAL's cursor backward and replay old audio. Exact
  timeline clicks/keyboard seeks set `UiState::seek_request_exact`; live drags remain approximate.
  `VideoPipeline::decode_ref` also treats a cached frame up to one frame after an exact target as
  covering that target, because an exact seek can legitimately land on the first frame at/after
  the requested timestamp. Without that tolerance, resuming from an exact seek could immediately
  classify the unchanged transport as a backward jump and pay for a redundant real seek.
- `FREEMUSIC_INTERACTION_LOG=1` enables narrow resume diagnostics only after Play is pressed
  (pause stops the trace). The kept logs are env-gated and intentionally focused:
  `[interaction:frame-start]`, `transport`, `decode`, `audio`, `render`, and `schedule`.
  `[interaction:render]` breaks stalls down into tessellate/texture/acquire/encoder/buffer/
  compositor/egui-render/submit/present timings; keep this gated diagnostic unless it becomes
  misleading.
- **cpal sample-format handling**: mirrors Neothesia's own `SynthBackend::run` pattern exactly
  (`neothesia/src/output_manager/synth_backend.rs`) — a generic `build_typed_stream<T: SizedSample
  + FromSample<f32>>` dispatched from a `match config.sample_format() { F32 => ::<f32>, I16 =>
  ::<i16>, U16 => ::<u16>, .. }`, and `fill_buffer` duplicates L/R into however many channels the
  device actually reports (`channels[ch % 2]`), not assuming exactly stereo.
- **Real bug found and fixed during manual verification**: `AudioPlayback::load` originally called
  `stream.play()` immediately after building the stream, so audio started playing right on video
  load regardless of `ui_state.playing` (always `false` immediately after any load). Caught by
  inspecting `pactl list sink-inputs` right after launching with an audio-bearing test file and
  seeing `Corked: no` before ever touching Play. Per `cpal::traits::StreamTrait::play`'s own doc
  comment ("Streams returned by `build_*_stream` are always stopped, so `play` must be called
  before the data callback will fire"), the fix is simply to *not* call `.play()` in `load()` at
  all — `AppState::redraw`'s existing `audio_playing` dirty-check (mirroring
  `applied_calibration`'s pattern) already calls `set_playing(true)` the first time the user
  actually presses Play, which is the only place `.play()` needs to be called.
- **Not manually verified by ear** — this dev machine's audio capture path (`parec`, including
  against the plain default source with no app involved) captures zero bytes in this environment,
  so even indirect verification (recording the output and checking for the test tone) isn't
  possible here. Verified instead by code review against `cpal`'s documented stream-lifecycle
  contract and by confirming (via `pactl`/process-id cross-checking) that the pre-fix version
  really was producing an uncorked sink-input at load before any Play click. **Whoever next has
  working speakers/mic loopback on this machine should confirm real audio actually plays in sync**
  (load a video with an audio track, hit Play, confirm the audio and the video/notes all move
  together; pause/scrub and confirm it stays aligned) — flagging this explicitly rather than
  silently assuming the code-level verification was sufficient.
- No-audio-track videos: `AudioPlayback::load` checks `has_audio_stream` first and returns
  `Ok(())` without building any stream at all if there isn't one — `is_active()` stays `false`,
  `set_position_seconds`/`set_playing` remain safe no-ops (they just check `self.stream.is_some()`
  first).

### Timeline waveform, scroll UX, and collapsible side panel (post-6e polish)

- **Audio waveform on the timeline**: `AudioPlayback` (in `crates/audio-playback`) now computes a
  downsampled peak-amplitude summary of the whole track at load time —
  `compute_waveform_peaks` buckets the decoded stereo samples into fixed 10ms
  (`WAVEFORM_BUCKET_SECONDS`) windows, taking the louder of L/R per bucket — and exposes it via
  `waveform_peaks()`/`waveform_bucket_seconds()`. A several-minute track produces tens of
  thousands of `f32` entries, not millions of raw samples, so the UI can cheaply re-bucket it
  into however many on-screen columns it needs at any zoom level instead of re-scanning raw audio
  every redraw. `main.rs::load_video` mirrors both into `UiState` right after `self.audio.load`
  (same mirror-into-`UiState` pattern `midi_note_times` already used), and `new_project` clears
  it — this mirroring, not `AudioPlayback` itself, is what `ui.rs` actually reads from, since
  `ui.rs` has no access to `AppState`/`AudioPlayback`.
- **Timeline strip is split into a top half (waveform) and bottom half (MIDI note density)**,
  computed once per redraw as a simple midpoint split of the scrubber's `rect`
  (`draw_timeline_scrubber`). They used to share the full height — one center-aligned, the other
  bottom-aligned — and visually interfered. `draw_waveform` centers within whatever rect it's
  given (unchanged function, just now called with the top half instead of the full strip);
  `draw_note_density`'s bottom margin shrank from a hardcoded 16px to 6px since it no longer
  needs headroom for the time-ruler labels (which are drawn separately, later, against the full
  rect's top regardless of this split).
- **Timeline auto-scroll, two distinct behaviors**, both in `draw_timeline_scrubber`:
  - *Follow-on-seek*: every redraw, if `state.position_seconds` has moved outside
    `[view_start, view_end]` (an arrow-key/Home/End seek, or ordinary playback outrunning a
    zoomed-in view), the view shifts by just enough to bring the playhead back to the edge it
    crossed — not re-centered, so a small nudge stays a small nudge.
  - *Edge auto-scroll while dragging the playhead* (`edge_auto_scroll`): if the pointer enters a
    28px (`EDGE_SCROLL_ZONE_PX`) dead zone at either edge of the timeline widget during an active
    drag, the view scrolls in that direction (speed ramps up toward the physical edge, capped at
    1.5×/sec of the visible duration — `EDGE_SCROLL_MAX_FRACTION_PER_SEC`), so a single drag can
    reach times currently off-screen. Uses `ui.input(|i| i.stable_dt)` for frame-rate-independent
    speed and calls `ui.ctx().request_repaint()` explicitly — needed because holding the pointer
    still at the edge produces no input event of its own to trigger the next frame.
- **Collapsible left side panel** (`draw_side_panel`), via egui 0.35's `Panel::show_switched`
  (built specifically for this: two `Panel` definitions, one narrow/collapsed
  (`SIDE_PANEL_COLLAPSED_WIDTH` 28–56px) and one full (`220–420px`, unchanged from before), both
  `.resizable(true)`, sharing one resize-handle widget). Dragging the expanded panel's edge past
  its `min_size` collapses it; dragging the collapsed strip's edge past its own `max_size`
  expands it back — gives the video/timeline more room without losing the tabs. A small `«`/`»`
  button is a discoverable alternative to the drag, not a replacement for it.
  - **Why the toggle button needs a request flag instead of writing straight into the bool**:
    `show_switched` takes `is_expanded: &mut bool` as a separate argument from the content
    closure, and the closure needs `&mut UiState` as a whole (to reach `draw_project_tab` etc.),
    so `is_expanded` can't be `&mut state.side_panel_expanded` directly — that would be two
    overlapping mutable borrows of `state` alive as sibling arguments to the same call. Fixed by
    copying `state.side_panel_expanded` out into a local `expanded` before the call (which
    `show_switched` mutates directly for drag-driven changes), having the button set
    `state.side_panel_toggle_requested = true` instead of touching `expanded`, then applying that
    flag to `expanded` right after the call returns and writing the merged result back to
    `state.side_panel_expanded`. Same "request flag consumed same redraw" shape already used
    throughout `UiState` (`save_requested`, `open_video_requested`, etc.), just applied to a new
    problem (a bool that two different call sites need to mutate in the same frame).
- **Fixed: side-panel collapse/expand and the File menu's open animation appeared to have a
  "big delay" before responding to a click.** Root cause was unrelated to either feature
  specifically — it was the unthrottled-redraw fix from milestone 6's early days coming back to
  bite in a new way: this app's redraw loop only ever self-schedules for playback cadence or
  export progress (see `about_to_wait`), so it never accounted for egui's *own* animation repaint
  requests (the collapse/expand slide, a menu's open/hover animation, etc. all call
  `Context::request_repaint[_after]` internally while they're mid-transition). A click did
  trigger one immediate redraw (via the generic `on_window_event` repaint nudge), but that single
  frame was all the animation got — with no further redraws scheduled, it looked frozen partway
  through until some unrelated event (e.g. mouse movement) happened to trigger the next frame,
  which read as "a big delay" rather than "stuck". Fixed by reading `full_output.viewport_output`
  for the root viewport's `repaint_delay` each redraw (`Duration::MAX` = "nothing to animate,
  don't schedule"; anything else, including `ZERO`, is a real deadline) into a new
  `AppState::next_ui_redraw_at`, and folding that into `about_to_wait` alongside the existing
  `next_playback_redraw_at` — whichever deadline is sooner wins, and (unlike the playback one)
  this one applies regardless of whether the video is playing, since a menu can be open or the
  panel mid-collapse while paused.

### Fixed: slider text fields fought manual typing instead of validating on commit

- **Symptom**: typing a value directly into any slider's numeric field (Transform tab, Keyboard
  tab) was effectively impossible — the field would revert/snap while still mid-keystroke, before
  a full number could ever be entered.
- **Root cause**: plain `egui::Slider` defaults to `SliderClamping::Always` combined with
  `update_while_editing(true)`, meaning every keystroke that parses to a number is immediately
  written back into the bound value *and* clamped to the slider's range, live, on that same
  frame — there's no notion of "finish typing, then validate." An out-of-range intermediate (or
  final) value doesn't get a chance to exist even transiently.
- **Fix**: `app/src/ui.rs::validated_slider` wraps `egui::Slider` with
  `.clamping(SliderClamping::Never)` and `.update_while_editing(false)`, so the bound value is
  left untouched while the field has keyboard focus (only egui's internal text buffer changes as
  you type), and only commits — unclamped — when the edit ends (Enter or focus loss). Right after
  `ui.add`, if `response.lost_focus()` and the committed value falls outside `range`, it's reverted
  to whatever the field held *before* this edit began (not clamped to the nearest bound). All
  sliders in both tabs (brightness/scale/rotation/tilt/translate/crop in Transform, keyboard
  calibration/barrier/note-roundedness in Keyboard) now go through this helper. Mouse-dragging the
  slider handle itself is unaffected — egui always keeps drag interactions within `range`
  regardless of `SliderClamping`.
- **Crop/keyboard-calibration cross-field min-gap clamps** (`CROP_MIN_GAP`/`CALIBRATION_MIN_GAP`,
  reasserted unconditionally every frame after those sliders) were deliberately left as-is rather
  than folded into `validated_slider`'s revert-on-commit logic: since `update_while_editing(false)`
  now freezes the bound value for the whole duration of a typing session, that per-frame
  reassignment is already a no-op while typing (nothing to fight) and only does anything on the
  commit frame — where it still clamps a gap-violating typed value into the valid band rather than
  fully reverting it. That's a minor inconsistency with the "revert to previous" behavior described
  above, scoped narrowly to the crop-left/right and crop-top/bottom and calibration-left/right
  pairs specifically, accepted as a low-risk tradeoff rather than reworking that gap system (which
  also protects the separate canvas drag handles in `draw_crop_handles`/`draw_calibration_handles`,
  unaffected by any of this).
- **Translate X/Y now show 3 decimal places** (`validated_slider(..., Some(3))`) instead of the
  auto-computed ~2 the rest of the sliders still use, per explicit request — `validated_slider`'s
  `decimals: Option<usize>` parameter drives `Slider::min_decimals`/`max_decimals` only when
  `Some`, so other fields keep egui's default auto-precision.
- Verified by `cargo build`/`cargo fmt`/`cargo clippy --all-targets` (all clean); not yet manually
  exercised in the running app by a human — worth confirming typing an out-of-range value into,
  say, Brightness or Translate X actually reverts to the prior value on Enter, and that a normal
  in-range typed value commits correctly, the next time someone has hands on it.

### Widened rotation and roundedness slider ranges

- **Rotation** (Transform tab) widened from `-45.0..=45.0` to `-180.0..=180.0` degrees
  (`app/src/ui.rs::draw_transform_tab`), so it can flip footage upside-down, not just apply
  small camera-correction angles. This relies on the rotation-shear fix (see the video-quad
  aspect-correction bug above) actually rotating rigidly at any angle — before that fix a wide
  range would have made the warp bug far more visible. `project::VideoTransform`'s doc comment
  updated to reflect that `rotation_degrees` is now a full-range control, distinct from
  `tilt_x`/`tilt_y` which remain small-angle keystone-only terms.
- **Note roundedness** (Keyboard tab) widened from `0.0..=1.0` to `0.0..=3.0`
  (`app/src/ui.rs::draw_keyboard_tab`) so notes can go rounder than Neothesia's own default
  corner radius (still `1.0`), up to fully pill/capsule-shaped for typical note widths. No
  shader-side clamp exists on `radius` in the vendored waterfall SDF
  (`neothesia-core/.../waterfall/pipeline/shader.wgsl::dist`) — pushing `roundedness` far enough
  that `radius` exceeds half a note's shorter dimension makes the rounded-rect distance field
  overestimate distance in the squeezed middle and can visually shrink/distort the note rather
  than erroring, so `3.0` was picked as generous headroom without being unbounded. Neither range
  change touches persistence (`project::NoteStyle`/`VideoTransform` still just store the `f32`,
  no enum/validation) or the crop/calibration min-gap clamp system, which is unrelated.
- Verified by `cargo build`/`scripts/check.sh` (fmt+clippy clean); not yet manually exercised —
  worth dragging both sliders to their new extremes (rotation to ±180, roundedness to 3.0) and
  confirming visually the results look like an upside-down rotation and a fully rounded/pill note
  shape respectively, next time someone has hands on the running app.


### Camera-stretch calibration (per-octave keyboard perspective correction)

- **Motivation**: `KeyboardCalibration` originally only stored `left_fraction`/`right_fraction`
  (the two edges of the keyboard) and every one of the 88 keys was spaced uniformly between them
  (`render::notes::keyboard_layout`'s old single-`neutral_width` approach). That's correct for a
  camera shooting the keyboard head-on from far away, but a real camera at finite distance/angle
  makes octaves further from the lens's center appear stretched or compressed relative to
  octaves near it — a uniform layout drifts out of alignment with the footage the further a note
  is from wherever the two calibration edges happen to sit.
- **Calibration model** (`crates/project/src/lib.rs`): `KeyboardCalibration` gained
  `stretch: Option<CameraStretch>`, `#[serde(default)]` so every project saved before this field
  existed loads as `None` (uniform spacing, byte-for-byte the old behavior).
  `CameraStretch { c_fractions: [f32; 8] }` holds the canvas-width fractions of the left edge of
  C1 through C8 — the 8 interior octave boundaries of a standard 88-key keyboard (A0..C8). The
  other two boundaries needed to fully bound all 9 octave segments (a partial A0-B0 segment,
  seven full octaves, and a final C8-alone segment) are the pre-existing `left_fraction` (A0's
  left edge) and `right_fraction` (C8's right edge) — 10 boundary points for 9 segments.
- **Layout math** (`crates/render/src/notes/mod.rs`): `keyboard_layout` no longer builds one
  `piano_layout::KeyboardLayout` with a single global `neutral_width`. It now calls
  `KeyboardLayout::from_range` once per octave segment, each with its own `neutral_width` derived
  from that segment's *target pixel width* (the distance between its two calibrated boundary
  fractions) divided by its own white-key count — `piano_layout::Octave`'s existing per-key ratio
  math (unmodified, still vendored) then lays out that segment's keys correctly scaled to fit.
  Segments are contiguous (each starts exactly where the previous ends), so keys never gap or
  overlap at octave boundaries. `octave_boundary_fractions` computes the 10 boundary fractions:
  when `stretch` is `None` it places the 8 interior boundaries proportionally to cumulative
  white-key count (2 for the A0-B0 partial octave, then +7 per full octave), which reproduces the
  old uniform layout exactly — so calibrated and uncalibrated projects share one code path.
  `piano_layout::Key`'s fields are getter-only (can't be re-offset from outside that crate once
  laid out independently per segment), so a plain local `LayoutKey { x, width, is_sharp }` replaces
  it as the per-note lookup table.
- **Capture UI** (`app/src/ui.rs`): a "Align notes to camera stretch…" button in the Keyboard tab
  starts `UiState::camera_stretch_capture` (`CameraStretchCapture { points: Vec<f32> }`), which
  `draw_camera_stretch_overlay` drives instead of the ordinary calibration/barrier/stretch-anchor
  handles: a crosshair follows the pointer over the preview image, an instruction label names the
  next of the 10 points (`CAMERA_STRETCH_POINT_LABELS`, in order: A0 left edge, C1..C8 left edges,
  C8 right edge), and each click records that point's x-fraction of the preview rect. Escape or
  the on-screen "Cancel (Esc)" button abort the whole sequence without touching `calibration`.
  Once all 10 land, `finalize_camera_stretch` sorts them (defensive against a click landing
  slightly out of left-to-right order) and splits them into `left_fraction`/`right_fraction` (the
  first/last point) and `CameraStretch::c_fractions` (the 8 interior points) — this is also why
  the plain left/right calibration handles get overwritten by running this flow, not just the 8
  interior anchors.
- **Post-capture editing**: once `calibration.stretch` is `Some`, `draw_camera_stretch_handles`
  draws the 8 anchors as persistent draggable vertical guides (green, labeled C1..C8) alongside
  the ordinary yellow left/right handles, so they can be nudged without redoing the whole click
  sequence. `clamp_camera_stretch` runs unconditionally every frame (same pattern as
  `CALIBRATION_MIN_GAP`/`CROP_MIN_GAP`) to keep the 8 anchors ascending and inside
  `(left_fraction, right_fraction)` regardless of how they got there — a drag, a slider edit to
  the plain left/right calibration fields, or a shrunk keyboard span — via a forward pass
  (ascending floor from `left_fraction`) then a backward pass (descending ceiling from
  `right_fraction`, applied second so a shrunk `right_fraction` wins). `keyboard_layout` also
  floors each segment's pixel width at a tiny minimum so a still-degenerate case can't divide by
  zero, just look visibly wrong until fixed.
- **Crash found on first real use**: opening a project, then a video, then a MIDI file panicked
  immediately with `slice index starts at 9 but ends at 3` inside vendored
  `piano_layout::Octave::sub_range`, loading any MIDI file at all (not specific to a stretch
  calibration — it hit the default/uncalibrated path too, since `keyboard_layout` always builds
  the 9 segments regardless of whether `stretch` is set). **Root cause**: the vendored crate's
  `split_range_by_octaves` chunks a queried `KeyboardRange` by absolute octave boundaries, and
  when a chunk gets truncated (queried range ends before a full 12-note octave completes), it
  computes the truncated end as `end - id` — an *absolute* remaining-note count — and uses it
  directly as an index into the current octave's 12-key table. That's only correct when the
  truncated chunk also starts at index 0 (a C), which happens to always hold for the one range
  shape this crate was ever exercised with before (`KeyboardRange::standard_88_keys()`, whose only
  truncated chunk is the real final one, C8 alone, which is C-aligned). Segment 0 of this
  feature's own layout (`21..24`, the A0-B0 partial octave) starts mid-octave (`21 % 12 == 9`)
  *and* is queried with an end that completes that same octave — from the crate's perspective
  indistinguishable from "truncated before finishing", so it computed the invalid slice `9..3` and
  panicked (via `Rc<[Key]>`'s `Index`, not a `Result`, so no way to recover short of not hitting
  it). **Fix**: rather than querying each segment's own narrow `start..end`,
  `keyboard_layout`/`from_range` is now always called with `start..109` (the real keyboard end),
  discarding the extra trailing keys via `.take(segment_len)` — this makes every segment's query
  an exact suffix of the one known-safe range shape, so the truncation branch only ever fires on
  the true final (always C-aligned) chunk, exactly like before this feature existed. White-key
  counting for the `neutral_width` scale factor still uses the narrow per-segment range (a plain
  scan, not the buggy chunking, so it's unaffected). Added two `#[cfg(test)]` regression tests in
  `crates/render/src/notes/mod.rs` (`keyboard_layout_does_not_panic_and_covers_all_88_keys`,
  `..._with_camera_stretch_does_not_panic`) covering both the default and a stretch-calibrated
  layout — the one piece of this feature judged worth an automated test despite this project's
  usual manual-verification convention, given it's pure per-key math that already caused a real
  crash once.
- Verified by `cargo build`/`cargo test -p render`/`scripts/check.sh` (fmt+clippy clean, both new
  tests pass); the capture flow itself is still not yet manually exercised — worth running it
  against real filmed footage (all 10 clicks, including deliberately clicking slightly out of
  order to confirm the sort-based recovery), dragging a couple of the resulting green anchors,
  saving/reloading the project to confirm `stretch` round-trips through `.fmproj.ron`, and loading
  an old pre-stretch project to confirm it still looks identical (uniform spacing) with the new
  "Clear camera stretch"/"Align notes to camera stretch…" buttons both behaving sensibly, next
  time someone has hands on the running app.

## Note editor (non-destructive MIDI note deletion)

A "Note editor" section at the top of the Keyboard tab (`ui::draw_note_editor`, called from
`draw_keyboard_tab`) lists notes currently playing at the transport's current frame and lets the
user exclude specific ones from the note highway and playback. The original design ask was to
actually rewrite the `.mid` file on disk; this was changed mid-implementation (see git history) to
a **non-destructive skip list persisted in the project file instead** — the loaded `.mid` file is
never read from disk after the initial parse, and never written to. Rationale: rewriting real MIDI
bytes correctly (preserving tempo maps, CC/pedal events, other tracks) needs surgical raw-event
editing that `midi-file`'s derived `MidiNote` model can't support without re-deriving a lot of
what that crate already does internally; a skip list sidesteps all of that risk entirely, at the
cost of the original file always containing the "deleted" notes if opened elsewhere.

- **Identity**: `project::SkippedNote { track_id, channel, note, start_seconds: f64, end_seconds:
  f64 }` (`crates/project/src/lib.rs`) names one specific note occurrence. This works as a stable
  key only because the app never rewrites the source `.mid` file — re-parsing the same bytes is
  deterministic, so a note's `Duration`-derived seconds are bit-identical across reloads.
  `end_seconds` is redundant in practice (track+channel+note+start is already unique for any
  realistic file) but cheap insurance against a pathological unison duplicate.
- **Persistence**: `Project.skipped_notes: Vec<SkippedNote>` (`#[serde(default)]`, so old
  `.fmproj.ron` files load with an empty list), round-tripped through `snapshot_project`/
  `load_project_from_path` in `app/src/main.rs` exactly like `calibration`/`note_style` already
  were — no new save/load code path needed. `export::run` also reads it and threads it into
  `Compositor::load_midi`, so a note deleted in the UI stays deleted in an exported MP4 too.
- **Filtering**: `render::notes::NotesRenderer::rebuild_instances` (`crates/render/src/notes/
  mod.rs`) gained a `skipped: &[SkippedNote]` parameter, threaded through `Compositor::load_midi`/
  `resize` from `crates/render/src/lib.rs`. The skip check is a third `.filter()` alongside the
  pre-existing in-range/non-drum ones, comparing `track_id`/`channel`/`note`/`start_seconds`
  directly against each `MidiNote` before it's turned into a `NoteInstance`. Since this is the same
  point that builds the falling-note quads, a skipped note simply never gets an instance — no
  separate "hide" step needed anywhere else (barrier pulse, particle effects, etc. all already key
  off the same filtered `note_intervals`/`active_notes`).
- **"Currently playing" query**: alongside the existing `NoteInterval` (x-position only, no note
  number — used by `effects::EffectsRenderer` for particles/flashes), `rebuild_instances` now also
  builds a parallel `active_notes: Vec<ActiveNote>` carrying the identifying fields
  (`track_id`/`channel`/`note`/`start_seconds`/`end_seconds`/`skipped`). Unlike `note_intervals`
  (built only for non-skipped notes, since a skipped note gets no rendered instance and shouldn't
  trigger barrier/particle effects), `active_notes` includes **every** in-range/non-drum note
  regardless of skip status, each tagged with `ActiveNote::skipped` — this is what lets the note
  editor list an already-skipped note with a restore option instead of it just vanishing.
  `NotesRenderer::notes_at(time)` / `Compositor::notes_at(time)` linearly scans it for notes whose
  window contains `time` — same "currently held" pattern already used by `effects.rs`'s
  continuous-particle check, just against the richer struct. `AppState::update_midi_position`
  (`app/src/main.rs`) calls this every redraw (cheap — a few hundred notes at most) and stashes the
  result in `UiState.notes_now`, since `ui.rs`'s drawing functions only ever see plain `UiState`
  data, never the compositor directly.
- **Delete/restore flow, entirely inside `ui.rs`, no staging or confirmation**: each row in the
  note table has a single icon reflecting `ActiveNote::skipped` — 🗑 for a still-playing note
  (clicking it pushes a `SkippedNote` key straight into `UiState.skipped_notes`) or ♻ for an
  already-skipped one (clicking it `Vec::retain`s that key back out). There is no queue, no
  "confirm delete" step, and the row never disappears — only its icon (and its text color, dimmed
  via `ui.visuals().weak_text_color()` for a skipped note) changes, so every note at the current
  frame always shows as exactly one of the two states and is always one click from being undone.
  An earlier version of this feature staged deletions in a `pending_delete_notes` queue behind a
  warning dialog; removed per explicit feedback that the immediate-toggle-with-undo design (𝑖.𝑒.
  the skip list itself, always right there to reverse, *is* the safety net) is simpler and doesn't
  need a second confirmation click. This can all happen inline because `ui::draw` already receives
  `&mut UiState` — no round-trip through `main.rs` is needed the way e.g.
  `save_requested`/`open_midi_requested` need one (those trigger real I/O — file dialogs, disk
  writes — that `ui.rs` has no access to).
- **Compositor rebuild**: `AppState` gained `applied_skipped_notes: Vec<SkippedNote>`, dirty-checked
  in `apply_post_ui_updates` alongside the existing `applied_calibration`/`applied_note_layer`
  checks — a change triggers the same full `compositor.resize` rebuild. This means a delete takes
  effect one frame after confirming (the next `apply_post_ui_updates`), same latency as every other
  calibration/style edit in this app.
- **Fixed-height table**: the currently-playing table originally sized itself to its content, so it
  grew/shrank every frame as notes started and stopped during playback. Fixed by wrapping it in an
  `egui::ScrollArea` with a constant `max_height` (`NOTE_EDITOR_TABLE_HEIGHT = 160.0`) and
  `auto_shrink([false, false])` (which otherwise collapses to fit shorter content instead of
  reserving the full height) — a longer list now scrolls within that fixed area instead of resizing
  it. The empty-state message ("No notes playing…") is rendered *inside* the same `ScrollArea`
  rather than replacing it, so the 0-notes case doesn't collapse the section either.
- **Real bug found from a real file** (`~/Downloads/valseexportsmall_final.mid`, reported: at
  4:30.696 the video/highway was clearly showing 3 held notes — G#, D#, G# — but the editor said
  "No notes playing"). Root cause: `ActiveNote::end_seconds` was built from the note's raw
  `note.end.as_secs_f64()`, but the note *rendered* on the highway (`NoteInstance.size`) and
  `NoteInterval::end_seconds` both use `note.duration.as_secs_f32().max(0.1)` — a 0.1s floor so
  very short notes are still visible as a real bar rather than an invisible sliver. This file has
  many staccato/ornament notes only 7–30ms long (confirmed by loading the actual file and dumping
  every note near that timestamp); each visually persists on-screen for the full 100ms floor, but
  the editor's window used to collapse back to the true ~10-30ms span, so it reported "nothing
  playing" tens of milliseconds before the bar actually left the screen. Fixed by building
  `ActiveNote::end_seconds` from the same `start_seconds + duration` (clamped) value as
  `NoteInterval`, so the editor's notion of "currently playing" now matches what's actually
  rendered. Verified by loading the real file directly in a throwaway test and confirming the
  three reported notes (80, 87, 92 = G#, D#, G#) are now flagged active at t=270.696 (removed after
  confirming; not kept as a permanent regression test since it depends on an external file not in
  the repo).
- **Stale-skip-list handling**: `AppState::load_midi` doc comment spells out the rule — it uses
  whatever's currently in `ui_state.skipped_notes` as-is and does not reset it, since a skip list
  keyed to one MIDI file's track/note/time structure is meaningless for a different file loaded
  later. The two sites that load an *unrelated* MIDI file (the Project tab's "Open MIDI…" button,
  and dropping a `.mid` file onto the window) explicitly clear `skipped_notes` immediately before
  calling `load_midi`. `load_project_from_path` instead sets `skipped_notes` from the loaded
  project just before its own `load_midi` call, so the project's own skip list survives that same
  reload.
- Verified by `cargo build`/`scripts/check.sh` (fmt+clippy clean) only — not yet manually exercised
  in the running app. Worth checking next time someone has hands on it: deleting a note actually
  removes it from the highway next frame, the confirm dialog's "Cancel" leaves it playing, deleting
  several notes across different tracks/channels doesn't cross-match the wrong one, saving then
  reloading a project round-trips `skipped_notes` correctly, opening a different MIDI file (via
  button or drag-drop) clears any leftover skip list instead of silently (and coincidentally)
  filtering the new file by stale keys, and an export reflects the same deletions as the live
  preview.

## Note editor: duration editing and adding new notes

Two roadmap items, built as extensions of the note editor above rather than new UI surfaces —
same table, same non-destructive philosophy (the loaded `.mid` file is never read from disk after
the initial parse, and never written to), same "no staging/confirm step, the undo is always right
there" design the skip list already established.

- **Duration editing**: every row in the "currently playing" table now has an editable
  `egui::DragValue` duration field (drag or type a new value, `0.02..=60.0` seconds) instead of a
  plain label. For a MIDI-derived note, committing a new value writes a
  `project::NoteDurationEdit { track_id, channel, note, start_seconds, new_duration_seconds }`
  into `Project.duration_edits` (same identity fields as `SkippedNote`, minus the redundant
  `end_seconds` "insurance" field — not meaningful once duration itself is the thing being
  overridden) — see `ui::apply_duration_edit`/`ui::duration_edit_matches` in `app/src/ui.rs`. If
  the typed value happens to match the note's original duration, the override is removed instead
  of stored as a no-op edit. An edited row gets a ↺ button that removes its `NoteDurationEdit`
  outright, snapping the field back to the original parsed duration next redraw.
- **Adding notes**: a small form below the table (`ui.add_note_pitch`/`add_note_velocity`/
  `add_note_duration_seconds` — plain UI-local fields, not persisted) has an "Add at current
  frame" button that pushes a `project::AddedNote { id, channel: 0, note, start_seconds, \
  duration_seconds, velocity }` into `Project.added_notes`, `start_seconds` taken from the current
  transport position minus the sync offset (the same `midi_time` convention `update_midi_position`
  uses everywhere else). `id` is simply one past the current max id in `added_notes` (`0` if
  empty) — an added note has no MIDI-derived identity to key off the way `SkippedNote`/
  `NoteDurationEdit` do, so an arbitrary per-project counter stands in for one; it doesn't need to
  be persisted separately since it's always recomputed as "next after the current max."
- **Rendering an added note**: `render::notes::rebuild_instances` (`crates/render/src/notes/
  mod.rs`) now builds a `Vec<NoteSource>` — a small normalized struct — by chaining the real
  MIDI-parsed notes (each with any matching `NoteDurationEdit` already resolved into its
  `duration_seconds`) with `added_notes` mapped into the same shape, before the existing sort/
  layout/instance-building loop runs unchanged over the merged list. An added note gets a sentinel
  `track_id` (`ADDED_NOTE_TRACK_ID = usize::MAX`, guaranteed disjoint from any real track's index)
  and is otherwise indistinguishable from a real note everywhere downstream — it lands on the
  highway, triggers barrier/particle effects via `NoteInterval`, and shows up in the timeline's
  note-density strip exactly like a parsed one. `note_starts` (backing that density strip) moved
  from being computed once in `NotesRenderer::load` to being rebuilt every `rebuild_instances`
  call, so an added note (or a duration edit shifting nothing visually but still meaningful) shows
  up without needing a full MIDI reload.
- **Skip/restore has no meaning for an added note**: `ActiveNote::skipped` is always `false` for
  one (checked via `added_id.is_none()` before ever consulting `skipped_notes`) — there's no MIDI
  original to exclude in favor of. Deleting an added note instead removes its entry from
  `Project.added_notes` outright (`ui.rs`'s 🗑 handler branches on `ActiveNote::added_note_id`
  before falling back to the skip-list 🗑/♻ pair), which is also why there's no restore icon for
  one — un-deleting would mean re-creating it from scratch anyway, so a delete is already final in
  the same way it would be for a fresh "never added" state.
- **Precision note**: `NoteSource::start_seconds` is kept as full `f64` (unlike the `f32` used for
  on-screen geometry elsewhere in this file) specifically so identity comparisons against
  `SkippedNote`/`NoteDurationEdit::start_seconds` don't pick up f64→f32→f64 rounding drift — those
  keys are round-tripped through `ActiveNote::start_seconds`, which is itself sourced from this
  same field, so keeping it at full precision preserves the exact-equality matching the original
  `SkippedNote`-only code had before this feature existed.
- **Wiring**: both new lists thread through exactly the same set of call sites `skipped_notes`
  already did — `NotesRenderer`/`Compositor`'s `load`/`resize` signatures, `AppState`'s
  `applied_duration_edits`/`applied_added_notes` dirty-check fields (`app/src/main.rs`, checked
  alongside `applied_skipped_notes` in `apply_post_ui_updates`), `snapshot_project`/
  `load_project_from_path` round-tripping through `Project`, and `export::run` reading both
  straight off the `Project` passed to it. The two "loading an unrelated MIDI file" sites (Open
  MIDI button, drag-drop) now clear all three lists (`skipped_notes`, `duration_edits`,
  `added_notes`), not just the skip list — an added note's pitch/time is arbitrary, not derived
  from the old file's structure, but keeping it around across an unrelated song swap is more
  surprising than useful, so it's cleared the same way stale skip/duration keys already were.
- Verified by `cargo build`/`scripts/check.sh` (fmt+clippy clean) and `cargo test -p project` only
  — not yet manually exercised in the running app. Worth checking next time someone has hands on
  it: dragging a note's duration shorter/longer updates the highway bar and barrier-arrival timing
  next frame, ↺ correctly reverts to the original duration, adding a note at the current frame
  renders it immediately and shows up in the timeline's density strip, deleting an added note
  removes it for good (no restore), saving/reloading a project round-trips `duration_edits`/
  `added_notes`, and an export reflects the same edits/additions as the live preview.
