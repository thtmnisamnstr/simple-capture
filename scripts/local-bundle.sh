#!/usr/bin/env bash
# Builds the macOS app bundle for trying on this Mac and signs it with the local
# "Simple Capture Local Signing" certificate when the login keychain has one, so macOS keeps the
# Screen Recording, Camera and Microphone grants from one build to the next (an ad hoc signature
# changes with every build). Without the certificate the bundle stays ad hoc signed.
#   scripts/local-bundle.sh
set -euo pipefail
cd "$(dirname "$0")/.."
IDENTITY="Simple Capture Local Signing"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target/skia}"
scripts/stage-helpers.sh >/dev/null
pnpm --dir ui build >/dev/null
(cd app && nice -n 19 ../ui/node_modules/.bin/tauri build --config tauri.bundle.conf.json --bundles app -- --locked -j 4)
APP="$CARGO_TARGET_DIR/release/bundle/macos/Simple Capture.app"
if security find-certificate -c "$IDENTITY" >/dev/null 2>&1; then
  codesign --force --deep --options runtime --entitlements app/macos/entitlements.plist -s "$IDENTITY" "$APP"
  codesign -d -r- "$APP" 2>&1 | grep designated
else
  echo "local-bundle: no \"$IDENTITY\" certificate; the bundle is ad hoc signed" >&2
fi
echo "$APP"
