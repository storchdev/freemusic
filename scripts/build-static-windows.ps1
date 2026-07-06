#!/usr/bin/env pwsh
# Builds a standalone (FFmpeg + libx264 statically linked) release binary for Windows, the same
# way .github/workflows/release.yml does for its windows-x86_64 target. Run this ON Windows — it
# does not cross-compile. See the README's "Static/cross-platform builds" section for the full
# rationale and per-tool prerequisites.
#
# Prerequisites (see README):
#   - Rust (MSVC toolchain, the rustup default) with `cargo` on PATH.
#   - MSYS2 (https://www.msys2.org/) with `make`, `nasm`, `diffutils`, `pkgconf` installed via
#     `pacman -S make nasm diffutils pkgconf`. Pass its install dir via -Msys2Dir if not the
#     default C:\msys64.
#   - Visual Studio Build Tools (C++ workload) — this script must be run from a "Developer
#     PowerShell for VS" (or after dot-sourcing vcvarsall.bat) so cl.exe/lib.exe/link.exe are on
#     PATH. It will refuse to continue otherwise.
#
# Usage:
#   .\scripts\build-static-windows.ps1
#   .\scripts\build-static-windows.ps1 -Msys2Dir D:\msys64

param(
    [string]$Msys2Dir = "C:\msys64"
)

$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
    Write-Error "cl.exe not found on PATH. Run this from a 'Developer PowerShell for VS' (or 'x64 Native Tools Command Prompt') so the MSVC toolchain (cl.exe/lib.exe/link.exe) is available."
    exit 1
}

$msysBash = Join-Path $Msys2Dir "usr\bin\bash.exe"
if (-not (Test-Path $msysBash)) {
    Write-Error "MSYS2 bash.exe not found at $msysBash. Install MSYS2 from https://www.msys2.org/ (with 'pacman -S make nasm diffutils pkgconf') or pass -Msys2Dir."
    exit 1
}

