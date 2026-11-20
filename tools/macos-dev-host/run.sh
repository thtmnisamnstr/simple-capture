#!/bin/bash
set -uo pipefail
APP="$HOME/Applications/Simple Capture Dev.app"
[ -d "$APP" ] || "$(dirname "$0")/install.sh" >&2
[ $# -ge 1 ] || { echo "usage: $0 binary [args...]  (env SC_DEV_PID_FILE=path records the child pid)" >&2; exit 64; }
BIN=$(cd "$(dirname "$1")" && pwd)/$(basename "$1"); shift
D=$(mktemp -d "${TMPDIR:-/tmp}/sc-dev-host.XXXXXX")
: > "$D/out"; : > "$D/err"
PIDARGS=()
[ -n "${SC_DEV_PID_FILE:-}" ] && PIDARGS=(--pid-file "$SC_DEV_PID_FILE")
ENVARGS=()
for v in $(compgen -e | grep -E '^(SC_|RUST_)'); do ENVARGS+=(--env "$v=${!v}"); done
open -n -W -g ${ENVARGS[@]+"${ENVARGS[@]}"} --stdout "$D/out" --stderr "$D/err" -a "$APP" --args \
  --exit-file "$D/rc" ${PIDARGS[@]+"${PIDARGS[@]}"} --cwd "$PWD" -- "$BIN" "$@" 2>"$D/openerr" &
OPEN_PID=$!
tail -n +1 -f "$D/err" >&2 & T1=$!
tail -n +1 -f "$D/out" & T2=$!
wait "$OPEN_PID"
for _ in $(seq 100); do [ -s "$D/rc" ] && break; sleep 0.1; done
[ -s "$D/rc" ] || cat "$D/openerr" >&2
sleep 0.2
kill "$T1" "$T2" 2>/dev/null; wait "$T1" "$T2" 2>/dev/null
RC=$(cat "$D/rc" 2>/dev/null || echo 1)
rm -rf "$D"
exit "$RC"
