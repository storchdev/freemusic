# freemusic

A native desktop app (Rust, `winit` + `wgpu` + `egui`) for making those Synthesia-style
piano-cover videos you see on YouTube and TikTok, with a real UI instead of manual video editing.
Cross-platform (Windows/macOS/Linux), and a free alternative to tools like SeeMusic.

## Features

- Load a video file and a MIDI file, and preview them composited together in real time
- Manual audio/MIDI sync with keyboard-driven fine calibration
- Video transforms: brightness, scale, crop, rotate, tilt, translate
- A "barrier" and note-highway renderer with an extensible `.fmstyle.ron` visual style format
  (gradients, sheen, glow, particles, per-key colors, wavy barrier, and more — see
  `docs/fmstyle-format.md`)
- Synced audio playback of the loaded video's own audio track during preview
- Native Open/Save dialogs, a File menu, keyboard shortcuts, and project files (`.fmproj.ron`)
  that save/restore the whole session
- Offline MP4 export (video + audio) of the composited result

> **Note:** the in-app UI only exposes a limited subset of what `.fmstyle.ron` can actually do —
> full styling control (gradients, sheen, glow, particles, per-key colors, custom shaders, etc.)
> currently requires hand-writing or generating a `.fmstyle.ron` file yourself and loading it via
> the Project tab's "Import style…" button (or a CLI arg — see below). A practical way to do this
> without learning the format by hand: hand `docs/fmstyle-format.md` (the field-by-field spec) to
> an LLM and ask it to generate a `.ron` file for the look you want. Broader UI support for
> `.fmstyle.ron` is on the roadmap below.

## Building

There are two different things "building this project" can mean, and they need different setups:

- **Developing/running it yourself** (below): dynamically link against an FFmpeg you already have
  (or download) — fast to compile, no third-party build tools beyond the FFmpeg package itself.
  The resulting binary only runs on machines with those same FFmpeg libraries/DLLs available.
- **Producing a standalone release binary** (see "Static/cross-platform builds" further down):
  vendor and statically link FFmpeg + libx264 so the binary runs standalone on any machine, with
  no FFmpeg install step for whoever downloads it. Slower to compile, more build-time tools, and
  the whole point of the exercise is that *you* eat that cost so users don't have to. This is what
  `.github/workflows/release.yml` uses to build the binaries attached to GitHub Releases — if you
  just want a working `.exe`/binary and aren't modifying the code, download one from
  [Releases](../../releases) instead of building anything at all.

### For development (dynamic linking)

System dependencies (not vendored):

