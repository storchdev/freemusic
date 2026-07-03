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
plain video playback) and 2 (MIDI + note highway overlay) are implemented so far.

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
    src/video_quad.rs       # aspect-correct textured-quad render pass for the decoded video frame
    src/midi_overlay.rs      # loads a MIDI file, wraps Neothesia's WaterfallRenderer (see below)
    src/ui.rs                 # egui transport bar (play/pause, timecode, scrub slider, drop overlay)
    src/shader.wgsl             # vertex/fragment shader for the video quad
  crates/
    video-pipeline/       # ffmpeg-next decode + seek, no GPU/UI dependency
```

Crates from the plan's full architecture (`project`, `render`, `mp4-encoder`, `export`) don't
exist yet — they land in later milestones. Don't scaffold them speculatively; add each when its
milestone starts.

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
- Milestone 2 calibration is intentionally naive: note lanes always span the full window width
  with the standard 88-key range (`piano_layout::KeyboardRange::standard_88_keys()`), not yet
  aligned to the keyboard visible in the footage — that's milestone 3. `MidiOverlay::update`'s
  `time_seconds` is the raw transport position with no sync offset applied yet either.
- `neothesia_core::config::Config::default()` is used as-is rather than `Config::new()`, which
  would read `~/.config/neothesia/settings.ron` if the user happens to have real Neothesia
  installed — harmless (read-only, falls back to defaults) but an unnecessary external coupling
  we don't need since we don't call `.save()`.

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
  `midi_overlay.update` → run egui (`ui::draw`) → **two** render passes on the swapchain view →
  present. Playback continuation is driven by `window.request_redraw()` called at the end of
  `redraw` whenever `ui_state.playing` is true; the event loop otherwise sits in
  `ControlFlow::Wait`.
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
cargo run --bin app -- <video-file>
```

For anything touching seek/playback timing specifically, a static test pattern isn't enough to
tell whether frames are actually advancing — generate a clip with a visible per-frame marker and a
realistic (multi-second) keyframe interval, e.g.:

```sh
ffmpeg -y -f lavfi -i "testsrc=size=640x360:rate=30:duration=10" \
  -vf "drawtext=fontfile=/usr/share/fonts/TTF/DejaVuSans-Bold.ttf:text=frame\ %{n}:fontcolor=white:fontsize=64:x=20:y=20:box=1:boxcolor=black@0.6" \
  -c:v libx264 -g 60 -pix_fmt yuv420p out.mp4
```

This is how the reseek-every-frame and seek-timestamp-units bugs above were actually caught —
`cargo build` and `cargo clippy` were clean for both.

For a synthetic MIDI file to pair with the above (no note-visualizer-specific tooling needed —
any minimal single-track SMF works), a short Python script writing raw MIDI bytes is enough; see
git history for milestone 2's throwaway generator if a reference is useful.

### Screenshotting the app under WSL2

WSLg's Weston compositor does **not** support the `wlr-screencopy` protocol, so `grim` fails
with "compositor doesn't support the screen capture protocol", and the app's winit window (real
Wayland, not XWayland) isn't visible to X11 tools like `xdotool`/`import` either — only
WSLg-internal windows (e.g. "Weston WM") show up there. What does work: WSLg surfaces each app
window as a *native Windows window*, so PowerShell interop from WSL can find and capture it —
`Get-Process | Where-Object MainWindowTitle -like "freemusic*"` finds the window (title is
`"freemusic (<distro>)"`), then `user32.dll`'s `SetForegroundWindow`/`GetWindowRect` +
`System.Drawing.Graphics.CopyFromScreen` over that rect captures it to a PNG. Invoke via
`powershell.exe -NoProfile -ExecutionPolicy Bypass -File <script>.ps1 <out-path>`, converting
WSL paths to Windows paths with `wslpath -w` first. `System.Windows.Forms.SendKeys` from the
same script can drive keyboard input (e.g. Space to toggle play) after focusing the window.
