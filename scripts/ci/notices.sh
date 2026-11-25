#!/bin/bash
# Third-party notices for the Rust crates in the app (docs/LICENSES.md §2.2), via cargo-about
# 0.9.3 (`cargo install --locked cargo-about@0.9.3 --features cli`). Release artifact, never
# checked in. stage-helpers.sh reads the default OUT of each mode.
#   scripts/ci/notices.sh [OUT]              every workspace crate on every about.toml target
#                                            (Windows, Linux), default target/notices/THIRD_PARTY_RUST.html
#   scripts/ci/notices.sh --macos [OUT]      the macOS app (DMG and Mac App Store alike): app crate,
#                                            arm64, default target/notices/macos/THIRD_PARTY_RUST.html
#   scripts/ci/notices.sh --check-macos DIR  DIR (staged app/gen/licenses or a bundle's
#       Contents/Resources/licenses) must hold exactly the macOS licenses: LGPL FFmpeg and
#       permissive libraries, no GPL/AGPL text, x264, x265 or gifski
set -euo pipefail
cd "$(dirname "$0")/../.."

MACOS_LICENSES='LICENSE
THIRD_PARTY_RUST.html
helpers/ffmpeg/ffmpeg-COPYING.LGPLv2.1
helpers/ffmpeg/ffmpeg-LICENSE.md
helpers/ffmpeg/libogg-COPYING
helpers/ffmpeg/libvorbis-COPYING
helpers/ffmpeg/libvpx-LICENSE
helpers/ffmpeg/libvpx-PATENTS
helpers/ffmpeg/libwebp-COPYING
helpers/ffmpeg/libwebp-PATENTS
helpers/ffmpeg/opus-COPYING
helpers/ffmpeg/svt-av1-LICENSE.md
helpers/ffmpeg/svt-av1-PATENTS.md'

check_macos() {
    local dir=$1 want got copyleft
    [ -d "$dir" ] || { echo "notices.sh: $dir is not a directory" >&2; exit 1; }
    want=$(printf '%s\n' "$MACOS_LICENSES" | LC_ALL=C sort)
    got=$(cd "$dir" && find . -type f | sed 's|^\./||' | LC_ALL=C sort)
    if [ "$got" != "$want" ]; then
        diff <(printf '%s\n' "$want") <(printf '%s\n' "$got") >&2 || true
        echo "notices.sh: $dir is not the macOS license set (< expected, > found). It is made by third_party/ffmpeg/build-macos.sh, notices.sh --macos and scripts/stage-helpers.sh; change MACOS_LICENSES only when the shipped libraries change." >&2
        exit 1
    fi
    copyleft=$(grep -rlE 'GNU (AFFERO )?GENERAL PUBLIC LICENSE' "$dir" || true)
    [ -z "$copyleft" ] || { echo "notices.sh: GPL/AGPL text in the macOS licenses: $copyleft" >&2; exit 1; }
    echo "notices.sh: $dir holds exactly the macOS licenses"
}

case "${1:-}" in
    --check-macos)
        [ $# -eq 2 ] || { echo "usage: scripts/ci/notices.sh --check-macos DIR" >&2; exit 2; }
        check_macos "$2"
        exit 0
        ;;
    --macos)
        shift
        scope=(-c about.toml -m app/Cargo.toml --target aarch64-apple-darwin)
        OUT=${1:-target/notices/macos/THIRD_PARTY_RUST.html}
        ;;
    *)
        scope=(--workspace)
        OUT=${1:-target/notices/THIRD_PARTY_RUST.html}
        ;;
esac
command -v cargo-about >/dev/null || { echo "notices.sh: cargo-about missing (cargo install --locked cargo-about@0.9.3 --features cli)" >&2; exit 1; }
mkdir -p "$(dirname "$OUT")"
cargo about generate --locked --fail "${scope[@]}" -o "$OUT" scripts/ci/about.hbs
echo "notices.sh: wrote $OUT"
