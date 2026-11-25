#!/usr/bin/env bash
# Usage: third_party/gifski/build.sh [arm64|x86_64|linux|windows]
#   arm64, x86_64  macOS; linux  this Linux machine's arch; windows  x86_64, cross-built with
#   mingw-w64 from Linux
# Builds the gifski CLI from the pinned crates.io source with its default features
# (binary + gifsicle). The "video" feature would link FFmpeg into gifski; it stays off.
set -euo pipefail
. "$(dirname -- "$0")/../lib.sh"

ARCH=${1:-arm64}
EXE=
case "$ARCH-$(uname -m)" in
    arm64-*) TRIPLE=aarch64-apple-darwin; PLAT=macos-arm64 ;;
    x86_64-*) TRIPLE=x86_64-apple-darwin; PLAT=macos-x86_64 ;;
    linux-x86_64) TRIPLE=x86_64-unknown-linux-gnu; PLAT=linux-x86_64 ;;
    linux-aarch64) TRIPLE=aarch64-unknown-linux-gnu; PLAT=linux-arm64 ;;
    windows-*)
        TRIPLE=x86_64-pc-windows-gnu; PLAT=windows-x86_64; EXE=.exe
        export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
        export CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc AR_x86_64_pc_windows_gnu=x86_64-w64-mingw32-ar
        rustup target add "$TRIPLE" >/dev/null 2>&1 || true
        ;;
    *) die "unknown target $ARCH" ;;
esac
MAC=false
[[ $TRIPLE == *apple-darwin ]] && MAC=true

VERSION=$(pin gifski version)
CRATE=$(fetch gifski)
LICENSE=$(fetch gifski license_url license_file license_sha256)
LOCK="$TP_ROOT/gifski/$(pin gifski lock_file)"
want=$(pin gifski lock_sha256)
got=$(sha256_of "$LOCK")
[ "$got" = "$want" ] || die "sha256 mismatch for $LOCK: expected $want, got $got"
SRC="$TP_BUILD/gifski-$VERSION"
OUT="$TP_OUT/$PLAT"
mkdir -p "$OUT"
unpack "$CRATE" "$SRC"
cp "$LOCK" "$SRC/Cargo.lock"

$MAC && export MACOSX_DEPLOYMENT_TARGET=14.0
export CARGO_TARGET_DIR="$TP_BUILD/gifski-target"
export CARGO_PROFILE_RELEASE_LTO=true
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1
export CARGO_PROFILE_RELEASE_STRIP=symbols
CARGO_HOME_DIR=${CARGO_HOME:-$HOME/.cargo}
# Panic-location strings would otherwise embed the build machine's home directory.
export RUSTFLAGS="--remap-path-prefix=$CARGO_HOME_DIR=/cargo --remap-path-prefix=$TP_ROOT=third_party ${RUSTFLAGS:-}"

log "cargo build gifski $VERSION for $TRIPLE"
cargo build --manifest-path "$SRC/Cargo.toml" --release --locked --target "$TRIPLE" --bin gifski

cp "$CARGO_TARGET_DIR/$TRIPLE/release/gifski$EXE" "$OUT/gifski$EXE"
cp "$LICENSE" "$OUT/gifski.LICENSE"
cargo tree --manifest-path "$SRC/Cargo.toml" --locked --target "$TRIPLE" -e normal --prefix none \
    --format '{p} {l}' | sort -u > "$OUT/gifski.deps.txt"
{
    echo "gifski $VERSION ($(pin gifski git_commit)), crate sha256 $(pin gifski sha256)"
    echo "Cargo.lock: third_party/gifski/$(pin gifski lock_file), sha256 $(pin gifski lock_sha256)"
    echo "cargo build --release --locked --target $TRIPLE --bin gifski (default features: binary, gifsicle; no video)"
    echo "MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-} lto=true codegen-units=1 strip=symbols"
    rustc -vV
    if $MAC; then otool -L "$OUT/gifski"; vtool -show-build "$OUT/gifski"
    elif [ -n "$EXE" ]; then x86_64-w64-mingw32-objdump -p "$OUT/gifski$EXE" | grep 'DLL Name'
    else ldd "$OUT/gifski"; fi
} > "$OUT/gifski.buildinfo.txt"
[ -z "$EXE" ] && "$OUT/gifski" --version
log "done: $OUT/gifski$EXE"
