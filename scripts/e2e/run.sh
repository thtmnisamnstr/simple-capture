#!/bin/bash
# just e2e [STEP...]: record → switch → stop → export through the real CLI under the dev host
# (M1 plan §8.2 wp-e2e-cli). Steps are 1..14 (default: all); SC_E2E_KEEP=1 keeps the recordings.
#
# Builds simple-capture and sc-devtools, takes the live lock (scripts/lib/live.sh: banner,
# caffeinate, envcheck), starts the fixture apps outside the dev host, then runs
# scripts/e2e/e2e.py, which launches every recording as
# `tools/macos-dev-host/run.sh <target>/debug/simple-capture record …`.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
case "$TARGET_DIR" in /*) ;; *) TARGET_DIR="$ROOT/$TARGET_DIR" ;; esac

if [ "$(uname -s)" != Darwin ]; then
  echo "SKIPPED: e2e needs macOS in M1"
  exit 0
fi

if [ -z "${SC_E2E_INNER:-}" ]; then
  cargo build --locked -p simple-capture-cli -p sc-devtools || exit 1
  # Private copies outside ~/Documents: other worktrees share CARGO_TARGET_DIR and may relink
  # these mid-run, and the fixture apps are their own responsible process (disclaimed), so
  # launching them from ~/Documents blocks in exec on a Documents-folder TCC check.
  BIN="${TMPDIR:-/tmp}/sc-e2e-bin"
  mkdir -p "$BIN"
  for b in sc-devtools simple-capture; do
    # A new inode each time: overwriting a signed binary in place gets it SIGKILLed at exec.
    if ! cmp -s "$TARGET_DIR/debug/$b" "$BIN/$b"; then
      cp "$TARGET_DIR/debug/$b" "$BIN/$b.new" && mv -f "$BIN/$b.new" "$BIN/$b" || exit 1
    fi
  done
  export SC_DEVTOOLS="$BIN/sc-devtools"
  export SC_E2E_CLI="$BIN/simple-capture"
  export SC_E2E_INNER=1
  # A freshly linked binary can take minutes to exec the first time while the system scans it;
  # pay that here, outside the lock, not inside the fixture's 20 s start window.
  "$SC_DEVTOOLS" --help >/dev/null
  "$SC_E2E_CLI" --help >/dev/null
  exec "$ROOT/scripts/lib/live.sh" "$0" "$@"
fi

# Helpers never come from $PATH: SC_FFMPEG/SC_FFPROBE, else this checkout's third_party/out,
# else the main checkout's (a worktree under .wt/ has no third_party/out of its own).
MAIN_ROOT="$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)"
MAIN_ROOT="${MAIN_ROOT%/.git}"
ARCH=$(uname -m); [ "$ARCH" = arm64 ] || ARCH=x86_64
for kind in ffmpeg ffprobe; do
  var="SC_$(echo "$kind" | tr a-z A-Z)"
  if [ -z "${!var:-}" ]; then
    for dir in "$ROOT/third_party/out/macos-$ARCH" "$MAIN_ROOT/third_party/out/macos-$ARCH"; do
      if [ -x "$dir/$kind" ]; then
        export "$var=$dir/$kind"
        break
      fi
    done
  fi
  if [ -z "${!var:-}" ]; then
    echo "e2e: no $kind helper (run \`just helpers\` or set $var)" >&2
    exit 1
  fi
done

# shellcheck source=../lib/live.sh
source "$ROOT/scripts/lib/live.sh"
# live.sh's fixture_start gives the apps 20 s; the first disclaimed launch of a new sc-devtools
# can take minutes (system scan), so wait longer here and reuse fixture_stop.
mkdir -p "$LIVE_DIR"
trap 'fixture_stop' EXIT
for attempt in 1 2; do
  rm -f "$LIVE_DIR/fixture.json"
  (cd "$(dirname "$SC_DEVTOOLS")" && exec "$SC_DEVTOOLS" testapps fixture --out "$LIVE_DIR/fixture.json") \
    2>"$LIVE_DIR/fixture.err" &
  echo "$!" > "$LIVE_DIR/fixture.pid"
  for _ in $(seq 3000); do
    [ -s "$LIVE_DIR/fixture.json" ] && break
    kill -0 "$(cat "$LIVE_DIR/fixture.pid")" 2>/dev/null || break
    sleep 0.1
  done
  [ -s "$LIVE_DIR/fixture.json" ] && break
  echo "e2e: the fixture apps did not start (attempt $attempt): $(tail -c 400 "$LIVE_DIR/fixture.err")" >&2
  fixture_stop
done
[ -s "$LIVE_DIR/fixture.json" ] || exit 1
export SC_FIXTURE="$LIVE_DIR/fixture.json"
python3 -I "$ROOT/scripts/e2e/e2e.py" "$@" &
PY=$!
trap 'kill -TERM "$PY" 2>/dev/null' INT TERM HUP
while :; do
  wait "$PY"
  RC=$?
  kill -0 "$PY" 2>/dev/null || break
done
exit "$RC"
