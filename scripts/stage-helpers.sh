#!/usr/bin/env bash
# Stages what the app bundle ships beside the app: the helpers for this OS in app/binaries
# (Tauri externalBin names them with the Rust host triple) and every license text in
# app/gen/licenses (bundled as the `licenses` resource folder the About window reads).
#   scripts/stage-helpers.sh [HELPERS_DIR]    default third_party/out/<os>-<arch>
set -euo pipefail
cd "$(dirname "$0")/.."
triple=$(rustc -vV | sed -n 's/^host: //p')
case "$triple" in
  aarch64-apple-darwin) plat=macos-arm64 ;;
  x86_64-apple-darwin) plat=macos-x86_64 ;;
  x86_64-pc-windows-msvc) plat=windows-x86_64 ;;
  x86_64-unknown-linux-gnu) plat=linux-x86_64 ;;
  aarch64-unknown-linux-gnu) plat=linux-arm64 ;;
  *) echo "stage-helpers: unknown host $triple" >&2; exit 1 ;;
esac
SRC=${1:-third_party/out/$plat}
exe=""
[[ $triple == *windows* ]] && exe=.exe
mkdir -p app/binaries
for h in ffmpeg ffprobe gifski; do
  [ -f "$SRC/$h$exe" ] || { echo "stage-helpers: $SRC/$h$exe missing" >&2; exit 1; }
  cp "$SRC/$h$exe" "app/binaries/$h-$triple$exe"
done
rm -rf app/gen/licenses
mkdir -p app/gen/licenses/helpers
cp LICENSE app/gen/licenses/LICENSE
[ -d "$SRC/licenses" ] && cp -R "$SRC/licenses/." app/gen/licenses/helpers/
[ -f "$SRC/gifski.LICENSE" ] && cp "$SRC/gifski.LICENSE" app/gen/licenses/helpers/gifski-LICENSE
if [ -f target/notices/THIRD_PARTY_RUST.html ]; then
  cp target/notices/THIRD_PARTY_RUST.html app/gen/licenses/
else
  echo "stage-helpers: no target/notices/THIRD_PARTY_RUST.html (run scripts/ci/notices.sh); the bundle will lack the Rust notices"
fi
echo "stage-helpers: $plat helpers and $(find app/gen/licenses -type f | wc -l | tr -d ' ') license files staged"
