#!/usr/bin/env bash
# Builds a standalone (FFmpeg + libx264 statically linked) release binary for Linux, the same
# way .github/workflows/release.yml does for its linux-x86_64 target. Run this ON Linux — it
# does not cross-compile. See the README's "Static/cross-platform builds" section for the full
# rationale, especially the "never let a system libx264 exist" gotcha this script works around
# by building libx264 from source into a private prefix.
set -euo pipefail
cd "$(dirname "$0")/.."

# Not necessarily on PATH in a fresh/non-interactive shell (see CLAUDE.md).
source "$HOME/.cargo/env" 2>/dev/null || true

echo "== Checking build-time tools (nasm, pkg-config, clang, make) =="
missing=()
for tool in nasm pkg-config clang make; do
  command -v "$tool" >/dev/null 2>&1 || missing+=("$tool")
done
if [ "${#missing[@]}" -gt 0 ]; then
  echo "Missing required tools: ${missing[*]}" >&2
  echo "Install them via your distro's package manager" >&2
  exit 1
fi

x264_prefix="$HOME/x264-static"
x264_src="/tmp/freemusic-x264-build"

if [ ! -f "$x264_prefix/lib/pkgconfig/x264.pc" ]; then
  echo "== Building static libx264 from source into $x264_prefix =="
  rm -rf "$x264_src"
  git clone --depth 1 https://code.videolan.org/videolan/x264.git "$x264_src"
  (
    cd "$x264_src"
    ./configure --enable-static --disable-cli --enable-pic --prefix="$x264_prefix"
    make -j"$(nproc)"
    make install
  )
else
  echo "== Static libx264 already built at $x264_prefix, skipping =="
fi

export PKG_CONFIG_PATH="$x264_prefix/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"

# PKG_CONFIG_PATH alone only controls FFmpeg's own build of libavcodec/etc; it doesn't stop the
# final link of the app binary from preferring a system libx264.so over our static libx264.a if
# one exists on the linker's default search path (see README's static-build gotcha). Force it:
# `-l static=x264` tells rustc it must find a static archive for x264 and refuse a shared lib
# outright, so a system-installed libx264.so can no longer win regardless of search order.
export RUSTFLAGS="${RUSTFLAGS:-} -L native=$x264_prefix/lib -l static=x264"

echo "== Building app (release, static-ffmpeg feature) =="
cargo build --release -p app --features static-ffmpeg

mkdir -p dist
cp target/release/app dist/freemusic-linux-x86_64
chmod +x dist/freemusic-linux-x86_64

echo "== Verifying the binary has no dynamic FFmpeg/x264 dependency =="
if ldd dist/freemusic-linux-x86_64 | grep -Ei 'avcodec|avformat|avutil|swscale|swresample|libx264'; then
  echo "WARNING: binary still dynamically links FFmpeg/x264 — see README's static-build gotcha" >&2
else
  echo "OK: no dynamic FFmpeg/x264 dependency found"
fi

echo "Done: dist/freemusic-linux-x86_64"
