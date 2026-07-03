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

The project is being built milestone-by-milestone per that plan. Only milestone 1 (scaffolding +
plain video playback) is implemented so far.

## Commands

```sh
# Rust toolchain isn't necessarily on PATH in a fresh shell:
source "$HOME/.cargo/env"

cargo build                       # debug build, whole workspace
cargo build --release             # release build
cargo run --bin app -- <video-file>   # launch the app on a video file (required CLI arg for now)
cargo fmt                         # this repo is fmt-clean; run before committing
cargo clippy --all-targets        # this repo is clippy-clean; run before committing
```

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
    src/ui.rs                # egui transport bar (play/pause, timecode, scrub slider)
    src/shader.wgsl            # vertex/fragment shader for the video quad
  crates/
    video-pipeline/       # ffmpeg-next decode + seek, no GPU/UI dependency
```

Crates from the plan's full architecture (`project`, `render`, `mp4-encoder`, `export`) don't
exist yet — they land in later milestones. Don't scaffold them speculatively; add each when its
milestone starts.

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
  position → `video_pipeline.seek_and_decode` → `video_quad.upload_frame` +
  `update_viewport` → run egui (`ui::draw`) → single render pass drawing the video quad then the
  egui paint jobs on top (via `RenderPass::forget_lifetime()`, since `egui-wgpu`'s `render` wants a
  `'static` pass) → present. Playback continuation is driven by `window.request_redraw()` called at
  the end of `redraw` whenever `ui_state.playing` is true; the event loop otherwise sits in
  `ControlFlow::Wait`.
- The plan's longer-term design renders the composite into an *offscreen* texture shown via
  `egui::Image`, to decouple preview resolution from window size and avoid interleaving a raw pass
  around egui's own swapchain pass. Milestone 1 draws directly into the swapchain pass instead
  (simpler, sufficient for plain playback) — revisit this when the `WaterfallRenderer` overlay
  (milestone 2) needs compositing above the video quad.

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
