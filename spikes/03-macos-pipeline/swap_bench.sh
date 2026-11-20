#!/bin/bash
# The binary is staged outside ~/Documents because the dev host has no Documents-folder grant (see bench.sh).
set -uo pipefail
[ -n "${1:-}" ] || { echo "usage: $0 OUTDIR [strategy:audio[:order[:route]] ...]" >&2; exit 64; }
mkdir -p "$1" && OUT=$(cd "$1" && pwd); shift
VARIANTS=("$@")
[ ${#VARIANTS[@]} -gt 0 ] || VARIANTS=(update:shared restart:separate overlap:separate)
cd "$(dirname "$0")"
SPIKE=$(pwd)
HOST_RUN=$(cd ../../tools/macos-dev-host && pwd)/run.sh
cargo build --release -q 2>/dev/null || { echo "cargo build failed" >&2; exit 1; }
STAGE=$(mktemp -d "${TMPDIR:-/tmp}/sc-spike03-swap.XXXXXX")
cp "$SPIKE/target/release/sc-spike" "$STAGE/sc-spike"
log() { echo "[swap $(date +%H:%M:%S)] $*" >&2; }
log "machine: $(sysctl -n machdep.cpu.brand_string), $(sw_vers -productVersion), load: $(uptime | sed 's/.*load averages: //')"
for v in "${VARIANTS[@]}"; do
  IFS=: read -r strat audio order route <<< "$v"
  name="$strat-$audio${order:+-$order}${route:+-$route}"
  log "$name"
  args=(swap --out "$STAGE/$name" --strategy "$strat" --audio "$audio")
  [ -n "${order:-}" ] && args+=(--order "$order")
  [ -n "${route:-}" ] && args+=(--route "$route")
  (cd "$STAGE" && "$HOST_RUN" "$STAGE/sc-spike" "${args[@]}") > "$OUT/$name.stdout" 2> "$OUT/$name.stderr"
  echo "rc=$?" >> "$OUT/$name.stderr"
  cp -R "$STAGE/$name" "$OUT/"
  python3 -I swap_analyze.py "$OUT/$name" > /dev/null || log "analyze failed for $name"
  sleep 2
done
rm -rf "$STAGE"
log "done: $OUT"
