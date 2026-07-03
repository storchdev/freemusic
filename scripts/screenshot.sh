#!/usr/bin/env bash
# Screenshots the running app window. Requires the app to have been launched via run-app.sh
# (forced X11 backend) -- see CLAUDE.md's "Screenshotting the app under WSL2" section for why
# the default Wayland-backend window isn't visible to X11 tools like `import` at all.
#
# Usage: screenshot.sh <output.png> [crop WxH+X+Y]
set -euo pipefail

out="${1:?usage: screenshot.sh <output.png> [crop WxH+X+Y]}"
crop="${2:-}"

win="$("$(dirname "$0")/find-window.sh")"
if [ -z "$win" ]; then
    echo "freemusic window not found (is it running? see run-app.sh)" >&2
    exit 1
fi

if [ -n "$crop" ]; then
    DISPLAY=:0 import -window "$win" -crop "$crop" "$out"
else
    DISPLAY=:0 import -window "$win" "$out"
fi
echo "$out"
