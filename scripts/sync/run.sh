#!/bin/bash
# just sync-test: flash + beep (+ camera) sync test (M1 plan §8.3 wp-sync-test).
# SC_SYNC_RUNS=N (default 1) records N runs of 70 s; SC_SYNC_KEEP=1 keeps the recordings.
# Reports land in target/sync/<stamp>/. The flashbeep window runs outside the dev host; every
# recording goes through tools/macos-dev-host/run.sh inside the live lock.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
case "$TARGET_DIR" in /*) ;; *) TARGET_DIR="$ROOT/$TARGET_DIR" ;; esac

if [ "$(uname -s)" != Darwin ]; then
  echo "SKIPPED: the sync test needs macOS in M1"
  exit 0
fi

if [ -z "${SC_SYNC_INNER:-}" ]; then
  cargo build --locked -p simple-capture-cli -p sc-devtools || exit 1
  # Private copies outside ~/Documents (as scripts/e2e/run.sh): other worktrees may relink the
  # shared target mid-run, and a new inode avoids SIGKILL on exec of a re-signed binary.
  BIN="${TMPDIR:-/tmp}/sc-sync-bin"
  mkdir -p "$BIN"
  for b in sc-devtools simple-capture; do
    if ! cmp -s "$TARGET_DIR/debug/$b" "$BIN/$b"; then
      cp "$TARGET_DIR/debug/$b" "$BIN/$b.new" && mv -f "$BIN/$b.new" "$BIN/$b" || exit 1
    fi
  done
  export SC_DEVTOOLS="$BIN/sc-devtools"
  export SC_SYNC_CLI="$BIN/simple-capture"
  export SC_SYNC_INNER=1
  "$SC_DEVTOOLS" --help >/dev/null
  "$SC_SYNC_CLI" --help >/dev/null
  exec "$ROOT/scripts/lib/live.sh" "$0" "$@"
fi

python3 -I "$ROOT/scripts/sync/sync.py" "$@" &
PY=$!
trap 'kill -TERM "$PY" 2>/dev/null' INT TERM HUP
while :; do
  wait "$PY"
  RC=$?
  kill -0 "$PY" 2>/dev/null || break
done
exit "$RC"
