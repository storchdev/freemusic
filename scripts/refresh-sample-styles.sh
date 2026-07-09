#!/usr/bin/env bash
# Regenerates examples/styles/*.fmstyle.ron from crates/project/examples/dump_sample_styles.rs's
# stdout output — one Style literal per shipped sample, guaranteed to match whatever RON syntax
# this version of the `ron` crate actually produces (see that file's own header comment; this
# script automates the "copy stdout sections into the corresponding files" step it describes).
#
# Writes each `=== name ===` block's RON verbatim, with NO reformatting — a freshly-refreshed file
# may cosmetically differ from what's checked in today (the pretty-printer's expanded
# `layers: ((\n amplitude: ...,\n sigma_px: ...,\n), ...)` instead of the compact one-line-per-entry
# form some files were hand-tidied into, and explicit default-valued fields like
# `edge_blend_px: 0.0` that are conventionally stripped by hand) even when the underlying `Style`
# value is unchanged. Diff before committing.
set -euo pipefail
cd "$(dirname "$0")/.."

# Not necessarily on PATH in a fresh/non-interactive shell (see CLAUDE.md).
source "$HOME/.cargo/env" 2>/dev/null || true

cargo run -p project --example dump_sample_styles 2>/dev/null | awk '
/^=== .* ===$/ {
    name = $0
    sub(/^=== /, "", name)
    sub(/ ===$/, "", name)
    outfile = "examples/styles/" name ".fmstyle.ron"
    next
}
outfile != "" { print > outfile }
'

echo "Refreshed examples/styles/*.fmstyle.ron from dump_sample_styles."
