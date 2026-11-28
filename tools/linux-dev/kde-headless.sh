#!/usr/bin/env bash
# The KDE twin of headless.sh: a private headless KWin session (own runtime, config, data and
# state folders, D-Bus bus, PipeWire, WirePlumber with hardware monitors off, a null sink,
# kglobalacceld, xdg-desktop-portal-kde, the accessibility bus) that never touches the user's
# session, then runs "$@" inside it. KDE's portal shows its dialogs here: approve them with
# tools/linux-dev/approve-portal.py (AT-SPI), and trigger shortcuts with qdbus6/gdbus on
# org.kde.kglobalaccel.
# Needs kwin-wayland, xdg-desktop-portal-kde, kglobalacceld, the Qt QML modules Kirigami needs,
# pipewire, wireplumber, at-spi2-core. Paths to KDE's helpers are found under the libexec folders.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
find_exe() {
  local name=$1
  for d in ${KDE_PREFIX:-} /usr ; do
    for sub in libexec lib/libexec lib/x86_64-linux-gnu/libexec lib/aarch64-linux-gnu/libexec bin; do
      [ -x "$d/$sub/$name" ] && { echo "$d/$sub/$name"; return; }
    done
  done
  command -v "$name" || { echo "kde-headless: $name not found" >&2; exit 1; }
}
KWIN=$(command -v kwin_wayland)
XDPK=$(find_exe xdg-desktop-portal-kde)
KGA=$(find_exe kglobalacceld)
XDP=$(find_exe xdg-desktop-portal)
ATSPI=$(find_exe at-spi-bus-launcher)
BASE="/run/user/$(id -u)/sc-kde"
fusermount3 -u "$BASE/rt/doc" 2>/dev/null || true
# SC_HL_KEEP=1 keeps the private data, config and state (portal restore data, shortcut
# grants) from the previous run, to test what a relaunch remembers.
if [ "${SC_HL_KEEP:-0}" = 1 ]; then rm -rf "$BASE/rt"; else rm -rf "$BASE"; fi
mkdir -p "$BASE"/{rt,cfg,state,cache,data/applications}
chmod 700 "$BASE/rt"
export XDG_RUNTIME_DIR="$BASE/rt" XDG_CONFIG_HOME="$BASE/cfg" XDG_STATE_HOME="$BASE/state" XDG_DATA_HOME="$BASE/data" \
  XDG_CACHE_HOME="$BASE/cache"
export XDG_CURRENT_DESKTOP=KDE XDG_SESSION_TYPE=wayland KDE_FULL_SESSION=true
export QT_LINUX_ACCESSIBILITY_ALWAYS_ON=1 QT_ACCESSIBILITY=1
export XDG_DESKTOP_PORTAL_DIR="${SC_HL_PORTALS:-${KDE_PREFIX:-/usr}/share/xdg-desktop-portal/portals}"
unset DISPLAY WAYLAND_DISPLAY DBUS_SESSION_BUS_ADDRESS PIPEWIRE_REMOTE
pids=()
cleanup() {
  [ -f "$BASE/dbus.pid" ] && pkill -P "$(cat "$BASE/dbus.pid")" 2>/dev/null || true
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
  wait 2>/dev/null || true
  fusermount3 -u "$BASE/rt/doc" 2>/dev/null || true
}
trap cleanup EXIT
# D-Bus activates the portal backends (KWin itself calls the portal at startup), so point the
# services at the real binaries; dbus-daemon only watches service folders that exist when it starts.
mkdir -p "$XDG_DATA_HOME/dbus-1/services"
mkdir -p "$BASE/bin"
service() {
  printf '#!/bin/sh\nexec %s %s >> %s 2>&1\n' "$2" "${3:-}" "$BASE/$1.log" > "$BASE/bin/$1"
  chmod +x "$BASE/bin/$1"
  printf '[D-BUS Service]\nName=%s\nExec=%s\n' "$1" "$BASE/bin/$1" > "$XDG_DATA_HOME/dbus-1/services/$1.service"
}
service org.freedesktop.impl.portal.desktop.kde "$XDPK"
service org.kde.kglobalaccel "$KGA"
service org.freedesktop.portal.Desktop "$XDP" -r
DBUS_SESSION_BUS_ADDRESS=$(dbus-daemon --session --fork --print-address=1 --print-pid=3 3>"$BASE/dbus.pid")
export DBUS_SESSION_BUS_ADDRESS
pids+=("$(cat "$BASE/dbus.pid")")
mkdir -p "$XDG_CONFIG_HOME"/{xdg-desktop-portal,wireplumber/wireplumber.conf.d}
printf '[preferred]\ndefault=kde\n' > "$XDG_CONFIG_HOME/xdg-desktop-portal/kde-portals.conf"
printf 'wireplumber.profiles = {\n  main = {\n    monitor.alsa = disabled\n    monitor.alsa-midi = disabled\n    monitor.bluez = disabled\n    monitor.v4l2 = disabled\n    monitor.libcamera = disabled\n  }\n}\n' \
  > "$XDG_CONFIG_HOME/wireplumber/wireplumber.conf.d/90-sc-headless.conf"
