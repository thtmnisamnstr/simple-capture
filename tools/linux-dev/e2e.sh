#!/usr/bin/env bash
# Drives the debug app through a driver flow (app/src/driver.rs) inside tools/linux-dev/headless.sh:
# the portal picks the virtual screen without a dialog, a moving test pattern plays on it, and the
# app's settings, projects and log live in target/linux-e2e/<stamp>. Prints the failed steps.
#   cargo build -p simple-capture-app && tools/linux-dev/e2e.sh tools/linux-dev/flows/editor.txt
# The test pattern needs gstreamer1.0-plugins-bad (waylandsink); without it the screen is static.
# SC_E2E_DATA=dir keeps the app's data folder (settings, restore token) across runs.
set -euo pipefail
cd "$(dirname "$0")/../.."
FLOW=$(realpath "${1:?usage: tools/linux-dev/e2e.sh FLOW}")
APP=${SC_E2E_APP:-$PWD/target/debug/simple-capture-app}
OUT=$PWD/target/linux-e2e/$(date +%Y%m%d-%H%M%S)
DATA=${SC_E2E_DATA:-$OUT/data}
mkdir -p "$DATA" "$OUT/projects"
cat > "$OUT/inside.sh" <<EOF
#!/usr/bin/env bash
gst-launch-1.0 -q videotestsrc pattern=ball is-live=true ! video/x-raw,width=1280,height=720,framerate=30/1 \\
  ! waylandsink fullscreen=false > "$OUT/pattern.log" 2>&1 &
pattern=\$!
trap 'kill \$pattern 2>/dev/null' EXIT
sleep 2
SC_DATA_DIR="$DATA" SC_PROJECTS_DIR="$OUT/projects" SC_APP_SCRIPT="$FLOW" RUST_LOG=\${RUST_LOG:-info} \\
  timeout \${SC_E2E_TIMEOUT:-900} "$APP" > "$OUT/app.out" 2> "$OUT/app.log"
echo "e2e: app exit \$?" >&2
EOF
chmod +x "$OUT/inside.sh"
tools/linux-dev/headless.sh "$OUT/inside.sh"
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
