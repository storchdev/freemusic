#!/usr/bin/env bash
# Launches the app under a forced X11 backend, required for xdotool/import to see the window
# at all under WSLg's Wayland compositor -- see CLAUDE.md's "Screenshotting the app under
# WSL2" section.
#
# Runs in the FOREGROUND of this script on purpose. To background it, invoke this script
# itself via the Bash tool's run_in_background:true rather than adding `&`/`disown` here --
# that combination has silently failed before (see CLAUDE.md's "Errors and fixes" history).
#
# Usage: run-app.sh [video-file] [midi-file]
set -euo pipefail
cd "$(dirname "$0")/.."

# Not necessarily on PATH in a fresh/non-interactive shell (see CLAUDE.md).
source "$HOME/.cargo/env" 2>/dev/null || true

exec env -u WAYLAND_DISPLAY DISPLAY=:0 cargo run --bin app -- "$@"
