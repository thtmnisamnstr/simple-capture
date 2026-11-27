#!/usr/bin/env bash
# Renders the App Store app previews (1920x1080, 30 fps, H.264 with a silent AAC track, under
# 30 s) from the real UI on its mock backend: scene.html lays out the windows and drives them
# with a pointer, demo-live.html is the recorded content, record.mjs captures and encodes.
# Needs Google Chrome, ffmpeg with libx264, and the ui/ dependencies (pnpm --dir ui install).
#   appstore/previews/tools/make.sh [scene...]
set -euo pipefail
cd "$(dirname "$0")/../../.."
TOOLS=appstore/previews/tools
OUT=appstore/previews/en-US
PORT="${SC_PREVIEWS_PORT:-5198}"
FFMPEG="${FFMPEG:-ffmpeg}"
STAGE=ui/public/__previews
WORK="$(mktemp -d)"
mkdir -p "$STAGE" "$OUT"
cleanup() {
  [ -z "${VITE:-}" ] || kill "$VITE" 2>/dev/null || true
  rm -rf "$STAGE" "$WORK"
  rmdir ui/public 2>/dev/null || true
}
trap cleanup EXIT
cp "$TOOLS/scene.html" "$TOOLS/demo-live.html" "$STAGE/"
pnpm --dir ui exec vite --port "$PORT" --strictPort >"$WORK/vite.log" 2>&1 &
VITE=$!
for _ in $(seq 1 60); do curl -fs "http://localhost:$PORT/" >/dev/null && break; sleep 0.5; done
names=(01-record 02-edit 03-export)
for n in "${@:-1 2 3}"; do
  for v in $n; do
    node "$TOOLS/record.mjs" "http://localhost:$PORT/__previews/scene.html?v=$v" "$OUT/${names[$((v - 1))]}.mp4" "$FFMPEG"
  done
done
for f in "$OUT"/*.mp4; do
  printf '%s ' "$(basename "$f")"
  ffprobe -v error -show_entries stream=codec_name,width,height,r_frame_rate:format=duration -of csv=p=0 "$f" | tr '\n' ' '
  echo
done
