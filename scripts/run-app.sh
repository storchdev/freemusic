#!/usr/bin/env bash
# Launches the app under a forced X11 backend so xdotool/import can see the window under WSLg and
# Hyprland. Runs in this script's foreground on purpose; see `docs/implementation-notes.md`.
#
# Usage: run-app.sh [video-file] [midi-file]
set -euo pipefail
cd "$(dirname "$0")/.."

# Not necessarily on PATH in a fresh/non-interactive shell (see CLAUDE.md).
source "$HOME/.cargo/env" 2>/dev/null || true

exec env -u WAYLAND_DISPLAY DISPLAY=:0 cargo run --bin app -- "$@"
