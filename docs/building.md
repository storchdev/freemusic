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
# the Rust toolchain's target arch — see the gotchas below), and needs MSYS2 installed (with
# `pacman -S make nasm diffutils pkgconf git`) for the sh/make/nasm/pkgconf/git that FFmpeg's and
# libx264's build scripts need. Defaults to C:\msys64; pass -Msys2Dir if installed elsewhere.
# scripts\setup-msvc-x64.ps1 loads a correct x64 dev environment into the current session if you
# don't already have one open (see the gotchas below for why this needs its own script).
.\scripts\setup-msvc-x64.ps1
.\scripts\build-static-windows.ps1
# -> dist\freemusic-windows-x86_64.exe
```

Both scripts are exactly what `.github/workflows/release.yml` does per-OS (see below), just
runnable locally instead of in CI. Unlike the CI runners (which start from a clean image), a dev
machine may well already have a system `libx264` installed, so both scripts also set
`RUSTFLAGS="-L native=<x264-static>/lib -l static=x264"` before the `cargo build` step — this
forces rustc to require a static `x264` archive and outright refuse a shared/import lib, so the
search-order gotcha above can't silently reintroduce a dynamic dependency even if one exists on
the system. The Linux script still runs an `ldd` check afterwards as a belt-and-suspenders
sanity check. There's no equivalent macOS script yet — follow the manual recipe above (or read
the release workflow's macOS steps) until one is added.

### Windows static-build gotchas found the hard way

These are already fixed (vendored patches, or the scripts themselves) — you don't need to do
anything about them — but if a similar-looking error shows up again on Windows, this is the
first place to check, since FFmpeg's `configure` tends to collapse very different underlying
causes into the same generic-sounding error message.

**`ffmpeg-sys-next` MSVC bug #1 — bogus GCC flags passed to `cl.exe`:** the published
`ffmpeg-sys-next 8.1.0` crate's `build.rs` unconditionally adds `--extra-cflags=-march=native
-mtune=native` to FFmpeg's own `configure` invocation whenever `target == host` (every
non-cross-compiling build), with no MSVC awareness at all. `cl.exe` has no `-march`/`-mtune`
equivalent (GCC/Clang-only syntax) and rejects it with `cl : Command line error D8043 : unknown
option '-mtune=native'`, which `configure` then reports as the much more confusing "cl.exe is
unable to create an executable file" / "C compiler test failed" — indistinguishable at a glance
from a missing-Windows-SDK or wrong-toolchain problem. (An unreleased post-8.1.0 version of the
crate added `FFMPEG_MARCH`/`FFMPEG_MTUNE` env vars to override this, but that escape hatch doesn't
work on Windows regardless of crate version: Win32's `SetEnvironmentVariable`, which PowerShell/
.NET use to set child-process environment, treats an empty-string value as "delete the variable,"
so `$env:FFMPEG_MARCH = ""` can't express "set to empty" at all.) Fixed by a vendored patch in
`vendor/ffmpeg-sys-next/` that special-cases `cfg!(target_env = "msvc")` to skip adding the flag.
`config.log`'s tail (under `target\release\build\ffmpeg-sys-next-*\out\ffmpeg-*\ffbuild\`) always
has the real underlying `cl.exe` error if this recurs.

**`ffmpeg-sys-next` MSVC bug #2 — libpath flags misparsed as library names:** once FFmpeg's
`configure` succeeds, `build.rs` parses `ffbuild/config.mak`'s `EXTRALIBS` line to forward extra
linker flags (e.g. from libx264's pkg-config output) as `cargo:rustc-link-lib=...`/
`cargo:rustc-link-search=...` directives, filtering for library-name flags via
`flag.starts_with("-l")`. MSVC's linker spells a library *search path* as `-libpath:DIR` (a
lowercased, single-dash rendering of `/LIBPATH:DIR`), which also starts with `-l`. The unpatched
code matched that, stripped its first 2 characters, and emitted `cargo:rustc-link-lib=ibpath:/c/
Users/.../x264-static/lib` — cargo passes that to rustc as `-l ibpath:/c/Users/.../lib`, i.e.
"library named `ibpath`, renamed to `/c/Users/.../lib`" (rustc's `-l NAME:RENAME` syntax), which
fails with `error: renaming of the library `ibpath` was specified, however this crate contains no
#[link(...)] attributes referencing this library` (E0459) — `cargo build -v`, grepping the failing
rustc invocation for `-l` flags, is what cracks this one. Fixed by the same vendored patch adding
an `is_msvc_libpath` check (case-insensitive `-libpath:` prefix) that routes these into the
search-path loop instead of the library-name loop.

