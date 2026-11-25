#!/bin/bash
# just test-live PKG PREFIX [FILTER...]
# Builds PKG's test binaries, picks those whose target name starts with PREFIX and runs each under
# scripts/lib/live.sh + the dev host with SC_LIVE=1, with the fixture apps running (SC_LIVE_FIXTURE=0
# skips them). libtest targets get `--ignored --test-threads=1 FILTER...`; harness = false targets
# get the filters only.
# Fails when 0 tests ran or any failed (M1 plan §2 rule 3).
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 1
[ $# -ge 2 ] || { echo "usage: scripts/test-live.sh PKG PREFIX [FILTER...]" >&2; exit 2; }
PKG=$1 PREFIX=$2
shift 2
FILTERS=("$@")
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
case "$TARGET_DIR" in /*) ;; *) TARGET_DIR="$ROOT/$TARGET_DIR" ;; esac

cargo build --locked -q -p sc-devtools || exit 1
export SC_DEVTOOLS="$TARGET_DIR/debug/sc-devtools"

JSON=$(cargo test --locked -p "$PKG" --no-run --message-format=json) || exit 1
LIST=$(printf '%s\n' "$JSON" | python3 -I -c '
import json, sys
prefix = sys.argv[1]
for line in sys.stdin:
    try:
        m = json.loads(line)
    except ValueError:
        continue
    if m.get("reason") != "compiler-artifact" or not m.get("executable"):
        continue
    t = m["target"]
    if not m.get("profile", {}).get("test") or "test" not in t.get("kind", []):
        continue
    if t["name"].startswith(prefix):
        print(m["manifest_path"] + "\t" + t["name"] + "\t" + m["executable"])
' "$PREFIX")
if [ -z "$LIST" ]; then
  echo "test-live: no test targets of $PKG start with \"$PREFIX\"" >&2
  exit 1
fi

TOTAL_PASSED=0 TOTAL_FAILED=0 STATUS=0
LOG=$(mktemp "${TMPDIR:-/tmp}/sc-test-live.XXXXXX")
while IFS=$'\t' read -r MANIFEST NAME BIN; do
  DIR=$(dirname "$MANIFEST")
  if (cd "$DIR" && "$BIN" --help 2>&1) | grep -q -- '--test-threads'; then
    ARGS=(--ignored --test-threads=1 ${FILTERS[@]+"${FILTERS[@]}"})
  else
    ARGS=(${FILTERS[@]+"${FILTERS[@]}"})
  fi
  echo "test-live: $PKG/$NAME ${ARGS[*]-}" >&2
  # The fixture apps start inside the live lock but outside the dev host, unless the caller
  # already provides SC_FIXTURE or opts out with SC_LIVE_FIXTURE=0.
  FIXTURE=()
  if [ -z "${SC_FIXTURE:-}" ] && [ "${SC_LIVE_FIXTURE:-1}" != 0 ]; then
    FIXTURE=("$ROOT/scripts/lib/live.sh" fixture run)
  fi
  (cd "$DIR" && SC_LIVE=1 "$ROOT/scripts/lib/live.sh" ${FIXTURE[@]+"${FIXTURE[@]}"} "$ROOT/tools/macos-dev-host/run.sh" "$BIN" ${ARGS[@]+"${ARGS[@]}"}) 2>&1 | tee "$LOG"
  RC=${PIPESTATUS[0]}
  [ "$RC" -eq 0 ] || STATUS=1
  P=$(sed -nE 's/^test result: [A-Za-z]+\. ([0-9]+) passed; ([0-9]+) failed.*/\1/p' "$LOG" | awk '{s+=$1} END {print s+0}')
  F=$(sed -nE 's/^test result: [A-Za-z]+\. ([0-9]+) passed; ([0-9]+) failed.*/\2/p' "$LOG" | awk '{s+=$1} END {print s+0}')
  TOTAL_PASSED=$((TOTAL_PASSED + P))
  TOTAL_FAILED=$((TOTAL_FAILED + F))
done <<< "$LIST"
rm -f "$LOG"

TOTAL=$((TOTAL_PASSED + TOTAL_FAILED))
echo "test-live: $PKG $PREFIX*: ran $TOTAL tests, $TOTAL_PASSED passed, $TOTAL_FAILED failed" >&2
if [ "$TOTAL" -eq 0 ]; then
  echo "test-live: FAILED: 0 tests ran" >&2
  exit 1
fi
[ "$TOTAL_FAILED" -eq 0 ] && [ "$STATUS" -eq 0 ] || exit 1
exit 0
