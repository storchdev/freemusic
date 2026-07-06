#!/usr/bin/env pwsh
# Imports the x64 MSVC Developer Shell environment (cl.exe/lib.exe/link.exe on PATH,
# VSCMD_ARG_TGT_ARCH=x64, etc.) into the CURRENT PowerShell session, by running vcvars64.bat
# directly rather than through VS's "Developer PowerShell for VS 2022" shortcut or
# Enter-VsDevShell — see docs/building.md's "Wrong Developer Shell shortcut" and "Batch files
# don't persist environment variables" gotchas for why both of those are unreliable here.
#
# Usage (run in the PowerShell session you intend to build in, then run the build script in the
# SAME window right after — both work fine as plain, non-dot-sourced invocations, since $env:
# changes apply to the whole process regardless of script scope):
#   .\scripts\setup-msvc-x64.ps1
#   .\scripts\build-static-windows.ps1

$candidates = @(
    "C:\Program Files\Microsoft Visual Studio\2022\Enterprise\VC\Auxiliary\Build\vcvars64.bat",
    "C:\Program Files\Microsoft Visual Studio\2022\Professional\VC\Auxiliary\Build\vcvars64.bat",
    "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat",
    "C:\Program Files\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
)
$vcvars = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $vcvars) {
    Write-Error "Could not find vcvars64.bat in any of the usual VS 2022 install locations. Edit this script's `$candidates list with your actual install path."
    exit 1
}

cmd /c "`"$vcvars`" && set" | ForEach-Object {
    if ($_ -match '^([^=]+)=(.*)$') {
        [System.Environment]::SetEnvironmentVariable($matches[1], $matches[2])
    }
}

if ($env:VSCMD_ARG_TGT_ARCH -eq "x64") {
    Write-Host "== x64 Developer Shell environment loaded from $vcvars (VSCMD_ARG_TGT_ARCH=x64) =="
} else {
    Write-Warning "Imported environment but VSCMD_ARG_TGT_ARCH is '$($env:VSCMD_ARG_TGT_ARCH)', not 'x64' -- something is off, check `$vcvars above."
}
