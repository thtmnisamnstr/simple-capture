#!/usr/bin/env bash
# Runs every preview-transport mode in sequence and writes results/<set>/<mode>.json
# plus results/<set>/summary.md. Each run opens a window for ~warmup+duration seconds.
#   MODES="a-gl-540 c-native-1080" ./bench.sh      # subset
#   CONSTRAINED=1 ./bench.sh                       # leave only ~4 cores free (M1 proxy)
#   SC_DURATION=10 SC_WARMUP=3 SC_FPS=60           # passed through to the app
#   SC_SNAPSHOT=1                                  # also run the window snapshot check (off by default)
# The app is a bare binary (no .app bundle, no Info.plist, no bundle ID is
# registered) and asks for no macOS permission. With SC_SNAPSHOT=1 it runs under
# tools/macos-dev-host/run.sh so the snapshot uses the Simple Capture Dev
# Screen Recording grant (CLAUDE.md); it never prompts and records "skipped"
# when no grant exists.
set -euo pipefail
cd "$(dirname "$0")"

DEFAULT_MODES="idle a-gl-540 a-2d-540 a-gl-720 a-2d-720 a-gl-1080 a-2d-1080 b-gl-540 b-gl-720 b-gl-1080 b-gl-1080-raf c-native-540 c-native-1080 c-above-1080"
MODES=${MODES:-$DEFAULT_MODES}
SET=${SET:-$([ "${CONSTRAINED:-0}" = 1 ] && echo constrained || echo default)}
OUT="$PWD/results/$SET"
mkdir -p "$OUT"

[ -d node_modules ] || pnpm install --frozen-lockfile
pnpm build >/dev/null
cargo build --release --manifest-path src-tauri/Cargo.toml
BIN="$PWD/src-tauri/target/release/preview-transport-spike"
SNAP=${SC_SNAPSHOT:-0}
RUN=()
[ "$SNAP" = 1 ] && RUN=("$PWD/../../tools/macos-dev-host/run.sh")

HOGS=()
cleanup() { for p in "${HOGS[@]:-}"; do [ -n "$p" ] && kill "$p" 2>/dev/null || true; done; }
trap cleanup EXIT
if [ "${CONSTRAINED:-0}" = 1 ]; then
  NCPU=$(sysctl -n hw.ncpu)
  FREE=${FREE_CORES:-4}
  N=$(( NCPU > FREE ? NCPU - FREE : 0 ))
  echo "constrained: starting $N busy-loop hogs on $NCPU cores (leaves ~$FREE)"
  for _ in $(seq 1 "$N"); do (while :; do :; done) & HOGS+=($!); done
fi

{ date; uptime; sysctl -n machdep.cpu.brand_string; sw_vers; } > "$OUT/env.txt"

for m in $MODES; do
  for attempt in 1 2; do
    echo "=== $m (attempt $attempt)"
    rm -f "$OUT/$m.json"
    SC_SNAPSHOT="$SNAP" SC_MODE="$m" SC_RESULTS="$OUT" ${RUN[@]+"${RUN[@]}"} "$BIN" 2> "$OUT/$m.log" || echo "  exit code $? (see $OUT/$m.log)"
    sleep 2
    if [ -f "$OUT/$m.json" ]; then break; fi
    echo "  no result for $m"
  done
done
uptime >> "$OUT/env.txt"

node scripts/summarize.mjs "$OUT" | tee "$OUT/summary.md"
