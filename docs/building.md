# Building

There are two different things "building this project" can mean, and they need different setups:

- **Developing/running it yourself**: dynamically link against an FFmpeg you already have (or
  download) — fast to compile, no third-party build tools beyond the FFmpeg package itself. The
  resulting binary only runs on machines with those same FFmpeg libraries/DLLs available. The
  README's "Building" section covers the quick-start Linux/macOS path; this doc has the fuller
  Windows dev setup below.
- **Producing a standalone release binary** (below): vendor and statically link FFmpeg + libx264
  so the binary runs standalone on any machine, with no FFmpeg install step for whoever downloads
  it. Slower to compile, more build-time tools, and the whole point of the exercise is that *you*
  eat that cost so users don't have to. This is what `.github/workflows/release.yml` uses to
  build the binaries attached to GitHub Releases — if you just want a working `.exe`/binary and
  aren't modifying the code, download one from [Releases](../../../releases) instead of building
  anything at all.

## Windows dev setup (dynamic linking)

**Windows** has no package manager for FFmpeg dev libraries, so the easiest path there is:

1. Install [Rust](https://rustup.rs/) (defaults to the MSVC toolchain) and
   [Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/) (C++ workload, for
   the linker) and LLVM (`winget install LLVM.LLVM`, for `ffmpeg-sys-next`'s bindgen step).
2. A Vulkan driver — normally already present via your GPU driver.
3. Download a prebuilt FFmpeg **shared** dev package from
   [BtbN/FFmpeg-Builds releases](https://github.com/BtbN/FFmpeg-Builds/releases) and extract it,
   e.g. to `C:\ffmpeg` (it already has the `lib\`+`include\` layout `ffmpeg-sys-next` expects).
   Any BtbN shared build (7.x or 8.x) should work — this repo vendors a patched copy of
   `ffmpeg-next 8.1.0` (in `vendor/ffmpeg-next/`) that handles the deprecated API fields and enum
   variants that BtbN builds omit (BtbN compiles with `--disable-deprecated`). If you already have
   an FFmpeg 8.x SDK on your machine, point `FFMPEG_DIR` at it directly.
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

## Static/cross-platform builds (standalone release binaries)

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
binary can quietly depend on that shared lib after all, with no warning from cargo or the linker.
Always build `libx264` from source yourself as a static-only archive (`--enable-static`, no
`--enable-shared`) and point `PKG_CONFIG_PATH` at it, so no shared alternative exists anywhere for
the linker to prefer. See `docs/implementation-notes.md` for the history behind this invariant.

```sh
git clone --depth 1 https://code.videolan.org/videolan/x264.git
cd x264
./configure --enable-static --disable-cli --enable-pic --prefix="$HOME/x264-static"
make -j"$(nproc)" && make install
export PKG_CONFIG_PATH="$HOME/x264-static/lib/pkgconfig"
```

**Scripted, on your own machine:** `scripts/build-static-linux.sh` (Linux) and
`scripts/build-static-windows.ps1` (Windows) automate the whole recipe above — building static
`libx264`, setting `PKG_CONFIG_PATH`, and running the `static-ffmpeg` release build — and drop the
result in `dist/`. Run the one matching the OS you're on (neither cross-compiles):

```sh
# Linux — needs nasm, pkg-config, clang, and make/build-essential on PATH; the script checks and
# tells you what's missing. Re-running is cheap: it skips the libx264 build if already present at
# ~/x264-static.
scripts/build-static-linux.sh
# -> dist/freemusic-linux-x86_64
```

```powershell
# Windows — must be run from an x64 Developer Shell (cl.exe/lib.exe/link.exe on PATH, matching
# the Rust toolchain's target arch), and needs MSYS2 installed (with
# `pacman -S make nasm diffutils pkgconf git`) for the sh/make/nasm/pkgconf/git that FFmpeg's and
# libx264's build scripts need. Defaults to C:\msys64; pass -Msys2Dir if installed elsewhere.
# scripts\setup-msvc-x64.ps1 loads a correct x64 dev environment into the current session.
.\scripts\setup-msvc-x64.ps1
.\scripts\build-static-windows.ps1
# -> dist\freemusic-windows-x86_64.exe
```

Both scripts are exactly what `.github/workflows/release.yml` does per-OS (see below), just
runnable locally instead of in CI. Unlike the CI runners (which start from a clean image), a dev
machine may well already have a system `libx264` installed, so both scripts also set
`RUSTFLAGS="-L native=<x264-static>/lib -l static=x264"` before the `cargo build` step — this
forces rustc to require a static `x264` archive and outright refuse a shared/import lib, so the
search-order issue above can't silently reintroduce a dynamic dependency even if one exists on
the system. The Linux script still runs an `ldd` check afterwards as a belt-and-suspenders
sanity check. There's no equivalent macOS script yet — follow the manual recipe above (or read
the release workflow's macOS steps) until one is added.

### Windows troubleshooting pointers

FFmpeg's `configure` often collapses unrelated Windows failures into generic messages like
`C compiler test failed`, `x264 not found using pkg-config`, or unresolved x264 symbols. When that
happens, check `ffbuild/config.log` under
`target\release\build\ffmpeg-sys-next-*\out\ffmpeg-*\ffbuild\` for the real compiler/linker error.

The current scripts and vendored patches handle the known MSVC/static-build traps: MSVC-incompatible
`ffmpeg-sys-next` flags, MSVC `-libpath:` parsing, x264 architecture mismatches, Developer Shell
architecture setup, PowerShell environment propagation, and Windows PowerShell 5.1 script encoding.
Historical details live in `docs/implementation-notes.md`.

**MSYS2's own `link.exe` shadowing MSVC's**, inside any `shell: msys2 {0}` step (CI) or a raw
MSYS2 bash invocation (local): MSYS2 ships a coreutils `link.exe` (the `link(1)` hard-link tool,
unrelated to linking object files) under `usr/bin`, and that shell type always puts `usr/bin`
ahead of whatever `msvc-dev-cmd`/a Developer Shell already put on `PATH` — regardless of what
order the setup steps ran in. The symptom is `cargo build`'s link step failing with `link.exe`
"extra operand" errors while pointing at a path under `...\msys64\usr\bin\link.exe` instead of
the real MSVC linker. Fix: reorder `PATH` so `usr/bin` comes *after* the MSVC dirs, right before
the `cargo build`/link invocation, e.g. `export PATH="${PATH/\/usr\/bin:/}:/usr/bin"` in bash.
Both `release.yml`'s Windows `cargo build` step and `scripts/build-static-windows.ps1` do this.

Per-OS prerequisites for the `static-ffmpeg` feature (on top of the Vulkan/`libxkbcommon-x11`
requirements above, which are unrelated to FFmpeg and still apply, and the from-source `libx264`
build above, which applies to every OS):

- **Linux**: `nasm`, `pkg-config`, `clang`, `make`/`build-essential` (to build `libx264` and
  FFmpeg itself).
- **macOS**: Xcode Command Line Tools (`clang`, `make`), plus `nasm` from Homebrew
  (`brew install nasm`) — again, don't `brew install x264`, build it from source per above.
- **Windows**: this is the fussiest platform, since FFmpeg's build system is a `sh`/`configure`/
  `make` script, not something MSVC or `cargo` understands natively:
  - [MSYS2](https://www.msys2.org/), with `make`, `nasm`, `diffutils`, `pkgconf`, and `git`
    installed via its `pacman` (`pacman -S make nasm diffutils pkgconf git`) — its `usr/bin` needs
    to be on `PATH` so `ffmpeg-sys-next`'s build script can find `sh.exe`, and `git` is what
    `build-static-windows.ps1` uses (from inside that same MSYS2 shell) to clone libx264's source.
  - Visual Studio Build Tools (the MSVC C++ toolchain), with its environment set up (`cl.exe`,
    `lib.exe`, `link.exe` on `PATH` — e.g. via a "Developer Command Prompt", or the
    `ilammy/msvc-dev-cmd` GitHub Action in CI) *before* running `cargo build`. FFmpeg's configure
    script auto-detects the MSVC toolchain from `rustc`'s target (`--toolchain=msvc`), so no extra
    flags are needed once both of the above are on `PATH`.
  - `libx264` built statically for MSVC — via the same MSYS2 shell (`sh`/`make`/`nasm` are all
    already there), *not* vcpkg: unlike the from-source build above, vcpkg's `x264` port has no
    shared-lib variant to worry about only if you use its `x64-windows-static` triplet
    specifically, which is easy to misconfigure; building it the same way as the other two platforms
    keeps the recipe (and the "never let a shared libx264 exist" invariant) identical everywhere.

  `.github/workflows/release.yml` (below) automates all of this for CI; it's the easiest
  reference for the exact commands if you're setting this up by hand.

## Release binaries

`.github/workflows/release.yml` builds standalone (`static-ffmpeg`) binaries for Linux
(x86_64), Windows (x86_64), and macOS (x86_64 and arm64) and attaches them to a GitHub Release.
It runs on any pushed tag matching `v*` (e.g. `v0.1.0`), or manually via the Actions tab's
"Run workflow" button (`workflow_dispatch`), which is useful for testing the build without
cutting a real release.
