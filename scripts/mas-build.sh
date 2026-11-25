#!/usr/bin/env bash
# Builds the Mac App Store edition from this checkout and packages it for upload. It is the same
# app as the DMG (macOS notices, LGPL helpers, tauri.bundle.conf.json and
# tauri.bundle.macos.conf.json) plus tauri.appstore.conf.json: the provisioning profile embedded,
# scripts/macos-sign.sh with the Apple Distribution identity and
# app/macos/entitlements.appstore.plist, a productbuild .pkg, then scripts/mas-check.sh. Prints
# the .pkg path last.
# The profile and identities are checked before the build (scripts/mas-check.sh --profile). The
# Developer ID bundle that the shared target dir holds in release/bundle/macos is moved aside
# during the build and put back afterwards, also when it fails.
#   scripts/mas-build.sh [--adhoc] [--app PATH.app]
#     --adhoc         or SC_MAS_ADHOC=1: sign ad hoc with the same entitlements and leave the .pkg
#                     unsigned, to check the structure without certificates
#     --app PATH.app  skip the build: sign, package and check a copy of PATH.app
# Env:
#   SC_MAS_APP_IDENTITY        name or SHA-1, default "Apple Distribution: Gavin Johnson (K9LZ5QLRFQ)";
#                              signs with the SHA-1 of its certificate that the profile lists
#   SC_MAS_INSTALLER_IDENTITY  default "3rd Party Mac Developer Installer: Gavin Johnson (K9LZ5QLRFQ)"
#   SC_MAS_PROFILE             the Mac App Store provisioning profile (required unless ad hoc)
#   SC_MAS_BUILD_NUMBER        CFBundleVersion, above every earlier upload: the plain integer that
#                              scripts/asc/asc.py next-build prints (default: the version)
#   SC_MAS_HELPERS             the LGPL helpers (default third_party/out/macos-arm64)
#   SC_MAS_JOBS                cargo build jobs (default 4)
#   CARGO_TARGET_DIR           default target/skia; output in $CARGO_TARGET_DIR/release/bundle/mas
set -euo pipefail
die() {
  echo "mas-build: $*" >&2
  exit 1
}
abspath() { echo "$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"; }
adhoc=${SC_MAS_ADHOC:-0}
src_app=""
while [ $# -gt 0 ]; do
  case "$1" in
    --adhoc | --unsigned) adhoc=1 ;;
    --app)
      src_app=${2:?--app needs a path}
      shift
      ;;
    *) die "usage: scripts/mas-build.sh [--adhoc] [--app PATH.app]" ;;
  esac
  shift
done
APP_IDENTITY=${SC_MAS_APP_IDENTITY:-Apple Distribution: Gavin Johnson (K9LZ5QLRFQ)}
INSTALLER_IDENTITY=${SC_MAS_INSTALLER_IDENTITY:-3rd Party Mac Developer Installer: Gavin Johnson (K9LZ5QLRFQ)}
PROFILE=${SC_MAS_PROFILE:-}
BUILD_NUMBER=${SC_MAS_BUILD_NUMBER:-}
HELPERS=${SC_MAS_HELPERS:-}
[[ -z $BUILD_NUMBER || $BUILD_NUMBER =~ ^[0-9]+(\.[0-9]+){0,2}$ ]] || die "SC_MAS_BUILD_NUMBER must be 1-3 dot-separated integers"
if [ -n "$PROFILE" ]; then
  [ -f "$PROFILE" ] || die "no provisioning profile at $PROFILE"
  PROFILE=$(abspath "$PROFILE")
fi
if [ -n "$src_app" ]; then
  [ -d "$src_app" ] || die "no app at $src_app"
  src_app=$(abspath "${src_app%/}")
fi
if [ -n "$HELPERS" ]; then
  [ -d "$HELPERS" ] || die "no helpers at $HELPERS"
  HELPERS=$(abspath "${HELPERS%/}")