- FFmpeg dev libraries (`libavcodec`, `libavformat`, `libavutil`, `libswscale`, `libswresample`)
  plus `clang`/`llvm` (needed by `ffmpeg-sys-next`'s bindgen step)
- A Vulkan loader and driver (or another `wgpu`-supported backend)
- `libxkbcommon-x11` if running under X11 (native Wayland doesn't need it)

```sh
cargo build --release
cargo run --bin app -- [video-file] [midi-file]                      # both args optional; drag-and-drop also works
cargo run --bin app -- project.fmproj.ron                            # or open a saved project directly
cargo run --bin app -- project.fmproj.ron mystyle.fmstyle.ron        # or open a project and a style file
```

**Windows** has no package manager for FFmpeg dev libraries, so the easiest path there is:

1. Install [Rust](https://rustup.rs/) (defaults to the MSVC toolchain) and
   [Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/) (C++ workload, for
   the linker) and LLVM (`winget install LLVM.LLVM`, for `ffmpeg-sys-next`'s bindgen step).
2. A Vulkan driver — normally already present via your GPU driver.
3. Download a prebuilt FFmpeg **shared** dev package — specifically a build pinned to **FFmpeg
   7.1**:
   [`ffmpeg-n7.1-latest-win64-gpl-shared-7.1.zip`](https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n7.1-latest-win64-gpl-shared-7.1.zip)
   (from [BtbN/FFmpeg-Builds releases](https://github.com/BtbN/FFmpeg-Builds/releases)) — and
   extract it, e.g. to `C:\ffmpeg` (it already has the `lib\`+`include\` layout `ffmpeg-sys-next`
   expects).
   **Don't grab `ffmpeg-master-latest-*`**: that tracks FFmpeg's git master, which is already past
   the FFmpeg 8.0 release and has dropped several `AVCodec`/`AVFrame`/`AVPacket` fields
   (`sample_fmts`, `pix_fmts`, `supported_framerates`, `ch_layouts`, etc.) that the `ffmpeg-next
   8.1.0` crate this project pins still reads directly — building against it fails with a wall of
   `E0609`/`E0425`/`E0004` errors about those fields/enum variants not existing. FFmpeg 7.1 still
   has them.
4. Set `FFMPEG_DIR=C:\ffmpeg`, then `cargo build --release` (no extra features — this is the
   default dynamic-linking path, not the static one below):
   ```powershell
   $env:FFMPEG_DIR = "C:\ffmpeg"    # PowerShell
   ```
   ```cmd
   set FFMPEG_DIR=C:\ffmpeg         :: cmd.exe
   ```
5. Copy the DLLs from that package's `bin\` folder next to `target\release\app.exe` (or add that
   `bin\` to `PATH`) so the binary can find them at runtime.

No MSYS2, no compiling FFmpeg/libx264 from source, no MSVC-toolchain-in-a-shell setup — that's all
only needed for the static/release path below, which exists to save *end users* from doing any of
this, not developers building their own copy.

### Static/cross-platform builds (standalone release binaries)

By default `cargo build` dynamically links against FFmpeg dev libraries already installed on
your system (see above) — fastest to compile, but the resulting binary only runs on machines that
also have those libraries installed.

Passing `--features static-ffmpeg` instead vendors FFmpeg's source (via `ffmpeg-sys-next`'s
`build` feature) and compiles + statically links it — including `libx264`, since the exporter
prefers the `libx264` encoder by name for MP4 output — straight into the binary. The result runs
standalone on any machine, which is what release binaries (see below) are built with. It's much
slower to compile (FFmpeg + libx264 get built from source on every clean build) and needs a few
more build-time tools than the dynamic path:

```sh
cargo build --release -p app --features static-ffmpeg
```

FFmpeg's own configure step links `libx264` in by *name* (`-lx264`), not by embedding it — so it
still needs a `libx264` it can find at final-link time, vendored FFmpeg or not. **Never install a
system/Homebrew/apt `libx264`/`x264` package for this** — if a `libx264.so`/dylib is anywhere on
the linker's default search path, the linker silently prefers it over a static `libx264.a` sitting
in an explicit `-L` directory (search order, not "prefer static" wins), so the "statically linked"
binary quietly ends up dynamically depending on that shared lib after all, with no warning from
cargo or the linker. This was confirmed the hard way: the same build that came out fully static in
a clean container (no system `libx264` present) still linked `libx264.so` on a dev machine that
happened to already have the `x264` package installed. Always build `libx264` from source yourself
as a static-only archive (`--enable-static`, no `--enable-shared`) and point `PKG_CONFIG_PATH` at
it, so no shared alternative exists anywhere for the linker to prefer:

```sh
git clone --depth 1 https://code.videolan.org/videolan/x264.git
cd x264
./configure --enable-static --disable-cli --enable-pic --prefix="$HOME/x264-static"
make -j"$(nproc)" && make install
export PKG_CONFIG_PATH="$HOME/x264-static/lib/pkgconfig"
```

Per-OS prerequisites for the `static-ffmpeg` feature (on top of the Vulkan/`libxkbcommon-x11`
requirements above, which are unrelated to FFmpeg and still apply, and the from-source `libx264`
build above, which applies to every OS):

- **Linux**: `nasm`, `pkg-config`, `clang`, `make`/`build-essential` (to build `libx264` and
  FFmpeg itself).
- **macOS**: Xcode Command Line Tools (`clang`, `make`), plus `nasm` from Homebrew
  (`brew install nasm`) — again, don't `brew install x264`, build it from source per above.
- **Windows**: this is the fussiest platform, since FFmpeg's build system is a `sh`/`configure`/
  `make` script, not something MSVC or `cargo` understands natively:
  - [MSYS2](https://www.msys2.org/), with `make`, `nasm`, `diffutils`, and `pkgconf` installed via
    its `pacman` (`pacman -S make nasm diffutils pkgconf`) — its `usr/bin` needs to be on `PATH`
    so `ffmpeg-sys-next`'s build script can find `sh.exe`.
  - Visual Studio Build Tools (the MSVC C++ toolchain), with its environment set up (`cl.exe`,
    `lib.exe`, `link.exe` on `PATH` — e.g. via a "Developer Command Prompt", or the
    `ilammy/msvc-dev-cmd` GitHub Action in CI) *before* running `cargo build`. FFmpeg's configure
    script auto-detects the MSVC toolchain from `rustc`'s target (`--toolchain=msvc`), so no extra
    flags are needed once both of the above are on `PATH`.
  - `libx264` built statically for MSVC — via the same MSYS2 shell (`sh`/`make`/`nasm` are all
    already there), *not* vcpkg: unlike the from-source build above, vcpkg's `x264` port has no
    shared-lib variant to worry about only if you use its `x64-windows-static` triplet
    specifically, which is easy to get wrong; building it the same way as the other two platforms
    keeps the recipe (and the "never let a shared libx264 exist" invariant) identical everywhere.

  `.github/workflows/release.yml` (below) automates all of this for CI; it's the easiest
  reference for the exact commands if you're setting this up by hand.

### Release binaries

`.github/workflows/release.yml` builds standalone (`static-ffmpeg`) binaries for Linux
(x86_64), Windows (x86_64), and macOS (x86_64 and arm64) and attaches them to a GitHub Release.
It runs on any pushed tag matching `v*` (e.g. `v0.1.0`), or manually via the Actions tab's
"Run workflow" button (`workflow_dispatch`), which is useful for testing the build without
cutting a real release.

See `CLAUDE.md` and `docs/` for a full architecture writeup, including the phased build history
and the design decisions behind each subsystem.

## Roadmap

Ideas being considered for future work, roughly grouped:

**`.fmstyle.ron` (visual style format)**
- Y-level-dependent note styles
- Flashes/particles that match note color
- Alpha (transparency) on notes
- Custom note textures and background textures, both compatible with note alpha — alpha would
  let a note "see through" into a static background
- Octave lines
- Reflectivity settings, for a metal-bar look
- Key-property-based styles (e.g. driven by pitch or velocity)
- Custom shaders

**UI**
- Better slider input/dragging mechanics
- More export options
- Broader `.fmstyle.ron` feature support in the UI

**End-to-end**
- Multiple styles within the same timeline (would require reworking the timeline UI and the
  `.fmstyle.ron` pipeline)

## License

This project is licensed under the GNU General Public License v3.0 (GPL-3.0-only) — see
[`LICENSE`](LICENSE).

### Third-party code and licenses

- `crates/render` depends on [`midi-file`](https://github.com/PolyMeilex/Neothesia) and
  [`piano-layout`](https://github.com/PolyMeilex/Neothesia), pinned git dependencies from the
  [Neothesia](https://github.com/PolyMeilex/Neothesia) project (GPL-3.0), by PolyMeilex and
  contributors.
- `crates/mp4-encoder` is a fork of Neothesia's own `ffmpeg-encoder` crate (GPL-3.0), adapted here
  with a parameterized frame rate, explicit codec selection, and optional audio muxing. See
  `docs/architecture.md` for the details of what changed.
- `crates/render`'s note-highway shader and rendering approach were originally based on
  Neothesia's vendored `neothesia-core` waterfall renderer before being rewritten in-tree; see
  `docs/fmstyle-milestone.md` for that history.
- All other dependencies are pulled from crates.io under their own published licenses (see
  `Cargo.lock` and each crate's own `Cargo.toml`/license file).

Neothesia is also GPL-3.0-licensed, so this project's own GPL-3.0-only license is compatible with
reusing and adapting its code. If you redistribute this project or a derivative of it, you must
keep the GPL-3.0 license and preserve copyright/attribution notices for the above third-party
code, per the terms in `LICENSE`.
