#!/usr/bin/env bash
# On a Linux desktop host (no container): a private headless sway session with its own runtime,
# config and state folders, D-Bus bus, PipeWire, WirePlumber (hardware monitors off, so the host's
# sound devices are left alone, plus a null sink as the default output) and
# xdg-desktop-portal-wlr picking its only output without a dialog. Nothing touches the user's
# session or takes focus. Then runs "$@" inside it.
#   tools/linux-dev/headless.sh cargo test -p simple-capture-capture-linux --test backend -- --ignored portal
#   tools/linux-dev/e2e.sh tools/linux-dev/flows/editor.txt      (the app, driven end to end)
# Needs: sway, xdg-desktop-portal, xdg-desktop-portal-wlr, pipewire, wireplumber, dbus-daemon.
#   XDPW=path   xdg-desktop-portal-wlr binary   XDP=path   xdg-desktop-portal binary
#   SC_HL_PORTALS=dir   the folder holding wlr.portal
set -euo pipefail
XDPW=${XDPW:-/usr/libexec/xdg-desktop-portal-wlr}
XDP=${XDP:-/usr/libexec/xdg-desktop-portal}
BASE="/run/user/$(id -u)/sc-headless"
fusermount3 -u "$BASE/rt/doc" 2>/dev/null || true
# SC_HL_KEEP=1 keeps the private data, config and state (portal restore data, shortcut
# grants) from the previous run, to test what a relaunch remembers.
if [ "${SC_HL_KEEP:-0}" = 1 ]; then rm -rf "$BASE/rt"; else rm -rf "$BASE"; fi
mkdir -p "$BASE"/{rt,cfg,state,data/applications}
chmod 700 "$BASE/rt"
export XDG_RUNTIME_DIR="$BASE/rt" XDG_CONFIG_HOME="$BASE/cfg" XDG_STATE_HOME="$BASE/state" XDG_DATA_HOME="$BASE/data"
# The portals accept the app ID only with a desktop file whose Exec program exists.
sed "s#^Exec=.*#Exec=${SC_APP:-$(cd "$(dirname "$0")/../.." && pwd)/target/debug/simple-capture-app}#" \
  "$(dirname "$0")/../../app/linux/io.github.thtmnisamnstr.simple-capture.desktop" \
  > "$XDG_DATA_HOME/applications/io.github.thtmnisamnstr.simple-capture.desktop"
export XDG_CURRENT_DESKTOP=sway XDG_SESSION_TYPE=wayland
export XDG_DESKTOP_PORTAL_DIR="${SC_HL_PORTALS:-/usr/share/xdg-desktop-portal/portals}"
unset DISPLAY WAYLAND_DISPLAY DBUS_SESSION_BUS_ADDRESS PIPEWIRE_REMOTE
pids=()
cleanup() {
  # Services the private bus activated (the document portal, the permission store) are its
  # children; the document portal also leaves a FUSE mount in the runtime folder.
  [ -f "$BASE/dbus.pid" ] && pkill -P "$(cat "$BASE/dbus.pid")" 2>/dev/null || true
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
  wait 2>/dev/null || true
  fusermount3 -u "$BASE/rt/doc" 2>/dev/null || true
}
trap cleanup EXIT
DBUS_SESSION_BUS_ADDRESS=$(dbus-daemon --session --fork --print-address=1 --print-pid=3 3>"$BASE/dbus.pid")
export DBUS_SESSION_BUS_ADDRESS
pids+=("$(cat "$BASE/dbus.pid")")
mkdir -p "$XDG_CONFIG_HOME"/{xdg-desktop-portal-wlr,xdg-desktop-portal,sway,wireplumber/wireplumber.conf.d}
printf '[screencast]\nchooser_type=none\noutput_name=HEADLESS-1\nmax_fps=30\n' \
  > "$XDG_CONFIG_HOME/xdg-desktop-portal-wlr/config"
printf '[preferred]\ndefault=wlr\norg.freedesktop.impl.portal.ScreenCast=wlr\n' \
  > "$XDG_CONFIG_HOME/xdg-desktop-portal/sway-portals.conf"
printf 'output HEADLESS-1 resolution 1280x720 bg #336699 solid_color\n' > "$XDG_CONFIG_HOME/sway/config"
printf 'wireplumber.profiles = {\n  main = {\n    monitor.alsa = disabled\n    monitor.alsa-midi = disabled\n    monitor.bluez = disabled\n    monitor.v4l2 = disabled\n    monitor.libcamera = disabled\n  }\n}\n' \
  > "$XDG_CONFIG_HOME/wireplumber/wireplumber.conf.d/90-sc-headless.conf"
WLR_BACKENDS=headless WLR_HEADLESS_OUTPUTS=1 WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 \
  sway --unsupported-gpu > "$BASE/sway.log" 2>&1 & pids+=($!)
sock=
for _ in $(seq 50); do
  for s in "$XDG_RUNTIME_DIR"/wayland-[0-9]*; do [[ $s == *.lock ]] || { [ -S "$s" ] && sock=${s##*/}; }; done
  [ -n "$sock" ] && break
  sleep 0.1
done
export WAYLAND_DISPLAY=${sock:?sway did not start: $(tail -5 "$BASE/sway.log")}
dbus-update-activation-environment WAYLAND_DISPLAY XDG_CURRENT_DESKTOP XDG_RUNTIME_DIR XDG_CONFIG_HOME XDG_DESKTOP_PORTAL_DIR
pipewire > "$BASE/pipewire.log" 2>&1 & pids+=($!)
sleep 0.5
wireplumber > "$BASE/wireplumber.log" 2>&1 & pids+=($!)
sleep 0.5
pw-cli create-node adapter '{ factory.name=support.null-audio-sink node.name=sc-null-sink node.description="SC null sink" media.class=Audio/Sink object.linger=true audio.position=[FL,FR] }' > /dev/null
"$XDPW" -l DEBUG > "$BASE/xdpw.log" 2>&1 & pids+=($!)
sleep 0.5
"$XDP" -r > "$BASE/xdp.log" 2>&1 & pids+=($!)
sleep 1.5
echo "headless: WAYLAND_DISPLAY=$WAYLAND_DISPLAY runtime=$XDG_RUNTIME_DIR logs=$BASE" >&2
"$@"