# The portals accept the app ID only with a desktop file whose Exec program exists.
sed "s#^Exec=.*#Exec=${SC_APP:-$(cd "$(dirname "$0")/../.." && pwd)/target/debug/simple-capture-app}#" \
  "$(dirname "$0")/../../app/linux/io.github.thtmnisamnstr.SimpleCapture.desktop" \
  > "$XDG_DATA_HOME/applications/io.github.thtmnisamnstr.SimpleCapture.desktop"
export WAYLAND_DISPLAY=wayland-sc
dbus-update-activation-environment WAYLAND_DISPLAY XDG_CURRENT_DESKTOP XDG_RUNTIME_DIR XDG_CONFIG_HOME \
  XDG_CACHE_HOME XDG_DATA_HOME XDG_DATA_DIRS XDG_CONFIG_DIRS XDG_DESKTOP_PORTAL_DIR QT_LINUX_ACCESSIBILITY_ALWAYS_ON \
  QT_ACCESSIBILITY KDE_FULL_SESSION ${LD_LIBRARY_PATH:+LD_LIBRARY_PATH} \
  ${QT_PLUGIN_PATH:+QT_PLUGIN_PATH} ${QML_IMPORT_PATH:+QML_IMPORT_PATH} ${QML2_IMPORT_PATH:+QML2_IMPORT_PATH} PATH
dbus-update-activation-environment QT_QPA_PLATFORM=wayland QT_QUICK_BACKEND=software
"$ATSPI" --launch-immediately > "$BASE/atspi.log" 2>&1 & pids+=($!)
for _ in $(seq 50); do
  AT_SPI_BUS_ADDRESS=$(gdbus call --session --dest org.a11y.Bus --object-path /org/a11y/bus \
    --method org.a11y.Bus.GetAddress 2>/dev/null | sed -E "s/^\('(.*)',\)$/\1/") && [ -n "$AT_SPI_BUS_ADDRESS" ] && break
  sleep 0.1
done
export AT_SPI_BUS_ADDRESS
dbus-update-activation-environment AT_SPI_BUS_ADDRESS
pipewire > "$BASE/pipewire.log" 2>&1 & pids+=($!)
sleep 0.5
wireplumber > "$BASE/wireplumber.log" 2>&1 & pids+=($!)
sleep 0.5
pw-cli create-node adapter '{ factory.name=support.null-audio-sink node.name=sc-null-sink node.description="SC null sink" media.class=Audio/Sink object.linger=true audio.position=[FL,FR] }' > /dev/null
# KWin grants privileged protocols (screencast) to clients its service cache trusts; this
# private session has no cache, so the check is lifted here (a test switch KWin provides), and
# likewise for window screenshots (tools/linux-dev/kde-shot.py).
env -u WAYLAND_DISPLAY -u QT_LINUX_ACCESSIBILITY_ALWAYS_ON -u QT_ACCESSIBILITY KWIN_WAYLAND_NO_PERMISSION_CHECKS=1 \
  KWIN_SCREENSHOT_NO_PERMISSION_CHECKS=1 \
  ${SC_KWIN_WRAP:-} "$KWIN" --virtual --width "${SC_KDE_WIDTH:-1280}" --height "${SC_KDE_HEIGHT:-720}" \
  --no-lockscreen --socket wayland-sc > "$BASE/kwin.log" 2>&1 & pids+=($!)
for _ in $(seq 200); do [ -S "$XDG_RUNTIME_DIR/wayland-sc" ] && break; sleep 0.1; done
[ -S "$XDG_RUNTIME_DIR/wayland-sc" ] || { echo "kde-headless: KWin did not start: $(tail -3 "$BASE/kwin.log")" >&2; exit 1; }
export QT_QPA_PLATFORM=wayland
for name in org.kde.kglobalaccel org.freedesktop.impl.portal.desktop.kde; do
  gdbus call --session --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus \
    --method org.freedesktop.DBus.StartServiceByName "$name" 0 > /dev/null 2>&1 || true
done
# KWin's early portal call may have started the frontend before the backend could run; restart it.
for p in $(pgrep -f "^$XDP( |$)" 2>/dev/null); do
  tr '\0' '\n' < "/proc/$p/environ" 2>/dev/null | grep -q "^XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR$" && kill "$p"
done
sleep 0.3
gdbus call --session --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus \
  --method org.freedesktop.DBus.StartServiceByName org.freedesktop.portal.Desktop 0 > /dev/null 2>&1 || true
sleep 1
echo "kde-headless: WAYLAND_DISPLAY=$WAYLAND_DISPLAY runtime=$XDG_RUNTIME_DIR logs=$BASE" >&2
"$@"
