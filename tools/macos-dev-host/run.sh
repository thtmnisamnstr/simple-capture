#!/bin/bash
# Runs a binary as a child of "Simple Capture Dev.app" (com.simplecapture.dev), which holds the
# Screen Recording / Camera / Microphone grants. See CLAUDE.md.
#   env SC_DEV_PID_FILE=path   records the child pid (a temp file is used otherwise)
#   env SC_DEV_STDIN=fifo      connects the child's stdin to that fifo (otherwise /dev/null)
# The child always gets SC_DEV_HOST=1 plus every SC_* and RUST_* variable of this shell.
# SIGTERM/SIGINT/SIGHUP sent to this script are forwarded to the child.
set -uo pipefail
APP="$HOME/Applications/Simple Capture Dev.app"
[ -d "$APP" ] || "$(dirname "$0")/install.sh" >&2
[ $# -ge 1 ] || { echo "usage: $0 binary [args...]  (env SC_DEV_PID_FILE=path, SC_DEV_STDIN=fifo)" >&2; exit 64; }
BIN=$(cd "$(dirname "$1")" && pwd)/$(basename "$1"); shift
D=$(mktemp -d "${TMPDIR:-/tmp}/sc-dev-host.XXXXXX")
: > "$D/out"; : > "$D/err"
PID_FILE="${SC_DEV_PID_FILE:-$D/pid}"
rm -f "$PID_FILE"
ENVARGS=(--env "SC_DEV_HOST=1")
for v in $(compgen -e | grep -E '^(SC_|RUST_)'); do
  [ "$v" = SC_DEV_HOST ] && continue
  ENVARGS+=(--env "$v=${!v}")
done
STDINARGS=()
[ -n "${SC_DEV_STDIN:-}" ] && STDINARGS=(--stdin "$SC_DEV_STDIN")

forward() {
  local sig=$1 pid=""
  for _ in $(seq 100); do
    [ -s "$PID_FILE" ] && break
    sleep 0.02
  done
  [ -s "$PID_FILE" ] && pid=$(cat "$PID_FILE")
  [ -n "$pid" ] && kill -"$sig" "$pid" 2>/dev/null
}
trap 'forward TERM' TERM
trap 'forward INT' INT
trap 'forward HUP' HUP

open -n -W -g "${ENVARGS[@]}" ${STDINARGS[@]+"${STDINARGS[@]}"} --stdout "$D/out" --stderr "$D/err" -a "$APP" --args \
  --exit-file "$D/rc" --pid-file "$PID_FILE" --cwd "$PWD" -- "$BIN" "$@" 2>"$D/openerr" &
OPEN_PID=$!
tail -n +1 -f "$D/err" >&2 & T1=$!
tail -n +1 -f "$D/out" & T2=$!
# A trapped signal interrupts `wait`; keep waiting until `open` returns.
while kill -0 "$OPEN_PID" 2>/dev/null; do
  wait "$OPEN_PID"
done
for _ in $(seq 100); do [ -s "$D/rc" ] && break; sleep 0.1; done
[ -s "$D/rc" ] || cat "$D/openerr" >&2
sleep 0.2
kill "$T1" "$T2" 2>/dev/null; wait "$T1" "$T2" 2>/dev/null
RC=$(cat "$D/rc" 2>/dev/null || echo 1)
rm -rf "$D"
exit "$RC"
