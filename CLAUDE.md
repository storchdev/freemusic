# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**Keep this file up to date after every task**, not just milestone completions or dependency
changes — any change worth explaining to the next agent (a bug found and fixed, a design
decision, a gotcha) gets a note here in the same session it happens, in whatever section it fits
best (or a new one). This file is the fastest way for the next agent to get oriented — don't let
it drift from what the code actually does.

**Commit as the repo owner, no AI attribution.** Do not append a `Co-Authored-By: Claude ...`
trailer (or any other AI-attribution line) to commit messages — commits should read like
ordinary commits from the repo owner. This only affects the commit message body; the actual git
author/committer identity already comes from local git config and needs no special handling.

**Prefer a human for interactive UI verification over an automated screenshot loop.** For
anything that needs eyeballing in the running app (drag handles, sliders, dialogs, visual
correctness), ask the user to drive it and report back rather than iterating with
`scripts/click.sh`/`scripts/drag.sh`/`scripts/screenshot.sh` yourself — see "Screenshotting/
driving the app under native Hyprland" below for why this is especially true on this machine
(tiling-WM coordinates are unreliable and slow to re-derive per click). Still fine to build,
launch (`scripts/run-app.sh`), and kill (`scripts/kill-app.sh`) the app yourself, and to use the
scripts for a quick one-off sanity screenshot when no human is available to check — just don't
default to a full click/drag/screenshot verification loop when a human can look instead.

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
cargo fmt                         # this repo is fmt-clean; run before committing
cargo clippy --all-targets        # this repo is clippy-clean; run before committing
```

Both CLI args are optional (drag-drop a video and/or a `.mid`/`.midi` file onto the window
instead, or in addition — dropping either replaces whatever of that kind was already loaded).

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

### Neothesia reuse (`midi-file`, `neothesia-core`)

`crates/render/Cargo.toml` depends on `midi-file` and `neothesia-core` as git deps pinned to an
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
  `mat3_aspect(window_width/window_height)`: scale `y` up by the window aspect ratio before
  rotating, rotate in that now-isotropic space, then scale back down by the same factor
  afterward.
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
- `VideoPipeline::seek_and_decode(target_seconds, exact)` is the only entry point, called once per
  redraw with the current transport position. It is deliberately *not* a naive
  seek-then-decode-once call:
  - It holds the last-decoded frame (`current_frame`) and skips touching the decoder entirely if
    `target_seconds` is still covered by it — the common case, since redraws happen far more often
    than the source frame rate requires new frames.
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
  position → `video_pipeline.seek_and_decode` → `compositor.upload_frame` + `update_viewport` →
  `compositor.update_midi(position - sync_offset)` → run egui (`ui::draw`) → apply any
  calibration change / Save-Load/Export button press queued by that egui pass (see the
  `project` crate section above and the MP4 export section below) → **two** render passes on
  the swapchain view → present. Playback continuation is driven by `window.request_redraw()`
  called at the end of `redraw` whenever `ui_state.playing` **or an export is running** is true
  (see below); the event loop otherwise sits in `ControlFlow::Wait`.
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
- **Fix**: exclude `WindowEvent::RedrawRequested` from the generic nudge —
  `if response.repaint && !matches!(event, WindowEvent::RedrawRequested) { ... }` — so the
  generic check still does its job for events that legitimately want a one-shot repaint (mouse
  moved, focus changed, modifiers changed, etc.) without re-arming itself on every frame it
  draws. Verified with the same event-tally instrumentation: idle `RedrawRequested` rate dropped
  from ~120/sec to ~0-1/sec (only real input causes a redraw now), and idle CPU for the process
  dropped from continuous background load to ~1-2% (`ps -o pcpu`). Playback (`ui_state.playing`)
  still self-sustains its own redraw loop correctly via `redraw`'s existing end-of-frame check,
  confirmed by playing a 30s synthetic clip end-to-end and watching the on-screen frame counter
  and transport position advance smoothly and land exactly on the expected frame for elapsed
  wall-clock time.
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
  `midi_name`.
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

### File menu bar and remaining native dialogs (milestone 6b)

- `ui::draw_menu_bar` (`egui::Panel::top("menu_bar")` + a single `ui.menu_button("File", ..)`)
  adds New Project, Open Project…, Save Project (Ctrl+S), Save Project As…, and Exit alongside the
  Open Video…/Open MIDI… buttons 6c already added to the Project tab — all of it routes through
  the same `UiState` request-flag-consumed-next-redraw pattern the Project tab's buttons already
  used (`new_project_requested`/`open_project_requested`/`save_project_as_requested`/
  `exit_requested`), so the menu, the tab buttons, and 6d's keyboard shortcuts are three ways to
  trigger the exact same `AppState` methods, never three separate code paths.
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
  adds: Left/Right seek ±5s, Shift+Left/Right ±1s (relative to `seek_request.unwrap_or(position)`,
  not the raw position, so two quick presses before a redraw consumes the first compound rather
  than clobber each other), Home/End jump to start/end, Ctrl+S save project, Ctrl+O open project,
  Esc cancel an in-progress export. Space (play/pause) moved into the same function, unchanged in
  behavior. Every action other than Space sets the same `UiState` request flag the menu bar/tab
  buttons use (see 6b above) rather than calling `AppState` methods directly — one code path.
- J/K/L (the "nice-to-have, skip unless there's time" item from the plan) was skipped.

### Synced audio playback (milestone 6e)

- New crate `crates/audio-playback`, built on `cpal = "0.18"` (matches Neothesia's own workspace
  pin). Decodes a video's audio track fully upfront (`decode_all`, duplicated from — not shared
  with — `crates/export/src/audio.rs`'s near-identical function, since that crate's `audio`
  module is private and this crate has no other reason to depend on `export`) into stereo `f32`
  at whatever sample rate the *output device* reports via `default_output_config()` (not a fixed
  rate — mirrors export's own "ask the encoder what rate it actually chose" approach).
- **Sync design**: `AudioPlayback` stores only a transport position (`position_bits: Arc<AtomicU64>`,
  an `f64::to_bits` seconds value) plus the `cpal::Stream`. The output callback
  (`fill_buffer`) re-reads that atomic fresh on *every* invocation and maps it straight to a
  sample index — it never advances an internal counter between callbacks. `AppState::redraw`
  calls `audio.set_position_seconds(ui_state.position_seconds)` unconditionally every redraw (the
  same position value already driving video decode and `midi_time`), so audio can't accumulate
  drift over a long playback the way an independently-clocked stream could; a scrub is handled by
  nothing more than the position jumping, no special-cased "seek" method needed.
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

## Verifying changes to `app` or `video-pipeline`

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
