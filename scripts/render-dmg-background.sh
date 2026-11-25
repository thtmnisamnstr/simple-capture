#!/usr/bin/env bash
# Renders assets/brand/dmg-background.svg to the 1x+2x TIFF the DMG window shows (needs Chrome).
set -euo pipefail
cd "$(dirname "$0")/.."
CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
OUT=assets/brand/dmg-background.tiff
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
for s in 1 2; do
  "$CHROME" --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=$s \
    --window-size=660,400 --default-background-color=00000000 \
    --screenshot="$TMP/bg@$s.png" "file://$PWD/assets/brand/dmg-background.svg" >/dev/null 2>&1
done
tiffutil -cathidpicheck "$TMP/bg@1.png" "$TMP/bg@2.png" -out "$TMP/bg.tiff" >/dev/null
tiffutil -lzw "$TMP/bg.tiff" -out "$OUT" >/dev/null
sips -g pixelWidth -g pixelHeight "$TMP/bg@2.png" | tail -2
echo "$OUT"
