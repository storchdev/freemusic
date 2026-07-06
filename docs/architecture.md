# Architecture

Detailed architecture notes for freemusic, split out of CLAUDE.md to keep that file short.
See also `docs/ui-milestones.md`, `docs/fmstyle-milestone.md`, `docs/verification.md`, and `docs/fmstyle-format.md`.

### Workspace layout (current)

```
freemusic/
  Cargo.toml            # workspace root; pins wgpu ecosystem versions must stay in lockstep, see below
  app/                   # binary: winit + egui-wgpu shell
    src/main.rs           # event loop, AppState (owns everything), redraw/composite/present, export thread wiring
    src/gpu.rs             # wgpu Instance/Adapter/Device/Surface setup (interactive window only)
    src/ui.rs                # menu bar, tabbed side panel, timeline, calibration/crop/barrier drag handles
  crates/
    project/              # RON project model: paths, sync offset, calibration (incl. barrier), transform, styles
    video-pipeline/       # ffmpeg-next decode + seek, no GPU/UI dependency
    render/                # UI-agnostic compositor (video quad + MIDI waterfall), used headless by export too
    mp4-encoder/            # forked ffmpeg-encoder: parameterized fps, explicit codec selection, optional audio
    export/                  # headless-GPU offline render loop, audio mux, progress/cancel channel
    audio-playback/          # cpal output stream for the loaded video's own audio, driven by transport position
  scripts/               # cargo check, run/screenshot/click/drag the app, gen synthetic test clips
```

### Neothesia reuse (`midi-file`, `neothesia-core`) — superseded by Phase B, kept for history

**As of Phase B of the `.fmstyle.ron` milestone (see "Vendored note pipeline" further below),
`crates/render` no longer depends on `neothesia-core` or wraps `WaterfallRenderer` at all** — the
note pipeline was vendored in-tree specifically to drop this dependency. This subsection describes
the pre-Phase-B design (`midi_overlay.rs`, since deleted) for historical context on *why* certain
things (the `GpuHandles` shape, the calibration-offset trick, the barrier hack fixed and later
replaced) ended up the way they did; for the current implementation see "Vendored note pipeline,
pixel-parity (Phase B..." below. `midi-file` is still a direct dependency (parsing MIDI files was
never Neothesia-render-specific); `piano-layout` is now also a direct dependency instead of going
through `neothesia-core`'s re-export.

`crates/render/Cargo.toml` used to depend on `midi-file` and `neothesia-core` as git deps pinned to an
exact commit SHA of `PolyMeilex/Neothesia` (`e61639b12cc8e466b90406c564da5f9f54d8d1a3`, fetched
2026-06-30) — never `master`, per the plan's "no semver safety net" risk. `neothesia-core`
re-exports `wgpu_jumpstart::{Gpu, TransformUniform, Uniform, Color}` and the whole
`piano_layout` crate at its root, so those don't need separate git-dep entries; add direct
`wgpu-jumpstart`/`piano-layout` deps (same rev) only when a future crate needs them without
going through `neothesia-core`. Both resolve to `wgpu 29.0.4`, matching our own pin — verified
by `cargo build` producing a single `wgpu` entry in `Cargo.lock`, no duplicate-version errors.

