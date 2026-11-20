#!/usr/bin/env bash
# Usage: spikes/01-helpers/bench.sh [extra spike-helpers bench args, e.g. --seconds 2 --only gif]
# Builds any missing helper, then runs every encode, validation, cancel and robustness test and
# writes results/run-<UTC>/results.{json,md} plus results/latest.{json,md}.
# Env: ARCH (arm64), FFPROBE_REF / FFMPEG_REF (independent validator; default: the ones on PATH).
set -euo pipefail
HERE=$(cd -- "$(dirname -- "$0")" && pwd)
ROOT=$(cd -- "$HERE/../.." && pwd)
ARCH=${ARCH:-arm64}
BIN="$ROOT/third_party/out/macos-$ARCH"

[ -x "$BIN/ffmpeg" ] || "$ROOT/third_party/ffmpeg/build-macos.sh" "$ARCH"
[ -x "$BIN/gifski" ] || "$ROOT/third_party/gifski/build.sh" "$ARCH"
FFPROBE_REF=${FFPROBE_REF:-$(command -v ffprobe || echo "$BIN/ffprobe")}
FFMPEG_REF=${FFMPEG_REF:-$(command -v ffmpeg || echo "$BIN/ffmpeg")}

cargo build --release --manifest-path "$HERE/Cargo.toml"
RUN="$HERE/results/run-$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "$RUN"
{
    echo "git: $(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo none)"
    shasum -a 256 "$BIN/ffmpeg" "$BIN/ffprobe" "$BIN/gifski" | sed "s|$ROOT/||"
    echo "validator: $FFPROBE_REF ($("$FFPROBE_REF" -hide_banner -version | head -1))"
} > "$RUN/binaries.txt"

"$HERE/target/release/spike-helpers" bench \
    --ffmpeg "$BIN/ffmpeg" --gifski "$BIN/gifski" \
    --ffprobe "$FFPROBE_REF" --ffmpeg-ref "$FFMPEG_REF" \
    --out "$RUN" "$@"

cp "$RUN/results.json" "$HERE/results/latest.json"
cp "$RUN/results.md" "$HERE/results/latest.md"
echo "results: $RUN/results.md"
