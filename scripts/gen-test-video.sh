#!/usr/bin/env bash
# Generates a synthetic test clip with a per-frame counter overlay, realistic keyframe interval,
# and long enough default duration for real MIDI overlay checks. See
# `docs/implementation-notes.md` and `docs/verification.md`.
#
# Usage: gen-test-video.sh <output.mp4> [duration-seconds]
set -euo pipefail

out="${1:?usage: gen-test-video.sh <output.mp4> [duration-seconds]}"
duration="${2:-30}"
font="/usr/share/fonts/TTF/DejaVuSans-Bold.ttf"

ffmpeg -y -f lavfi -i "testsrc=size=640x360:rate=30:duration=${duration}" \
    -vf "drawtext=fontfile=${font}:text=frame\ %{n}:fontcolor=white:fontsize=64:x=20:y=20:box=1:boxcolor=black@0.6" \
    -c:v libx264 -g 60 -pix_fmt yuv420p "$out"
