#!/bin/bash
# Shared-resource guard for every live run (M1 plan §0.4).
#
#   scripts/lib/live.sh CMD [ARGS...]     lock, banner, caffeinate, envcheck, run CMD, unlock
#   scripts/lib/live.sh fixture start     start the fixture test apps (prints SC_FIXTURE=path)
#   scripts/lib/live.sh fixture stop
#   source scripts/lib/live.sh            defines live_lock, live_unlock, live_envcheck,
#                                         fixture_start (exports SC_FIXTURE), fixture_stop
#
# Lock: `mkdir target/live.lock` holding the owner pid (macOS has no flock(1)); a lock whose pid
# is dead is stale. Waits up to SC_LIVE_LOCK_WAIT_S (default 1800 s), then fails with exit 4.
# Nested calls from the lock owner's children reuse the lock (SC_LIVE_LOCK_OWNER).
# The fixture runs OUTSIDE the dev host: `$SC_DEVTOOLS testapps fixture --out FILE` writes pids,
# titles and window IDs to FILE once its windows are up and runs until SIGTERM.

LIVE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LIVE_LOCK="$LIVE_ROOT/target/live.lock"
LIVE_DIR="$LIVE_ROOT/target/live"

live_banner() {
  printf '\n==== LIVE RUN %s (pid %s): %s ====\n' "$1" "$$" "$2" >&2
  [ "$1" = START ] && printf '==== screen, speakers, mic and camera in use: please stay off the screen ====\n\n' >&2
  return 0
}

live_devtools() {
  if [ -n "${SC_DEVTOOLS:-}" ] && [ -x "$SC_DEVTOOLS" ]; then
    return 0
  fi
  (cd "$LIVE_ROOT" && cargo build --locked -q -p sc-devtools) || return 1
  SC_DEVTOOLS="${CARGO_TARGET_DIR:-$LIVE_ROOT/target}/debug/sc-devtools"
  case "$SC_DEVTOOLS" in /*) ;; *) SC_DEVTOOLS="$LIVE_ROOT/$SC_DEVTOOLS" ;; esac
  export SC_DEVTOOLS
}

live_lock() {
  if [ -n "${SC_LIVE_LOCK_OWNER:-}" ] && [ "$(cat "$LIVE_LOCK/pid" 2>/dev/null)" = "$SC_LIVE_LOCK_OWNER" ]; then
    LIVE_LOCK_NESTED=1
    return 0
  fi
  LIVE_LOCK_NESTED=0
  mkdir -p "$LIVE_ROOT/target"
  local waited=0 limit="${SC_LIVE_LOCK_WAIT_S:-1800}" owner
  while ! mkdir "$LIVE_LOCK" 2>/dev/null; do
    owner=$(cat "$LIVE_LOCK/pid" 2>/dev/null)
    if [ -n "$owner" ] && ! kill -0 "$owner" 2>/dev/null; then
      echo "live: removing stale lock of dead pid $owner" >&2
      rm -rf "$LIVE_LOCK"
      continue
    fi
    if [ "$waited" -ge "$limit" ]; then
      echo "live: lock $LIVE_LOCK still held by pid ${owner:-?} after ${limit}s" >&2
      return 4
    fi
    [ $((waited % 60)) -eq 0 ] && echo "live: waiting for lock held by pid ${owner:-?}" >&2
    sleep 2
    waited=$((waited + 2))
  done
  echo "$$" > "$LIVE_LOCK/pid"
  export SC_LIVE_LOCK_OWNER="$$"
}

live_unlock() {
  [ "${LIVE_LOCK_NESTED:-0}" = 1 ] && return 0
  [ "$(cat "$LIVE_LOCK/pid" 2>/dev/null)" = "$$" ] && rm -rf "$LIVE_LOCK"
  return 0
}

live_envcheck() {
  live_devtools || { echo "live: cannot build sc-devtools" >&2; return 1; }
  "$SC_DEVTOOLS" envcheck
  local rc=$?
  if [ "$rc" -eq 3 ]; then
    echo "live: needs an unlocked, awake screen" >&2
  fi
  return "$rc"
}

fixture_start() {
  live_devtools || return 1
  mkdir -p "$LIVE_DIR"
  local out="$LIVE_DIR/fixture.json"
  rm -f "$out"
  "$SC_DEVTOOLS" testapps fixture --out "$out" &
  echo "$!" > "$LIVE_DIR/fixture.pid"
  for _ in $(seq 200); do
    [ -s "$out" ] && break
    kill -0 "$(cat "$LIVE_DIR/fixture.pid")" 2>/dev/null || break
    sleep 0.1
  done
  if [ ! -s "$out" ]; then
    echo "live: fixture did not start (no $out)" >&2
    fixture_stop
    return 1
  fi
  export SC_FIXTURE="$out"
}

fixture_stop() {
  local pid
  pid=$(cat "$LIVE_DIR/fixture.pid" 2>/dev/null)
  if [ -n "$pid" ]; then
    kill -TERM "$pid" 2>/dev/null
    for _ in $(seq 30); do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
    kill -KILL "$pid" 2>/dev/null
  fi
  rm -f "$LIVE_DIR/fixture.pid" "$LIVE_DIR/fixture.json"
  return 0
}

live_main() {
  set -uo pipefail
  if [ $# -eq 0 ]; then
    echo "usage: scripts/lib/live.sh CMD [ARGS...] | fixture start|stop" >&2
    return 2
  fi
  if [ "$1" = fixture ]; then
    case "${2:-}" in
      start) fixture_start && echo "SC_FIXTURE=$SC_FIXTURE" ;;
      stop) fixture_stop ;;
      *) echo "usage: scripts/lib/live.sh fixture start|stop" >&2; return 2 ;;
    esac
    return
  fi
  live_lock || return $?
  trap 'live_unlock' EXIT
  live_envcheck || { local rc=$?; live_unlock; return "$rc"; }
  caffeinate -dimsu -w $$ &
  disown
  local caf=$!
  live_banner START "$*"
  # Async children of a non-interactive shell ignore SIGINT, so both signals forward as TERM.
  "$@" &
  LIVE_CHILD=$!
  trap 'kill -TERM "$LIVE_CHILD" 2>/dev/null' INT TERM HUP
  local rc
  while :; do
    wait "$LIVE_CHILD"
    rc=$?
    kill -0 "$LIVE_CHILD" 2>/dev/null || break
  done
  trap - INT TERM HUP
  live_banner END "exit $rc"
  kill "$caf" 2>/dev/null
  live_unlock
  return "$rc"
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  live_main "$@"
  exit $?
fi
