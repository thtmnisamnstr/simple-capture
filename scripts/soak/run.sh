#!/bin/bash
# just soak [N]: N-minute soak (default 10) through the real CLI under the dev host (M1 plan §8.3
# wp-soak-test). 30 minutes is the ADR-010 decision run; shorter runs are regression checks.
#   SC_SOAK_KEEP=1          keep the recording (otherwise deleted after the summary)
#   SC_SOAK_SWITCH_S=S      display ↔ window switch interval (default 300)
#   SC_SOAK_CAMERA / SC_SOAK_MIC (default "default"), SC_SOAK_SYSTEM_AUDIO (default "all")
# The summary lands in <main checkout>/target/soak/<stamp>/summary.md (plus run.json, report.json).
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
N="${1:-10}"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
case "$TARGET_DIR" in /*) ;; *) TARGET_DIR="$ROOT/$TARGET_DIR" ;; esac

if [ "$(uname -s)" != Darwin ]; then
  echo "SKIPPED: soak needs macOS in M1"
  exit 0
fi

if [ -z "${SC_SOAK_INNER:-}" ]; then
  cargo build --locked -p simple-capture-cli -p sc-devtools || exit 1
  # Private copies outside ~/Documents: other worktrees may relink the shared target mid-run,
  # and nothing the dev host's child touches should depend on a Documents-folder TCC answer.
  BIN="${TMPDIR:-/tmp}/sc-soak-bin"
  mkdir -p "$BIN"
  for b in sc-devtools simple-capture; do
    # A new inode each time: overwriting a signed binary in place gets it SIGKILLed at exec.
    if ! cmp -s "$TARGET_DIR/debug/$b" "$BIN/$b"; then
      cp "$TARGET_DIR/debug/$b" "$BIN/$b.new" && mv -f "$BIN/$b.new" "$BIN/$b" || exit 1
    fi
  done
  export SC_DEVTOOLS="$BIN/sc-devtools"
  export SC_SOAK_CLI="$BIN/simple-capture"
  export SC_SOAK_INNER=1
  "$SC_DEVTOOLS" --help >/dev/null
  "$SC_SOAK_CLI" --help >/dev/null
  exec "$ROOT/scripts/lib/live.sh" "$0" "$@"
fi

MAIN_ROOT="$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)"
MAIN_ROOT="${MAIN_ROOT%/.git}"
[ -d "$MAIN_ROOT" ] || MAIN_ROOT="$ROOT"
STAMP="$(date +%Y%m%d-%H%M%S)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/sc-soak-$STAMP.XXXXXX")"
OUT="$MAIN_ROOT/target/soak/$STAMP"
mkdir -p "$OUT"
ARGS=(soak run --minutes "$N" --cli "$SC_SOAK_CLI" --runsh "$ROOT/tools/macos-dev-host/run.sh"
      --work "$WORK" --switch-every "${SC_SOAK_SWITCH_S:-300}"
      --camera "${SC_SOAK_CAMERA:-default}" --mic "${SC_SOAK_MIC:-default}"
      --system-audio "${SC_SOAK_SYSTEM_AUDIO:-all}")
[ "${SC_SOAK_KEEP:-0}" = 1 ] && ARGS+=(--keep)
echo "soak: $N min; work $WORK; summary → $OUT" >&2
(cd "$WORK" && exec "$SC_DEVTOOLS" "${ARGS[@]}") &
CHILD=$!
trap 'kill -TERM "$CHILD" 2>/dev/null' INT TERM HUP
while :; do
  wait "$CHILD"
  RC=$?
  kill -0 "$CHILD" 2>/dev/null || break
done
for f in summary.md report.json run.json record.stderr; do
  [ -f "$WORK/$f" ] && cp "$WORK/$f" "$OUT/"
done
if [ "${SC_SOAK_KEEP:-0}" != 1 ]; then
  rm -rf "$WORK"
fi
echo "soak: exit $RC; summary in $OUT/summary.md" >&2
exit "$RC"