`crates/render/src/midi_overlay.rs` wraps `neothesia_core::render::WaterfallRenderer` (this used
to live in `app/src/`, moved out in milestone 5 — see the MP4 export section below for why):
- `WaterfallRenderer::new`/`resize` take a `&neothesia_core::Gpu` (`wgpu_jumpstart::Gpu`), a
  different type from either of our own GPU structs (the interactive window's `app::gpu::Gpu`,
  or export's headless `export::gpu::HeadlessGpu`). Rather than tying `render::midi_overlay` to
  one of those, `midi_overlay::wrap_gpu` builds a `neothesia_core::Gpu` on the fly from a
  `render::GpuHandles<'_>` — a small struct of borrowed `wgpu::Instance`/`Adapter`/`Device`/
  `Queue` refs plus a `TextureFormat`, which both `app::gpu_handles(&gpu)` (interactive) and
  `export::run_inner` (headless) construct from whichever concrete GPU struct they own.
  `neothesia_core::Gpu`'s fields are all `pub` with no constructor invariants to preserve, so
  cloning the cheap `Arc`-backed handles into it works regardless of which side built them.
- Note lanes use the standard 88-key range (`piano_layout::KeyboardRange::standard_88_keys()`)
  always, but are now aligned to the keyboard visible in the footage via `KeyboardCalibration`
  (milestone 3, see below) rather than always spanning the full window width.
  `MidiOverlay::update`'s `time_seconds` argument is expected to already have the sync offset
  subtracted by the caller (`main.rs` computes `midi_time = position_seconds -
  sync_offset_seconds` once per redraw) — `midi_overlay.rs` itself has no notion of sync offset.
- `neothesia_core::config::Config::default()` is used as-is rather than `Config::new()`, which
  would read `~/.config/neothesia/settings.ron` if the user happens to have real Neothesia
  installed — harmless (read-only, falls back to defaults) but an unnecessary external coupling
  we don't need since we don't call `.save()`.
- **Keyboard calibration has no support in `piano_layout` itself** — `KeyboardLayout::from_range`
  always lays keys out starting at local x=0, with no offset parameter. Rather than forking
  `piano-layout`, `midi_overlay::keyboard_layout` sizes the layout to fit the *calibrated width*
  (`(right_fraction - left_fraction) * window_width`, not the full window width), and a separate
  helper `apply_left_offset` shifts every already-built `NoteInstance.position[0]` right by the
  calibrated left edge in pixels, then re-uploads via `WaterfallPipeline::prepare` — both
  `WaterfallRenderer::pipeline()` and `WaterfallPipeline::instances()`/`prepare()` are plain public
  methods, so this needed no upstream changes. Called after both `WaterfallRenderer::new` (in
  `load`) and `::resize` (in `resize`), since either rebuilds instances from scratch at local
  x=0.

### Manual sync, calibration, and project persistence (`project` crate)

- `crates/project` is a small serde/RON model (`Project { video_path, midi_path,
  sync_offset_seconds, calibration, transform }` plus `KeyboardCalibration { left_fraction,
  right_fraction }` and `VideoTransform` — see the milestone 4 section below for the latter)
  with `Project::save`/`Project::load` (`Result<_, String>`, matching the error-handling style
  already used at `midi_overlay::load`'s boundary). `KeyboardCalibration` stores *fractions* of
  window width (0.0–1.0), not pixels, so a calibration survives a window resize or reloading a
  differently-sized video.
- **Sync semantics**: `midi_time = position_seconds - sync_offset_seconds`, computed once per
  redraw in `AppState::redraw` before calling `midi_overlay.update`. Video (plus its audio, once
  export exists) is always the master clock; dragging the offset (an `egui::DragValue` in the
  floating "Sync & Project" window) only moves where notes render relative to it, never touches
  playback position — matches the plan's sync design.
- **Calibration UI is drag handles directly on the video preview**, not sliders — two vertical
  lines (`ui::draw_calibration_handles`) the user drags left/right to mark the real keyboard's
  edges in the footage. Implemented with `ui.interact(rect, id, Sense::drag())` per handle and
  `Response::drag_delta()` accumulated into the fraction (not `interact_pointer_pos()`, which is
  less robust once the pointer leaves the handle's hit-rect mid-drag). Handles stop 60px above
  the bottom of the window so they don't fight the transport bar for drag input.
- **The sync offset / project controls live in a floating `egui::Window`**, not a
  `egui::Panel::left`/`::right`. A side panel would work fine functionally, but panels
  permanently reserve screen space every frame — and since the video quad still renders directly
  to the full swapchain (no offscreen-texture indirection yet, see below), a side panel would
  just paint its background over a strip of the video with no way to shrink the video to match.
  A floating, user-dragged-out-of-the-way `Window` avoids that without requiring the
  offscreen-texture refactor early.
- `AppState` tracks `applied_calibration` (last `KeyboardCalibration` actually used to build the
  waterfall layout) and compares it against `ui_state.calibration` once per redraw, only calling
  `midi_overlay.resize` (a full note-instance rebuild) when they differ — avoids rebuilding every
  single frame during an active drag.
- Project save/load is driven by a text field (typed project file path) plus Save/Load buttons,
  not a native file-picker dialog — deliberately, to avoid an `rfd`-style dependency for what the
  plan doesn't otherwise require yet. `default_project_path` in `main.rs` prefills the field from
  the loaded video's path (`song.mp4` → `song.fmproj.ron`) the first time a video loads, but never
  overwrites a path the user already typed in.

### Video transform: brightness/scale/crop/rotate/tilt/translate (milestone 4)

- `project::VideoTransform` holds `brightness`, `scale` (zoom), `rotation_degrees`,
  `translate_x`/`translate_y` (pan), `tilt_x`/`tilt_y` (keystone), and a crop rect
  (`crop_left`/`crop_right`/`crop_top`/`crop_bottom`, fractions of the source frame like
  `KeyboardCalibration`). All of it lives in `crates/render/src/video_quad.rs` (moved out of
  `app/src/` in milestone 5, unchanged otherwise — see below), applied in a single WGSL pass.
- **Everything except brightness and crop is folded into one 3x3 homography matrix**
  (`video_quad::build_transform`), uploaded as a `mat3x3<f32>` uniform and applied to the quad's
  local `(x, y, 1)` coordinates as `(x', y', w') = transform * (x, y, 1)`; the vertex shader
  feeds `w'` straight into `clip_position.w` and lets the GPU's own perspective divide do the
  keystone distortion (and, for free, perspective-correct the uv interpolation too). This
  directly follows the plan's "build the matrix as a full projective/homography structure from
  day one" note — rotate/translate are the affine terms, tilt is what actually uses the
  projective (third-row) part, and a future true corner-pin tilt would just change how the
  matrix is built, not the shader.
- **Matrix build order matters**: `scale` (letterbox fit × user zoom) → rotate (done in a
  temporarily aspect-corrected space, see below) → translate (pan, applied in the same space the
  final rectangle sits in, not before rotating around the origin) → tilt (last, so it distorts
  the final on-screen rectangle rather than something that then gets rotated again).
- **Rotation needs an aspect-correction step or it shears the image**: NDC's `x` and `y` axes
  don't correspond to equal physical pixel counts unless the window is square, so a plain
  rotation matrix applied directly in NDC visibly skews non-square windows. Fixed by
  `mat3_aspect`: scale `y` by `1/window_aspect` (`window_width/window_height`) *before* rotating
  to enter an isotropic (equal-pixels-per-NDC-unit-on-both-axes) space, rotate there, then scale
  `y` back by `window_aspect` afterward to return to NDC.
- **Bug found and fixed: the two `mat3_aspect` factors above were swapped**, i.e. the pre-rotate
  step scaled `y` by `window_aspect` and the post-rotate step by `1/window_aspect` — backwards
  from the previous bullet's correct derivation. Reported as "the rotate slider warps the footage
  severely" rather than rotating it normally. Confirmed algebraically (not just by eyeballing):
  tracing a purely-horizontal NDC point through a 90° rotation with the swapped factors scales
  its resulting vertical pixel offset by `window_aspect²` relative to the correct answer instead
  of preserving magnitude — e.g. at `window_aspect = 2` (a 2:1 wide window) an 800px horizontal
  offset became 200px vertical instead of 800px, a 4x error, growing/shrinking with how far the
  window departs from square. Fixed in `build_transform`
  (`crates/render/src/video_quad.rs`) by swapping which factor is applied before vs. after
  `rotation` in the `aspect_corrected_rotation` composition. `cargo build`/`clippy` can't catch a
  wrong-but-type-correct matrix composition like this — worth a manual re-check (drag the
  rotation slider on a non-square window/video and confirm the footage rotates rigidly, edges
  staying straight, rather than skewing) next time someone has hands on the running app.
- **WGSL `mat3x3<f32>` uniform-buffer layout gotcha**: in the uniform address space each column
  is padded to 16 bytes (a `vec4`), not the 12 bytes you'd expect from 3 `f32`s — so the matrix
  is really 48 bytes, not 36. The Rust-side `Uniforms` struct mirrors this explicitly as
  `[[f32; 4]; 3]` (`pad_columns` appends a trailing `0.0` per column) rather than `[[f32; 3]; 3]`;
  getting this wrong doesn't error, it just silently misaligns every uniform field declared
  after the matrix in the shader's view of the buffer.
- **Crop is UV remapping, not geometry**: `crop_uv_min`/`crop_uv_max` remap the quad's existing
  `0..1` uvs to a sub-rect of the texture (`crop_min + uv * (crop_max - crop_min)`), so it needs
  no vertex/matrix changes. It does, however, change the *effective aspect ratio* fed into the
  letterbox `scale` calculation in `update_viewport` (`video_w * crop_width_fraction` /
  `video_h * crop_height_fraction`) — forgetting that would letterbox to the uncropped aspect
  and leave bars that don't match the actually-visible (cropped) content.
- **Bind group layout visibility gotcha**: brightness is applied in the *fragment* shader
  (`color.rgb * uniforms.brightness`), but the uniform buffer's `BindGroupLayoutEntry` was still
  `visibility: ShaderStages::VERTEX` only (unchanged from milestone 1, when the uniform only held
  the vertex-only letterbox scale). This built fine but panicked at pipeline-creation time at
  first run (`wgpu error: ... Shader global ResourceBinding ... is not available in the pipeline
  layout ... Visibility flags don't include the shader stage`) — `cargo build`/`clippy` can't
  catch this, it only surfaces from actually running the app. Fixed by widening visibility to
  `ShaderStages::VERTEX | ShaderStages::FRAGMENT`.
- **UI**: brightness/scale/rotation/tilt/translate/crop are all sliders in the Transform tab
  (`ui::draw_transform_tab`) — there is deliberately no on-preview draggable overlay for crop (see
  the removal note below); the keyboard calibration readout has its own matching sliders
  (`Keyboard left`/`Keyboard right` in the Keyboard tab) for the same reason: precise numeric
  entry is awkward with only a drag handle, and those *do* still have a preview overlay (yellow,
  `ui::draw_calibration_handles`) since their geometry — a vertical strip fraction of window width
  — doesn't depend on `VideoTransform` at all.
- `update_viewport` (renamed in spirit, same name) is now cheap enough — one small uniform
  write — that it's called unconditionally every redraw rather than dirty-checked like
  `midi_overlay.resize`'s full note-instance rebuild; it runs *after* the egui pass specifically
  so a slider drag this frame is reflected in this same frame's render instead of lagging by one.
- **The cyan on-preview crop-box overlay was removed entirely** (`ui::draw_crop_handles`, plus the
  `ui::video_display_rect` helper added to try to fix it, both deleted from `app/src/ui.rs`) rather
  than kept working. It never actually tracked the real, on-screen video: `draw_crop_handles`
  positioned the box using only the four crop fractions against the raw `image_rect` — as if the
  video always exactly filled it, `scale == 1.0`, and `translate_x`/`translate_y == 0`. The real
  video quad's position also depends on `scale`, `translate_x`/`translate_y`, *and* crop's own
  effect on the letterbox aspect (the "Crop is UV remapping" bullet above), so the box quietly
  stopped matching the video the moment any of those moved off their defaults — reported as "the
  top of the cyan box is much higher than the top of the video" (a `scale < 1.0` project centers a
  shrunk video with visible margin on every side; the box, oblivious to `scale`, still hugged the
  untransformed frame's edges). A first attempt replicated `video_quad::update_viewport`'s exact
  letterbox-from-crop-aspect formula plus `scale`/`translate_x`/`translate_y` in a new
  `video_display_rect` helper (an NDC → `image_rect`-pixel-space conversion) and repositioned the
  box/handles against that instead — correct for scale/translate, but still deliberately not
  accounting for `rotation_degrees`/`tilt_x`/`tilt_y` (a rotated/tilted quad isn't axis-aligned, so
  it can't be represented by a plain `egui::Rect` without a polygon overlay and a rework of the
  edge-drag hit-testing). Rather than keep that remaining rotation/tilt gap around, the whole
  overlay was deleted — crop is edited via the Transform tab's `crop_left`/`crop_right`/
  `crop_top`/`crop_bottom` sliders only now, same as `scale`/`rotation_degrees`/`translate_x`/
  `translate_y`/`tilt_x`/`tilt_y` already were. `CROP_MIN_GAP` (the `crop_right - crop_left >= 0.1`
  guard, shared with those sliders) is unaffected — only the preview-overlay draw/hit-test code
  and its `draw()` call site were removed.

### `wgpu`/`egui-wgpu` version pinning

`app/Cargo.toml` pins `wgpu = "29.0"` deliberately, *not* the latest wgpu release. `egui-wgpu
0.35` depends on `wgpu 29.0` internally, and having two different `wgpu` major versions in the
dependency graph causes hard type errors (`Renderer::render` expecting a different `RenderPass`
type than the one you built) — the compiler error mentions "multiple different versions of crate
`wgpu`". When bumping `egui`/`egui-wgpu`, check what wgpu version *they* require first and match
it, rather than bumping `wgpu` independently.

### Data flow (video-pipeline)

- All timing is `f64` seconds end-to-end, never frame counts, until a future
  decode/encode boundary — avoids drift across mixed source frame rates (23.976/29.97/30/60).
- `VideoPipeline::seek_and_decode(target_seconds, exact)` is the clone-returning entry point used
  by export/bench code that really needs to own a frame. The interactive app uses
  `seek_and_decode_ref(target_seconds, exact)` instead, which borrows the cached frame and returns
  `DecodedFrameRef { frame, changed }` so playback can skip GPU texture uploads when the requested
  timestamp is still covered by the previous source frame. The decode path is deliberately *not* a
  naive
  seek-then-decode-once call:
  - It holds the last-decoded frame (`current_frame`) and skips touching the decoder entirely if
    `target_seconds` is still covered by it. "Covered" means `target_seconds < pts +
    frame_duration_seconds`, not just `target_seconds <= pts`; using only the exact PTS as the
    cache boundary treats a 30fps frame as valid for a single instant instead of its ~33ms display
    interval, which defeats caching on most redraws. Do not regress the app back to the owned
    `seek_and_decode` path here: at 1080p, returning the cached frame by value clones ~8MB and the
    app used to re-upload that same texture every display redraw (e.g. ~120 times/sec for a 30fps
    video), causing a large CPU/memory/GPU-queue spike without decoding any new video.
  - It only issues a real `Input::seek` for a backward jump or a forward jump bigger than
    `MAX_FORWARD_STEP_SECONDS` (1.0s) — i.e. an actual scrub, not ordinary playback advancing a few
    milliseconds per redraw. Reseeking every redraw would land on/near the *same* nearest keyframe
    every time for any video with a keyframe interval longer than a redraw's time delta, which
    freezes playback at the keyframe instead of animating (this was a real bug, caught by
    screenshotting a burned-in frame counter mid-playback — static color test patterns won't
    reveal it).
  - `exact=false` (scrub/preview) returns the first frame decoded after a seek — cheap,
    approximate, lands near the nearest preceding keyframe. `exact=true` (future export use)
    decodes forward until the frame's timestamp reaches the target — frame-accurate, slower.
- **Seek timestamp units gotcha**: `Input::seek` calls `avformat_seek_file` with
  `stream_index = -1`, which means the timestamp argument must be in `AV_TIME_BASE` (microsecond)
  units (`ffmpeg::rescale::TIME_BASE`), *not* the stream's own `time_base`. Using the stream time
  base there silently produces near-zero seek targets in most cases (a small stream-timebase
  count reinterpreted as microseconds is much smaller than intended) — the failure mode looks like
  "seeking always jumps back near the start" and is easy to misdiagnose as a decoder or GOP issue.
- **Fixed: decoder never opted into multithreading, so real (non-synthetic) footage stuttered**.
  `VideoPipeline::open` used to call `context.decoder().video()?` straight after
  `Context::from_parameters`, never touching `thread_count`/`thread_type` — `avcodec_open2`
  (invoked inside `.video()`) reads those at open time and defaults to single-threaded decode
  when they're left unset. Invisible with `scripts/gen-test-video.sh`'s small 640x360 synthetic
  clips (sub-millisecond decode either way) but real camera footage is a different story: a
  1920x1080 H.264 clip measured (via `crates/video-pipeline/examples/decode_bench.rs`, a
  headless bench that calls `seek_and_decode` in a tight loop with no window/GPU involved —
  `cargo run --release --example decode_bench -p video-pipeline -- <video>`) averaged 4.7ms/frame
  single-threaded, p95 7.1ms, with spikes over 20ms — enough to blow through a single redraw's
  budget and visibly stutter. Fixed by `context.set_threading(threading::Config { kind:
  threading::Type::Frame, count: 0 })` before opening: `Type::Frame` (not `Type::Slice`) because
  consumer camera encoders typically write one slice per frame, so slice-threading has nothing to
  parallelize, while frame-threading decodes multiple frames concurrently at the cost of a few
  frames of decode latency — irrelevant here since nothing in this pipeline decodes "live" against
  a real-time deadline. `count: 0` lets libavcodec pick its own thread count. Re-measured on the
  same clip: avg 1.5ms, p95 2-3.4ms, no more scattered spikes through steady-state playback — the
  only elevated readings left are the first one or two calls (thread-pool spin-up / frame-pipeline
  fill latency), a one-time cost paid once at load, not a recurring stutter.
- **Fixed: catch-up bursts scaled+copied every discarded intermediate frame, not just the one
  actually shown**. Reported as "playback of a real ~1080p30 camera clip is laggy, one CPU core
  pegged" even after the threading fix above (later, once threading also brought all 8 cores into
  it: "all cores spike, still laggy"). Root-caused with a temporary call counter in
  `seek_and_decode` (not kept — added, used, then stripped back out once the fix was confirmed):
  logging showed `decodes/s` in the hundreds while `calls/s` (redraws that actually touched the
  decoder) was only 6-40, i.e. dozens of frames decoded per call. That's `exact = true`'s
  catch-up loop working as designed (decode forward until reaching `target_seconds`) — but every
  loop iteration, including every discarded intermediate frame, was still paying for a full
  `self.scaler.run` (YUV→BGRA swscale over the whole frame) *and* a fresh ~8MB `Vec`
  allocation+copy in `to_decoded_frame`, immediately thrown away the instant the next
  `receive_frame` succeeded. A second counter (gap between `target_seconds` and the held frame's
  `pts_seconds`, plus wall-clock time per call) showed *why* this compounds instead of staying a
  one-off hiccup: gap and call duration grew in lockstep call over call (e.g. 0.079s/80ms →
  0.199s/220ms → ... → 0.466s/485ms before a hard reseek reset it) — a real feedback spiral,
  since `position_seconds` advances by wall-clock `dt` every redraw regardless of how long the
  *previous* redraw's decode took, so a slow catch-up call directly inflates the gap the *next*
  call has to close. Fixed by moving the scale+copy after the "have we reached target yet" check
  instead of before it — `if exact && pts_seconds < target_seconds { continue; }` skips straight
  to the next `receive_frame` for any frame that isn't the one about to be returned, so raw h264
  decode (needed regardless, P/B-frames require their references decoded either way) still
  happens for every frame in the burst, but the expensive per-frame conversion only happens once,
  for whichever frame actually reaches the caller. Measured end-to-end on the same 1080p30 clip:
  process CPU during playback dropped from 300%+ (main thread ~80%, eight `av:h264` worker
  threads ~25-35% each) to ~85%. Some residual periodic ~100ms stutter remained even after this
  fix in testing — not yet root-caused; worth checking GPU-present/window-occlusion behavior (see
  the Hyprland section below) before assuming it's decode-related, since this machine has a
  documented history of occlusion-driven multi-hundred-ms `Surface::get_current_texture` stalls
  that look identical from the decode side.
- **Fixed: video played back visibly darker than reference players (mpv, iOS Camera/Photos)**
  even with no brightness adjustment applied in the editor. Root cause: `ScalingContext::get`
  (`sws_getContext`) only takes pixel format and dimensions — it has no colorspace/color-range
  parameters — so swscale silently used its own hardcoded default (BT.601 matrix, limited/MPEG
  16-235 range in and out) regardless of what the source actually was. Camera-originated footage
  (phones especially) is very commonly BT.709 and/or full-range, so without an explicit
  `sws_setColorspaceDetails` call, values stayed compressed toward the middle of the 8-bit range
  compared to players that read and correct for the stream's real color metadata. Fixed by adding
  `apply_colorspace_details` in `crates/video-pipeline/src/lib.rs`, called right after each
  `ScalingContext::get` (both the initial one in `open` and the `AVERROR_INPUT_CHANGED` reinit
  path in `decode_ref`): maps the decoded frame's/decoder's `color::Space` to the matching
  `SWS_CS_*` constant (falling back to a resolution-based guess — BT.709 for `height >= 720`, else
  BT.601 — when the stream doesn't tag a colorspace, which is common), reads `color::Range` to
  determine whether the source is limited or full range, and calls
  `ffmpeg_next::ffi::sws_setColorspaceDetails` via the crate's `as_mut_ptr()` escape hatch (not
  wrapped safely by `ffmpeg-next`). `dstRange` is always passed as full (1) since the destination
  is BGRA, which has no limited-range encoding of its own.

### RESOLVED: "playback goes very laggy whenever the mouse moves anywhere over the window"

Root cause: a **hybrid-core scheduling artifact** on the dev machine (Intel Core Ultra 7 258V,
Lunar Lake — 8 cores, no SMT: CPUs 0–3 are fast P-cores w/ shared L3, CPUs 4–7 are slow LP-E cores
w/ no L3, per `lscpu -e`). The H.264 decoder was opened with `threading::Config { count: 0 }`, which
lets libavcodec spawn ~one frame-decode worker per logical CPU (8+). Under the ~1000Hz `CursorMoved`
event/system churn from moving the mouse, the scheduler kept the app's main thread on a P-core but
descheduled the decode *workers* onto (or off) the slow LP-E island. That surfaced as
`avcodec_send_packet` blocking while it waited for a not-yet-free worker slot — measured ~150×
(≈200us → 20–30ms/frame), i.e. multi-second playback lag for as long as the mouse moved.

**Primary fix (`crates/video-pipeline/src/lib.rs`): cap the decode worker count**
(`default_decode_threads()` = `min(available_parallelism, 4)`, overridable by
`FREEMUSIC_DECODE_THREADS`). Verified by an on-machine sweep with the mouse moving: `count: 0` →
`send` 20–30ms; `=1` → ~5–8ms (works, thin headroom, too slow for real footage); `=2` → ~3–5ms;
**`=4` → ~150us, flat as steady-state — lag gone**, because 4 workers stay resident on the 4 P-cores
and never spill to the LP-E cores or oversubscribe, with no loss of steady-state throughput. `=0`
restores the old pick-everything behaviour (reproduces the bug on such a CPU).

The cap is safe on other machines (fewer-core boxes get ≤4 anyway; `available_parallelism` respects
cgroup/affinity limits) — the only mild downside is that `export` (offline, decode-heavy, no mouse
contention) also opens its own `VideoPipeline` and so is capped at 4 too, leaving some cores idle on
a big all-P-core machine. Not worth fixing now; if it ever matters, have the export path request a
higher/uncapped thread count so only interactive playback stays capped.

**Secondary fix (`about_to_wait` in `app/src/main.rs`): during playback the video cadence governs;
`next_ui_redraw_at` may no longer schedule a redraw *sooner* than the next frame.** Passive mouse
movement makes egui request a hover repaint every update, which flows through `next_ui_redraw_at`
and *bypasses* the `passive_playback_cursor_move` guard in `window_event` (that guard only suppresses
the *direct* `request_redraw` nudge, not the egui-animation deadline path). Left unclamped it drove
the redraw rate to 40–54fps on a 30fps clip — wasted full redraws that decode no new frame. This
alone did NOT fix the lag (a thread-capped run is smooth even at the inflated fps), but it's correct
and removes real waste, so it stays. While paused, `next_ui_redraw_at` governs fully (smooth
menu/panel animations); while playing, egui animations advance at the frame cadence (imperceptible).

**Also kept: the `MAX_PLAYBACK_DT_SECONDS` / one-frame `dt` cap** in `redraw` (from an earlier
session) — bounds the *runaway spiral* where a slow redraw inflates the next redraw's catch-up. It's
a correct cheap guard (see its doc comment for why capping position-advance beats capping
`decode_ref`'s own budget); it was never the fix for this bug but is worth keeping.

How it was diagnosed (method worth reusing for future playback-perf bugs): the pre-existing
`[perf]` log (`PerfStats::maybe_print`) already showed `decode` ballooning while the GPU-touching
timers `acquire`/`render_submit` stayed low — killing the earlier "GPU/compositor contention"
theory. A temporary per-stage split of `decode` (`VideoPipeline::last_timings()` → `DecodeTimings`,
timers around demux / `send_packet` / `receive_frame` / swscale / readback-copy) then showed the
main-thread userspace stages (swscale, copy) stayed flat while only `send_packet` blew up — the
asymmetry that pinpointed worker-thread starvation over global CPU load, and pointed straight at the
thread-count fix. That split is diagnostic scaffolding; keep it only if useful, it can be trimmed
back to the aggregate `decode` timer. Ruled out along the way: Xwayland (reproduces under native
Wayland too) and Hyprland's cursor path (`cursor:no_hardware_cursors`/`use_cpu_buffer` toggles had
no effect).

### Rendering (app)

- `Gpu` (`app/src/gpu.rs`) owns the wgpu `Instance`/`Device`/`Queue`/`Surface` for the *interactive*
  window. Instance creation goes through
  `wgpu::InstanceDescriptor::new_without_display_handle_from_env()` specifically so `WGPU_BACKEND`
  and friends are respected — needed for the WSL2 Vulkan-driver situation above. `export::gpu::
  HeadlessGpu` is the analogous struct for the export render loop, minus the `Surface`/`config`
  (see the MP4 export section below).
- `render::video_quad::VideoQuad` (`crates/render/src/video_quad.rs`) is a self-contained
  aspect-correct textured-quad pass: uploads the latest `DecodedFrame`'s BGRA bytes to a
  `wgpu::Texture` (recreated only when the frame size changes), and computes a letterbox/
  pillarbox scale uniform from `(video_size, window_size)` each frame. It renders via 6
  hardcoded vertices in the shader (no vertex buffer) — see `shader.wgsl`. Wrapped, along with
  `midi_overlay`, by `render::Compositor` — see the MP4 export section for why this is a
  separate crate rather than living directly in `app`.
- **Fixed: interactive preview was visibly darker than mpv/iOS, on top of (not fixed by) the
  video-pipeline colorspace/range fix above.** The colorspace/range fix made `video-pipeline`'s
  BGRA output match `ffmpeg`'s own reference conversion exactly (verified with a throwaway
  `dump_frame` example comparing mean RGB against `ffmpeg -noautorotate -ss <t> -frames:v 1
  -pix_fmt rgb24` on the same timestamp — they matched to 2 decimal places), but the app still
  looked dark, meaning the real bug was downstream in rendering, not decode. Root cause: the video
  texture (`VideoQuad::TEXTURE_FORMAT = Bgra8UnormSrgb`) is correctly sampled — `textureSample`
  auto-decodes sRGB→linear — but since milestone 6c the compositor renders into the offscreen
  preview texture, which is forced to `Rgba8Unorm` (`app/src/main.rs::PREVIEW_TEXTURE_FORMAT`,
  because `egui_wgpu::Renderer::register_native_texture` requires exactly that format), not an
  sRGB format. A non-sRGB render target does *not* auto-encode linear→gamma on store, so
  `shader.wgsl`'s `fs_main` was writing linear-space color directly into it — those bytes then got
  read back (by egui, and ultimately the display) as if they were already gamma-encoded, crushing
  every midtone dark (e.g. 50% linear gray stores as `~128/255` where correct sRGB-encoded 50%
  gray is `~188/255`). Export was unaffected (`crates/export/src/lib.rs` renders to
  `Bgra8UnormSrgb`, which does auto-encode correctly), which is why this was preview-only. Fixed by
  adding a `manual_srgb_encode` uniform flag (`VideoQuad::new` sets it from
  `!surface_format.is_srgb()`) and a `linear_to_srgb` function in `shader.wgsl`, applied in
  `fs_main` only when the flag is set — so the interactive preview now gets a manual gamma encode
  and export keeps relying on its target's automatic one.
- `AppState::redraw` in `main.rs` is the per-frame orchestrator: advance/clamp the transport
  position and decode → update MIDI → run egui (`ui::draw`) → if egui queued a seek, consume and
  decode it immediately in that same redraw → apply calibration/project/export changes → sync
  audio → render/present. `compositor.upload_frame` only runs when `DecodedFrameRef::changed`.
  Export progress still chains `window.request_redraw()` directly at the end of `redraw`, but
  playback does not: it sets `next_playback_redraw_at`, and `ApplicationHandler::about_to_wait`
  uses `ControlFlow::WaitUntil` to wake at the loaded video's own `frame_duration_seconds`.
  Schedule the next playback wake from `frame_start` when the frame finishes before its cadence
  deadline; if render has already overrun that deadline, schedule from `Instant::now()` plus one
  frame interval so the app does not try to catch up by immediately drawing several stale frames.
- **Two render passes, not one** (changed in milestone 2): a `scene_pass` (`LoadOp::Clear`)
  draws `compositor` (video quad then MIDI waterfall) with a normally-scoped (non-`'static`)
  `RenderPass`, followed by an `egui_pass` (`LoadOp::Load`, so it composites on top without
  clearing) using
  `RenderPass::forget_lifetime()` for egui-wgpu's `'static` requirement. Milestone 1 used a
  single pass for video quad + egui, but `WaterfallRenderer::render<'rpass>(&'rpass mut self,
  pass: &mut RenderPass<'rpass>)` ties its `&mut self` borrow to the pass's lifetime
  *parameter*, and `wgpu::RenderPass` is invariant over that parameter — so it cannot share a
  pass that's already been `forget_lifetime()`'d to `'static` (the borrow checker error is
  "borrowed data escapes outside of method... argument requires that `'1` must outlive
  `'static`"). Splitting into two passes keeps the scene pass's lifetime real (non-`'static`),
  which `WaterfallRenderer` is fine with. The plan's longer-term offscreen-texture design
  (`egui::Image`, decoupling preview resolution from window size) would sidestep this
  differently — still worth doing eventually for the resolution decoupling, but not required to
  unblock milestone 2's compositing.

### Fixed: unthrottled redraw loop pegging the GPU/CPU at all times (perf bug, not tied to a milestone)

- **Symptom**: the app was "incredibly laggy" — reported generally, not tied to any specific
  interaction. Root cause turned out to have nothing to do with decode speed, texture upload
  size, or egui overhead (all measured and found unremarkable); it was the *idle* state that was
  broken. Diagnosed by adding temporary `Instant`-based timing around every stage of `redraw`
  (decode/upload/egui-run/tessellate/acquire/submit/present) plus a periodic tally of
  `WindowEvent` variants received, run against `scripts/gen-test-video.sh`'s synthetic clip. The
  event tally was what actually revealed it: `WindowEvent::RedrawRequested` was firing ~120
  times/sec (matching this machine's display refresh rate) *before any video was even loaded or
  played*, with no other input events driving it — i.e. the window was continuously repainting
  forever at full vsync rate regardless of `ui_state.playing`, burning a full
  decode-check/egui-run/tessellate/compositor-render/egui-render/submit/present cycle every
  ~8ms, 24/7, for no reason. (The `acquire` — `Surface::get_current_texture` — stage dominated
  each of these idle frames at ~7-8ms, which is *expected* Vulkan FIFO/vsync blocking for a
  continuously-presenting loop, not itself a bug; the bug was that the loop was continuous at
  all when nothing had changed.)
- **Cause**: `window_event`'s top-of-function generic repaint nudge —
  ```rust
  let response = state.egui_state.on_window_event(&state.window, &event);
  if response.repaint { state.window.request_redraw(); }
  ```
  — ran for *every* incoming `WindowEvent`, including `WindowEvent::RedrawRequested` itself.
  `egui-winit`'s `on_window_event` (see `egui-winit-0.35.0/src/lib.rs`) returns
  `repaint: true` for `RedrawRequested` along with most other events — its own doc comment
  frames this as "a repaint just happened, the platform may want another one queued", i.e. it's
  deliberately permissive and leaves the actual repaint *policy* up to the caller. This app's
  policy already existed and was correct — `redraw`'s own last lines only call
  `window.request_redraw()` when `ui_state.playing || export_run.is_some()` — but the generic
  top-of-`window_event` check bypassed that policy entirely: handling a `RedrawRequested` event
  synchronously queued the *next* `RedrawRequested`, forever, independent of `redraw`'s own
  end-of-frame decision. A self-sustaining loop with no way to stop itself once started (which is
  immediately, at the first paint after window creation).
- **Fix**: exclude `WindowEvent::RedrawRequested` from the generic nudge, and later also exclude
  passive `CursorMoved` while `ui_state.playing` and no mouse button is held. The first part
  prevents a self-sustaining redraw loop; the second prevents a user simply moving the mouse over
  the window during playback from turning playback back into an input-rate redraw loop that
  bypasses the video-frame scheduler and causes stutter. Mouse-button state is tracked with a
  small `pointer_buttons_down` counter, reset on focus loss, so real drags/scrubs still request
  immediate repaints. Verified with the same event-tally instrumentation for the original bug:
  idle `RedrawRequested` rate dropped from ~120/sec to ~0-1/sec (only real input causes a redraw
  now), and idle CPU for the process dropped from continuous background load to ~1-2%
  (`ps -o pcpu`). Playback (`ui_state.playing`) still self-sustains its own redraw loop correctly
  via `about_to_wait`/`next_playback_redraw_at`, confirmed by playing a 30s synthetic clip
  end-to-end and watching the on-screen frame counter and transport position advance smoothly and
  land exactly on the expected frame for elapsed wall-clock time.
- **Play/Pause ordering gotcha**: keep audio play/pause synchronization *after* the egui pass and
  queued UI actions in `AppState::redraw`. The transport button toggles `ui_state.playing` inside
  `ui::draw`; syncing `AudioPlayback::set_playing` before that pass leaves CPAL in the previous
  state until a later repaint. The visible failure mode was clicking Pause while the cursor still
  hovered the button: hover/input repaints could make the frame/audio feel like it was
  jackhammering until the cursor left. Post-UI audio sync makes the button click affect the audio
  stream in the same redraw that processes the click. Also keep the `playing_before_ui` /
  `playing_changed_by_ui` one-shot redraw: the button label itself is computed before the click is
  processed, so the click frame still paints the old label and needs one immediate follow-up frame
  to show "Play"/"Pause" without waiting for the cursor to move.
- **A separate, unrelated observation from the same debugging session, not a bug**: while
  testing playback under this environment's background-job Xwayland session, `acquire` was once
  seen blocking for a full ~1 second per frame, persisting across many consecutive frames. This
  tracked exactly with the app window being tiled/occluded by Hyprland (not floated — see
  "Screenshotting/driving the app under native Hyprland" below) rather than any app-level issue:
  floating, resizing, and centering the window via `hyprctl` before retesting made it disappear
  completely, and it's consistent with compositors throttling swapchain presentation for
  occluded/non-visible surfaces. Worth remembering if a future perf report mentions "playback
  stalls for about a second at a time" specifically — check window occlusion/focus state before
  assuming it's a decode or render regression.

### MP4 export (milestone 5)

- **`crates/render` was extracted from `app/src/{video_quad,midi_overlay}.rs`** specifically so
  export could reuse the exact same compositing pipeline (video quad + `WaterfallRenderer`)
  against a second, headless GPU context — a binary crate (`app`) can't be a library dependency
  of another crate, and the plan's original architecture already reserved `render` for this. The
  move was mechanical (no behavior change): `render::Compositor` wraps `video_quad::VideoQuad`
  and `midi_overlay::MidiOverlay` with `new`/`load_midi`/`resize`/`upload_frame`/
  `update_viewport`/`update_midi`/`render` methods mirroring what `AppState` used to call on the
  two fields directly. The one real generalization: `midi_overlay::wrap_gpu` used to take
  `&app::gpu::Gpu`; it now takes a `render::GpuHandles<'_>` (borrowed `instance`/`adapter`/
  `device`/`queue`/`texture_format`) so it works identically for `app::gpu::Gpu` (has a
  `Surface`) and `export::gpu::HeadlessGpu` (doesn't).
- **`crates/mp4-encoder` is a fork of Neothesia's `ffmpeg-encoder`** (copied from the pinned
  checkout at `~/.cargo/git/checkouts/neothesia-*/*/ffmpeg-encoder`, not a git dependency —
  upstream needed real changes, matching the plan's flagged risk that this crate "cannot be used
  as-is"). Real deltas from upstream, everything else (the unsafe `ff.rs` FFI wrapper,
  panic-on-FFI-error idiom) copied verbatim:
  - `FRAME_RATE: i32 = 60` (hardcoded) → an `fps: u32` parameter on `mp4_encoder::new(..)`, and
    `gop_size` scales with it (`fps` → one keyframe/sec) instead of a `12` tuned for 60fps
    specifically.
  - Explicit codec selection: `ff::Codec::find_by_name(c"libx264")` / `c"aac"` (falling back to
    `output_format.video_codec()`/`audio_codec()` — i.e. `avcodec_find_encoder(id)` — if absent)
    instead of trusting whichever encoder is registered first for the container's default codec
    ID. `Codec::find_by_name`/`Codec::id` in `ff.rs` are the one small addition needed to make
    this possible; everything else in `ff.rs` is an unmodified copy.
  - **Audio is optional** (`with_audio: bool` on `mp4_encoder::new`) — upstream always created an
    audio stream/codec. `scripts/gen-test-video.sh`'s synthetic clips have no audio track at all
    (and real silent piano recordings are plausible too), so encoding silence would be both
    wasted work and a wrong default; skip the stream entirely instead.
  - `EncoderInfo` gained `sample_rate: i32` (the codec's actual chosen rate, read back after
    construction) — `crates/export`'s audio resampler targets this rather than assuming 44100,
    since the encoder (not the caller) is what actually decides the final rate.
  - `ffmpeg-sys-next` is a plain crates.io dependency (version `"8"`, aliased to the `ffmpeg`
    name in `Cargo.toml` since the copied source refers to it that way — matches Neothesia's own
    workspace-level rename), not a git dependency — it already resolves to the same `8.1.0`
    `ffmpeg-next` (video-pipeline's decoder) pulls in, so the "two different ffmpeg-sys-next
    versions linked" risk the plan flagged doesn't actually bite here; verified via a single
    `ffmpeg-sys-next` entry in `Cargo.lock`.
- **`crates/export`** is the headless render loop, meant to be driven from a background thread
  (`std::thread::spawn`) — `export::run(project: Project, settings: ExportSettings, progress:
  mpsc::Sender<Progress>, cancel: Arc<AtomicBool>)` blocks for the whole export. `Project` is
  taken by value (not `&Project`) specifically so the spawned closure can `move` an owned
  snapshot without fighting the thread's `'static` bound.
  - **Canvas size is the source video's own decoded width/height** (rounded down to even, since
    yuv420p requires it), not the interactive window size — decouples export resolution from
    whatever the window happened to be, and means export works correctly even if no window
    dimension ever matched the video's aspect ratio.
  - **Row-padding bug, found and fixed rather than copied from upstream**: `wgpu` requires
    `bytes_per_row` in a `copy_texture_to_buffer` call to be a multiple of `
    COPY_BYTES_PER_ROW_ALIGNMENT` (256). `neothesia-cli/src/main.rs`'s own example code (the
    pattern this export loop is otherwise modeled on) copies with unpadded `width * 4`, which
    only happens to validate for widths that are multiples of 64 — not true in general (e.g. a
    854-wide source would fail wgpu's validation). Fixed by padding the readback buffer's stride
    to 256 and stripping the padding back off per row before handing tightly-packed BGRA to the
    encoder (mirrors, in reverse, the same per-row trim `video-pipeline::to_decoded_frame`
    already does for the stride the *decoder* hands back).
  - **Audio is decoded in a second, independent `ffmpeg_next::format::input` open**
    (`crates/export/src/audio.rs`), not through `video-pipeline`'s `VideoPipeline` — that type's
    `Input` is mid-seek-state-driven for `exact=true` video decode and isn't structured to
    interleave audio packet extraction, and re-opening the file is cheap next to the cost of the
    render loop itself. `audio::has_audio_stream` is a cheap pre-check (used to decide
    `with_audio` for `mp4_encoder::new` before the encoder — and therefore its chosen
    `sample_rate` — exists yet); the real decode+resample (`audio::decode_all`, using
    `ffmpeg_next::software::resampling::Context`, part of `ffmpeg-next`'s default feature set —
    no extra Cargo features needed) happens once, upfront, targeting that `sample_rate`, then the
    export loop drains `EncoderInfo.frame_size`-sized chunks into `Frame::Audio` calls per output
    frame (same accumulate-then-drain shape as `neothesia-cli`'s own synth-audio loop). A video
    with no audio stream skips all of this and constructs the encoder with `with_audio: false`
    rather than encoding silence.
- **UI**: a floating "Export" window (`ui::draw_export_window`, same pattern as the other
  floating windows) holds the output path text field (defaulted to `<video_stem>_export.mp4`,
  never the source path itself), an fps `DragValue`, and swaps an "Export" button for a
  progress bar + "Cancel" button once `ui_state.export_progress` is `Some`. `AppState::
  start_export` snapshots a `Project` (via `snapshot_project`, shared with `save_project`) and
  spawns the background thread; `redraw` drains `ExportRun::rx` each frame with `try_recv` in a
  loop (non-blocking) and clears `export_run` once a `Done`/`Cancelled`/`Error` message arrives.
  The end-of-`redraw` `window.request_redraw()` condition gained `|| self.export_run.is_some()`
  so the progress bar keeps advancing even while playback is paused — otherwise the event loop's
  `ControlFlow::Wait` would leave it frozen until some unrelated input woke it up.
