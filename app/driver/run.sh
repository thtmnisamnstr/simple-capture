#!/bin/bash
# App-integration driver runs (M1 plan §8.2, §5.4) under the dev host and the live lock.
#   app/driver/run.sh [FLOW_PREFIX...]   default: every flow
#   env SC_KEEP_MEDIA=1                  keep recorded media (deleted after the checks otherwise)
#   env SC_DRV_CAMERA=ID SC_DRV_MIC=ID   devices for the display run (default: the system defaults)
#   env SC_DRV_PROFILE=release           drive an optimized build (with debug assertions, which the
#                                        driver needs) instead of the debug build
# Output: target/app-driver/<stamp>/ (logs, project metadata, report.json); exit 0 when every
# check in check.py passed.
#
# Everything the app and the fixture touch lives in $TMPDIR/sc-app-driver/<stamp>: a process whose
# responsibility is disclaimed (fixture children) or that runs under the dev host blocks in open(2)
# on a Files-and-Folders consent for ~/Documents, and the repo lives there.
set -uo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
MAIN="$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir)"
MAIN="${MAIN%/.git}"

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$MAIN/target/skia}"
if [ -z "${SC_LIVE_LOCK_OWNER:-}" ]; then
  exec "$ROOT/scripts/lib/live.sh" "$0" "$@"
fi
source "$ROOT/scripts/lib/live.sh"

STAMP="$(date +%Y%m%d-%H%M%S)"
RUN="${TMPDIR:-/tmp}/sc-app-driver/$STAMP"
OUT="$ROOT/target/app-driver/$STAMP"
mkdir -p "$RUN/projects" "$RUN/data" "$RUN/scripts" "$RUN/bin" "$OUT"
export SC_PROJECTS_DIR="$RUN/projects" SC_DATA_DIR="$RUN/data"
echo "app-driver: run folder $RUN (copied to $OUT)" >&2

PROFILE="${SC_DRV_PROFILE:-debug}"
PROFILE_FLAG=""
if [ "$PROFILE" = release ]; then
  PROFILE_FLAG="--release"
  export CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true
fi
(cd "$ROOT" && pnpm --dir ui build >/dev/null && cargo build --locked -q $PROFILE_FLAG -p simple-capture-app) || {
  echo "app-driver: build failed" >&2
  exit 1
}
live_devtools || exit 1
cp "$CARGO_TARGET_DIR/$PROFILE/simple-capture-app" "$SC_DEVTOOLS" "$RUN/bin/" || exit 1
APP="$RUN/bin/simple-capture-app"
export SC_DEVTOOLS="$RUN/bin/sc-devtools"
FFMPEG=""
if [ -x "$MAIN/third_party/out/macos-arm64/ffmpeg" ]; then
  cp "$MAIN/third_party/out/macos-arm64/ffmpeg" "$RUN/bin/ffmpeg" && FFMPEG="$RUN/bin/ffmpeg"
  cp "$MAIN/third_party/out/macos-arm64/ffprobe" "$RUN/bin/ffprobe" 2>/dev/null
  export SC_FFMPEG="$FFMPEG"
fi
cd "$RUN" || exit 1

fixture_start || exit 1
trap 'fixture_stop' EXIT
cp "$SC_FIXTURE" "$RUN/fixture.json"

fx() { python3 -I -c "import json,sys; f=json.load(open(sys.argv[1])); a={x['title']:x for x in f['apps']}; print(eval(sys.argv[2]))" "$RUN/fixture.json" "$1"; }
DISPLAY_ID="$(fx "f['display_id']")"
RED_WID="$(fx "a['SC-TEST-RED']['window_id']")"
GREEN_WID="$(fx "a['SC-TEST-GREEN']['window_id']")"
GREEN_PID="$(fx "a['SC-TEST-GREEN']['pid']")"
CAMERA=""
MIC=""

render() {
  sed -e "s|@DISPLAY@|$DISPLAY_ID|g" -e "s|@RED_WID@|$RED_WID|g" -e "s|@GREEN_WID@|$GREEN_WID|g" \
      -e "s|@GREEN_PID@|$GREEN_PID|g" -e "s|@CAMERA@|$CAMERA|g" -e "s|@MIC@|$MIC|g" \
      -e "s|@TITLE@|${3:-}|g" "$HERE/flows/$1.txt" > "$RUN/scripts/$2.txt"
}

