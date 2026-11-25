#!/usr/bin/env bash
# Packs a signed app into the installer DMG: background, icon layout and an Applications link,
# written by dmgbuild without scripting Finder.
#   scripts/make-dmg.sh PATH.app OUT.dmg
set -euo pipefail
cd "$(dirname "$0")/.."
APP="$1"
OUT="$2"
VENV="${SC_DMGBUILD_VENV:-target/dmgbuild-venv}"
if [ ! -x "$VENV/bin/dmgbuild" ]; then
  python3 -m venv "$VENV"
  "$VENV/bin/pip" install --quiet dmgbuild==1.6.7
fi
rm -f "$OUT"
"$VENV/bin/dmgbuild" -s app/macos/dmg-settings.py \
  -D app="$APP" -D background="$PWD/assets/brand/dmg-background.tiff" \
  "Simple Capture" "$OUT"
