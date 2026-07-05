# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**Keep this file up to date after every task**, not just milestone completions or dependency
changes — any change worth explaining to the next agent (a bug found and fixed, a design
decision, a gotcha) gets a note here in the same session it happens, in whatever section it fits
best (or a new one). This file is the fastest way for the next agent to get oriented — don't let
it drift from what the code actually does.

**Commit as the repo owner, no AI attribution, short message only.** Do not append a
`Co-Authored-By: Claude ...` trailer (or any other AI-attribution line) to commit messages —
commits should read like ordinary commits from the repo owner. The actual git author/committer
identity already comes from local git config and needs no special handling. When the user asks
you to commit, write a short one-line commit message (a plain subject line, no body/description
paragraph, no bullet list of changes) — do not use the longer "why"-focused commit body format
that generic Claude Code guidance elsewhere suggests. Any longer explanation of what changed and
why belongs in this file instead, per the "keep this file up to date" note above, not in the
commit message.

**Never run the app yourself. Build/compile only, then ask the user to run it.** Do not invoke
`scripts/run-app.sh`, `cargo run --bin app`, or `scripts/click.sh`/`scripts/drag.sh`/
`scripts/screenshot.sh` under any circumstances — not even for a "quick one-off sanity
screenshot" with no human available. Your own verification stops at `cargo build`/`cargo check`/
`cargo clippy`/`scripts/check.sh` succeeding. `scripts/kill-app.sh` is fine (it only kills a
process, doesn't start one).

When a change needs empirical, runtime confirmation — does the fix actually work, what does a log
show, does a slider/drag/dialog behave correctly — ask the user to run the app themselves and
report back, rather than trying to observe it yourself. Two ways to ask, depending on what's
needed:
- **Visual/interactive behavior** (drag handles, dialogs, on-screen correctness): ask the user to
  drive the app and describe or screenshot what they see — see "Screenshotting/driving the app
  under native Hyprland" below for why automating this yourself is also unreliable on this
  machine (tiling-WM coordinates drift between screenshots), on top of the blanket rule above.
- **Non-visual/diagnostic evidence** (timing, decode stats, crashes, a specific code path
  firing): ask the user to set the relevant environment variable(s) — e.g. `RUST_LOG=debug`, or
  an app-specific one like `FREEMUSIC_DECODE_THREADS`/`WGPU_BACKEND` — and tee the run's output
  into a log file you name, e.g.:
  ```sh
  RUST_LOG=debug scripts/run-app.sh video.mp4 midi.mid 2>&1 | tee /tmp/freemusic-debug.log
  ```
  then share back that file's contents (or the relevant excerpt) for you to read with the `Read`
  tool.

The rest of this document (the "Verifying ..." sections, the WSL2/Hyprland screenshotting
sections, the milestone 3-5 click/drag/screenshot patterns) predates this rule and documents *how
the app has been verified historically* and what tooling exists for the user's own use — read it
for context on what to ask the user to do and what output to expect, not as instructions for you
to execute those scripts yourself.

## What this is

A native desktop app (not Tauri/web — see rationale in the plan doc) that lets piano players
composite real filmed footage with an animated falling-notes MIDI overlay ("note highway"),
manually sync the two, apply basic video transforms, and export the result to a real MP4. It's a
cross-platform (Windows/macOS/Linux) alternative to SeeMusic. The full design — stack rationale,
data flow, phased milestones, and tracked risks — lives in
`~/.claude/plans/i-want-to-plan-vast-shore.md`; read it before making architectural changes.

The project is being built milestone-by-milestone per that plan. Milestones 1 (scaffolding +
plain video playback), 2 (MIDI + note highway overlay), 3 (manual sync + keyboard calibration +
persistence), 4 (brightness/scale/crop/rotate/tilt/translate video transform), and 5 (MP4 export)
are implemented so far. Milestone 6 (UI polish/restructure — full draft at
`~/.claude/plans/m6.md`) is now complete: 6c (offscreen-texture preview, tabbed side panel,
custom timeline), 6a (barrier + note-highway styling), 6b (native Open/Save dialogs and the File
menu bar), 6d (keyboard shortcuts), and 6e (synced audio playback via a new
`crates/audio-playback`) are all implemented — see below.

## Commands

```sh
# Rust toolchain isn't necessarily on PATH in a fresh shell:
source "$HOME/.cargo/env"

cargo build                       # debug build, whole workspace
cargo build --release             # release build
cargo run --bin app -- [video-file] [midi-file]   # both args optional; drag-drop also works
cargo run --bin app -- project.fmproj.ron         # or open a saved project directly
cargo fmt                         # this repo is fmt-clean; run before committing
cargo clippy --all-targets        # this repo is clippy-clean; run before committing
```

CLI args are optional and order-independent, classified by extension exactly like drag-drop is
(`main::main`/`WindowEvent::DroppedFile` in `app/src/main.rs`): `.mid`/`.midi` loads as MIDI,
`.ron` (a saved `.fmproj.ron` project file) loads as a project — same code path as the Project
tab's Load button, so it replaces video/MIDI/sync/calibration/transform/barrier/note style with
whatever the project file contains — and anything else is treated as the video. Passing a
project path alongside a separate video/MIDI path is unusual but not an error; the project load
simply runs and then loads whatever the project itself references, which typically supersedes a
separately-passed video/MIDI path since project load happens instead of (not before/after) the
plain video_path/midi_path branch in `AppState::new`. Drag-drop still only distinguishes
MIDI-vs-video (`WindowEvent::DroppedFile` has no `.ron` case) — dropping a project file onto the
window loads it as a "video" and will fail to open, since that path wasn't part of this change.

`~/.zshenv` on this machine now sources `$HOME/.cargo/env` (it previously only lived in
`.bashrc`/`.bash_profile`/`.profile`, none of which a non-interactive `zsh -c` invocation reads —
only `.zshenv` is read unconditionally by every zsh invocation, login or not). New sessions
should have `cargo` on `PATH` without the manual `source` above; if a given shell was already
running before that fix landed, it won't retroactively pick it up.

### `scripts/` — prefer these over ad-hoc `xdotool`/`ffmpeg` invocations

```sh
scripts/check.sh                          # cargo fmt + cargo clippy --all-targets
scripts/run-app.sh [video] [midi]         # forces the X11 backend; run in the FOREGROUND of a
                                           # backgrounded Bash call (run_in_background:true), do
                                           # NOT add `&`/`disown` inside the script itself — see
                                           # "Screenshotting the app under WSL2" below for why
scripts/kill-app.sh                       # kills a leftover run-app.sh instance by binary path
scripts/find-window.sh                    # prints the app's X11 window id, or nothing if not running
scripts/screenshot.sh <out.png> [WxH+X+Y] # screenshots the app window, optional ImageMagick crop
scripts/click.sh <x> <y> [button]         # click at WINDOW-RELATIVE coordinates
scripts/drag.sh <x1> <y1> <x2> <y2> [btn] # drag between two WINDOW-RELATIVE coordinates
scripts/gen-test-video.sh <out.mp4> [sec] # synthetic frame-counter test clip, default 30s (see
                                           # the video-pipeline verification section for why not 10s)
```

All of `click.sh`/`drag.sh`/`screenshot.sh` resolve the window id themselves via
`find-window.sh` and operate in coordinates relative to it (`xdotool ... --window <id> x y`).
Milestone 4's manual testing burned real time on exactly this distinction: several slider drags
were issued as bare `xdotool mousemove x y` (absolute screen coordinates) against coordinates
read off a window-relative screenshot crop, and every one of them silently no-opped — no error,
the click/drag just landed wherever the pointer already was, often outside the window entirely.
Going through these scripts instead of typing raw `xdotool` makes that mistake structurally
harder to make again.

No automated test suite exists or is planned to substitute for manual verification — the plan
explicitly calls this out: each milestone has a demoable checkpoint that should be run and
eyeballed (and, ideally, screenshotted) rather than covered by unit tests, given the visual/timing
nature of the tool.

### System dependencies (Linux dev environment)

Not vendored, must be present on the machine:
- FFmpeg dev libraries (`libavcodec`, `libavformat`, `libavutil`, `libswscale`, `libswresample`)
  for `ffmpeg-sys-next`'s bindgen step, plus `clang`/`llvm`.
- Vulkan loader + a driver. Under WSL2 specifically, `mesa`'s default packages ship no Vulkan ICD
  at all — install `vulkan-dzn` (Mesa's D3D12-passthrough driver, exposes the real GPU through
  `/dev/dxg`) or `vulkan-swrast` (lavapipe, software fallback) from the `extra` repo. `wgpu`
  respects `WGPU_BACKEND` (e.g. `WGPU_BACKEND=gl`) to force a specific backend if the default
  picks something broken.
- `libxkbcommon-x11` if winit falls back to the X11 backend (e.g. `WAYLAND_DISPLAY` unset) —
  without it winit panics at startup with "Library libxkbcommon-x11.so could not be loaded",
  it does not silently fall back further.

## Architecture

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
- **UI**: brightness/scale/rotation/tilt/translate are sliders in a new floating "Video
  Transform" window (`ui::draw_transform_window`); crop has both sliders in that window *and*
  four draggable edge handles directly on the video preview (`ui::draw_crop_handles`, cyan,
  same `Sense::drag()` + accumulated `drag_delta()` pattern as the yellow keyboard-calibration
  handles, extended to both axes) — both control the same `VideoTransform` fields, so either can
  be used interchangeably. The keyboard calibration readout also grew matching sliders
  (`Keyboard left`/`Keyboard right` in "Sync & Project") for the same reason: precise numeric
  entry is awkward with only a drag handle.
- `update_viewport` (renamed in spirit, same name) is now cheap enough — one small uniform
  write — that it's called unconditionally every redraw rather than dirty-checked like
  `midi_overlay.resize`'s full note-instance rebuild; it runs *after* the egui pass specifically
  so a slider drag this frame is reflected in this same frame's render instead of lagging by one.

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

### Extensible visual style format (Phase A of the `.fmstyle.ron` milestone)

Full plan: `~/.claude/plans/potentially-very-big-milestone-vectorized-seal.md`. This milestone's
goal is a data-driven `.fmstyle.ron` format that can describe note fills (gradient/sheen/glow),
barrier looks (glow/pulse), and barrier-hit transitions (particles/flash) — proving visuals can be
authored as data, not just via the existing color/roundedness/thickness sliders. **Phase A (format
+ plumbing, no visual change) is done; Phases B-F (vendoring the note renderer, actually drawing
any of this, sample-style screenshots) are not started.**

- **New module `crates/project/src/style.rs`** (re-exported flat from `crates/project/src/lib.rs`,
  same pattern as the other `project` types): `Style { version, notes: Timed<NoteLayer>, barrier:
  Timed<BarrierLayer>, transition: Timed<TransitionLayer> }`, with `Style::load`/`save` mirroring
  `Project::load`/`save` exactly (`Result<_, String>`, same RON pretty-printer). Every field is
  `#[serde(default)]`-compatible so older/partial files still load — verified by a unit test that
  strips the whole `style` line out of a serialized `Project` and confirms it still parses with
  `style: None`.
  - `Timed<T> = enum { Static(T), Keyed(Vec<(f64, T)>) }` is the time-keying spine:
    `resolve(t)` returns the last key `<= t`, clamped to the first key if `t` precedes all of
    them. **v1 never actually re-resolves during playback** — nothing calls `resolve` at any time
    other than a one-time `resolve(0.0)` once real rendering consumes a `Style` (Phase B+); the
    type and its boundary behavior are tested now so the spine is provably correct before
    anything is built on top of it.
  - `ColorBinding`/`ScalarBinding` are the per-note property-binding spine: `Constant` resolves
    exactly; `ByVelocity`/`ByPitchClass`/`ByTrack` parse and round-trip but aren't wired to real
    per-note data yet — `resolve_constant()` falls back to a representative constant (ramp's high
    end / first pitch-class entry / first track entry, warned once via a `std::sync::Once` guard
    so a style using these doesn't spam stderr once Phase B+ actually calls this per-note). This
    is intentionally the smallest possible extension point: the enum shape exists so a future
    session can wire real velocity/pitch/track data through without a format break, but nothing
    downstream depends on that data existing yet.
  - `NoteLayer`/`BarrierLayer`/`TransitionLayer` (plus `Fill`, `Sheen`, `Glow`, `Border`,
    `BarrierKind`, `Pulse`, `TransitionKind`, `ParticleSpec`, `FlashSpec`) are the effect-layer
    schema exactly as scoped in the plan. `Border` is schema-only (parses, round-trips, nothing
    reads it) — a deliberately documented no-op, not a bug.
  - `Style::from_legacy(&NoteStyle, &BarrierStyle) -> Style` produces the exact look the existing
    sliders already draw (`Fill::Solid`, no sheen/glow, `BarrierKind::Line`,
    `TransitionKind::None`) — the intended single rendering path once Phase B lands: the renderer
    always consumes a `Style`, either imported or synthesized from the legacy fields. **Nothing
    calls `from_legacy` yet outside tests** — Phase A stops at having it exist and be correct;
    wiring it into `AppState::redraw` has no purpose until Phase B's renderer actually accepts a
    `Style` argument, so that wiring was deliberately left out rather than adding a call site with
    no consumer (would just be dead-looking plumbing).
- **`Project` gained `pub style: Option<Style>`** (`#[serde(default)]`), alongside the existing
  `barrier_style`/`note_style` "quick controls." `snapshot_project`/`load_project_from_path`/
  `new_project` in `app/src/main.rs` all thread it through, same one-line-per-field pattern every
  other `Project` field already uses.
- **"Import style…" button** (Project tab, `app/src/ui.rs::draw_project_tab`) is the only UI
  surface: `UiState.import_style_requested` follows the exact `open_project_requested` template
  (flag set by the button, consumed same-redraw in `main.rs`'s `apply_post_ui_updates` via an
  `rfd::FileDialog` `.ron`-filtered picker, `Style::load`, set into `ui_state.style`). A one-line
  label under the button ("Custom style imported…" / "Using note/barrier sliders…") is the only
  feedback — not in the original plan's UI list verbatim, but cheap and necessary so importing a
  style (which currently changes nothing visible, since Phase B hasn't landed) doesn't look like
  the button silently did nothing.
- **Sample styles shipped early, ahead of the plan's Phase F**: `examples/styles/{gradient-glow,
  barrier-pulse,sparks}.fmstyle.ron` (repo root, not under `crates/`) exercise gradient+sheen+glow,
  barrier glow+pulse, and particles+flash respectively. Generated via a throwaway example binary
  (`crates/project/examples/dump_sample_styles.rs`, run once with `cargo run -p project --example
  dump_sample_styles` and its stdout copied into the three files) rather than hand-typed RON —
  guarantees the checked-in files exactly match what this `ron` version actually serializes
  (notably: unit enum variant `None` serializes as the raw identifier `r#None`, since `None` is a
  reserved word; easy to get wrong by hand). A unit test (`shipped_sample_styles_parse`) reads
  every `.ron` file in that directory and calls `Style::load` on it, so the checked-in samples
  can't silently drift out of sync with the schema as it evolves. They're already importable via
  the button and parse correctly, but **importing one still changes nothing on screen** — the
  note pipeline (see Phase B below) doesn't read `project::style::Style` yet, only the
  `NoteStyle`/`BarrierStyle` "quick controls"; that wiring is Phase C+.
- **Verified so far**: `cargo build`/`scripts/check.sh` (fmt+clippy) clean; `cargo test --workspace`
  passes, including 8 new tests in `crates/project` (`Timed::resolve` boundaries — static, keyed
  mid-range, keyed before-first-key clamp, keyed past-last-key — `ColorBinding::Constant`/
  non-`Constant`-fallback resolution, `Style`/`Project` RON round-trips, old-`Project`-without-
  `style` loading, and the shipped-sample-styles parse check). **Not yet manually exercised in the
  running app** — worth clicking "Import style…", picking one of the three samples, and
  confirming the label under the button flips to "Custom style imported…" and a project save/load
  round-trips `style` correctly (nothing else to check visually until Phase C).
- **Phase C (note fill effects actually rendered) is now done** — see "Note fill effects: gradient,
  sheen, glow" further below. **Phase D (barrier promoted from egui overlay to a real glow/pulse
  render pass) is now done** — see "Barrier glow/pulse pass" further below. **Not started**: Phase
  E (particle/flash transition pass + hit-event precompute).

