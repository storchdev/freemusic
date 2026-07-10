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

Beyond the plan's own milestones, the Keyboard tab also has a note editor (list notes currently
playing at the current frame, with an immediate delete/restore icon and no confirm step) that
excludes them from the note highway/playback/export via a persisted skip list rather than ever
rewriting the loaded `.mid` file. The same editor also supports editing a note's duration (a
persisted per-note override) and adding brand new notes at the current frame (persisted, not
written to the `.mid` file either) — see `docs/ui-milestones.md`'s "Note editor" section for the
full design (identity key, persistence, filtering, and why it's non-destructive).

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
`.fmstyle.ron` loads as a visual style (same effect as the Project tab's "Import style…" button —
see below), a remaining `.ron` (a saved `.fmproj.ron` project file) loads as a project — same code
path as the Project tab's Load button, so it replaces video/MIDI/sync/calibration/transform/
barrier/note style with whatever the project file contains — and anything else is treated as the
video. `app song.fmproj.ron look.fmstyle.ron` and `app video.mp4 song.mid look.fmstyle.ron` both
work, order-independent, without needing a separate flag.

Distinguishing `.fmstyle.ron` from a plain `.ron` project file needs a full-filename check
(`name.ends_with(".fmstyle.ron")`), not just `Path::extension()` — `extension()` only ever returns
the last dot-separated component (`"ron"` for both), so the classifier checks the whole file name
first and only falls through to the `.mid`/`.ron`/video match if that check misses. A style path is
applied *after* the project-path branch (not folded into it), so a CLI-passed style always wins
over whatever `style` field a loaded project itself carries — the same "more specific/later wins"
precedent already set by passing a project path alongside a separate video/MIDI path (next
paragraph). Both `App`'s CLI-arg fields and `AppState::new`'s parameter list gained a `style_path`
alongside `project_path` for this; `AppState::load_style` (shared with the Import button's
`rfd::FileDialog` picker) does the actual `Style::load` + `ui_state.style` assignment.

Passing a project path alongside a separate video/MIDI path is unusual but not an error; the
project load simply runs and then loads whatever the project itself references, which typically
supersedes a separately-passed video/MIDI path since project load happens instead of (not
before/after) the plain video_path/midi_path branch in `AppState::new`. Drag-drop still only
distinguishes MIDI-vs-video (`WindowEvent::DroppedFile` has no `.ron`/`.fmstyle.ron` case) —
dropping a project or style file onto the window loads it as a "video" and will fail to open,
since neither path was part of either change.

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
scripts/refresh-sample-styles.sh          # regenerates examples/styles/*.fmstyle.ron from
                                           # `cargo run -p project --example dump_sample_styles`'s
                                           # stdout, raw (no reformatting) — diff before committing,
                                           # see docs/fmstyle-milestone.md's Phase O follow-ups
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
- FFmpeg dev libraries (`libavcodec`, `libavformat`, `libavutil`, `libswscale`, `libswresample`,
  **and** `libavfilter`, `libavdevice` — `ffmpeg-next`'s default feature set enables its `filter`
  and `device` features, which pull in `ffmpeg-sys-next/avfilter` and `ffmpeg-sys-next/avdevice`
  respectively, so all seven `-dev` packages are required even though only five look obviously
  video-related; `.github/workflows/unit-tests.yml`'s `apt-get install` list was missing the last
  two and failed CI with a pkg-config "package libavfilter was not found" error until fixed) for
  `ffmpeg-sys-next`'s bindgen step, plus `clang`/`llvm`.
- Vulkan loader + a driver. Under WSL2 specifically, `mesa`'s default packages ship no Vulkan ICD
  at all — install `vulkan-dzn` (Mesa's D3D12-passthrough driver, exposes the real GPU through
  `/dev/dxg`) or `vulkan-swrast` (lavapipe, software fallback) from the `extra` repo. `wgpu`
  respects `WGPU_BACKEND` (e.g. `WGPU_BACKEND=gl`) to force a specific backend if the default
  picks something broken.
- `libxkbcommon-x11` if winit falls back to the X11 backend (e.g. `WAYLAND_DISPLAY` unset) —
  without it winit panics at startup with "Library libxkbcommon-x11.so could not be loaded",
  it does not silently fall back further.

**Dynamic-link FFmpeg version pin and vendored patch (matters most on Windows):** `ffmpeg-next
8.1.0` (pinned in `crates/{video-pipeline,export,audio-playback}/Cargo.toml`) wraps FFmpeg **7.x**'s
C API. The upstream `n7.1-latest` BtbN builds compile FFmpeg without the deprecated `AVCodec` struct
fields (`sample_fmts`, `pix_fmts`, `supported_framerates`, `ch_layouts`) — bindgen omits those fields
from the generated Rust struct — which caused `ffmpeg-next 8.1.0` to fail to compile with `E0609`/
`E0425`/`E0004` errors on these fields and on several `AVCodecID` enum variants added in 7.1.5.

The fix lives in `vendor/ffmpeg-next/` — a vendored copy of `ffmpeg-next 8.1.0` with four targeted
patches applied: (1) `src/codec/video.rs` + `src/codec/audio.rs`: return `None` from the methods
that accessed those deprecated struct fields (the deprecated API is no longer reachable anyway);
(2) `src/codec/id.rs`: map `V410`/`V308`/`V408` codec IDs to `AV_CODEC_ID_NONE` in the forward
direction and add a `_ => Id::None` wildcard to the reverse match for codec IDs added in 7.1.5+;
(3) `src/util/frame/side_data.rs` + `src/codec/packet/side_data.rs`: add `_ => todo!()` wildcard
arms for enum variants added in 7.1.5+ that the crate doesn't know about yet; (4)
`src/software/resampling/context.rs`: a hand-added `SwrContext::convert_planes` method (not part
of upstream `ffmpeg-next`) that calls `swr_convert` directly instead of `swr_convert_frame`, used
by `crates/export/src/audio.rs` and `crates/audio-playback/src/lib.rs` — see their doc comment for
why (`swr_convert_frame` requires frame metadata that Windows static FFmpeg 7.x AAC decode
sometimes leaves zeroed). `swr_convert`'s input-planes parameter is C's `const uint8_t **`, which
bindgen renders as `*mut *const u8` — `convert_planes` originally passed `in_planes.as_ptr()`
(`*const *const u8`) uncast, which type-checked against whatever bindgen output this repo's own
dev machine happened to generate but failed to compile (`E0308`, mismatched mutability) in GitHub
Actions CI, which generates its own bindings against the runner's system FFmpeg headers. Fixed by
casting: `in_planes.as_ptr() as *mut *const u8` (and `ptr::null_mut()` for the empty/flush case).
This is a reminder that this vendor tree's build-time-generated bindings aren't pinned/checked in
— a local `cargo check` passing doesn't guarantee CI will, whenever a patch's pointer types are
written to match one machine's bindgen output rather than the C header's actual signature. The
workspace `Cargo.toml` has `[patch.crates-io] ffmpeg-next = { path = "vendor/ffmpeg-next" }` to use
this patched copy. `crates/mp4-encoder/src/audio.rs` was also updated to not access the deprecated
fields directly (hardcoded `AV_SAMPLE_FMT_FLTP` + 44100 Hz for AAC, which are its only supported
values anyway). If `ffmpeg-next` is ever bumped to a version that handles this, remove the vendor
directory and the patch entry (patch 4's `convert_planes` method would need to be re-added by hand
since it isn't an upstream feature).

**`ffmpeg-sys-next` is also vendored/patched (`vendor/ffmpeg-sys-next/`, same `8.1.0` pin, same
`[patch.crates-io]` mechanism), for two unrelated MSVC-only build bugs found while getting the
`static-ffmpeg` feature working on Windows** — bogus `-march=native`/`-mtune=native` GCC flags
passed to `cl.exe`, and MSVC `-libpath:` linker flags misparsed as library names (E0459). Full
narrative (exact errors, why each is confusing, the patch mechanics) is in
**[`docs/building.md`](docs/building.md)**'s "Windows static-build gotchas" section — read that
before touching either patch. Same scope as the `ffmpeg-next` patch above: Windows/MSVC-specific,
inert on Linux/macOS. If `ffmpeg-sys-next` is ever bumped past both bugs being fixed upstream,
remove `vendor/ffmpeg-sys-next/` and its patch entry the same way as `ffmpeg-next` above.

### Static/cross-platform release builds

Added a `static-ffmpeg` cargo feature (on `app`, `export`, `video-pipeline`, `audio-playback`,
`mp4-encoder`) that vendors and statically links FFmpeg (via `ffmpeg-sys-next`'s `build` feature)
plus `libx264` (since `mp4-encoder` prefers the `libx264` encoder by name), so release binaries
run on machines with no FFmpeg installed. `.github/workflows/release.yml` builds this for Linux
(x86_64), Windows (x86_64), and macOS (arm64 only — x86_64/Intel macOS was dropped from the
matrix) on a pushed `v*` tag or manual dispatch (`workflow_dispatch`'s `only` input can also
trigger a single platform leg, useful for debugging one leg without a full release run).
`scripts/build-static-linux.sh`/`scripts/build-static-windows.ps1` reproduce that build locally,
with `scripts/setup-msvc-x64.ps1` to load a correct x64 MSVC dev environment first on Windows.
Full prerequisites, the from-source-static-libx264 recipe, and every gotcha found getting this
working (two `ffmpeg-sys-next` MSVC bugs, the shared-libx264-search-order trap, three rounds of a
Windows libx264 architecture mismatch, and the Windows-specific shell/encoding pitfalls) are in
**[`docs/building.md`](docs/building.md)** — read that, not this file, before touching any of it.
`scripts/build-static-windows.ps1` has been run to a successful, verified-static completion on
real Windows hardware (`dumpbin /dependents` shows no avcodec/avformat/avutil/x264 DLL).

## Architecture

### Workspace layout (current)

```
freemusic/
  Cargo.toml            # workspace root; pins wgpu ecosystem versions must stay in lockstep, see below
  app/                   # binary: winit + egui-wgpu shell
    src/main.rs           # event loop, AppState (owns everything), redraw/composite/present, export thread wiring
    src/gpu.rs             # wgpu Instance/Adapter/Device/Surface setup (interactive window only)
    src/ui.rs                # tabbed side panel, timeline, calibration/crop/barrier drag handles
  crates/
    project/              # RON project model: paths, sync offset, calibration (incl. barrier), transform, styles
    video-pipeline/       # ffmpeg-next decode + seek, no GPU/UI dependency
    render/                # UI-agnostic compositor (video quad + note highway), used headless by export too
    mp4-encoder/            # forked ffmpeg-encoder: parameterized fps, explicit codec selection, optional audio
    export/                  # headless-GPU offline render loop, audio mux, progress/cancel channel
    audio-playback/          # cpal output stream for the loaded video's own audio, driven by transport position
  scripts/               # cargo check, run/screenshot/click/drag the app, gen synthetic test clips
  docs/                  # detailed narrative/design docs, split out of this file — see below
  explorations/          # standalone, non-integrated experiments — not wired into the app/build
```

**The rest of this project's history and design detail lives in `docs/`, not here**, to keep this
file short enough to stay useful as the fastest-orientation doc. Read the relevant one before
touching that area of the code:

- **`docs/architecture.md`** — Neothesia-reuse history, manual sync/calibration/persistence,
  video transform (brightness/scale/crop/rotate/tilt/translate) math and bugs found, wgpu/
  egui-wgpu version pinning, video-pipeline decode/seek data flow and perf bugs, interactive
  rendering (`app`), the unthrottled-redraw perf bug, and MP4 export (milestone 5).
- **`docs/ui-milestones.md`** — the 6c UI restructure (offscreen-texture preview, tabbed side
  panel, custom timeline), the two playback-timing bugs found testing it, barrier + note-highway
  styling (6a), fall-speed slider, the barrier-fade-out bug, the File-menu/native-dialogs
  milestone (6b), keyboard navigation (6d), synced audio playback (6e), the timeline
  waveform/scroll/collapsible-panel polish pass, the slider-validation fix, and the widened
  rotation/roundedness ranges.
- **`explorations/barrier-fx-lab/`** — a standalone WebGL2 HTML page (no build step, no app
  dependency) for prototyping new barrier looks — glow sigmas, wavy-edge modes, strand bundles, and
  electric/wispy filament/wisp effects not yet in `barrier.wgsl` — before committing any of it to
  the real renderer. Its `presets/` holds exported JSON snapshots of looks worth keeping, notably
  `seemusic-found.json`, the closest match found so far to the SeeMusic edge in `sm-ex.png`; see
  the directory's own `README.md`. **The strand bundle has since been ported into the real app**
  (Phase O, `project::StrandSpec`/`WavySpec::strands`, gated to `WavyMode::Edge` — see
  `docs/fmstyle-format.md` and `docs/fmstyle-milestone.md`); the sliding-filament/wisp controls in
  the lab remain unported experiments.
- **`docs/fmstyle-milestone.md`** — full phase-by-phase narrative (Phases A–R) of the
  `.fmstyle.ron` extensible visual style format: schema/plumbing, the vendored note pipeline
  (dropping the `neothesia-core` dependency), note fill effects (gradient/sheen/glow), barrier
  glow/pulse, transition particles/flash, per-key-color/wavy-barrier/elliptical-flash/continuous-
  particle follow-ups, the brightness/overexposure + white-hot-corona redesign, canvas background
  color, the barrier strand bundle ported from `explorations/barrier-fx-lab`, the canvas-Y-position
  note gradient (`Fill::CanvasGradient`, Phase P), flashes/particles/glow that match note color
  plus multicolor (author-painted or note-derived) flash gradients (Phase Q), and real per-note
  resolution for `ColorBinding::ByVelocity`/`ByPitchClass`/`ByTrack` (Phase R — see below).
- **`docs/fmstyle-format.md`** — the living field-by-field `.fmstyle.ron` format spec (defaults,
  meaning, RON snippets, breaking-change log) — keep this in sync whenever the schema changes,
  it's the spec, not narrative.
- **`docs/verification.md`** — how to verify changes to `app`/`video-pipeline`/export: generating
  synthetic test clips, screenshotting under WSL2 vs. native Hyprland, the drag-interaction/
  persistence verification pattern (milestones 3–4), and the MP4 export verification pattern
  (milestone 5). Per this file's own top-level rule, never run the app yourself — these are
  patterns to hand to the user, except where noted as safe static-file analysis (e.g. `ffprobe`).
- **`docs/building.md`** — the Windows dynamic-link dev setup, the `static-ffmpeg` feature and its
  from-source-static-libx264 recipe, the `scripts/build-static-*`/`scripts/setup-msvc-x64.ps1`
  helper scripts, every static-build gotcha found on each OS (two `ffmpeg-sys-next` MSVC bugs, the
  shared-libx264 search-order trap, the Windows libx264 architecture-mismatch saga, Developer Shell
  shortcut and PowerShell encoding pitfalls), and how the GitHub Releases binaries get built.

### `ColorBinding::ByVelocity`/`ByPitchClass`/`ByTrack` now really vary rendering (Phase R)

These three `ColorBinding` variants used to parse/round-trip but always resolve to one fixed
representative color (`resolve_constant()`), regardless of which note was being drawn. They now
resolve for real, per note, via a new `ColorBinding::resolve_for_note(velocity, pitch, track_id)`
— see `docs/fmstyle-format.md`'s `ColorBinding`/`ScalarBinding` section for the exact per-variant
mapping and `docs/fmstyle-milestone.md`'s Phase R for the full narrative (call sites touched,
what deliberately still uses `resolve_constant()` and why, new example styles). Short version:
note fill (`crates/render/src/notes/mod.rs`) and particle/flash colors triggered by a specific
note (`crates/render/src/effects.rs`) resolve per note now; the canvas background, the barrier
bar/glow (`crates/render/src/barrier.rs`), and the note-glow GPU uniform
(`crates/render/src/notes/pipeline.rs`) still use `resolve_constant()` on purpose — those contexts
have no single note to key off of. Four new sample styles demonstrate this:
`examples/styles/velocity-colored-notes.fmstyle.ron`, `pitch-rainbow.fmstyle.ron`,
`track-colored-notes.fmstyle.ron`, and `velocity-sparks.fmstyle.ron` (the last one exercises the
`effects.rs` particle/flash wiring specifically, not note fill).

**Follow-up in the same phase**: `ParticleSpec::brightness`/`FlashSpec::brightness` changed from a
plain `f32` to `ScalarBinding` (the numeric counterpart of `ColorBinding`, same four variants),
resolved per triggering note the same way particle/flash *color* already is. **This is a breaking
`.fmstyle.ron`/`.fmproj.ron` schema change** — a bare float (`brightness: 1.0`) no longer parses; it
needs `brightness: Constant(1.0)`. No backward-compat parsing shim was added for this (one was
tried and then deliberately removed per instruction) — a real project file that breaks on this gets
hand-migrated instead, not the schema growing bare-number-parsing logic to avoid it.
`Glow::brightness`/`Pulse::brightness` are untouched (stay a plain `f32`) — they're baked into a
shared GPU uniform / a canvas-wide barrier pulse, same "no single note to resolve against"
reasoning as `Glow::color` above. `velocity-sparks.fmstyle.ron` now uses `ScalarBinding::ByVelocity`
for brightness too, alongside its `ColorBinding::ByVelocity` color, so a soft keypress sparks a dim
burst/flash and a hard one a bright one. See `docs/fmstyle-milestone.md`'s "Phase R follow-up" for
the full narrative.
