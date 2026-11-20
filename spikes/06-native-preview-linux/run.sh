#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
IMG=sc-spike06:latest
mkdir -p "$HERE/results"

dock() {
  docker run --rm -i \
    -v "$HERE":/work -v sc06-cargo-registry:/opt/cargo/registry -v sc06-target:/target \
    -e CARGO_TARGET_DIR=/target \
    -e SC_SECONDS -e SC_HIDE_WEB -e SC_MARGIN -e SC_SHELL -e SC_URL -e SC_W -e SC_H -e SC_WGPU_BACKEND \
    -e WEBKIT_DISABLE_DMABUF_RENDERER -e WEBKIT_DISABLE_COMPOSITING_MODE \
    -w /work "$IMG" bash -s -- "$@"
}

case "${1:-}" in
  image) docker build -t "$IMG" "$HERE" ;;
  build) echo 'cargo build --release 2>&1 | tail -n 40' | dock ;;
  x11|wayland) dock "$1" "$2" "${3:-$1-$2}" < "$HERE/inner.sh" ;;
  *)
    echo "usage: $0 image | build | x11 <mode> [tag] | wayland <mode> [tag]"
    echo "mode: glarea | subsurface | x11child | pull"
    exit 1
    ;;
esac
