#!/bin/bash
# Spike 3 benchmark: records fixed-length runs and writes OUT/summary.json.
# Usage (from repo root): spikes/03-macos-pipeline/bench.sh [OUTDIR]
# Capture runs inside "Simple Capture Dev.app" (tools/macos-dev-host), the one app that holds the
# Screen Recording / Camera / Mic grants. The binary, its cwd and its output live in a temp stage dir
# outside ~/Documents, because the host app has no Documents-folder grant (dyld blocks on a TCC prompt
# otherwise); results are copied into OUTDIR afterwards. Recordings contain your real screen and mic.
set -uo pipefail
if [ -n "${1:-}" ]; then mkdir -p "$1" && OUT_ARG=$(cd "$1" && pwd); fi
cd "$(dirname "$0")"
SPIKE=$(pwd)
HOST_RUN=$(cd ../../tools/macos-dev-host && pwd)/run.sh
cargo build --release -q 2>/dev/null || { echo "cargo build failed" >&2; exit 1; }
RAW_BIN=$SPIKE/target/release/sc-spike
OUT=${OUT_ARG:-$SPIKE/out/bench-$(date +%Y%m%d-%H%M%S)}
mkdir -p "$OUT"; OUT=$(cd "$OUT" && pwd)
STAGE=$(mktemp -d "${TMPDIR:-/tmp}/sc-spike03.XXXXXX")
cp "$RAW_BIN" "$STAGE/sc-spike"
host() { (cd "$STAGE" && "$HOST_RUN" "$STAGE/sc-spike" "$@"); }
log() { echo "[bench $(date +%H:%M:%S)] $*" >&2; }
log "machine: $(sysctl -n machdep.cpu.brand_string), $(sw_vers -productVersion), load: $(uptime | sed 's/.*load averages: //')"
uptime > "$OUT/load_before.txt"
host list > "$OUT/list.json" 2>/dev/null

log "1/7 main: 10 s display (own app excluded) + camera + mic + system audio, 30 fps"
host record --out "$STAGE/main" --secs 10 > /dev/null 2> "$OUT/main.stderr"
host decode --dir "$STAGE/main" --seeks 20 > /dev/null 2>> "$OUT/main.stderr"
host decode --dir "$STAGE/main" --file camera.mov --seeks 20 > /dev/null 2>> "$OUT/main.stderr"
cp -R "$STAGE/main" "$OUT/"
python3 -I analyze.py "$OUT/main" > /dev/null
: > "$OUT/psnr.txt"
for f in screen camera; do
  [ -f "$OUT/main/$f.decode.json" ] || continue
  IDX=$(python3 -c "import json;print(json.load(open('$OUT/main/$f.decode.json'))['png']['frame_index'])")
  ffmpeg -v error -y -i "$OUT/main/$f.mov" -vf "select=eq(n\,$IDX),scale=in_color_matrix=bt709:in_range=tv:out_range=pc:flags=bilinear+accurate_rnd+full_chroma_int,format=rgb24" -frames:v 1 "$OUT/main/$f.ffmpeg_ref.png"
  echo "$f $(ffmpeg -v info -nostats -i "$OUT/main/$f.frame.png" -i "$OUT/main/$f.ffmpeg_ref.png" -lavfi '[0:v]format=rgb24[a];[1:v]format=rgb24[b];[a][b]psnr' -f null - 2>&1 | grep -o 'PSNR.*')" >> "$OUT/psnr.txt"
done

log "2/7 exclusion controls: nothing excluded, and app-level exclusion"
host record --out "$STAGE/noexcl" --secs 6 --exclude none --camera-mode off --no-mic > /dev/null 2> "$OUT/noexcl.stderr"
host decode --dir "$STAGE/noexcl" --seeks 0 > /dev/null 2>> "$OUT/noexcl.stderr"
cp -R "$STAGE/noexcl" "$OUT/"
host record --out "$STAGE/exclapp" --secs 4 --exclude app --camera-mode off --no-mic --no-sysaudio --no-activity > /dev/null 2> "$OUT/exclapp.stderr"
host decode --dir "$STAGE/exclapp" --seeks 0 > /dev/null 2>> "$OUT/exclapp.stderr"
cp -R "$STAGE/exclapp" "$OUT/"

log "3/7 idle vs active: single window, 30 and 60 fps"
for f in 30 60; do
  host record --out "$STAGE/window$f" --target activity-window --secs 10 --fps $f --camera-mode off --no-mic --no-test-window > /dev/null 2> "$OUT/window$f.stderr"
  cp -R "$STAGE/window$f" "$OUT/"
done

log "4/7 full display 30 vs 60 fps (no camera/mic)"
for f in 30 60; do
  host record --out "$STAGE/display$f" --secs 10 --fps $f --camera-mode off --no-mic > /dev/null 2> "$OUT/display$f.stderr"
  cp -R "$STAGE/display$f" "$OUT/"
done

log "5/7 SIGKILL at 6 s"
mkdir -p "$STAGE/kill"
host killtest --out "$STAGE/kill" --after 6 > /dev/null 2> "$OUT/kill.stderr"
cp -R "$STAGE/kill" "$OUT/"
python3 -I analyze.py "$OUT/kill" > /dev/null

log "6/7 encoder headroom (HEVC + H.264, realtime mode)"
"$RAW_BIN" encbench --out "$OUT/encbench" --frames 240 > /dev/null 2>&1

log "7/7 encoder headroom, E-core-only proxy (taskpolicy -c background)"
taskpolicy -c background "$RAW_BIN" encbench --out "$OUT/encbench_ecore" --frames 120 --hevc-only --sizes 7680x2160,5120x2880,3840x2160,1920x1080 > /dev/null 2>&1

uptime > "$OUT/load_after.txt"
rm -rf "$STAGE"
python3 -I analyze.py --summary "$OUT"
log "done: $OUT/summary.json"
