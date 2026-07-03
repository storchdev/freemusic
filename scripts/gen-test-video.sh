#!/usr/bin/env bash
# Generates a synthetic test clip with a per-frame counter overlay and a realistic
# (multi-second) keyframe interval. See CLAUDE.md's "Verifying changes to app or
# video-pipeline" section for why this specific pattern -- not a static test card -- is what
# actually caught the reseek-every-frame and seek-timestamp-units bugs during development.
#
# Default duration is 30s rather than the original 10s milestone-1 example: a real MIDI file
# used for overlay/sync testing may not have its first note until well into the file (this
# bit milestone 4 testing, where test.mid's first note was ~23s in), so a longer default
# clip is less likely to silently show nothing.
#
# Usage: gen-test-video.sh <output.mp4> [duration-seconds]
set -euo pipefail

out="${1:?usage: gen-test-video.sh <output.mp4> [duration-seconds]}"
duration="${2:-30}"
font="/usr/share/fonts/TTF/DejaVuSans-Bold.ttf"

ffmpeg -y -f lavfi -i "testsrc=size=640x360:rate=30:duration=${duration}" \
    -vf "drawtext=fontfile=${font}:text=frame\ %{n}:fontcolor=white:fontsize=64:x=20:y=20:box=1:boxcolor=black@0.6" \
    -c:v libx264 -g 60 -pix_fmt yuv420p "$out"