fi
case "${CARGO_TARGET_DIR:-}" in "" | /*) ;; *) CARGO_TARGET_DIR="$PWD/$CARGO_TARGET_DIR" ;; esac
cd "$(dirname "$0")/.."
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target/skia}"
REL="$CARGO_TARGET_DIR/release"
OUT="$REL/bundle/mas"
APP="$OUT/Simple Capture.app"

if [ "$adhoc" = 1 ]; then
  SIGNER=-
  if [ -n "$PROFILE" ]; then
    scripts/mas-check.sh --profile "$PROFILE" >/dev/null || die "$PROFILE is not a usable Mac App Store profile"
  fi
else
  [ -n "$PROFILE" ] || die "set SC_MAS_PROFILE to the Mac App Store provisioning profile, or pass --adhoc"
  grep -qF "\"$INSTALLER_IDENTITY\"" <<<"$(security find-identity -v)" ||
    die "no \"$INSTALLER_IDENTITY\" identity in the keychains"
  SIGNER=$(scripts/mas-check.sh --profile "$PROFILE" "$APP_IDENTITY") ||
    die "cannot sign with \"$APP_IDENTITY\" and $PROFILE"
  [[ $SIGNER =~ ^[0-9A-F]{40}$ ]] || die "no certificate SHA-1 for \"$APP_IDENTITY\" from scripts/mas-check.sh --profile"
fi

STASH="$REL/.mas-build-stash"
SHARED=("bundle/macos/Simple Capture.app")
absent=()
restore_shared() {
  local p
  for p in "${SHARED[@]}"; do
    if [ -e "$STASH/$p" ]; then
      rm -rf "${REL:?}/$p"
      mkdir -p "$(dirname "$REL/$p")"
      mv "$STASH/$p" "$REL/$p"
    elif [[ " ${absent[*]-} " == *" $p "* ]]; then
      rm -rf "${REL:?}/$p"
    fi
  done
  rm -rf "$STASH"
}

if [ -z "$src_app" ]; then
  [ ! -e "$STASH" ] || die "$STASH holds what an interrupted mas-build.sh moved aside from $REL; move it back or delete it"
  trap restore_shared EXIT
  mkdir -p "$STASH/bundle/macos"
  for p in "${SHARED[@]}"; do
    if [ -e "$REL/$p" ]; then
      mv "$REL/$p" "$STASH/$p"
    else
      absent+=("$p")
    fi
  done
  scripts/ci/notices.sh --macos
  scripts/stage-helpers.sh ${HELPERS:+"$HELPERS"}
  pnpm --dir ui build >/dev/null
  gen="$CARGO_TARGET_DIR/tauri.mas.gen.json"
  mkdir -p "$CARGO_TARGET_DIR"
  python3 -I - "$gen" "$PROFILE" "$BUILD_NUMBER" <<'PY'
import json, sys

out, profile, build = sys.argv[1:]
mac = {}
if profile:
    mac["files"] = {"embedded.provisionprofile": profile}
if build:
    mac["bundleVersion"] = build
with open(out, "w") as f:
    json.dump({"bundle": {"macOS": mac}}, f, indent=2)
PY
  (cd app && env -u APPLE_CERTIFICATE -u APPLE_CERTIFICATE_PASSWORD -u APPLE_SIGNING_IDENTITY \
    -u APPLE_ID -u APPLE_PASSWORD -u APPLE_TEAM_ID -u APPLE_API_KEY -u APPLE_API_ISSUER \
    -u APPLE_API_KEY_PATH nice -n 19 ../ui/node_modules/.bin/tauri build \
    --config tauri.bundle.conf.json --config tauri.bundle.macos.conf.json \
    --config tauri.appstore.conf.json --config "$gen" \
    --bundles app -- --locked -j "${SC_MAS_JOBS:-4}")
  rm -rf "$APP"
  mkdir -p "$OUT"
  mv "$REL/bundle/macos/Simple Capture.app" "$APP"
else
  if ! [ "$src_app" -ef "$APP" ]; then
    rm -rf "$APP"
    mkdir -p "$OUT"
    ditto "$src_app" "$APP"
  fi
  [ -z "$PROFILE" ] || cp "$PROFILE" "$APP/Contents/embedded.provisionprofile"
  [ -z "$BUILD_NUMBER" ] || plutil -replace CFBundleVersion -string "$BUILD_NUMBER" "$APP/Contents/Info.plist"
fi

SC_SIGN_IDENTITY=$SIGNER SC_ENTITLEMENTS=app/macos/entitlements.appstore.plist SC_NOTARIZE=0 \
  scripts/macos-sign.sh "$APP" >&2

VERSION=$(plutil -extract CFBundleShortVersionString raw "$APP/Contents/Info.plist")
PKG="$OUT/Simple Capture_${VERSION}_mas.pkg"
rm -f "$PKG"
pkgsign=()
[ "$adhoc" = 1 ] || pkgsign=(--sign "$INSTALLER_IDENTITY")
productbuild --component "$APP" /Applications ${pkgsign[@]+"${pkgsign[@]}"} "$PKG" >&2
check=(scripts/mas-check.sh)
[ "$adhoc" = 1 ] && check+=(--adhoc)
"${check[@]}" "$APP" "$PKG"
echo "$PKG"
