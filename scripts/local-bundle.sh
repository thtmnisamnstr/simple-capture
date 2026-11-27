#!/usr/bin/env bash
# Builds the macOS app bundle (the sandboxed app with the LGPL helpers, as the DMG ships) and
# signs it through scripts/macos-sign.sh with the best identity in the login keychain:
#   1. "Developer ID Application: …" (notarized and stapled once
#      `xcrun notarytool store-credentials simple-capture` has been run).
#   2. "Simple Capture Local Signing", the self-signed certificate: grants survive rebuilds on
#      this Mac only.
#   3. Otherwise ad hoc, which macOS treats as a new app on every build.
#   scripts/local-bundle.sh [--dmg]   (--dmg also packs, signs and notarizes the installer DMG)
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target/skia}"
if command -v cargo-about >/dev/null; then
  scripts/ci/notices.sh --macos >/dev/null
fi
scripts/stage-helpers.sh >/dev/null
pnpm --dir ui build >/dev/null
(cd app && nice -n 19 ../ui/node_modules/.bin/tauri build --config tauri.bundle.conf.json \
  --config tauri.bundle.macos.conf.json --bundles app -- --locked -j 4)
APP="$CARGO_TARGET_DIR/release/bundle/macos/simple-capture.app"

devid=0
if security find-identity -v -p codesigning | grep -q '"Developer ID Application: '; then
  devid=1
  scripts/macos-sign.sh "$APP"
elif security find-certificate -c "Simple Capture Local Signing" >/dev/null 2>&1; then
  SC_SIGN_IDENTITY="Simple Capture Local Signing" scripts/macos-sign.sh "$APP"
else
  echo "local-bundle: no signing certificate; the bundle is signed ad hoc" >&2
  SC_SIGN_IDENTITY=- scripts/macos-sign.sh "$APP"
fi
codesign -d -r- "$APP" 2>&1 | grep designated
echo "$APP"

if [ "${1:-}" = --dmg ]; then
  VERSION=$(sed -n 's/^  "version": "\(.*\)",$/\1/p' app/tauri.conf.json)
  DMG="$CARGO_TARGET_DIR/release/bundle/dmg/simple-capture_${VERSION}_aarch64.dmg"
  mkdir -p "$(dirname "$DMG")"
  scripts/make-dmg.sh "$APP" "$DMG"
  [ $devid = 0 ] || scripts/macos-sign.sh "$DMG"
  echo "$DMG"
fi
