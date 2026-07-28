# Building

There are two different things "building this project" can mean, and they need different setups:

- **Developing/running it yourself**: dynamically link against an FFmpeg you already have (or
  download) — fast to compile, no third-party build tools beyond the FFmpeg package itself. The
  resulting binary only runs on machines with those same FFmpeg libraries/DLLs available. The
  README's "Building" section covers the quick-start Linux/macOS path; this doc has the fuller
  Windows dev setup below.
- **Producing a standalone release binary** (below): vendor and statically link FFmpeg + libx264
  so the binary runs standalone on any machine, with no FFmpeg install step for whoever downloads
  it. Slower to compile and needs more build-time tools. This is what
  `.github/workflows/release.yml` uses to build the binaries attached to GitHub Releases — if you
  just want a working `.exe`/binary and aren't modifying the code, download one from
  [Releases](../../../releases) instead of building anything at all.

## Windows dev setup (dynamic linking)

Windows has no package manager for FFmpeg dev libraries, so the path there is:

1. Install [Rust](https://rustup.rs/) (defaults to the MSVC toolchain), the
   [Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/) (C++ workload, for
   the linker), and LLVM (`winget install LLVM.LLVM`, for `ffmpeg-sys-next`'s bindgen step).
2. A Vulkan driver — normally already present via the GPU driver.
3. Download a prebuilt FFmpeg **shared** dev package from
   [BtbN/FFmpeg-Builds releases](https://github.com/BtbN/FFmpeg-Builds/releases) and extract it,
   e.g. to `C:\ffmpeg` (it already has the `lib\`+`include\` layout `ffmpeg-sys-next` expects). Any
   BtbN shared build (7.x or 8.x) works, including builds compiled with `--disable-deprecated` —
   this repo vendors a patched copy of `ffmpeg-next 8.1.0` (`vendor/ffmpeg-next/`) that handles the
   resulting API gaps; see `crates/{video-pipeline,export,audio-playback}/Cargo.toml` for the pin.
   If an FFmpeg 8.x SDK is already on the machine, point `FFMPEG_DIR` at it directly.
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

This path needs no MSYS2, no compiling FFmpeg/libx264 from source, and no MSVC-toolchain-in-a-shell
setup — those are only needed for the static/release path below.

## Static/cross-platform builds (standalone release binaries)

By default `cargo build` dynamically links against FFmpeg dev libraries already installed on the
system (see above) — fastest to compile, but the resulting binary only runs on machines that also
have those libraries installed.

Passing `--features static-ffmpeg` instead vendors FFmpeg's source (via `ffmpeg-sys-next`'s `build`
feature) and compiles + statically links it — including `libx264`, since the exporter prefers the
`libx264` encoder by name for MP4 output — straight into the binary. The result runs standalone on
any machine, which is what release binaries (see below) are built with. It's slower to compile
(FFmpeg + libx264 get built from source on every clean build) and needs a few more build-time
tools than the dynamic path:

```sh
cargo build --release -p app --features static-ffmpeg
```

FFmpeg's own configure step links `libx264` in by *name* (`-lx264`), not by embedding it, so it
still needs a `libx264` it can find at final-link time, vendored FFmpeg or not. Do not install a
system/Homebrew/apt `libx264`/`x264` package for this build: the linker prefers whatever's on its
default search path over an explicit `-L` static archive by search order, not by "static wins", so
a shared lib anywhere on that path can quietly get linked into a build that otherwise looks static.
Always build `libx264` from source as a static-only archive (`--enable-static`, no
`--enable-shared`) and point `PKG_CONFIG_PATH` at it, so no shared alternative exists for the
linker to prefer:

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
result in `dist/`. Run the one matching the OS: neither cross-compiles.

```sh
# Linux — needs nasm, pkg-config, clang, and make/build-essential on PATH; the script checks and
# reports what's missing. Re-running is cheap: it skips the libx264 build if already present at
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

Both scripts are what `.github/workflows/release.yml` does per-OS (see below), runnable locally.
Both also set `RUSTFLAGS="-L native=<x264-static>/lib -l static=x264"` before the `cargo build`
step, forcing rustc to require a static archive and refuse a shared/import lib for x264 — required
on Windows for the final link to succeed at all, and a defensive safeguard on Linux/macOS against
a system `libx264` being present. The Linux script also runs `ldd` afterward as a final check.
There's no equivalent macOS script yet — follow the manual recipe above, or read the release
workflow's macOS steps.

### Windows prerequisites for `static-ffmpeg`

On top of the Vulkan/`libxkbcommon-x11` requirements above (unrelated to FFmpeg) and the
from-source `libx264` build above (applies to every OS):

- [MSYS2](https://www.msys2.org/), with `make`, `nasm`, `diffutils`, `pkgconf`, and `git` installed
  via its `pacman` (`pacman -S make nasm diffutils pkgconf git`) — its `usr/bin` needs to be on
  `PATH` so `ffmpeg-sys-next`'s build script can find `sh.exe`, and `git` is what
  `build-static-windows.ps1` uses (from inside that same MSYS2 shell) to clone libx264's source.
- Visual Studio Build Tools (the MSVC C++ toolchain), with its environment set up (`cl.exe`,
  `lib.exe`, `link.exe` on `PATH` — e.g. via a "Developer Command Prompt", or the
  `ilammy/msvc-dev-cmd` GitHub Action in CI) *before* running `cargo build`. FFmpeg's configure
  script auto-detects the MSVC toolchain from `rustc`'s target (`--toolchain=msvc`), so no extra
  flags are needed once both of the above are on `PATH`.
- `libx264` built statically for MSVC via that same MSYS2 shell (`sh`/`make`/`nasm` are all already
  there) — not vcpkg, to keep the recipe (and the "never let a shared libx264 exist" invariant)
  identical across platforms.

Linux needs `nasm`, `pkg-config`, `clang`, `make`/`build-essential` (to build `libx264` and FFmpeg
itself). macOS needs Xcode Command Line Tools (`clang`, `make`) plus `nasm` from Homebrew
(`brew install nasm`) — don't `brew install x264`, build it from source per above.

If FFmpeg's `configure` fails with a generic message (`C compiler test failed`, `x264 not found
using pkg-config`, unresolved x264 symbols), check `ffbuild/config.log` under
`target\release\build\ffmpeg-sys-next-*\out\ffmpeg-*\ffbuild\` for the real compiler/linker error
(`release.yml`'s "Show FFmpeg configure log on failure" step dumps this automatically in CI).
Detailed context on the specific Windows/MSVC/CI build traps these scripts and vendored patches
work around lives in `docs/narratives/building.md`.

## Release binaries

`.github/workflows/release.yml` builds standalone (`static-ffmpeg`) binaries for Linux (x86_64),
Windows (x86_64), and macOS (arm64) and attaches them to a GitHub Release. It runs on any pushed
tag matching `v*` (e.g. `v0.1.0`), or manually via the Actions tab's "Run workflow" button
(`workflow_dispatch`), which also accepts an `only` input to build a single platform leg.