### Vendored note pipeline, pixel-parity (Phase B of the `.fmstyle.ron` milestone)

Phase B replaces the `neothesia_core`-backed `MidiOverlay` with an in-tree note-highway renderer,
per the plan's call to vendor it the same way `mp4-encoder` was forked from `ffmpeg-encoder` —
done specifically so `barrier_fraction` could become a real shader uniform instead of the fragile
viewport-remapping hack documented (and now deleted) above. **No visual change from before** —
this phase is pixel-parity, proven by re-deriving the math (not just eyeballing), same as every
prior barrier-related change in this file.

- **New module `crates/render/src/notes/`** (`mod.rs`/`pipeline.rs`/`instance.rs`/`shader.wgsl`)
  replaces `crates/render/src/midi_overlay.rs` entirely. `NotesRenderer` (was `MidiOverlay`) keeps
  the exact same public surface `Compositor` already called (`new`/`loaded_name`/
  `note_start_times`/`load`/`resize`/`render`), so `crates/render/src/lib.rs` only needed
  `mod midi_overlay` → `mod notes` and field renames — `app/src/main.rs`'s call sites are
  untouched except `update_midi`, which now needs a `&wgpu::Queue` argument (see below). Exported
  `GpuHandles` (same shape: borrowed `instance`/`adapter`/`device`/`queue`/`texture_format`) moved
  from `midi_overlay` to `notes` but is otherwise unchanged, so `app::gpu_handles` and
  `export::run_inner` (the two places that build one) needed no changes at all.
