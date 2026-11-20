#!/bin/bash
# just killtest [--case NAME]...: crash safety under the dev host (M1 plan §8.3
# wp-kill-recovery-test). SC_KILLTEST_RUNS=N (default 3); SC_KILLTEST_KEEP=1 keeps the recordings.
#
# Builds sc-devtools and a `simple-capture` with test-hooks and without export in a private target
# dir (other worktrees sharing the Skia target replace same-named binaries there; neither needs
# Skia), and the normal `simple-capture` in $CARGO_TARGET_DIR for recover/export. Then, inside
# scripts/lib/live.sh, `sc-devtools killtest live` SIGKILLs recordings at each kill point and
# recovers, probes and exports them.
# The CI version is `cargo test --locked -p simple-capture-record --features test-hooks --test kill_synth`.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
case "$TARGET_DIR" in /*) ;; *) TARGET_DIR="$ROOT/$TARGET_DIR" ;; esac
MAIN_ROOT="$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)"
MAIN_ROOT="${MAIN_ROOT%/.git}"
[ -d "$MAIN_ROOT" ] || MAIN_ROOT="$ROOT"

if [ "$(uname -s)" != Darwin ]; then
  echo "SKIPPED: the dev-host kill test needs macOS in M1"
  exit 0
fi

if [ -z "${SC_KILLTEST_INNER:-}" ]; then
  HOOKS_TARGET="$MAIN_ROOT/target/wt/killtest-hooks"
  cargo build --locked -p simple-capture-cli || exit 1
  CARGO_TARGET_DIR="$HOOKS_TARGET" cargo build --locked -p sc-devtools || exit 1
  CARGO_TARGET_DIR="$HOOKS_TARGET" cargo build --locked -p simple-capture-cli \
    --no-default-features --features test-hooks || exit 1
  # Private copies outside ~/Documents (see scripts/e2e/run.sh); a new inode on every change.
  BIN="${TMPDIR:-/tmp}/sc-killtest-bin"
  mkdir -p "$BIN"
  stage() {
    if ! cmp -s "$1" "$BIN/$2"; then
      cp "$1" "$BIN/$2.new" && mv -f "$BIN/$2.new" "$BIN/$2" || exit 1
    fi
  }
  stage "$HOOKS_TARGET/debug/sc-devtools" sc-devtools
  stage "$TARGET_DIR/debug/simple-capture" simple-capture
  stage "$HOOKS_TARGET/debug/simple-capture" simple-capture-hooks
  export SC_DEVTOOLS="$BIN/sc-devtools"
  export SC_KILLTEST_CLI="$BIN/simple-capture"
  export SC_KILLTEST_HOOK_CLI="$BIN/simple-capture-hooks"
  export SC_KILLTEST_INNER=1
  for b in "$SC_DEVTOOLS" "$SC_KILLTEST_CLI" "$SC_KILLTEST_HOOK_CLI"; do
    "$b" --help >/dev/null 2>&1
  done
  exec "$ROOT/scripts/lib/live.sh" "$0" "$@"
fi

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
    echo "killtest: no $kind helper (run \`just helpers\` or set $var)" >&2
    exit 1
  fi
done

WORK="$(mktemp -d "${TMPDIR:-/tmp}/sc-killtest.XXXXXX")"
cleanup() {
  if [ "${SC_KILLTEST_KEEP:-0}" = 1 ]; then
    echo "killtest: kept recordings in $WORK"
  else
    rm -rf "$WORK"
  fi
}
trap cleanup EXIT
REPORT="$ROOT/target/killtest/report-$(date +%Y%m%d-%H%M%S).json"
"$SC_DEVTOOLS" killtest live --runsh "$ROOT/tools/macos-dev-host/run.sh" \
  --hook-cli "$SC_KILLTEST_HOOK_CLI" --cli "$SC_KILLTEST_CLI" --work "$WORK" \
  --runs "${SC_KILLTEST_RUNS:-3}" --report "$REPORT" "$@" &
CHILD=$!
trap 'kill -TERM "$CHILD" 2>/dev/null' INT TERM HUP
while :; do
  wait "$CHILD"
  RC=$?
  kill -0 "$CHILD" 2>/dev/null || break
done
exit "$RC"
