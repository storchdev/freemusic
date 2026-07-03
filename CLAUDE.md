# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**Keep this file up to date.** When you add a crate, change the workspace layout, pin a new
external dependency (especially git deps), or complete a milestone from the plan, update the
relevant section below in the same session. This file is the fastest way for the next agent to
get oriented — don't let it drift from what the code actually does.

## What this is

A native desktop app (not Tauri/web — see rationale in the plan doc) that lets piano players
composite real filmed footage with an animated falling-notes MIDI overlay ("note highway"),
manually sync the two, apply basic video transforms, and export the result to a real MP4. It's a
cross-platform (Windows/macOS/Linux) alternative to SeeMusic. The full design — stack rationale,
data flow, phased milestones, and tracked risks — lives in
`~/.claude/plans/i-want-to-plan-vast-shore.md`; read it before making architectural changes.

The project is being built milestone-by-milestone per that plan. Milestones 1 (scaffolding +
plain video playback), 2 (MIDI + note highway overlay), 3 (manual sync + keyboard calibration +
persistence), and 4 (brightness/scale/crop/rotate/tilt/translate video transform) are implemented
so far.

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
    src/main.rs           # event loop, AppState (owns everything), redraw/composite/present
    src/gpu.rs             # wgpu Instance/Adapter/Device/Surface setup
    src/video_quad.rs       # aspect-correct textured-quad pass + brightness/scale/crop/rotate/tilt/translate
    src/midi_overlay.rs      # loads a MIDI file, wraps Neothesia's WaterfallRenderer (see below)
    src/ui.rs                 # transport bar, calibration/crop drag handles, sync/project/transform windows
    src/shader.wgsl             # vertex/fragment shader for the video quad
  crates/
    project/              # RON project model: paths, sync offset, keyboard calibration, video transform
    video-pipeline/       # ffmpeg-next decode + seek, no GPU/UI dependency
  scripts/               # cargo check, run/screenshot/click/drag the app, gen synthetic test clips
```

Crates from the plan's full architecture (`render`, `mp4-encoder`, `export`) don't exist yet —
they land in later milestones. Don't scaffold them speculatively; add each when its milestone
starts.

### Neothesia reuse (`midi-file`, `neothesia-core`)

`app/Cargo.toml` depends on `midi-file` and `neothesia-core` as git deps pinned to an exact
commit SHA of `PolyMeilex/Neothesia` (`e61639b12cc8e466b90406c564da5f9f54d8d1a3`, fetched
2026-06-30) — never `master`, per the plan's "no semver safety net" risk. `neothesia-core`
re-exports `wgpu_jumpstart::{Gpu, TransformUniform, Uniform, Color}` and the whole
`piano_layout` crate at its root, so those don't need separate git-dep entries; add direct
`wgpu-jumpstart`/`piano-layout` deps (same rev) only when a future crate needs them without
going through `neothesia-core` (e.g. milestone 5's `export` crate wanting a headless
`wgpu_jumpstart::Gpu` directly). Both resolve to `wgpu 29.0.4`, matching our own pin — verified
by `cargo build` producing a single `wgpu` entry in `Cargo.lock`, no duplicate-version errors.

`app/src/midi_overlay.rs` wraps `neothesia_core::render::WaterfallRenderer`:
- `WaterfallRenderer::new`/`resize` take a `&neothesia_core::Gpu` (`wgpu_jumpstart::Gpu`), a
  different type from our own `app::gpu::Gpu`. Rather than restructuring the app around
  `wgpu_jumpstart::Gpu`, `midi_overlay::wrap_gpu` builds one on the fly from our existing
  `wgpu::Instance`/`Adapter`/`Device`/`Queue` handles (all cheap `Arc`-backed clones) — its
  fields are all `pub` with no constructor invariants to preserve. This is why `app::gpu::Gpu`
  now also stores `instance`/`adapter` (unused by the video quad path) alongside
  `device`/`queue`/`surface`.
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
  `KeyboardCalibration`). All of it lives in `app/src/video_quad.rs`, applied in a single WGSL
  pass — no new crate, no new render pass.
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

### Rendering (app)

- `Gpu` (`app/src/gpu.rs`) owns the wgpu `Instance`/`Device`/`Queue`/`Surface` for the *interactive*
  window. Instance creation goes through
  `wgpu::InstanceDescriptor::new_without_display_handle_from_env()` specifically so `WGPU_BACKEND`
  and friends are respected — needed for the WSL2 Vulkan-driver situation above.
- `VideoQuad` (`app/src/video_quad.rs`) is a self-contained aspect-correct textured-quad pass:
  uploads the latest `DecodedFrame`'s BGRA bytes to a `wgpu::Texture` (recreated only when the
  frame size changes), and computes a letterbox/pillarbox scale uniform from
  `(video_size, window_size)` each frame. It renders via 6 hardcoded vertices in the shader (no
  vertex buffer) — see `shader.wgsl`.
- `AppState::redraw` in `main.rs` is the per-frame orchestrator: advance/clamp the transport
  position → `video_pipeline.seek_and_decode` → `video_quad.upload_frame` + `update_viewport` →
  `midi_overlay.update(position - sync_offset)` → run egui (`ui::draw`) → apply any calibration
  change / Save-Load button press queued by that egui pass (see the `project` crate section
  above) → **two** render passes on the swapchain view → present. Playback continuation is
  driven by `window.request_redraw()` called at the end of `redraw` whenever `ui_state.playing`
  is true; the event loop otherwise sits in `ControlFlow::Wait`.
- **Two render passes, not one** (changed in milestone 2): a `scene_pass` (`LoadOp::Clear`)
  draws `video_quad` then `midi_overlay` with a normally-scoped (non-`'static`) `RenderPass`,
  followed by an `egui_pass` (`LoadOp::Load`, so it composites on top without clearing) using
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