**libx264 architecture mismatch — the same generic FFmpeg error, a third time:** FFmpeg's
`./configure` compiles+links a tiny x264 test program as part of its pkg-config-based x264
detection; linking a test object of one architecture against a `libx264.lib` built for a
*different* architecture produces `LNK2019 unresolved external symbol x264_encoder_encode` plus an
`LNK4272 library machine type 'x86' conflicts with target machine type 'x64'` warning (visible in
`ffbuild/config.log`'s `check_pkg_config`/`test_ld` section), which `configure` collapses into the
same generic `ERROR: x264 not found using pkg-config` as an actually-missing library.

This happened twice while getting `build-static-windows.ps1` working, for two different reasons.
First: the script's `libx264` build cache (`~/x264-static/lib/libx264.lib`) had been built for the
wrong architecture (x86) in an earlier session, and the script's only staleness check was
`Test-Path $pcFile` — it never verified the cached archive's arch matched the current one. Fixing
that by keying the cache directory on `$env:VSCMD_ARG_TGT_ARCH` (`~/x264-static-x64` instead of an
unsuffixed `~/x264-static`) surfaced it *again*, differently: a run from VS's **x86** Native Tools
shell (VS ships separate x64/x86/ARM64 "Native Tools"/"Developer PowerShell" shortcuts, and it's
easy to launch the wrong one while still thinking of it generically as "a Developer PowerShell")
built libx264 as a 32-bit archive, then failed the final link with 150 `LNK2019` unresolved
externals — because cargo/rustc's own MSVC-toolset discovery targets whatever the active Rust
toolchain's default host triple is (`x86_64-pc-windows-msvc`, confirmed by `...\bin\HostX64\x64\
link.exe` and `rustlib\x86_64-pc-windows-msvc\lib` in the failing link command) **independent of
which vcvars variant happens to be on PATH** — so `VSCMD_ARG_TGT_ARCH` isn't actually the
authoritative signal for which arch libx264 needs to be. (The `LNK4272` warning also showed up on
*every* linked rlib in that failure, not just x264's — link.exe cascades that warning onto all
remaining inputs once it hits one genuine mismatch; that's not evidence everything was actually
recompiled 32-bit.)

The actual fix, now in `build-static-windows.ps1`: derive the target arch from `rustc -vV`'s
`host:` triple (mapped to VS's `x64`/`x86`/`arm64` naming) instead of `VSCMD_ARG_TGT_ARCH`, and
fail fast with an actionable message if the active Developer Shell's arch doesn't match it, since
the x264 build still needs the *matching* cl.exe on PATH.

**Wrong Developer Shell shortcut — the "Developer PowerShell for VS 2022" Start Menu entry
defaults to x86:** that shortcut's target is `C:\Windows\SysWOW64\WindowsPowerShell\v1.0\
powershell.exe` — the 32-bit PowerShell host (SysWOW64 is where 32-bit binaries live on 64-bit
Windows) — and `Enter-VsDevShell` infers its default architecture from the host process's own
bitness, so this shortcut silently gives you an x86 environment with nothing in its name
suggesting that. `scripts/setup-msvc-x64.ps1` sidesteps this by calling `vcvars64.bat` directly
instead of going through `Enter-VsDevShell`'s auto-detection, and searches the common VS 2022
edition install paths for it.

**Batch files don't persist environment variables into a calling PowerShell session:** running a
`.bat` directly from PowerShell (`& vcvars64.bat`) executes it in a child `cmd.exe` process, so
every environment variable it sets (`PATH`, `INCLUDE`, `LIB`, `VSCMD_ARG_TGT_ARCH`, etc.) is lost
the instant that child process exits — `$env:VSCMD_ARG_TGT_ARCH` comes back empty in the calling
PowerShell session afterward, with no error. `setup-msvc-x64.ps1` works around this by running
`vcvars64.bat` inside `cmd /c "... && set"`, capturing the resulting environment as text, and
re-applying each `NAME=value` pair into the *current* process via
`[System.Environment]::SetEnvironmentVariable`.

**Windows PowerShell 5.1 misreads non-ASCII characters in a BOM-less script:** Windows PowerShell
5.1 (not PowerShell 7/`pwsh`) reads a script file without a byte-order mark using the system
codepage rather than UTF-8. Both `.ps1` scripts here have comments with em dashes (`—`); without a
BOM, PS 5.1 misdecodes those multi-byte UTF-8 sequences, which can corrupt the tokenizer's state
by the time it reaches later code (observed as a spurious "Missing closing '}' in statement block"
several lines after the actual em dash). Both scripts are saved with a UTF-8 BOM to force correct
decoding under PS 5.1; keep that BOM if re-saving either file. (This only bit on real Windows
hardware under Windows PowerShell 5.1 — `[System.Management.Automation.Language.Parser]::ParseFile`
under PowerShell 7 parses the same BOM-less file without complaint, so this class of bug is easy
to miss when only testing under `pwsh`.)

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
    specifically, which is easy to get wrong; building it the same way as the other two platforms
    keeps the recipe (and the "never let a shared libx264 exist" invariant) identical everywhere.

  `.github/workflows/release.yml` (below) automates all of this for CI; it's the easiest
  reference for the exact commands if you're setting this up by hand.

## Release binaries

`.github/workflows/release.yml` builds standalone (`static-ffmpeg`) binaries for Linux
(x86_64), Windows (x86_64), and macOS (x86_64 and arm64) and attaches them to a GitHub Release.
It runs on any pushed tag matching `v*` (e.g. `v0.1.0`), or manually via the Actions tab's
"Run workflow" button (`workflow_dispatch`), which is useful for testing the build without
cutting a real release.
