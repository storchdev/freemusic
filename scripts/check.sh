#!/usr/bin/env bash
# Repo convention (see CLAUDE.md): must stay fmt-clean and clippy-clean. Run before
# considering any change complete.
set -euo pipefail
cd "$(dirname "$0")/.."

# Not necessarily on PATH in a fresh/non-interactive shell (see CLAUDE.md).
source "$HOME/.cargo/env" 2>/dev/null || true

cargo fmt
cargo clippy --all-targets
