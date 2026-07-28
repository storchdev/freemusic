# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**Keep this file and `docs/` up to date after every task**, not just milestone completions or
dependency changes. If the current-state facts changed (a new file, a new API, a new feature, a
command that now behaves differently), update this file or the relevant `docs/*.md` in the same
session. If something worth explaining to the next agent happened along the way (a bug found and
fixed, a design decision, a gotcha), that's narrative — see the next paragraph for where it goes.
This file is the fastest way for the next agent to get oriented — don't let it drift from what the
code actually does.

**Decision history / bugfix / what-worked-vs-didn't narrative does not belong anywhere except
`docs/narratives/`.** `.rs` files, `Cargo.toml`, CI workflow YAML, scripts, example asset files
(`examples/styles/*.fmstyle.ron`, etc.), every doc under `docs/` outside `docs/narratives/`, and
this file itself are current-state-only — no "we used to do X until we found Y" comments in code,
no worked/didn't-work progress log bolted onto a format spec or architecture doc. Code comments
stay to the normal rule (short, only for non-obvious WHY, never a running history); every other doc
describes the code/software as it is right now, full stop. When something from a session is worth
recording — a bug found and fixed, a design decision, a gotcha, a "this was tried and reverted"
story — put it in whichever `docs/narratives/*.md` file already owns that area (`architecture.md`,
`ui-milestones.md`, `fmstyle-milestone.md`, `fmstyle-history.md`, `verification.md`, `building.md`
— see the doc index under "Architecture" below, and `docs/narratives/README.md` for the full
index), or create a new `docs/narratives/*.md` and add it to that index if none of the existing
ones fit. This file (`CLAUDE.md`) itself stays reserved for short, load-bearing orientation notes
describing the project as it is now — narrative goes to `docs/narratives/`, anything longer but
still current-state goes to the matching `docs/*.md`.

**Pre-1.0: don't design for backward compatibility.** This project hasn't shipped a 1.0 yet, so
`.fmproj.ron`/`.fmstyle.ron` are not stable formats — either can change shape at any time, and a
schema change that breaks existing project/style files is fine as-is. Don't add migration shims,
format-version fields, or bare-value/legacy-syntax parsing fallbacks to soften a breaking change
(see `docs/narratives/fmstyle-history.md`'s breaking-change log for a case — the `ScalarBinding`
change — where a compat shim was tried and then deliberately removed). If a real file breaks,
hand-migrate it by hand; don't grow the schema or parser to avoid breaking it. Revisit this policy
once the app is actually heading toward a 1.0 release.

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
  drive the app and describe or screenshot what they see — see `docs/verification.md`'s
  "Screenshotting/driving the app under native Hyprland" section for why automating this yourself
  is also unreliable on this machine (tiling-WM coordinates drift between screenshots), on top of
  the blanket rule above.
- **Non-visual/diagnostic evidence** (timing, decode stats, crashes, a specific code path
  firing): ask the user to set the relevant environment variable(s) — e.g. `RUST_LOG=debug`, or
  an app-specific one like `FREEMUSIC_DECODE_THREADS`/`WGPU_BACKEND` — and tee the run's output
  into a log file you name, e.g.:
  ```sh
  RUST_LOG=debug scripts/run-app.sh video.mp4 midi.mid 2>&1 | tee /tmp/freemusic-debug.log
  ```
  then share back that file's contents (or the relevant excerpt) for you to read with the `Read`
  tool.

