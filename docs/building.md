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
runnable locally instead of in CI. Both also set
`RUSTFLAGS="-L native=<x264-static>/lib -l static=x264"` before the `cargo build` step. On
Linux/macOS this is defensive (a dev machine, unlike a clean CI image, may already have a system
`libx264` installed, and this forces rustc to require a static archive and refuse a shared/import
lib so the search-order issue above can't reintroduce a dynamic dependency) — the Linux script
also runs an `ldd` check afterwards as a belt-and-suspenders sanity check. **On Windows it's not
just defensive, it's required**: see "FFmpeg finds x264 but the final link doesn't" below. There's
no equivalent macOS script yet — follow the manual recipe above (or read the release workflow's
macOS steps) until one is added.

### Windows troubleshooting pointers

FFmpeg's `configure` often collapses unrelated Windows failures into generic messages like
`C compiler test failed`, `x264 not found using pkg-config`, or unresolved x264 symbols. When that
happens, check `ffbuild/config.log` under
`target\release\build\ffmpeg-sys-next-*\out\ffmpeg-*\ffbuild\` for the real compiler/linker error
(`release.yml`'s "Show FFmpeg configure log on failure" step dumps this automatically in CI).

The current scripts and vendored patches handle the known MSVC/static-build traps: MSVC-incompatible
`ffmpeg-sys-next` flags, MSVC `-libpath:` parsing, x264 architecture mismatches, Developer Shell
architecture setup, PowerShell environment propagation, and Windows PowerShell 5.1 script encoding.
Historical details live in `docs/implementation-notes.md`.

**Why CI hit bugs the local script never did.** `scripts/build-static-windows.ps1` runs almost
entirely as one plain PowerShell session — it drops into MSYS2 only briefly, via a *non-login*
`bash -c`, to build x264 from source, converting any path crossing that boundary by hand
(`cygpath -u`/`-w`) exactly where it's used, then returns to plain PowerShell for the actual
`cargo build`. `release.yml` can't do that: `msys2/setup-msys2`'s `shell: msys2 {0}` wrapper is a
fixed *login* shell (`bash -leo pipefail`, no way to drop the `-l`), and env vars have to hop
between step types repeatedly — plain `pwsh` → MSYS2 login shell → back out to `cargo` (a *native*
process) → FFmpeg's own `configure` (an MSYS `sh` child of that native process). Each hop is a
place a path can get lost or silently rewritten, and each one below was hit for real in CI even
though the equivalent local step never needed a workaround:

- **MSYS2's own `link.exe` shadowing MSVC's.** MSYS2 ships a coreutils `link.exe` (the `link(1)`
  hard-link tool, unrelated to linking object files) under `usr/bin`, and inside a `msys2 {0}`
  step it always sits ahead of whatever `msvc-dev-cmd` put on `PATH`, regardless of step order —
  `cargo build`'s link step fails with `link.exe` "extra operand" errors pointing at
  `...\msys64\usr\bin\link.exe` instead of the real linker. Locally this doesn't come up, since
  PowerShell's `PATH` is a normal semicolon-joined string the script can just reorder directly. A
  first attempt to do the same PATH reordering *inside* the msys2 shell had no effect (`path-type:
  inherit` keeps `PATH` in native semicolon/backslash form, so a POSIX-style
  `${PATH/\/usr\/bin:/}` edit silently never matched it). The fix that works: resolve MSVC's
  `link.exe` absolute path in a plain (non-MSYS2) `pwsh` step right after `msvc-dev-cmd` runs, via
  `(Get-Command link.exe).Source`, and export it as `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER` —
  a `$GITHUB_ENV` var reaching every later step regardless of shell type — so `rustc` invokes that
  resolved path directly instead of searching `PATH` at all. See "Point cargo at the real MSVC
  link.exe" in `release.yml`.

- **`PKG_CONFIG_PATH` had to survive three separate hazards before FFmpeg's `configure` ever saw
  it correctly**, each only reachable because of the pwsh → login-shell → native-process chain
  above:
  1. *Mixed/drive-letter paths break it outright.* `PKG_CONFIG_PATH` is colon-delimited, and
     `pkgconf` (MSYS2's build is msys-runtime-linked, so it expects POSIX-style paths) reads a
     `cygpath -m` path like `D:/a/freemusic/.../pkgconfig` as two components split on the drive
     letter's own colon — neither of which resolves. `cygpath -u` (POSIX-style, no drive-letter
     colon) is the fix, matching `x264.pc`'s own `prefix=` line, which is POSIX-style too since
     `./configure --prefix=` gets `$HOME` as-is inside the MSYS2 shell.
  2. *Login shells re-source `/etc/profile` on every step.* The `-l` in `msys2/setup-msys2`'s
     `bash -leo pipefail` means every `msys2 {0}` step re-sources `/etc/profile.d/*.sh` before
     running anything else, and `pkgconf`'s own profile snippet unconditionally reassigns
     `PKG_CONFIG_PATH` to its defaults there — clobbering whatever an earlier step wrote to
     `$GITHUB_ENV` before the next step's own commands even start. `build-static-windows.ps1`
     avoids this entirely by using `bash -c` (no `-l`) locally. In CI, since the wrapper's `-l`
     can't be dropped, the fix is to never trust a variable to survive a step boundary under its
     real name: carry it as `FREEMUSIC_X264_PKGCONFIG` (a name no MSYS2 profile script has a
     reason to touch) and re-export the real `PKG_CONFIG_PATH` as the first line of the step that
     needs it, *after* that step's own profile-sourcing has already run.
  3. *MSYS2 auto-converts env vars crossing into a native child process.* Even with (1) and (2)
     both fixed, `configure` still failed with `x264 not found using pkg-config` — and a debug
     print in `vendor/ffmpeg-sys-next/build.rs` showed the mangled `D:/a/_temp/msys64/home/.../
     pkgconfig` form was already present the moment the build script read `PKG_CONFIG_PATH`,
     before `configure` was even spawned. MSYS2's runtime silently converts certain env vars from
     POSIX to native Windows form when it execs a *native* (non-MSYS) child — and `cargo build`,
     launched from the `msys2 {0}` step, is exactly that — reintroducing the same colon-splitting
     problem from (1) one step later, entirely outside the workflow file's control. This has no
     local equivalent because the local script never execs a native process *from inside* MSYS2 —
     it's MSYS2 that gets shelled out to from PowerShell, not the other way around.
     [`MSYS2_ENV_CONV_EXCL`](https://www.msys2.org/docs/filesystem-paths/) opts a named variable
     out of that conversion; `export MSYS2_ENV_CONV_EXCL='PKG_CONFIG_PATH'` before `cargo build`
     is the fix.

- **FFmpeg finds x264, but the final `app` link doesn't ("unresolved external symbol
  x264_param_default" etc).** This one isn't a shell/path-crossing bug — it's a gap in vendored
  `ffmpeg-sys-next`'s own build script. FFmpeg's `configure` links `libx264` in by *name*
  (`x264.lib`, MSVC's spelling of `-lx264`), so `build.rs` tries to auto-derive the equivalent
  `cargo:rustc-link-lib` directive from FFmpeg's own `config.mak` `EXTRALIBS`, filtering for
  tokens starting with `-l`. Under MSVC, FFmpeg's own toolchain translation rewrites `-lx264` into
  a bare `x264.lib` token with no `-l` prefix, so that filter silently drops it — no link
  directive for x264 is ever emitted, and the final link fails despite FFmpeg itself having found
  and built against x264 just fine one step earlier. This *does* affect the local script too, but
  it never surfaces there because `build-static-windows.ps1` already unconditionally sets
  `RUSTFLAGS="-L native=<dir> -l static=x264"` before every build (see above) — originally
  reasoned about purely as a defensive "refuse a shared lib" measure, but it turns out to also be
  the only thing that makes x264 link under MSVC at all. `release.yml` needed the same RUSTFLAGS
  line added explicitly (plus copying `libx264.lib` to `x264.lib`, since rustc's `-l static=x264`
  looks for that exact name) for the same reason.

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