drive() {
  echo "app-driver: flow $1" >&2
  SC_APP_SCRIPT="$RUN/scripts/$1.txt" "$ROOT/scripts/lib/live.sh" \
    "$ROOT/tools/macos-dev-host/run.sh" "$APP" > "$RUN/$1.log" 2>&1
  echo "$?" > "$RUN/$1.rc"
}

# TERM goes to run.sh, which forwards it; KILL goes straight to the app's pid.
drive_signal() {
  local name="$1" sig="$2" pidf="$RUN/$1.pid"
  echo "app-driver: flow $name (SIG$sig)" >&2
  SC_DEV_PID_FILE="$pidf" SC_APP_SCRIPT="$RUN/scripts/$name.txt" \
    "$ROOT/tools/macos-dev-host/run.sh" "$APP" > "$RUN/$name.log" 2>&1 &
  local host=$!
  for _ in $(seq 300); do
    grep -q '"step":' "$RUN/$name.log" 2>/dev/null && break
    sleep 0.1
  done
  sleep 4
  if [ "$sig" = KILL ]; then
    kill -KILL "$(cat "$pidf")" 2>/dev/null
  else
    kill -TERM "$host" 2>/dev/null
  fi
  wait "$host"
  echo "$?" > "$RUN/$name.rc"
}

FILTERS=("$@")
[ ${#FILTERS[@]} -gt 0 ] && export SC_APP_DRIVER_FILTERED=1
wanted() {
  [ ${#FILTERS[@]} -eq 0 ] && return 0
  local f
  for f in "${FILTERS[@]}"; do [[ "$1" == "$f"* ]] && return 0; done
  return 1
}
flow() { wanted "$1" || return 0; render "$1" "$1"; drive "$1"; }

render 01-onboarding 01-onboarding
drive 01-onboarding
DEVS="$(grep -m1 '"line":"cmd list_devices"' "$RUN/01-onboarding.log")"
pick_dev() { python3 -I -c "import json,sys; d=json.loads(sys.argv[1])['result'][sys.argv[2]]; d=[c for c in d if c.get('is_default')] or d; print(d[0]['id'] if d else '')" "$DEVS" "$1" 2>/dev/null; }
CAMERA="${SC_DRV_CAMERA:-$(pick_dev cameras)}"
MIC="${SC_DRV_MIC:-$(pick_dev mics)}"
echo "app-driver: camera=${CAMERA:-none} mic=${MIC:-none}" >&2

for f in 02-still-csp 03-sources 04-switch 05-exclusion 06-export 17-editor 18-diagnostics 19-library 20-pause 21-camera 22-silence 07-sleep 12-preview 14-preview-capture 08-quit-recording; do
  flow "$f"
done
if wanted 08-quit-recording; then
  render 10-relaunch 08b-relaunch
  drive 08b-relaunch
fi
if wanted 09-sigterm; then
  render 09-signal-recording 09-sigterm sigterm
  drive_signal 09-sigterm TERM
fi
if wanted 10-kill; then
  render 09-signal-recording 10-kill kill
  drive_signal 10-kill KILL
  render 10-relaunch 10b-relaunch
  drive 10b-relaunch
fi
flow 11-quit-export
[ ${#FILTERS[@]} -gt 0 ] && wanted 16-soak && flow 16-soak
sleep 1
[ -n "$FFMPEG" ] && pgrep -fl "$FFMPEG" > "$RUN/ffmpeg-after.txt" 2>/dev/null

fixture_stop
trap - EXIT
python3 -I "$HERE/check.py" "$RUN"
rc=$?
if [ "${SC_KEEP_MEDIA:-0}" != 1 ]; then
  find "$RUN/projects" -type f \( -name '*.mov' -o -name '*.mp4' -o -name '*.m4a' -o -name '*.wav' -o -name '*.part' \) -delete
  find "$RUN/projects" -type d -name thumbs -prune -exec rm -rf {} +
  rm -f "$RUN/bin/simple-capture-app" "$RUN/bin/sc-devtools" "$RUN/bin/ffmpeg" "$RUN/bin/ffprobe"
fi
rsync -a --exclude bin --exclude thumbs --exclude '*.mov' --exclude '*.mp4' --exclude '*.m4a' --exclude '*.wav' "$RUN/" "$OUT/"
echo "app-driver: report $OUT/report.json (exit $rc)" >&2
exit "$rc"
