#!/usr/bin/env bash
# Clicks inside the running app window at WINDOW-RELATIVE coordinates.
#
# IMPORTANT: coordinates here are relative to the app window (via `xdotool --window`), not
# absolute screen coordinates. Mixing the two silently no-ops -- every click/drag just lands
# wherever the pointer already was, with no error. This is exactly what happened during
# manual milestone 4 testing: several slider drags used bare `xdotool mousemove x y` (absolute)
# against coordinates read off a window-relative screenshot crop, and nothing moved. Always
# go through this script (or drag.sh) rather than calling xdotool directly with guessed
# absolute coordinates.
#
# Usage: click.sh <x> <y> [button]
set -euo pipefail

x="${1:?usage: click.sh <x> <y> [button]}"
y="${2:?usage: click.sh <x> <y> [button]}"
button="${3:-1}"

win="$("$(dirname "$0")/find-window.sh")"
if [ -z "$win" ]; then
    echo "freemusic window not found (is it running? see run-app.sh)" >&2
    exit 1
fi

DISPLAY=:0 xdotool mousemove --window "$win" "$x" "$y"
sleep 0.15
DISPLAY=:0 xdotool click "$button"