# ffmpeg-sys-next's build script (and the libx264 build below) needs sh.exe/make/nasm/pkgconf
# from MSYS2 on PATH, alongside the MSVC toolchain (cl.exe etc.) that must already be on PATH
# from the Developer shell. Do this before the libx264 build, not just before `cargo build`, so
# `make`/`nasm` resolve during that build too.
#
# Append, don't prepend: MSYS2's usr\bin ships its own `link.exe` (a coreutils hardlink utility,
# unrelated to MSVC's linker). If it comes before the Developer shell's PATH entries, cl.exe
# silently invokes MSYS2's link.exe instead of MSVC's when finishing a compile, and configure's
# test-compile fails with "cl.exe is unable to create an executable file" — a link failure
# disguised as a compiler failure. Putting MSYS2 last means cl.exe/link.exe from the Developer
# shell are always found first, and MSYS2 is only consulted for tools (sh, make, nasm, pkgconf)
# that don't exist in the Developer shell's own PATH at all.
#
# Strip out any pre-existing copy of this MSYS2 bin dir first: PowerShell's $env:PATH changes
# persist for the life of the shell process, so re-running this script in a session where an
# older/differently-ordered copy already got prepended (e.g. from a previous version of this
# script) would otherwise leave that stale, wrongly-ordered entry in place ahead of anything we
# add here, silently reintroducing the exact link.exe shadowing this is meant to prevent.
$msysBinDir = Join-Path $Msys2Dir "usr\bin"
$pathEntries = $env:PATH -split ';' | Where-Object { $_ -and ($_.TrimEnd('\') -ne $msysBinDir.TrimEnd('\')) }
$env:PATH = ($pathEntries + $msysBinDir) -join ';'

# Deliberately `-c`, not `-lc` (login shell): a login shell re-sources MSYS2's own /etc/profile,
# which resets PATH/INCLUDE/LIB/LIBPATH and can silently drop the vcvars environment this script's
# Developer-shell prerequisite set up — making cl.exe unreachable (or unusable) inside bash even
# though `Get-Command cl.exe` above found it fine in PowerShell. That's what produces x264's
# misleading "Microsoft Visual Studio support requires Visual Studio 2013 Update 2 or newer"
# error: it's cl.exe detection failing, not an actual VS-version problem. `-c` inherits this
# process's environment as-is instead.
$x264Prefix = Join-Path $env:USERPROFILE "x264-static"
$x264PrefixUnix = & $msysBash -c "cygpath -u '$x264Prefix'"

$pcFile = Join-Path $x264Prefix "lib\pkgconfig\x264.pc"
if (-not (Test-Path $pcFile)) {
    Write-Host "== Building static libx264 from source into $x264Prefix (via MSYS2) =="
    $buildScript = @"
set -e
rm -rf /tmp/freemusic-x264-build
git clone --depth 1 https://code.videolan.org/videolan/x264.git /tmp/freemusic-x264-build
cd /tmp/freemusic-x264-build
CC=cl ./configure --enable-static --disable-cli --disable-asm --prefix='$x264PrefixUnix'
make -j"`$(nproc)"
make install
"@
    & $msysBash -c $buildScript
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Building static libx264 failed."
        exit 1
    }
} else {
    Write-Host "== Static libx264 already built at $x264Prefix, skipping =="
}

# Deliberately `-u` (POSIX-style, e.g. /c/Users/.../pkgconfig), not `-m` (Windows-style
# C:/Users/...): the pkg-config binary that actually runs here is the one from this same MSYS2
# install, in the same POSIX-path world as the x264.pc file it needs to find (that file's own
# `prefix=` line is POSIX-style too, since x264's ./configure was given the POSIX-converted
# prefix above) — a Windows-style PKG_CONFIG_PATH left it unable to locate x264.pc at all
# ("Package x264 was not found in the pkg-config search path"), even though the directory and
# file both genuinely existed.
$env:PKG_CONFIG_PATH = & $msysBash -c "cygpath -u '$x264Prefix/lib/pkgconfig'"

# x264's own build/install step names the archive `libx264.lib` even under MSVC (it keeps the
# Unix `lib` prefix regardless of toolchain) — but MSVC's link.exe and rustc's `-l static=x264`
# below both look for a file named exactly `x264.lib` (no prefix, the MSVC convention), and error
# with "cannot open input file 'x264.lib'" (LNK1181) even though /LIBPATH correctly points at this
# directory, because that literal filename isn't in it. Make both names resolve.
$libx264 = Join-Path $x264Prefix "lib\libx264.lib"
$x264Lib = Join-Path $x264Prefix "lib\x264.lib"
if ((Test-Path $libx264) -and -not (Test-Path $x264Lib)) {
    Copy-Item $libx264 $x264Lib
}

# PKG_CONFIG_PATH alone only controls FFmpeg's own build of libavcodec/etc; it doesn't stop the
# final link of the app binary from preferring some other x264 lib (e.g. a vcpkg-installed one)
# earlier on the linker's search path over our static one (see README's static-build gotcha).
# `-l static=x264` tells rustc it must find a static .lib for x264 and refuse a shared/import lib
# outright, so that can't happen regardless of search order.
#
# Built fresh each run (not appended to whatever $env:RUSTFLAGS already held) so re-running this
# script repeatedly in the same PowerShell session doesn't keep compounding duplicate copies of
# this flag onto every rustc invocation.
$x264LibDir = Join-Path $x264Prefix "lib"
$env:RUSTFLAGS = "-L native=$x264LibDir -l static=x264"

Write-Host "== Building app (release, static-ffmpeg feature) =="
cargo build --release -p app --features static-ffmpeg
if ($LASTEXITCODE -ne 0) {
    Write-Error "cargo build failed."
    exit 1
}

New-Item -ItemType Directory -Force -Path dist | Out-Null
Copy-Item "target\release\app.exe" "dist\freemusic-windows-x86_64.exe" -Force

Write-Host "Done: dist\freemusic-windows-x86_64.exe"
