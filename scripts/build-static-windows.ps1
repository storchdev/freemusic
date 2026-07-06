#!/usr/bin/env pwsh
# Builds a standalone (FFmpeg + libx264 statically linked) release binary for Windows, the same
# way .github/workflows/release.yml does for its windows-x86_64 target. Run this ON Windows — it
# does not cross-compile. See docs/building.md for full prerequisites, rationale, and every
# gotcha found getting this working (read that before changing anything below).
#
# Prerequisites:
#   - Rust (MSVC toolchain, the rustup default) with `cargo`/`rustc` on PATH.
#   - MSYS2 (https://www.msys2.org/) with `make`, `nasm`, `diffutils`, `pkgconf` installed via
#     `pacman -S make nasm diffutils pkgconf`. Pass its install dir via -Msys2Dir if not the
#     default C:\msys64.
#   - An MSVC dev environment matching the active Rust toolchain's target arch (x64 by default) —
#     run scripts\setup-msvc-x64.ps1 first if you don't already have one loaded in this session.
#
# Usage:
#   .\scripts\setup-msvc-x64.ps1               # once per PowerShell session
#   .\scripts\build-static-windows.ps1
#   .\scripts\build-static-windows.ps1 -Msys2Dir D:\msys64

param(
    [string]$Msys2Dir = "C:\msys64"
)

$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
    Write-Error "cl.exe not found on PATH. Run scripts\setup-msvc-x64.ps1 first (see docs/building.md)."
    exit 1
}

$msysBash = Join-Path $Msys2Dir "usr\bin\bash.exe"
if (-not (Test-Path $msysBash)) {
    Write-Error "MSYS2 bash.exe not found at $msysBash. Install MSYS2 from https://www.msys2.org/ (with 'pacman -S make nasm diffutils pkgconf') or pass -Msys2Dir."
    exit 1
}

# Append (not prepend) MSYS2's usr\bin so ffmpeg-sys-next's/libx264's build scripts can find
# sh/make/nasm/pkgconf, without MSYS2's own `link.exe` (a coreutils tool, not MSVC's linker)
# shadowing the Developer Shell's — see docs/building.md if `cl.exe is unable to create an
# executable file` shows up. Strip any pre-existing copy first so re-running this in the same
# session doesn't accumulate stale, wrongly-ordered entries.
$msysBinDir = Join-Path $Msys2Dir "usr\bin"
$pathEntries = $env:PATH -split ';' | Where-Object { $_ -and ($_.TrimEnd('\') -ne $msysBinDir.TrimEnd('\')) }
$env:PATH = ($pathEntries + $msysBinDir) -join ';'

# The x264 cache dir is keyed on the arch derived from `rustc -vV`'s host triple, not vcvars'
# VSCMD_ARG_TGT_ARCH — see docs/building.md's "libx264 architecture mismatch" gotcha for why the
# latter isn't authoritative for what cargo/rustc actually link against.
$rustcHost = (rustc -vV | Select-String '^host:\s*(\S+)').Matches[0].Groups[1].Value
$targetArch = switch -Regex ($rustcHost) {
    '^x86_64-'  { 'x64' }
    '^i686-'    { 'x86' }
    '^aarch64-' { 'arm64' }
    default {
        Write-Error "Don't know how to map rustc host triple '$rustcHost' to a VS arch name (x64/x86/arm64). Update this script's mapping."
        exit 1
    }
}
if (-not $env:VSCMD_ARG_TGT_ARCH) {
    Write-Error "VSCMD_ARG_TGT_ARCH is not set. Run scripts\setup-msvc-x64.ps1 (or dot-source vcvarsall.bat) first so the target arch is known."
    exit 1
}
if ($env:VSCMD_ARG_TGT_ARCH -ne $targetArch) {
    Write-Error "This shell's Developer Shell arch (VSCMD_ARG_TGT_ARCH=$($env:VSCMD_ARG_TGT_ARCH)) doesn't match the active Rust toolchain's target arch ($targetArch, from rustc host '$rustcHost'). Open the '$targetArch Native Tools Command Prompt for VS' (or run scripts\setup-msvc-x64.ps1) instead — see docs/building.md."
    exit 1
}
$x264Prefix = Join-Path $env:USERPROFILE "x264-static-$targetArch"
$x264PrefixUnix = & $msysBash -c "cygpath -u '$x264Prefix'"

$pcFile = Join-Path $x264Prefix "lib\pkgconfig\x264.pc"
if (-not (Test-Path $pcFile)) {
    Write-Host "== Building static libx264 from source into $x264Prefix (via MSYS2) =="
    # `-c`, not `-lc` (login shell): a login shell re-sources MSYS2's /etc/profile, which can
    # drop the vcvars environment this script depends on — see docs/building.md.
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

# `-u` (POSIX-style), not `-m`: pkg-config and x264.pc's own `prefix=` line are both POSIX-path,
# since x264's ./configure got the POSIX-converted prefix above — see docs/building.md.
$env:PKG_CONFIG_PATH = & $msysBash -c "cygpath -u '$x264Prefix/lib/pkgconfig'"

# x264 installs its archive as `libx264.lib` even under MSVC, but link.exe/`-l static=x264` below
# expect exactly `x264.lib` (no prefix) — make both names resolve.
$libx264 = Join-Path $x264Prefix "lib\libx264.lib"
$x264Lib = Join-Path $x264Prefix "lib\x264.lib"
if ((Test-Path $libx264) -and -not (Test-Path $x264Lib)) {
    Copy-Item $libx264 $x264Lib
}

# `-l static=x264` forces rustc to require a static archive and refuse a shared/import lib, so a
# system-installed libx264 can't silently win the final link regardless of search order — see
# docs/building.md's "never let a shared libx264 exist" gotcha. Rebuilt fresh each run so
# re-running this script doesn't compound duplicate flags onto $env:RUSTFLAGS.
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
