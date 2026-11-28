#!/usr/bin/env bash
# Makes the AppStream screenshots in app/linux/screenshots: the debug app in a private headless
# KDE session (Breeze frames, kde-headless.sh) recording a made-up slide (cropped from the App Store
# screenshots), driven by flows/screenshots.txt, each window taken by kde-shot.py.
#   cargo build -p simple-capture-app && tools/linux-dev/screenshots.sh
# KDE_PREFIX and the KDE environment as for kde-headless.sh.
set -euo pipefail
cd "$(dirname "$0")/../.."
ROOT=$PWD
DEST=$ROOT/app/linux/screenshots
WORK=$ROOT/target/linux-screenshots
rm -rf "$WORK"
mkdir -p "$WORK" "$DEST"
gst-launch-1.0 -q filesrc location=appstore/screenshots/en-US/02-edit.png ! pngdec \
  ! videocrop left=442 right=1374 top=500 bottom=700 ! videoconvert ! pngenc snapshot=true \
  ! filesink location="$WORK/slide.png"
cat > "$WORK/shots.sh" <<SHOTS
#!/usr/bin/env bash
log=
for _ in \$(seq 100); do
  log=\$(ls -t "$ROOT"/target/linux-e2e/*/app.log 2>/dev/null | head -n 1)
  [ -n "\$log" ] && [ "\$log" -nt "$WORK/slide.png" ] && break
  sleep 0.2
done
tail -n +1 -F "\$log" 2>/dev/null | while IFS= read -r line; do
  [[ \$line == '{"line":"eval '*'SHOT '* ]] || continue
  spec=\${line#*SHOT }; spec=\${spec%% \*/*}
  python3 -I "$ROOT/tools/linux-dev/kde-shot.py" list
  python3 -I "$ROOT/tools/linux-dev/kde-shot.py" "\${spec#*|}" "$DEST/\${spec%%|*}.png"
done
SHOTS
chmod +x "$WORK/shots.sh"
# The Export sheet shows the projects folder: keep the owner's name out of it.
rm -rf /tmp/simple-capture-demo
SC_E2E_PROJECTS=/tmp/simple-capture-demo/Videos/simple-capture \
  SC_KDE_WIDTH=1920 SC_KDE_HEIGHT=1080 SC_E2E_SESSION=kde SC_E2E_APPROVE=1 SC_E2E_TIMEOUT=300 \
  SC_E2E_ALONGSIDE="$WORK/shots.sh" \
  SC_E2E_SCREEN="filesrc location=$WORK/slide.png ! pngdec ! imagefreeze is-live=true ! videoconvert ! waylandsink fullscreen=true" \
  tools/linux-dev/e2e.sh tools/linux-dev/flows/screenshots.txt
rm -rf /tmp/simple-capture-demo
ls -l "$DEST"
