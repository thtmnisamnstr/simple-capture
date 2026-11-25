#!/bin/bash
# Third-party notices for the Rust crates in the app (docs/LICENSES.md §2.2), via cargo-about
# 0.9.3 (`cargo install --locked cargo-about@0.9.3 --features cli`). Release artifact, never
# checked in. OUT defaults to target/notices/THIRD_PARTY_RUST.html, read by stage-helpers.sh.
#   scripts/ci/notices.sh [OUT]            every workspace crate on every about.toml target
#   scripts/ci/notices.sh --mas [OUT]      Mac App Store app: app crate + feature mas, arm64
#   scripts/ci/notices.sh --check-mas DIR  DIR (staged app/gen/licenses or a bundle's
#       Contents/Resources/licenses) must hold exactly the Mac App Store edition's licenses:
#       LGPL FFmpeg and permissive libraries, no GPL/AGPL text, x264, x265 or gifski
set -euo pipefail
cd "$(dirname "$0")/../.."

MAS_LICENSES='LICENSE
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

check_mas() {
    local dir=$1 want got copyleft
    [ -d "$dir" ] || { echo "notices.sh: $dir is not a directory" >&2; exit 1; }
    want=$(printf '%s\n' "$MAS_LICENSES" | LC_ALL=C sort)
    got=$(cd "$dir" && find . -type f | sed 's|^\./||' | LC_ALL=C sort)
    if [ "$got" != "$want" ]; then
        diff <(printf '%s\n' "$want") <(printf '%s\n' "$got") >&2 || true
        echo "notices.sh: $dir is not the Mac App Store license set (< expected, > found). It is made by SC_FFMPEG_LICENSE=lgpl third_party/ffmpeg/build-macos.sh, notices.sh --mas and scripts/stage-helpers.sh; change MAS_LICENSES only when the shipped libraries change." >&2
        exit 1
    fi
    copyleft=$(grep -rlE 'GNU (AFFERO )?GENERAL PUBLIC LICENSE' "$dir" || true)
    [ -z "$copyleft" ] || { echo "notices.sh: GPL/AGPL text in the Mac App Store licenses: $copyleft" >&2; exit 1; }
    echo "notices.sh: $dir holds exactly the Mac App Store licenses"
}

case "${1:-}" in
    --check-mas)
        [ $# -eq 2 ] || { echo "usage: scripts/ci/notices.sh --check-mas DIR" >&2; exit 2; }
        check_mas "$2"
        exit 0
        ;;
    --mas)
        shift
        scope=(-c about.toml -m app/Cargo.toml --features simple-capture-app/mas --target aarch64-apple-darwin)
        ;;
    *) scope=(--workspace) ;;
esac
OUT=${1:-target/notices/THIRD_PARTY_RUST.html}
command -v cargo-about >/dev/null || { echo "notices.sh: cargo-about missing (cargo install --locked cargo-about@0.9.3 --features cli)" >&2; exit 1; }
mkdir -p "$(dirname "$OUT")"
cargo about generate --locked --fail "${scope[@]}" -o "$OUT" scripts/ci/about.hbs
echo "notices.sh: wrote $OUT"
