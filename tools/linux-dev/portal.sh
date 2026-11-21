#!/usr/bin/env bash
# Inside the linux-dev container: a headless sway session with PipeWire, WirePlumber and
# xdg-desktop-portal(-wlr) that picks the only output without asking, then runs "$@".
# Example: tools/linux-dev/run.sh tools/linux-dev/portal.sh cargo test -p simple-capture-capture-linux -- --ignored
set -euo pipefail
export XDG_RUNTIME_DIR=/tmp/xdg-runtime
mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR"
export XDG_CURRENT_DESKTOP=sway XDG_SESSION_TYPE=wayland
DBUS_SESSION_BUS_ADDRESS=$(dbus-daemon --session --fork --print-address=1)
export DBUS_SESSION_BUS_ADDRESS
mkdir -p ~/.config/xdg-desktop-portal-wlr ~/.config/xdg-desktop-portal ~/.config/sway
printf '[screencast]\nchooser_type=none\noutput_name=HEADLESS-1\nmax_fps=30\n' \
  > ~/.config/xdg-desktop-portal-wlr/config
printf '[preferred]\ndefault=wlr\norg.freedesktop.impl.portal.ScreenCast=wlr\n' \
  > ~/.config/xdg-desktop-portal/sway-portals.conf
printf 'output HEADLESS-1 resolution 1280x720 bg #336699 solid_color\n' > ~/.config/sway/config
WLR_BACKENDS=headless WLR_HEADLESS_OUTPUTS=1 WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 \
  sway --unsupported-gpu > /tmp/sway.log 2>&1 &
for _ in $(seq 50); do
  sock=
  for s in "$XDG_RUNTIME_DIR"/wayland-[0-9]*; do
    [[ $s == *.lock ]] || { [ -S "$s" ] && sock=${s##*/}; }
  done
  [ -n "$sock" ] && break
  sleep 0.1
done
export WAYLAND_DISPLAY=${sock:?sway did not start: $(tail -5 /tmp/sway.log)}
dbus-update-activation-environment WAYLAND_DISPLAY XDG_CURRENT_DESKTOP XDG_RUNTIME_DIR
pipewire > /tmp/pipewire.log 2>&1 &
sleep 0.5
wireplumber > /tmp/wireplumber.log 2>&1 &
sleep 0.5
/usr/libexec/xdg-desktop-portal-wlr -l DEBUG > /tmp/xdpw.log 2>&1 &
sleep 0.5
/usr/libexec/xdg-desktop-portal -r > /tmp/xdp.log 2>&1 &
sleep 1.5
gst-launch-1.0 -q videotestsrc pattern=smpte is-live=true ! video/x-raw,format=BGRx,width=1280,height=720,framerate=30/1 \
  ! pipewiresink stream-properties="p,node.name=sc-test-source,media.class=Video/Source" > /tmp/gst.log 2>&1 &
for _ in $(seq 30); do
  SC_PW_TEST_NODE=$(pw-cli ls Node 2>/dev/null | awk '/^[ \t]*id [0-9]+/{id=$2} /node.name = "sc-test-source"/{print id; exit}' | tr -d ,)
  [ -n "$SC_PW_TEST_NODE" ] && break
  sleep 0.2
done
export SC_PW_TEST_NODE
"$@" || { s=$?; echo "--- xdpw"; grep -v "^$" /tmp/xdpw.log | tail -30; echo "--- xdp"; tail -20 /tmp/xdp.log; exit $s; }
