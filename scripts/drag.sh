#!/usr/bin/env bash
# Drags inside the running app window from one WINDOW-RELATIVE point to another (calibration
# handles, crop handles, sliders, DragValues). See click.sh for why these must be
# window-relative, not absolute screen coordinates.
#
# Usage: drag.sh <x1> <y1> <x2> <y2> [button]
set -euo pipefail

x1="${1:?usage: drag.sh <x1> <y1> <x2> <y2> [button]}"
y1="${2:?usage: drag.sh <x1> <y1> <x2> <y2> [button]}"
x2="${3:?usage: drag.sh <x1> <y1> <x2> <y2> [button]}"
y2="${4:?usage: drag.sh <x1> <y1> <x2> <y2> [button]}"
button="${5:-1}"

win="$("$(dirname "$0")/find-window.sh")"
if [ -z "$win" ]; then
    echo "freemusic window not found (is it running? see run-app.sh)" >&2
    exit 1
fi

# Separate xdotool invocations with real sleeps between them -- a single combined
# mousedown/move/mouseup call doesn't reliably register as a drag (see CLAUDE.md).
DISPLAY=:0 xdotool mousemove --window "$win" "$x1" "$y1"
sleep 0.2
DISPLAY=:0 xdotool mousedown "$button"
sleep 0.2
DISPLAY=:0 xdotool mousemove --window "$win" "$x2" "$y2"
sleep 0.2
DISPLAY=:0 xdotool mouseup "$button"