- **`crates/render/Cargo.toml` dropped the `neothesia-core` git dependency entirely** and added
  `piano-layout` (same pinned rev) as a *direct* dependency — exactly the situation CLAUDE.md's
  Neothesia-reuse section flagged as the trigger for doing this ("add direct wgpu-jumpstart/
  piano-layout deps... only when a future crate needs them without going through
  neothesia_core"). `midi-file` stays a direct dependency, unchanged. Verified via `Cargo.lock`:
  zero `neothesia-core` entries, one `wgpu` entry (no duplicate-version risk), one `piano-layout`
  entry, one `midi-file` entry.
- **Own render pipeline, hand-rolled rather than reusing `wgpu_jumpstart`'s generic `Uniform`/
  `Instances`/`Shape` helpers** (`notes/pipeline.rs::NotesPipeline`) — same manual-wgpu-calls
  style `video_quad.rs` already used elsewhere in this crate, now applied here too since owning
  the shader removed the only reason to keep linking against Neothesia's renderer-side crate.
  Two uniform bind groups (view, time — same split as upstream: view changes on
  resize/calibration, time changes every frame) and one instance buffer that grows (doubling via
  `create_buffer_init`-style recreate, not amortized-growth — fine, MIDI files are not
  update-hot) whenever a MIDI file needs more instance slots than the current capacity.
- **Own `notes/shader.wgsl`**, forked from the vendored `neothesia-core/.../waterfall/pipeline/
  shader.wgsl` with exactly one behavioral change: `keyboard_y` is no longer hardcoded to
  `view_uniform.size.y / 5.0` (i.e. always 80% down) — it reads a new `barrier_fraction` field on
  `ViewUniform` directly (`keyboard_y = view_uniform.size.y * view_uniform.barrier_fraction`).
  Because `ViewUniform.size` is now always built from the *real* canvas size (no more feeding it
  a `virtual_height` that differs from what `set_viewport` gets), `builtin(position)` and the
  vertex shader's `note_pos`/`size` varyings are automatically in the same coordinate system at
  any `barrier_fraction` — the exact bug class the "notes fading out before reaching the barrier"
  section above had to work around now can't occur, because there's no second coordinate system
  to disagree with the first. `render::notes::NotesRenderer::render` now does only a
  `set_scissor_rect` (real canvas pixels, no `set_viewport` override at all) to clip notes past
  the barrier line — the whole `virtual_canvas_height`/`HARDCODED_HIT_LINE_FRACTION` apparatus
  and its long doc-comment derivation in the old `midi_overlay.rs` are gone.
- **`NoteInstance` gained `velocity: f32` and `track_index: f32`** (normalized 0.0-1.0 velocity,
  raw MIDI track index as a float since vertex attributes are all floats), per the plan's explicit
  ask to future-proof for `ColorBinding::ByVelocity`/`ByTrack` (`project::style`) — both fields
  are populated when instances are built but **not read anywhere in the v1 shader**, matching the
  plan's "cheap, future-proofs... even though v1 ignores them in-shader" framing.
- **Instances are built directly in `NotesRenderer::rebuild_instances`** (was
  `WaterfallRenderer::resize`'s internal loop) — same algorithm, ported rather than changed:
  filter notes to the standard 88-key range and non-drum channel, sort by start time (newer notes
  draw on top, matching Neothesia's own convention), look up each note's `piano_layout::Key` for
  x/width/sharpness, and combine the calibrated left-offset + roundedness directly into each
  instance's `position`/`radius` at construction time (previously a second pass,
  `apply_note_adjustments`, mutated already-built instances after the fact — folding it into the
  single construction loop is a minor simplification enabled by no longer needing a
  `piano_layout`-agnostic upstream method signature to work around). The sRGB→linear color
  conversion (`color_to_linear`) is copied verbatim from `wgpu_jumpstart::Color::into_linear_rgb`
  (same source, credited in a doc comment) since that's the one small piece of math actually worth
  keeping rather than re-deriving.
- **`Compositor::update_midi` and `NotesRenderer::update` now take a `&wgpu::Queue` argument**
  (previously the old `MidiOverlay` didn't need one at the call site, since `WaterfallRenderer`
  cloned and kept its own `wgpu::Queue` internally). Both of this phase's two call sites
  (`app/src/main.rs::update_midi_position`, `crates/export/src/lib.rs`'s render loop) already had
  a `Gpu`/`gpu` in scope, so this was a one-line change at each — same pattern
  `update_viewport`/`upload_frame` already used.
- **Verified**: `cargo build`, `scripts/check.sh` (fmt+clippy), and `cargo test --workspace` all
  clean; the vertex-shader math (barrier line position, note fall trajectory, rounded-rect
  distance field) was re-derived by hand against the original vendored shader line-by-line rather
  than assumed correct from a clean build — `cargo build`/`clippy` cannot catch a
  wrong-but-type-correct shader port, per this file's own repeated caution on shader-side bugs
  elsewhere. **Not yet manually run** (per the "never run the app yourself" rule) — worth
  confirming, next time someone has hands on the app, that a loaded MIDI file's notes fall,
  clip at the barrier, and are colored exactly as before at a few different `barrier_fraction`
  values (including far from the 0.8 default, which is what previously exposed the fade-out bug),
  since this phase touches the same code path that bug lived in.

### Barrier glow/pulse pass (Phase D of the `.fmstyle.ron` milestone) — DONE

Phase D promotes the barrier from a plain `egui` overlay (milestone 6a) to a real wgpu render
pass, so it now shows up in exported video too — and reads `project::BarrierLayer`'s
`kind`/`color`/`thickness`/`glow_radius_px`/`pulse` fields instead of just the legacy
`BarrierStyle`'s color/thickness.

- **New module `crates/render/src/barrier.rs`** (+ `barrier.wgsl`), structured like
  `video_quad.rs`: no vertex buffer, six hardcoded unit-quad corners positioned/sized in the
  vertex shader from a uniform, one bind group. `Compositor` gained a `barrier:
  barrier::BarrierRenderer` field, constructed unconditionally in `Compositor::new` (no
  `BarrierLayer` needed at construction time, unlike `notes::NotesRenderer` — see below for why).
  Render order is now video quad → notes → **barrier** (`Compositor::render`), so the bar draws
  on top of falling notes, matching how the old egui overlay always painted on top of everything.
- **Barrier params are cheap uniform writes, not a dirty-checked rebuild** — unlike
  `NoteLayer`'s fill/sheen/glow (baked into `NoteInstance`s at build time, needing a full
  `compositor.resize`), every `BarrierLayer` field only drives a handful of uniform floats. So
  `Compositor::update_barrier` is called *unconditionally every redraw* (`app/src/main.rs`'s
  `apply_post_ui_updates`, right after the existing `update_viewport` call; `crates/export`'s
  render loop, right after its own `update_viewport`/`update_midi` calls) — the same treatment
  `update_viewport` already gets, no `applied_barrier_layer` dirty-check field needed at all.
- **Geometry/color** (`BarrierRenderer::set_style`): a full-canvas-width bar centered at
  `canvas_height * barrier_fraction`, thickness in canvas pixels (not on-screen/logical UI
  points like the old egui bar was) — so at a given `thickness`, how thick the bar reads on
  screen now depends on how much the preview image is scaled to fit the panel, same as the
  falling notes always did. This is an intentional consequence of the bar now living in the same
  canvas-pixel coordinate space export renders in, not a bug.
- **Glow** (`BarrierKind::Glow`) uses the exact same "inflate the rasterized quad by the glow
  radius, zero margin when disabled" trick `notes/shader.wgsl`'s note glow already uses — see
  `barrier.wgsl`'s `glow_margin`/`half_extent`. `BarrierKind::Line` (what `Style::from_legacy`
  produces, so a project with no imported style behaves exactly as before) leaves `flags.x`
  (glow_enabled) at 0 in the shader, matching the old flat-line look regardless of
  `glow_radius_px`.
- **Pulse** (`Option<Pulse>` — brightens on note arrival, decays over `decay_seconds`) is
  **stateless by design**, computed fresh every frame from the sorted note-onset list
  (`notes::NotesRenderer::note_start_times`, the same cached list the timeline's note-density
  strip already uses) rather than any spawned/tracked event queue: `BarrierRenderer::
  pulse_intensity` binary-searches (`partition_point`) for the most recent note start at or
  before the current (sync-offset-subtracted) transport time and linearly decays from
  `pulse.intensity` to 0 over `decay_seconds`. This works because a note's *leading edge* reaches
  the barrier exactly at `note.start` — re-derived from `notes/shader.wgsl`'s vertex math by
  hand: at `time == note.start` the position-offset term is exactly zero, leaving the quad's
  bottom edge sitting precisely at `keyboard_y`. Being stateless also means scrubbing anywhere
  (forward or backward) just recomputes correctly with no "clear on seek" bookkeeping — unlike
  Phase E's transition pass, whose particle pool *is* inherently stateful and will need that.
- **Scissor-rect gotcha**: `notes::NotesRenderer::render` (drawn immediately before barrier in
  the same render pass) leaves a scissor rect clipping to everything *above* the barrier line —
  wgpu scissor state persists across draw calls within one render pass until changed again, so
  `BarrierRenderer::render` must reset it to the full canvas before drawing, or the bar itself
  (which sits at/below that clip edge, and extends further below when glow is enabled) would be
  clipped away instead of rendered. Caught by re-deriving the render-pass state machine by hand,
  not by running the app — `cargo build`/`clippy` can't catch a wrong-but-type-correct scissor
  rect left over from a previous draw call in the same pass.
- **`ui::draw_barrier_handle` now only owns the drag hit-region** — the color/thickness-styled
  rect-fill and "barrier" text label it used to paint are gone (that's the compositor's job now);
  it keeps exactly the `Sense::drag()` + accumulated `drag_delta()` interaction that edits
  `calibration.barrier_fraction`, same pattern `draw_calibration_handles`/`draw_crop_handles` use.
- `Project::effective_barrier_layer()` mirrors `effective_note_layer()` exactly (imported style's
  `barrier` layer wins, else synthesized from `barrier_style`/`note_style` via
  `Style::from_legacy`); `app/src/main.rs` has its own free-function mirror
  (`effective_barrier_layer(&UiState)`) for the same reason `effective_note_layer`'s mirror
  exists (`UiState` isn't a `Project`).
- **Verified**: `cargo build`, `scripts/check.sh` (fmt+clippy), and `cargo test --workspace` all
  clean. **Not yet manually run** (per the "never run the app yourself" rule) — worth importing
  `examples/styles/barrier-pulse.fmstyle.ron` (kind: Glow, thickness 6px, glow_radius_px 24,
  pulse intensity 0.8 / decay 0.35s — already shipped, exercises every new code path at once)
  next time someone has hands on the app: confirm the bar glows continuously at rest, briefly
  flares brighter each time a note arrives then decays back over ~0.35s, and that dragging the
  barrier handle still moves it (now with no visible egui-drawn line under the cursor, since the
  rendered bar itself is what moves). Also worth exporting a short clip and confirming the barrier
  bar (previously invisible in exports) now appears baked into the output frame.

### Note fill effects: gradient, sheen, glow (Phase C of the `.fmstyle.ron` milestone) — DONE

Phase C makes the vendored note pipeline (Phase B) actually read `project::NoteLayer`'s
`fill`/`sheen`/`glow` fields instead of only `roundedness`/`fall_speed` — a vertical gradient fill,
a diagonal specular sheen stripe, and a soft outer glow, all driven by data, no new UI beyond the
existing "Import style…" button.

- **Effective-style wiring**: `render::Compositor::new`/`load_midi`/`resize` (and
  `render::notes::NotesRenderer`'s equivalents) now take `&project::NoteLayer` instead of
  `&project::NoteStyle`. Both `app` and `export` compute this the same way — added
  `Project::effective_note_layer(&self) -> NoteLayer` (`crates/project/src/lib.rs`):
  `self.style.clone().unwrap_or_else(|| Style::from_legacy(&self.note_style,
  &self.barrier_style)).notes.resolve(0.0).clone()`. `app/src/main.rs` has its own
  `effective_note_layer(&UiState) -> NoteLayer` free function doing the identical computation off
  `UiState` fields (can't call the `Project` method directly — building a whole `Project` just to
  resolve this would mean cloning video/MIDI paths for no reason). `AppState::applied_note_layer`
  replaces the old `applied_note_style` dirty-check field — comparing the *resolved* `NoteLayer`
  means a style import (which doesn't touch `note_style`/`barrier_style` at all) is caught by the
  exact same dirty check as a slider drag, one code path instead of two.
- **`NoteInstance` gained `color_bottom`** (`crates/render/src/notes/instance.rs`) alongside the
  existing `color` (renamed `color_top`) — a vertical-gradient fill is baked into each instance at
  build time as two endpoints rather than needing a second draw call or a per-fragment lookup into
  the style layer. For `Fill::Solid`, `color_top == color_bottom`, so the shader's gradient mix is
  unconditionally a no-op and the default (no imported style) look is pixel-identical to Phase B —
  this is what keeps `Style::from_legacy`'s output looking exactly like the pre-Phase-C sliders.
  `NotesRenderer::rebuild_instances` resolves `NoteLayer::fill` once per rebuild (not per note) via
  `ColorBinding::resolve_constant()`, then applies the existing sharp-key darkening (`* 0.6`) to
  both endpoints independently, same shape as the old single-`color` darkening.
- **Sheen and glow are style-wide uniforms, not per-note data** — a `StyleUniform` (new bind
  group 2 in `crates/render/src/notes/pipeline.rs`/`shader.wgsl`) carries fill kind
  (solid/gradient), sheen intensity/width/angle, and glow color/radius/intensity, uploaded once via
  `NotesPipeline::set_style` whenever `apply_view` runs (same call sites as `set_view`/`set_speed`).
  **Deliberately packed as four plain `vec4<f32>`s**, not a natural Rust-shaped struct with
  `vec3`/`f32`/`u32` fields — mirrors the milestone-4 lesson (documented above) that WGSL's
  std140-like uniform layout silently pads odd-sized fields (there it was `mat3x3<f32>`; here it
  would have been every `vec3<f32>` bumping to 16 bytes and every scalar needing manual trailing
  padding). All-vec4 sidesteps needing to reason about that padding at all.
- **Glow needs the rasterized quad to extend past the note's own box**, since the fragment shader
  can only paint pixels the rasterizer actually covers. `shader.wgsl`'s `vs_main` computes the
  note's true (unpadded) `position`/`size` first — fed to the fragment shader unchanged, so the
  rounded-rect distance field and gradient math are unaffected — then, only if `glow_enabled`,
  additionally inflates the *vertex transform's* position/size by `glow_radius_px` on all sides
  before applying `view_uniform.transform`. When glow is disabled the inflation margin is exactly
  `0.0`, making this an algebraic no-op (not just "visually close") — re-derived by hand rather
  than assumed, same standard this file has held every prior shader change to (the rotation-shear
  and barrier-fade-out bugs earlier in this file were both exactly this class of mistake slipping
  past `cargo build`/`clippy`).
- **Fragment shader composition order**: base fill (solid or gradient) → sheen (additive
  brightening along a fixed diagonal band, computed from the fragment's position relative to the
  note's true top-left, independent of any glow inflation) → glow (computed last, since it needs
  the already-composited fill color for `mix(glow_color, fill_color, base_alpha)` — glow_alpha is
  scaled by `(1 - base_alpha)` so it only shows outside/at the note's edge rather than washing out
  the note's own interior).
- **Not yet manually run** (per the "never run the app yourself" rule) — worth importing
  `examples/styles/gradient-glow.fmstyle.ron` (exercises all three: gradient + sheen + glow
  together) via the Project tab's "Import style…" button next time someone has hands on the app,
  scrubbing to where notes are visible, and confirming: notes show a top-to-bottom color blend, a
  diagonal bright stripe sweeps across each note, and a soft halo extends past each note's edges.
  Also worth confirming a project *without* an imported style still looks exactly like before this
  phase (the pixel-parity claim above).

## Verifying changes to `app` or `video-pipeline`

> Per the "never run the app yourself" rule above: don't execute the commands in this section
> yourself. Ask the user to run them and report back what they observed or a teed log file.

Since there's no test suite for playback/timing correctness, changes to the decode or render path
should be checked by actually running the app, not just by `cargo build` succeeding:

```sh
scripts/run-app.sh <video-file>   # or plain `cargo run --bin app -- <video-file>`
```

For anything touching seek/playback timing specifically, a static test pattern isn't enough to
tell whether frames are actually advancing — generate a clip with a visible per-frame marker and a
realistic (multi-second) keyframe interval, e.g. `scripts/gen-test-video.sh out.mp4 30` (wraps the
exact command below):

```sh
ffmpeg -y -f lavfi -i "testsrc=size=640x360:rate=30:duration=30" \
  -vf "drawtext=fontfile=/usr/share/fonts/TTF/DejaVuSans-Bold.ttf:text=frame\ %{n}:fontcolor=white:fontsize=64:x=20:y=20:box=1:boxcolor=black@0.6" \
  -c:v libx264 -g 60 -pix_fmt yuv420p out.mp4
```

This is how the reseek-every-frame and seek-timestamp-units bugs above were actually caught —
`cargo build` and `cargo clippy` were clean for both.

For a synthetic MIDI file to pair with the above (no note-visualizer-specific tooling needed —
any minimal single-track SMF works), a short Python script writing raw MIDI bytes is enough; see
git history for milestone 2's throwaway generator if a reference is useful.

**Real `.mid` files can have their first note far into the file** — if reusing an existing
performance recording (rather than a purpose-built synthetic MIDI) to test the note overlay,
check where its first note-on actually lands before assuming "no notes visible" is a rendering
bug. This is a real trap: `/home/hs/midimaxxing/test.mid` (division=220 ticks/quarter, 120bpm)
has its first note-on at tick 10193 ≈ 23.2s in, so a 10-second test video paired with it never
overlaps any note content at all — visually indistinguishable from the overlay being broken.
Fixed by either using a longer test clip (`gen-test-video.sh`'s default duration is 30s
specifically because of this) or temporarily dragging the sync offset very negative
(`midi_time = position - offset`, so a large negative offset pulls a late note into an early
video position) to confirm the overlay itself does render once the timeline actually overlaps.

### Screenshotting the app under WSL2

WSLg's Weston compositor does **not** support the `wlr-screencopy` protocol, so `grim` fails with
"compositor doesn't support the screen capture protocol" against the default `WAYLAND_DISPLAY`.

**Simplest working method**: force the X11 backend by launching with `WAYLAND_DISPLAY` unset
(`scripts/run-app.sh`, or `env -u WAYLAND_DISPLAY DISPLAY=:0 cargo run --bin app -- ...`
directly; see the `libxkbcommon-x11` dependency note above for why this fallback exists at
all). Once running on X11/XWayland, the window shows up for standard tools:
`scripts/find-window.sh` finds the window id, `scripts/screenshot.sh` captures it (optionally
cropped), and `scripts/click.sh`/`scripts/drag.sh` drive it — all four resolve the window
themselves and operate in coordinates *relative to it*, which matters (see below). Don't bother
with the default Wayland-backend run for screenshotting — that window isn't visible to any X11
tool.

**Window-relative vs. absolute screen coordinates — the mistake that actually happened**:
`xdotool mousemove --window <id> x y` moves relative to the target window, but plain
`xdotool mousemove x y` (no `--window`) is *absolute screen coordinates*. Mixing the two doesn't
error — it just silently no-ops, since the click/drag lands wherever the pointer physically
already was (often outside the window, or over a completely different widget). This actually
happened during milestone 4 testing: coordinates were read off a `screenshot.sh`-style
window-relative crop, but then fed to bare `xdotool mousemove x y` calls, and every single
slider drag did nothing with no error to indicate why. `scripts/click.sh`/`scripts/drag.sh`
exist specifically so this class of mistake requires actively bypassing them to make again —
prefer those over hand-rolled `xdotool` invocations.

A related false lead when a widget doesn't seem to respond: check for a **stuck mouse button**
left down from an earlier `xdotool mousedown` in the same X session that was never matched by a
`mouseup` (X11 button state persists across separate process launches, since it's server-side,
not per-app) — this can make freshly-created windows misinterpret the first pointer motion as a
continuation of an old drag. `xdotool mouseup 1` (and `2`, `3`) unconditionally clears it.

Fallback if X11-backend forcing ever stops working: WSLg surfaces each app window as a *native
Windows window* too, so PowerShell interop from WSL can find and capture it —
`Get-Process | Where-Object MainWindowTitle -like "freemusic*"` finds the window (title is
`"freemusic (<distro>)"`), then `user32.dll`'s `SetForegroundWindow`/`GetWindowRect` +
`System.Drawing.Graphics.CopyFromScreen` over that rect captures it to a PNG. Invoke via
`powershell.exe -NoProfile -ExecutionPolicy Bypass -File <script>.ps1 <out-path>`, converting WSL
paths to Windows paths with `wslpath -w` first. `System.Windows.Forms.SendKeys` from the same
script can drive keyboard input (e.g. Space to toggle play) after focusing the window.

### Screenshotting/driving the app under native Hyprland (current dev machine, not WSL2)

Development moved off WSL2 partway through milestone 6 onto a native Arch/Hyprland (Wayland)
machine. Most of the WSL2 section above still applies unchanged (the X11-backend-forcing trick
in `run-app.sh`, window-relative-vs-absolute coordinates), but a few things differ:

- `DISPLAY=:0` is a real Xwayland server here too (Hyprland spawns one for XWayland app
  compatibility, running as `Xwayland :0 -rootless ...`), so nothing about `run-app.sh` needed to
  change — `find-window.sh`/`screenshot.sh`/`click.sh`/`drag.sh` all keep working as documented
  above.
- `xdotool` is **not preinstalled** on this machine's base image (unlike whatever WSL2 image had
  it already) — `sudo pacman -S xdotool` if `which xdotool` comes back empty.
- **Hyprland tiles new windows by default**, so a freshly launched app window's geometry is
  whatever the tiling layout assigns it (observed: a non-16:9 shape like 760×922 logical), not
  the `1280×720` `with_inner_size` the app actually requested — this makes coordinates computed
  from a screenshot unreliable until the window is floated. Find the window's address (`hyprctl
  clients -j`, match on `.title == "freemusic"` — its `.class` is just `"app"`, the binary name,
  not useful for matching), then `hyprctl dispatch focuswindow "address:<addr>"` →
  `togglefloating "address:<addr>"` → `resizewindowpixel "exact <W> <H>,address:<addr>"` →
  `centerwindow`. Even then, the resize can be applied asynchronously — a screenshot taken
  immediately after dispatching it can still capture the pre-resize size, so re-check the
  screenshot's own reported dimensions (or re-query `hyprctl clients -j`) before trusting
  coordinates computed from it, rather than assuming the size you just requested took effect.
- **Prefer asking the user to drive interactive verification themselves** (clicking tabs,
  dragging calibration/crop handles, exercising playback) rather than automating it with
  `click.sh`/`drag.sh` — this was an explicit ask during milestone 6 work, and matches the
  environment reality above: coordinate-based automation against a tiling WM is slow (every
  click needs a fresh screenshot to re-derive coordinates against, since geometry can shift
  between screenshots) and doesn't obviously get more reliable with more scripting. Building,
  launching (`scripts/run-app.sh`, backgrounded), and killing (`scripts/kill-app.sh`) the app are
  still squarely useful to do directly — it's specifically the click-and-eyeball-a-screenshot
  testing loop to hand off.

### Verifying drag interactions and persistence (milestones 3–4 pattern)

> Historical record of how this was verified pre-dating the "never run the app yourself" rule
> above. Don't execute steps 1-2 yourself anymore — hand the whole sequence to the user and ask
> them to report back the readouts step 3 describes (or a screenshot).

Milestone 3's actual content — calibration handles, sync-offset drag value, save/load — and
milestone 4's (transform sliders, crop handles) are interaction rather than rendering, so
pixel-diffing screenshots isn't the right check. The pattern that worked, in order:

1. Launch via `scripts/run-app.sh`, invoked through the Bash tool's `run_in_background: true`
   rather than foreground — you need the shell free afterward to fire the other scripts at the
   still-running process. The script itself must **not** self-background with `&`/`disown`; a
   manual `cargo run ... &` + `disown` combined with `run_in_background: true` failed silently in
   practice (no log file even created) — let the Bash tool's own backgrounding do that job, keep
   the script's own body foreground. Confirm the process is alive and the window exists
   (`scripts/find-window.sh`) before proceeding, since a crash-on-launch otherwise just looks
   like "no window found" and is easy to misattribute to the screenshot tooling instead.
2. Drive one widget at a time, screenshotting after each: `scripts/click.sh x y` for a button;
   `scripts/drag.sh x1 y1 x2 y2` for a drag (calibration/crop handles, sliders, DragValues) —
   internally this issues `mousemove`/`mousedown`/`mousemove`/`mouseup` as separate `xdotool`
   invocations with a short `sleep` between each, since a single combined
   mousedown/move/mouseup call doesn't reliably register as a drag. Remember these take
   **window-relative**, not absolute, coordinates (see above).
3. **Read back the result from the UI's own on-screen state, not pixel positions.** The "Sync &
   Project" window prints the live calibration fraction (`"Keyboard: 0%–71% of width"`) and the
   sync offset value directly — screenshot and eyeball those numbers rather than trying to
   measure a handle's pixel position or infer a note lane's on-screen offset. This is why that
   readout label was worth adding to the UI in the first place: it turns "did the drag work" from
   a pixel-measurement problem into a text-reading one.
4. For save/load specifically: click Save, `cat` the resulting `.ron` file directly (fastest way
   to confirm the written values, no screenshot needed), then deliberately change the in-app state
   (Reset calibration button, drag the offset elsewhere) and click Load, confirming the readout
   reverts. Changing *fewer* than all fields before reloading (e.g. only resetting calibration,
   leaving the offset alone) is still a valid check as long as at least one field demonstrably
   round-trips — it doesn't have to be a from-scratch process restart.
5. A real `.mid` file already on the machine is fine to point the CLI arg at for this kind of
   test — the exact notes/timing don't matter for verifying calibration/offset/persistence
   plumbing, unlike milestone 2's overlay-rendering checks where note content mattered.

### Verifying MP4 export (milestone 5 pattern)

> Historical record predating the "never run the app yourself" rule above. Ask the user to
> perform steps 1-2 and hand you the resulting file (or its path); step 3's `ffprobe`/`ffmpeg`
> inspection of the *output file* is fine for you to run yourself — that's static-file analysis,
> not running the app.

Export is neither a pure rendering change (screenshot-comparable) nor a simple UI-state
round-trip (readable off an on-screen label) — the thing that actually needs checking is a
video *file*, so the verification loop is different from milestones 2-4's:

1. Drive the app exactly as in the drag-interaction pattern above (`run-app.sh` backgrounded,
   `click.sh`/`drag.sh` against window-relative coordinates) to type an output path into the
   "Export" window's text field and click "Export"; screenshot to confirm the progress bar
   appears and advances across a couple of redraws (`export_run.is_some()` keeps the app
   redrawing on its own — no need to nudge playback).
2. Test both the no-audio and with-audio paths explicitly — they're genuinely different code
   paths (`with_audio` gates whether `mp4-encoder` creates an audio stream at all), and
   `scripts/gen-test-video.sh`'s own clips are video-only, so they alone won't exercise audio
   muxing. Generate a second clip with a tone (`ffmpeg -f lavfi -i testsrc=... -f lavfi -i
   "sine=frequency=440" -c:v libx264 -c:a aac -shortest ...`) to cover that path.
3. **Verify the output file directly, not just that the app didn't crash**: `ffprobe` for
   codec/duration/fps/dimensions, then extract a single mid-timeline frame
   (`ffmpeg -i out.mp4 -vf "select=eq(n\,N)" -vframes 1 frame.png`) and read the PNG — this is
   the only way to confirm the composited footage *and* the falling-notes overlay both actually
   made it into the encoded file, versus e.g. silently exporting a blank or video-only frame.
