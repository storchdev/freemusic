# Building: what worked and gotchas found

Narrative companion to `docs/building.md` — the historical failure modes that shaped the build
scripts, vendored FFmpeg patches, and CI workflow, and why each fix looks the way it does.

## `cargo` missing from PATH in non-interactive shells

`~/.zshenv` on the dev machine now sources `$HOME/.cargo/env`. It previously only lived in
`.bashrc`/`.bash_profile`/`.profile`, none of which a non-interactive `zsh -c` invocation reads —
only `.zshenv` is read unconditionally by every zsh invocation, login or not. New sessions have
`cargo` on `PATH` without a manual `source` step; a shell already running before that fix landed
won't retroactively pick it up.

## CI missing `libavfilter`/`libavdevice` dev packages

`ffmpeg-next`'s default feature set enables its `filter` and `device` features, which pull in
`ffmpeg-sys-next/avfilter` and `ffmpeg-sys-next/avdevice` respectively — so all seven FFmpeg `-dev`
packages (`libavcodec`, `libavformat`, `libavutil`, `libswscale`, `libswresample`, `libavfilter`,
`libavdevice`) are required for `ffmpeg-sys-next`'s bindgen step, even though only five look
obviously video-related. `.github/workflows/unit-tests.yml`'s `apt-get install` list was originally
missing the last two and failed CI with a pkg-config "package libavfilter was not found" error
until fixed.

## The `ffmpeg-next 8.1.0` / BtbN-builds incompatibility, and the vendored patch

`ffmpeg-next 8.1.0` wraps FFmpeg 7.x's C API. The upstream `n7.1-latest` BtbN builds compile FFmpeg
without the deprecated `AVCodec` struct fields (`sample_fmts`, `pix_fmts`, `supported_framerates`,
`ch_layouts`) — bindgen omits those fields from the generated Rust struct — which caused
`ffmpeg-next 8.1.0` to fail to compile with `E0609`/`E0425`/`E0004` errors on these fields and on
several `AVCodecID` enum variants added in 7.1.5.

The fix lives in `vendor/ffmpeg-next/` — a vendored copy of `ffmpeg-next 8.1.0` with four targeted
patches applied: (1) `src/codec/video.rs` + `src/codec/audio.rs`: return `None` from the methods
that accessed those deprecated struct fields (the deprecated API is no longer reachable anyway);
(2) `src/codec/id.rs`: map `V410`/`V308`/`V408` codec IDs to `AV_CODEC_ID_NONE` in the forward
direction and add a `_ => Id::None` wildcard to the reverse match for codec IDs added in 7.1.5+;
(3) `src/util/frame/side_data.rs` + `src/codec/packet/side_data.rs`: add `_ => todo!()` wildcard
arms for enum variants added in 7.1.5+ that the crate doesn't know about yet; (4)
`src/software/resampling/context.rs`: a hand-added `SwrContext::convert_planes` method (not part of
upstream `ffmpeg-next`) that calls `swr_convert` directly instead of `swr_convert_frame`, used by
`crates/export/src/audio.rs` and `crates/audio-playback/src/lib.rs` because `swr_convert_frame`
requires frame metadata that Windows static FFmpeg 7.x AAC decode sometimes leaves zeroed.

