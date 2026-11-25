#!/usr/bin/env bash
# Signs a macOS app bundle or DMG: the helpers in Contents/MacOS first (identifier
# com.simplecapture.desktop.<name>, app/macos/helper.entitlements.plist: App Sandbox, inherited
# from the app), then the app with its sandbox entitlements, all with the hardened runtime. A
# Developer ID signature gets secure timestamps and is then notarized and stapled.
#   scripts/macos-sign.sh PATH.app|PATH.dmg
# Env:
#   SC_SIGN_IDENTITY   name or SHA-1 (default: the first "Developer ID Application" in the
#                      keychains); "-" signs ad hoc. Only a Developer ID is timestamped and notarized.
#   SC_ENTITLEMENTS    the app's entitlements (default app/macos/entitlements.plist;
#                      scripts/mas-build.sh passes app/macos/entitlements.appstore.plist)
#   SC_NOTARY_PROFILE  a `notarytool store-credentials` profile (default: simple-capture), or
#   APPLE_API_KEY_PATH, APPLE_API_KEY_ID, APPLE_API_ISSUER   an App Store Connect API key (CI)
#   SC_NOTARIZE=0      sign only
set -euo pipefail
TARGET="$1"
case "$TARGET" in /*) ;; *) TARGET="$PWD/$TARGET" ;; esac
cd "$(dirname "$0")/.."
ENTITLEMENTS=${SC_ENTITLEMENTS:-app/macos/entitlements.plist}
HELPER_ENTITLEMENTS=app/macos/helper.entitlements.plist
BUNDLE_ID=com.simplecapture.desktop
IDENTITY="${SC_SIGN_IDENTITY:-$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -1)}"
[ -n "$IDENTITY" ] || { echo "macos-sign: no Developer ID Application identity (SC_SIGN_IDENTITY=- signs ad hoc)" >&2; exit 1; }
[ -f "$ENTITLEMENTS" ] || { echo "macos-sign: no entitlements at $ENTITLEMENTS" >&2; exit 1; }

devid=0
if [ "$IDENTITY" != - ] &&
  security find-identity -v -p codesigning | grep -F -- "$IDENTITY" | grep -q '"Developer ID Application: '; then
  devid=1
fi
stamp=--timestamp=none
[ $devid = 0 ] || stamp=--timestamp
sign() { codesign --force --options runtime "$stamp" -s "$IDENTITY" "$@"; }

case "$TARGET" in
  *.app)
    # No --deep: it is deprecated for signing and would give the helpers the app's entitlements.
    main=$(plutil -extract CFBundleExecutable raw "$TARGET/Contents/Info.plist")
    for exe in "$TARGET"/Contents/MacOS/*; do
      name=$(basename "$exe")
      [ "$name" = "$main" ] || sign -i "$BUNDLE_ID.$name" --entitlements "$HELPER_ENTITLEMENTS" "$exe"
    done
    sign --entitlements "$ENTITLEMENTS" "$TARGET"
    codesign --verify --strict --deep "$TARGET"
    ;;
  *.dmg) sign "$TARGET" ;;
  *) echo "macos-sign: expected a .app or .dmg: $TARGET" >&2; exit 1 ;;
esac

[ "${SC_NOTARIZE:-1}" = 1 ] && [ $devid = 1 ] || exit 0
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
