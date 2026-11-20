set -uo pipefail
DS="$1"; MODE="$2"; TAG="$3"
OUT=/work/results
BIN=/target/release/native-preview-linux-spike
export SC_MODE="$MODE" SC_HIDE_WEB="${SC_HIDE_WEB:-}" SC_MARGIN="${SC_MARGIN:-0}" SC_SECONDS="${SC_SECONDS:-14}" NO_AT_BRIDGE=1
export XDG_RUNTIME_DIR=/tmp/xdg; mkdir -p -m 700 "$XDG_RUNTIME_DIR"
LOG="$OUT/$TAG.log"; : > "$LOG"

cpu_ticks() {
  for p in $(pgrep -f 'native-preview-linux-spike|WebKitWebProcess|WebKitNetworkProcess|WebKitGPUProcess|Xvfb|weston'); do
    [ -r /proc/$p/stat ] || continue
    awk -v p=$p '{c=$2; sub(/^.*\) /,""); split($0,a," "); print p, c, a[12]+a[13]}' /proc/$p/stat
  done
}

shot() {
  if [ "$DS" = x11 ]; then
    import -display :99 -window root "$1" 2>>"$LOG"
  else
    (cd /tmp && rm -f wayland-screenshot-*.png && weston-screenshooter >>"$LOG" 2>&1; mv /tmp/wayland-screenshot-*.png "$1" 2>>"$LOG")
  fi
}

if [ "$DS" = x11 ]; then
  Xvfb :99 -screen 0 1400x900x24 >/dev/null 2>&1 &
  export DISPLAY=:99 GDK_BACKEND=x11
  unset WAYLAND_DISPLAY
  sleep 1
else
  weston --backend=headless --renderer=pixman --width=1400 --height=900 --socket=wayland-1 --debug \
    --shell="${SC_SHELL:-kiosk}-shell.so" >"$OUT/$TAG.weston.log" 2>&1 &
  export WAYLAND_DISPLAY=wayland-1 GDK_BACKEND=wayland
  unset DISPLAY
  for i in $(seq 1 50); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
fi

{ echo "== vulkaninfo summary"; vulkaninfo --summary 2>/dev/null | sed -n '/Devices:/,$p' | head -20; } >>"$LOG"
[ -n "${SC_URL:-}" ] && export SC_URL
dbus-run-session -- "$BIN" >>"$LOG" 2>&1 &
APP=$!
sleep 5
cpu_ticks > /tmp/cpu1; T1=$(date +%s.%N)
sleep 3
cpu_ticks > /tmp/cpu2; T2=$(date +%s.%N)
shot "$OUT/$TAG.png"
sleep 1.5
shot "$OUT/$TAG-b.png"
[ "$DS" = x11 ] && { echo "== xwininfo -root -tree"; xwininfo -display :99 -root -tree; } >>"$LOG" 2>&1
HZ=$(getconf CLK_TCK)
echo "== CPU over a 3 s window, % of one core (SOFTWARE RENDERING, not representative)" >>"$LOG"
join <(sort /tmp/cpu1) <(sort /tmp/cpu2) | awk -v hz="$HZ" -v t1="$T1" -v t2="$T2" '{printf "%s %s %.1f%%\n", $1, $2, ($5-$3)/hz/(t2-t1)*100}' >>"$LOG"
wait $APP
echo "== exit $?" >>"$LOG"
