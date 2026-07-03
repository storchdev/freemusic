#!/usr/bin/env bash
# Prints the X11 window id of the running app (forced onto the X11 backend by run-app.sh),
# or exits non-zero with nothing on stdout if it isn't running/visible. Used by
# screenshot.sh/click.sh/drag.sh so they don't each re-implement the lookup.
set -euo pipefail

DISPLAY=:0 xdotool search --name '^freemusic' | head -n1
