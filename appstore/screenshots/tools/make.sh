#!/usr/bin/env bash
# Renders the App Store screenshots (2880x1800) from the real UI running on its mock backend:
# shot.html frames the windows and fills the video areas with demo-frame.html.
# Needs Google Chrome and the ui/ dependencies (pnpm --dir ui install).
#   appstore/screenshots/tools/make.sh
set -euo pipefail
cd "$(dirname "$0")/../../.."
CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
TOOLS=appstore/screenshots/tools
OUT=appstore/screenshots/en-US
PORT="${SC_SHOTS_PORT:-5199}"
STAGE=ui/public/__shots
WORK="$(mktemp -d)"
mkdir -p "$STAGE"
cleanup() {
  [ -z "${VITE:-}" ] || kill "$VITE" 2>/dev/null || true
  rm -rf "$STAGE" "$WORK"
  rmdir ui/public 2>/dev/null || true
}
trap cleanup EXIT
"$CHROME" --headless=new --disable-gpu --hide-scrollbars --window-size=1920,1080 \
  --screenshot="$STAGE/demo-frame.png" "file://$PWD/$TOOLS/demo-frame.html" >/dev/null 2>&1
cp "$TOOLS/shot.html" "$STAGE/shot.html"
pnpm --dir ui exec vite --port "$PORT" --strictPort >"$WORK/vite.log" 2>&1 &
VITE=$!
for _ in $(seq 1 60); do curl -fs "http://localhost:$PORT/" >/dev/null && break; sleep 0.5; done
names=(01-record 02-edit 03-export 04-library)
for n in 1 2 3 4; do
  node "$TOOLS/cdp-shot.mjs" "http://localhost:$PORT/__shots/shot.html?n=$n" "$OUT/${names[$((n - 1))]}.png" 9000
done
sips -g pixelWidth -g pixelHeight "$OUT"/*.png | grep -E 'pixel' | sort | uniq -c