`docs/verification.md` has the current, standing verification procedures — always current-state,
meant for you to hand to the user, never to run yourself. `docs/narratives/verification.md` has
the history behind those procedures (specific mistakes found, and the milestone 3-5 patterns from
before the "never run the app yourself" rule existed) — read it for context, not as instructions
for you to execute yourself.

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
written to the `.mid` file either) — see `docs/ui.md`'s "Note editor" section for the full design
(identity key, persistence, filtering, and why it's non-destructive).

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
path as the Project tab's Load button, so it replaces video/MIDI/sync/calibration/transform/style
with whatever the project file contains — and anything else is treated as the video.
`app song.fmproj.ron look.fmstyle.ron` and `app video.mp4 song.mid look.fmstyle.ron` both work,
order-independent, without needing a separate flag.

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

`~/.zshenv` on this machine sources `$HOME/.cargo/env`, so `cargo` is on `PATH` in fresh sessions
without the manual `source` above (see `docs/narratives/building.md` for why `.zshenv`
specifically).

### System dependencies (Linux dev environment)

Not vendored, must be present on the machine:
- FFmpeg dev libraries (`libavcodec`, `libavformat`, `libavutil`, `libswscale`, `libswresample`,
  **and** `libavfilter`, `libavdevice` — `ffmpeg-next`'s default feature set enables its `filter`
  and `device` features, which pull in `ffmpeg-sys-next/avfilter` and `ffmpeg-sys-next/avdevice`
  respectively, so all seven `-dev` packages are required even though only five look obviously
  video-related) for `ffmpeg-sys-next`'s bindgen step, plus `clang`/`llvm`.
- Vulkan loader + a driver. Under WSL2 specifically, `mesa`'s default packages ship no Vulkan ICD
  at all — install `vulkan-dzn` (Mesa's D3D12-passthrough driver, exposes the real GPU through
  `/dev/dxg`) or `vulkan-swrast` (lavapipe, software fallback) from the `extra` repo. `wgpu`
  respects `WGPU_BACKEND` (e.g. `WGPU_BACKEND=gl`) to force a specific backend if the default
  picks something broken.
- `libxkbcommon-x11` if winit falls back to the X11 backend (e.g. `WAYLAND_DISPLAY` unset) —
  without it winit panics at startup with "Library libxkbcommon-x11.so could not be loaded",
  it does not silently fall back further.

**Dynamic-link FFmpeg version pin and vendored patch (matters most on Windows):** `ffmpeg-next
8.1.0` (pinned in `crates/{video-pipeline,export,audio-playback}/Cargo.toml`) wraps FFmpeg 7.x's C
API. `vendor/ffmpeg-next/` is a vendored, patched copy of `ffmpeg-next 8.1.0` that makes it compile
against FFmpeg builds compiled with `--disable-deprecated` (as BtbN's `n7.1-latest` builds are) and
against codec IDs/enum variants added in FFmpeg 7.1.5+; the workspace `Cargo.toml` has
`[patch.crates-io] ffmpeg-next = { path = "vendor/ffmpeg-next" }` to use it. The patch also adds a
`SwrContext::convert_planes` method (not part of upstream `ffmpeg-next`), used by
`crates/export/src/audio.rs` and `crates/audio-playback/src/lib.rs` in place of
`swr_convert_frame` (see their doc comments for why). `crates/mp4-encoder/src/audio.rs` hardcodes
`AV_SAMPLE_FMT_FLTP` + 44100 Hz for AAC rather than reading the same deprecated fields. If
`ffmpeg-next` is ever bumped to a version that handles all of this upstream, remove the vendor
directory and the patch entry (the `convert_planes` method would need to be re-added by hand since
it isn't an upstream feature). Full narrative — the exact compile errors, why the vendor tree's
generated bindings aren't pinned/checked in and what that means for CI vs. local builds, and the
`in_planes` pointer-cast fix — is in `docs/narratives/building.md`.

**`ffmpeg-sys-next` is also vendored/patched** (`vendor/ffmpeg-sys-next/`, same `8.1.0` pin, same
`[patch.crates-io]` mechanism), fixing two unrelated MSVC-only build bugs found while getting the
`static-ffmpeg` feature working on Windows: bogus `-march=native`/`-mtune=native` GCC flags passed
to `cl.exe`, and MSVC `-libpath:` linker flags misparsed as library names (E0459). Both patches are
Windows/MSVC-specific and inert on Linux/macOS; full narrative is in `docs/narratives/building.md`
— read that before touching either patch. If `ffmpeg-sys-next` is ever bumped past both bugs being
fixed upstream, remove `vendor/ffmpeg-sys-next/` and its patch entry the same way as `ffmpeg-next`
above.

### Versioning

All crates (`app` and every `crates/*` member) share one version number via `version.workspace =
true`, sourced from `[workspace.package].version` in the root `Cargo.toml` — bump that one field to
match the `v*` git tag before pushing a release tag (e.g. `version = "0.2.0"` for tag `v0.2.0`).
Still pre-1.0 per this file's own versioning policy above, so `0.x.y` for now.

### Static/cross-platform release builds

Added a `static-ffmpeg` cargo feature (on `app`, `export`, `video-pipeline`, `audio-playback`,
`mp4-encoder`) that vendors and statically links FFmpeg (via `ffmpeg-sys-next`'s `build` feature)
plus `libx264` (since `mp4-encoder` prefers the `libx264` encoder by name), so release binaries
run on machines with no FFmpeg installed. `.github/workflows/release.yml` builds this for Linux
(x86_64), Windows (x86_64), and macOS (arm64 only) on a pushed `v*` tag or manual dispatch
(`workflow_dispatch`'s `only` input can also trigger a single platform leg, useful for debugging
one leg without a full release run). `scripts/build-static-linux.sh`/
`scripts/build-static-windows.ps1` reproduce that build locally, with `scripts/setup-msvc-x64.ps1`
to load a correct x64 MSVC dev environment first on Windows. Full prerequisites and the
from-source-static-libx264 recipe are in **[`docs/building.md`](docs/building.md)**; every gotcha
found getting this working (two `ffmpeg-sys-next` MSVC bugs, the shared-libx264-search-order trap,
the Windows libx264 architecture-mismatch saga, and the Windows-specific shell/encoding pitfalls)
is in `docs/narratives/building.md`.

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
  docs/                  # detailed current-state design docs, split out of this file — see below
    narratives/            # what-worked/gotcha/bugfix narrative for each docs/*.md area
  explorations/          # standalone, non-integrated experiments — not wired into the app/build
```

**The rest of this project's design detail lives in `docs/`, and the history/bug-postmortem
narrative behind it lives in `docs/narratives/`** — both split out of this file to keep it short.
Read the relevant descriptive doc before touching that area of the code; its `docs/narratives/`
counterpart (same filename) has the story behind a design decision or a bug that shaped it — useful
context, not instructions to follow.

- **`docs/architecture.md`** — Neothesia-derived dependencies, the `project` crate (sync,
  calibration, persistence), video transform (brightness/scale/crop/rotate/tilt/translate) math,
  wgpu/egui-wgpu version pinning, video-pipeline decode/seek data flow, interactive rendering
  (`app`), and MP4 export, all as current-state description. `docs/narratives/architecture.md` has
  the bug postmortems and design decisions behind it (a rotation-matrix sign bug, a hybrid-core
  scheduling bug behind mouse-move playback lag, an unthrottled-redraw perf bug, sRGB/darkness
  bugs, a crop-box preview overlay that was built and then removed, and more).
- **`docs/ui.md`** — the current UI: the tabbed side panel (Project/Keyboard/Style/Transform/
  Export), the custom timeline (waveform/scroll/collapsible panel), the Style tab's full live
  `.fmstyle.ron` editing (background/notes/barrier/transitions/octave lines, via the reusable
  `app/src/style_ui.rs` widget library) and its Project-tab file import/export counterpart, the
  File menu bar and native dialogs, keyboard shortcuts, synced audio playback, per-octave keyboard
  calibration, and the note editor (list/delete/restore, duration editing, adding new notes).
  `docs/narratives/ui-milestones.md` has the milestone-by-milestone history (6a–6e, and the later
  Style-tab unification) and the bugs found building each one.
- **`explorations/barrier-fx-lab/`** — a standalone WebGL2 HTML page (no build step, no app
  dependency) for prototyping barrier looks — glow sigmas, wavy-edge modes, strand bundles, and
  electric/wispy filament/wisp effects not yet in `barrier.wgsl` — before committing any of it to
  the real renderer. Its `presets/` holds exported JSON snapshots of looks worth keeping, notably
  `seemusic-found.json`, the closest match found so far to the SeeMusic edge in `sm-ex.png`; see the
  directory's own `README.md`. The barrier strand bundle and the god-ray/halo-ring/chromatic-
  aberration flash group have both since been ported into the real app
  (`project::StrandSpec`/`WavySpec::strands`, `project::GodRaySpec`/`RingSpec`/`FlashSpec::ring`/
  `god_rays`/`chromatic_aberration` — see `docs/fmstyle-format.md`); the lab's sliding-filament/wisp
  controls remain the only unported experiments.
- **`docs/fmstyle-format.md`** — the living field-by-field `.fmstyle.ron` format spec (defaults,
  meaning, RON snippets) — keep this in sync whenever the schema changes, it's the spec, not
  narrative. `docs/narratives/fmstyle-milestone.md` and `docs/narratives/fmstyle-history.md` have
  the phase-by-phase development narrative, design history, bug-fix postmortems (e.g. the black-key
  gradient bug, the three-generation glow/brightness redesign), and the breaking-change migration
  log for hand-migrating an old `.fmstyle.ron` file.
- **`docs/verification.md`** — the current, standing procedures for verifying changes to
  `app`/`video-pipeline`/export: generating synthetic test clips, screenshotting under WSL2 vs.
  native Hyprland, verifying drag/persistence interactions, and verifying MP4 export. Per this
  file's own top-level rule, never run the app yourself — these are patterns to hand to the user,
  except where noted as safe static-file analysis (e.g. `ffprobe`). `docs/narratives/
  verification.md` has the specific mistakes and discoveries behind these procedures.
- **`docs/building.md`** — the Windows dynamic-link dev setup, the `static-ffmpeg` feature and its
  from-source-static-libx264 recipe, the `scripts/build-static-*`/`scripts/setup-msvc-x64.ps1`
  helper scripts, and how the GitHub Releases binaries get built. `docs/narratives/building.md` has
  every gotcha found getting this working (the FFmpeg-vendoring story, two `ffmpeg-sys-next` MSVC
  bugs, the shared-libx264 search-order trap, the Windows libx264 architecture-mismatch saga,
  Developer Shell/PowerShell encoding pitfalls, and CI-specific MSYS2/pkg-config hazards).
- **`docs/narratives/`** — every "what worked / what didn't / gotcha found / bug postmortem /
  design decision" narrative for this project, one file per area, named to match its descriptive
  counterpart above. See `docs/narratives/README.md` for the full index.
