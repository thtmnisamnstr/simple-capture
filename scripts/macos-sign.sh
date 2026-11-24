#!/usr/bin/env bash
# Signs a macOS app bundle or DMG with a Developer ID (helpers first, then the app with its
# entitlements, hardened runtime and secure timestamps), then notarizes and staples it.
#   scripts/macos-sign.sh PATH.app|PATH.dmg
# Env:
#   SC_SIGN_IDENTITY   the identity (default: the first "Developer ID Application" in the keychains)
#   SC_NOTARY_PROFILE  a `notarytool store-credentials` profile (default: simple-capture), or
#   APPLE_API_KEY_PATH, APPLE_API_KEY_ID, APPLE_API_ISSUER   an App Store Connect API key (CI)
#   SC_NOTARIZE=0      sign only
set -euo pipefail
cd "$(dirname "$0")/.."
TARGET="$1"
ENTITLEMENTS=app/macos/entitlements.plist
IDENTITY="${SC_SIGN_IDENTITY:-$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -1)}"
[ -n "$IDENTITY" ] || { echo "macos-sign: no Developer ID Application identity" >&2; exit 1; }

sign() { codesign --force --options runtime --timestamp -s "$IDENTITY" "$@"; }

case "$TARGET" in
  *.app)
    # No --deep: it is deprecated for signing and would give the helpers the app's entitlements.
    for exe in "$TARGET"/Contents/MacOS/*; do
      [ "$exe" -ef "$TARGET/Contents/MacOS/simple-capture-app" ] || sign "$exe"
    done
    sign --entitlements "$ENTITLEMENTS" "$TARGET"
    codesign --verify --strict --deep "$TARGET"
    ;;
  *.dmg) sign "$TARGET" ;;
  *) echo "macos-sign: expected a .app or .dmg: $TARGET" >&2; exit 1 ;;
esac

[ "${SC_NOTARIZE:-1}" = 1 ] || exit 0
PROFILE="${SC_NOTARY_PROFILE:-simple-capture}"
if [ -n "${APPLE_API_KEY_PATH:-}" ]; then
  AUTH=(--key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER")
elif xcrun notarytool history --keychain-profile "$PROFILE" >/dev/null 2>&1; then
  AUTH=(--keychain-profile "$PROFILE")
else
  echo "macos-sign: signed, not notarized (no \"$PROFILE\" notarytool profile or API key)" >&2
  exit 0
fi
SUBMIT="$TARGET"
if [ "${TARGET##*.}" = app ]; then
  SUBMIT="$(mktemp -d)/$(basename "$TARGET" .app).zip"
  ditto -c -k --keepParent "$TARGET" "$SUBMIT"
fi
xcrun notarytool submit "$SUBMIT" "${AUTH[@]}" --wait --timeout 30m
xcrun stapler staple "$TARGET"
[ "$SUBMIT" = "$TARGET" ] || rm -f "$SUBMIT"
if [ "${TARGET##*.}" = app ]; then
  spctl --assess --type execute -vv "$TARGET"
else
  spctl --assess --type open --context context:primary-signature -vv "$TARGET"
fi
