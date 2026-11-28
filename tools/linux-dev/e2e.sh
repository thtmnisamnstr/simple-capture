#!/usr/bin/env bash
# Drives the debug app through a driver flow (app/src/driver.rs) inside tools/linux-dev/headless.sh:
# the portal picks the virtual screen without a dialog, a moving test pattern plays on it, and the
# app's settings, projects and log live in target/linux-e2e/<stamp>. Prints the failed steps.
#   cargo build -p simple-capture-app && tools/linux-dev/e2e.sh tools/linux-dev/flows/editor.txt
# The test pattern needs gstreamer1.0-plugins-bad (waylandsink); without it the screen is static.
# SC_E2E_DATA=dir keeps the app's data folder (settings, restore token) across runs.
# SC_E2E_PROJECTS=dir is the projects folder (default: the run's own).
# SC_E2E_SESSION=kde runs it in tools/linux-dev/kde-headless.sh (KDE's portal shows its dialogs);
# SC_E2E_APPROVE=1 approves the app's portal dialogs there through AT-SPI (approve-portal.py).
# SC_E2E_ALONGSIDE=script runs a script inside the session next to the app (to press shortcuts).
# SC_E2E_SCREEN='gst-launch pipeline' replaces the test pattern on the screen.
set -euo pipefail
cd "$(dirname "$0")/../.."
FLOW=$(realpath "${1:?usage: tools/linux-dev/e2e.sh FLOW}")
APP=${SC_E2E_APP:-$PWD/target/debug/simple-capture-app}
OUT=$PWD/target/linux-e2e/$(date +%Y%m%d-%H%M%S)
DATA=${SC_E2E_DATA:-$OUT/data}
PROJECTS=${SC_E2E_PROJECTS:-$OUT/projects}
mkdir -p "$DATA" "$PROJECTS"
cat > "$OUT/inside.sh" <<EOF
#!/usr/bin/env bash
gst-launch-1.0 -q \${SC_E2E_SCREEN:-videotestsrc pattern=ball is-live=true ! video/x-raw,width=1280,height=720,framerate=30/1 \\
  ! waylandsink fullscreen=false} > "$OUT/pattern.log" 2>&1 &
pattern=\$!
trap 'kill \$pattern 2>/dev/null' EXIT
approve=
if [ -n "\${SC_E2E_ALONGSIDE:-}" ]; then "\$SC_E2E_ALONGSIDE" > "$OUT/alongside.log" 2>&1 & fi
if [ "\${SC_E2E_APPROVE:-0}" = 1 ]; then
  python3 -u -I "$PWD/tools/linux-dev/approve-portal.py" all --timeout "\${SC_E2E_APPROVE_S:-120}" > "$OUT/approve.log" 2>&1 &
  approve=\$!
fi
trap 'kill \$pattern \$approve 2>/dev/null' EXIT
sleep 2
SC_DATA_DIR="$DATA" SC_PROJECTS_DIR="$PROJECTS" SC_APP_SCRIPT="$FLOW" RUST_LOG=\${RUST_LOG:-info} \\
  timeout \${SC_E2E_TIMEOUT:-900} "$APP" > "$OUT/app.out" 2> "$OUT/app.log"
echo "e2e: app exit \$?" >&2
EOF
chmod +x "$OUT/inside.sh"
session=tools/linux-dev/headless.sh
[ "${SC_E2E_SESSION:-sway}" = kde ] && session=tools/linux-dev/kde-headless.sh
"$session" "$OUT/inside.sh"
echo "e2e: output in $OUT" >&2
python3 -I - "$OUT/app.log" <<'PY'
import json, sys
failed = 0
for line in open(sys.argv[1], errors="replace"):
    if not line.startswith('{"'):
        continue
    try:
        j = json.loads(line)
    except ValueError:
        continue
    if "line" in j and not j.get("ok"):
        failed += 1
        print(f"FAILED step {j.get('step')}: {j['line'][:90]} -> {j.get('error', '')[:160]}")
print(f"e2e: {failed} failed step(s)")
PY
