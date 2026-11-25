#!/usr/bin/env bash
# Stages what the app bundle ships beside the app: the helpers for this OS in app/binaries as
# sc-<name>-<host triple> (Tauri externalBin drops the triple when bundling) and every license
# text in app/gen/licenses (bundled as the `licenses` resource folder the About window reads).
#   scripts/stage-helpers.sh [HELPERS_DIR]          default third_party/out/<os>-<arch>
#   scripts/stage-helpers.sh --mas [HELPERS_DIR]    Mac App Store edition: LGPL ffmpeg and ffprobe
#     only, default third_party/out/<os>-<arch>-lgpl, into app/binaries-mas and
#     app/gen/licenses-mas (tauri.appstore.conf.json), Rust notices from
#     target/notices/mas/THIRD_PARTY_RUST.html (scripts/ci/notices.sh --mas OUT)
set -euo pipefail
cd "$(dirname "$0")/.."
mas=0
if [ "${1:-}" = --mas ]; then
  mas=1
  shift
fi
triple=$(rustc -vV | sed -n 's/^host: //p')
case "$triple" in
  aarch64-apple-darwin) plat=macos-arm64 ;;
  x86_64-apple-darwin) plat=macos-x86_64 ;;
  x86_64-pc-windows-msvc) plat=windows-x86_64 ;;
  x86_64-unknown-linux-gnu) plat=linux-x86_64 ;;
  aarch64-unknown-linux-gnu) plat=linux-arm64 ;;
  *) echo "stage-helpers: unknown host $triple" >&2; exit 1 ;;
esac
if [ $mas = 1 ]; then
  [[ $plat == macos-* ]] || { echo "stage-helpers: --mas needs a macOS host" >&2; exit 1; }
  SRC=${1:-third_party/out/$plat-lgpl}
  BIN=app/binaries-mas
  LIC=app/gen/licenses-mas
  NOTICES=target/notices/mas/THIRD_PARTY_RUST.html
  helpers=(ffmpeg ffprobe)
else
  SRC=${1:-third_party/out/$plat}
  BIN=app/binaries
  LIC=app/gen/licenses
  NOTICES=target/notices/THIRD_PARTY_RUST.html
  helpers=(ffmpeg ffprobe gifski)
fi
exe=""
[[ $triple == *windows* ]] && exe=.exe
rm -rf "$BIN"
mkdir -p "$BIN"
for h in "${helpers[@]}"; do
  [ -f "$SRC/$h$exe" ] || { echo "stage-helpers: $SRC/$h$exe missing" >&2; exit 1; }
  if [ $mas = 1 ] && { ! grep -qaF 'LGPL version 2.1 or later' "$SRC/$h" || grep -qaF -- '--enable-gpl' "$SRC/$h"; }; then
    echo "stage-helpers: $SRC/$h is not an LGPL 2.1 build (SC_FFMPEG_LICENSE=lgpl third_party/ffmpeg/build-macos.sh)" >&2
    exit 1
  fi
  cp "$SRC/$h$exe" "$BIN/sc-$h-$triple$exe"
done
rm -rf "$LIC"
mkdir -p "$LIC/helpers"
cp LICENSE "$LIC/LICENSE"
[ -d "$SRC/licenses" ] && cp -R "$SRC/licenses/." "$LIC/helpers/"
[ $mas = 0 ] && [ -f "$SRC/gifski.LICENSE" ] && cp "$SRC/gifski.LICENSE" "$LIC/helpers/gifski-LICENSE"
if [ -f "$NOTICES" ]; then
  cp "$NOTICES" "$LIC/THIRD_PARTY_RUST.html"
else
  echo "stage-helpers: no $NOTICES (run scripts/ci/notices.sh$([ $mas = 1 ] && echo " --mas $NOTICES")); the bundle will lack the Rust notices"
fi
echo "stage-helpers: $plat$([ $mas = 1 ] && echo ' LGPL') helpers and $(find "$LIC" -type f | wc -l | tr -d ' ') license files staged"
