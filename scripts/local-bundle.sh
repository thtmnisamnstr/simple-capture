#!/usr/bin/env bash
# Builds the macOS app bundle and signs it with the best identity in the login keychain:
#   1. "Developer ID Application: …", through scripts/macos-sign.sh (notarized and stapled once
#      `xcrun notarytool store-credentials simple-capture` has been run).
#   2. "Simple Capture Local Signing", the self-signed certificate: grants survive rebuilds on
#      this Mac only.
#   3. Otherwise ad hoc, which macOS treats as a new app on every build.
#   scripts/local-bundle.sh
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target/skia}"
scripts/stage-helpers.sh >/dev/null
pnpm --dir ui build >/dev/null
(cd app && nice -n 19 ../ui/node_modules/.bin/tauri build --config tauri.bundle.conf.json --bundles app -- --locked -j 4)
APP="$CARGO_TARGET_DIR/release/bundle/macos/Simple Capture.app"

if security find-identity -v -p codesigning | grep -q '"Developer ID Application: '; then
  scripts/macos-sign.sh "$APP"
elif security find-certificate -c "Simple Capture Local Signing" >/dev/null 2>&1; then
  codesign --force --deep --options runtime --entitlements app/macos/entitlements.plist \
    -s "Simple Capture Local Signing" "$APP"
else
  echo "local-bundle: no signing certificate; the bundle stays ad hoc signed" >&2
fi
codesign -d -r- "$APP" 2>&1 | grep designated
echo "$APP"
