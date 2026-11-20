#!/bin/bash
# Type-check and lint the Windows code paths from macOS (M1 plan §6.4): clippy for
# x86_64-pc-windows-msvc on every crate that does not link Skia or Tauri, plus
# `cli --no-default-features`. Nothing is linked, so no MSVC toolchain is needed. Crates that
# link Skia/Tauri (text, export, app, cli default) are only proven on GitHub's Windows runner.
set -euo pipefail
cd "$(dirname "$0")/../.."
T=x86_64-pc-windows-msvc
rustup target list --installed | grep -qx "$T" || { echo "windows-check: rustup target add $T" >&2; exit 1; }
cargo clippy --locked --target "$T" --all-targets \
    -p simple-capture-core -p simple-capture-geometry -p simple-capture-platform \
    -p simple-capture-capture-windows -p simple-capture-record -p simple-capture-decode \
    -p simple-capture-helpers -- -D warnings
cargo clippy --locked --target "$T" --all-targets -p simple-capture-cli --no-default-features -- -D warnings
echo "windows-check: ok"
