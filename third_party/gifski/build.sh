#!/usr/bin/env bash
# Usage: third_party/gifski/build.sh [arm64|x86_64]
# Builds the gifski CLI from the pinned crates.io source with its default features
# (binary + gifsicle). The "video" feature would link FFmpeg into gifski; it stays off.
set -euo pipefail
. "$(dirname -- "$0")/../lib.sh"

ARCH=${1:-arm64}
case "$ARCH" in
    arm64) TRIPLE=aarch64-apple-darwin ;;
    x86_64) TRIPLE=x86_64-apple-darwin ;;
    *) die "unknown arch $ARCH" ;;
esac

VERSION=$(pin gifski version)
CRATE=$(fetch gifski)
LICENSE=$(fetch gifski license_url license_file license_sha256)
SRC="$TP_BUILD/gifski-$VERSION"
OUT="$TP_OUT/macos-$ARCH"
mkdir -p "$OUT"
unpack "$CRATE" "$SRC"

export MACOSX_DEPLOYMENT_TARGET=14.0
export CARGO_TARGET_DIR="$TP_BUILD/gifski-target"
export CARGO_PROFILE_RELEASE_LTO=true
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1
export CARGO_PROFILE_RELEASE_STRIP=symbols
CARGO_HOME_DIR=${CARGO_HOME:-$HOME/.cargo}
# Panic-location strings would otherwise embed the build machine's home directory.
export RUSTFLAGS="--remap-path-prefix=$CARGO_HOME_DIR=/cargo --remap-path-prefix=$TP_ROOT=third_party ${RUSTFLAGS:-}"

log "cargo build gifski $VERSION for $TRIPLE"
cargo build --manifest-path "$SRC/Cargo.toml" --release --locked --target "$TRIPLE" --bin gifski

cp "$CARGO_TARGET_DIR/$TRIPLE/release/gifski" "$OUT/gifski"
cp "$LICENSE" "$OUT/gifski.LICENSE"
cargo tree --manifest-path "$SRC/Cargo.toml" --locked --target "$TRIPLE" -e normal --prefix none \
    --format '{p} {l}' | sort -u > "$OUT/gifski.deps.txt"
{
    echo "gifski $VERSION ($(pin gifski git_commit)), crate sha256 $(pin gifski sha256)"
    echo "cargo build --release --locked --target $TRIPLE --bin gifski (default features: binary, gifsicle; no video)"
    echo "MACOSX_DEPLOYMENT_TARGET=$MACOSX_DEPLOYMENT_TARGET lto=true codegen-units=1 strip=symbols"
    rustc -vV
    otool -L "$OUT/gifski"
    vtool -show-build "$OUT/gifski"
} > "$OUT/gifski.buildinfo.txt"
otool -L "$OUT/gifski"
"$OUT/gifski" --version
log "done: $OUT/gifski"