`swr_convert`'s input-planes parameter is C's `const uint8_t **`, which bindgen renders as `*mut
*const u8` — `convert_planes` originally passed `in_planes.as_ptr()` (`*const *const u8`) uncast,
which type-checked against whatever bindgen output the dev machine happened to generate but failed
to compile (`E0308`, mismatched mutability) in GitHub Actions CI, which generates its own bindings
against the runner's system FFmpeg headers. Fixed by casting: `in_planes.as_ptr() as *mut *const
u8` (and `ptr::null_mut()` for the empty/flush case). This is a reminder that this vendor tree's
build-time-generated bindings aren't pinned/checked in — a local `cargo check` passing doesn't
guarantee CI will, whenever a patch's pointer types are written to match one machine's bindgen
output rather than the C header's actual signature.

`crates/mp4-encoder/src/audio.rs` was also updated to not access the deprecated fields directly
(hardcoded `AV_SAMPLE_FMT_FLTP` + 44100 Hz for AAC, which are its only supported values anyway),
avoiding the need for a similar patch there.

## Windows static build: verified on real hardware

`scripts/build-static-windows.ps1` has been run to a successful, verified-static completion on
real Windows hardware — `dumpbin /dependents` showed no avcodec/avformat/avutil/x264 DLL
dependency in the resulting binary.

## The static-`libx264` invariant

Static release builds must use a static-only `libx264` built from source and surfaced through
`PKG_CONFIG_PATH`. Installing a system/Homebrew/apt `libx264` is unsafe for this project: if a
shared `libx264.so`/dylib appears on the linker's default search path, the linker can silently
prefer it over the explicit static archive — search order, not "static wins". The build then looks
static from cargo's point of view while still depending on a shared x264 library at runtime. The
helper scripts force `-l static=x264` and the Linux script runs `ldd` as a final sanity check.

## Two `ffmpeg-sys-next` MSVC build-script bugs

The published `ffmpeg-sys-next 8.1.0` build script passed GCC-only `-march=native -mtune=native`
flags to FFmpeg's `configure` even under MSVC. `cl.exe` rejects those flags, and FFmpeg reported
the failure as the much less specific "C compiler test failed." The vendored
`vendor/ffmpeg-sys-next/` patch skips those flags for `target_env = "msvc"`.

The same build script also parsed MSVC `-libpath:DIR` flags as `-l` library-name flags, because
both start with `-l`. That produced invalid `cargo:rustc-link-lib=ibpath:...` output and rustc
E0459. The vendored patch detects MSVC libpath flags case-insensitively and routes them into the
link-search-path handling instead.

## The Windows x264 architecture-mismatch saga

Static x264 must match rustc's target architecture, not just whichever Visual Studio shell happens
to be open. Early Windows attempts reused a cached x86 x264 archive for x64 builds, and later built
x264 from an x86 Developer Shell while cargo still targeted `x86_64-pc-windows-msvc`. The current
Windows script derives the required architecture from `rustc -vV`, keys the x264 cache by that
architecture, and fails fast if the active MSVC environment doesn't match.

The generic "Developer PowerShell for VS 2022" shortcut can start a 32-bit PowerShell host and
therefore default to an x86 developer environment. `scripts/setup-msvc-x64.ps1` avoids that by
calling `vcvars64.bat` directly rather than relying on the shortcut.

Running `vcvars64.bat` directly from PowerShell sets environment variables only inside a child
`cmd.exe`, so the caller loses `PATH`, `INCLUDE`, `LIB`, and related variables as soon as the batch
file exits. `setup-msvc-x64.ps1` runs `vcvars64.bat` under `cmd /c "... && set"`, captures the
resulting environment, and applies it to the current PowerShell process.

Windows PowerShell 5.1 reads BOM-less scripts through the system codepage rather than UTF-8. The
PowerShell scripts are saved with a UTF-8 BOM so comments containing non-ASCII punctuation don't
corrupt tokenization under PS 5.1 — this BOM needs to be preserved when editing them.

## Why CI hit bugs the local script never did

`scripts/build-static-windows.ps1` runs almost entirely as one plain PowerShell session — it drops
into MSYS2 only briefly, via a *non-login* `bash -c`, to build x264 from source, converting any
path crossing that boundary by hand (`cygpath -u`/`-w`) exactly where it's used, then returns to
plain PowerShell for the actual `cargo build`. `release.yml` can't do that: `msys2/setup-msys2`'s
`shell: msys2 {0}` wrapper is a fixed *login* shell (`bash -leo pipefail`, no way to drop the
`-l`), and env vars have to hop between step types repeatedly — plain `pwsh` → MSYS2 login shell →
back out to `cargo` (a *native* process) → FFmpeg's own `configure` (an MSYS `sh` child of that
native process). Each hop is a place a path can get lost or silently rewritten, and each one below
was hit for real in CI even though the equivalent local step never needed a workaround:

- **MSYS2's own `link.exe` shadowing MSVC's.** MSYS2 ships a coreutils `link.exe` (the `link(1)`
  hard-link tool, unrelated to linking object files) under `usr/bin`, and inside a `msys2 {0}` step
  it always sits ahead of whatever `msvc-dev-cmd` put on `PATH`, regardless of step order — `cargo
  build`'s link step fails with `link.exe` "extra operand" errors pointing at
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
  a bare `x264.lib` token with no `-l` prefix, so that filter silently drops it — no link directive
  for x264 is ever emitted, and the final link fails despite FFmpeg itself having found and built
  against x264 just fine one step earlier. This *does* affect the local script too, but it never
  surfaces there because `build-static-windows.ps1` already unconditionally sets
  `RUSTFLAGS="-L native=<dir> -l static=x264"` before every build — originally reasoned about
  purely as a defensive "refuse a shared lib" measure, but it turns out to also be the only thing
  that makes x264 link under MSVC at all. `release.yml` needed the same RUSTFLAGS line added
  explicitly (plus copying `libx264.lib` to `x264.lib`, since rustc's `-l static=x264` looks for
  that exact name) for the same reason.

## Vendored FFmpeg/x264 gotcha found via vcpkg

vcpkg's `x264` port has no shared-lib variant to worry about only if the `x64-windows-static`
triplet specifically is used, which is easy to misconfigure. Building `libx264` from source the
same way on every platform (rather than reaching for vcpkg on Windows) keeps the recipe — and the
"never let a shared libx264 exist" invariant — identical everywhere, which is why the Windows
prerequisites in `docs/building.md` build x264 via the MSYS2 shell instead of vcpkg.
