#!/usr/bin/env bash
set -euo pipefail
. "$(dirname -- "$0")/lib.sh"

for sec in ffmpeg x264 x265 svt-av1 libvpx opus libogg libvorbis zlib libwebp nv-codec-headers gifski; do
    fetch "$sec" >/dev/null
done
fetch gifski license_url license_file license_sha256 >/dev/null
log "all sources present and verified in $TP_DOWNLOADS"
