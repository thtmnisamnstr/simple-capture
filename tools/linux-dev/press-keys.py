#!/usr/bin/env python3
"""Presses key combinations on the GNOME desktop for tests, through Mutter's RemoteDesktop D-Bus
API: the compositor handles them like real keys, so global shortcuts fire. Only press keys the
app has bound; otherwise they reach whatever window has focus.
  press-keys.py Alt+Shift+r [Alt+Shift+p ...]   (--gap S between combinations, default 0.5)
"""

import sys
import time

from gi.repository import Gio

MODIFIERS = {
    "alt": 0xFFE9,
    "shift": 0xFFE1,
    "ctrl": 0xFFE3,
    "control": 0xFFE3,
    "super": 0xFFEB,
    "logo": 0xFFEB,
}
NAMED = {"space": 0x20, "return": 0xFF0D, "enter": 0xFF0D, "escape": 0xFF1B, "tab": 0xFF09}


def keysym(name):
    low = name.lower()
    if low in MODIFIERS:
        return MODIFIERS[low]
    if low in NAMED:
        return NAMED[low]
    if len(name) == 1:
        return ord(low)
    if low.startswith("f") and low[1:].isdigit():
        return 0xFFBE + int(low[1:]) - 1
    raise SystemExit(f"press-keys: unknown key {name!r}")


def main(argv):
    gap = 0.5
    if "--gap" in argv:
        i = argv.index("--gap")
        gap = float(argv[i + 1])
        del argv[i : i + 2]
    combos = argv[1:]
    if not combos:
        print(__doc__, file=sys.stderr)
        return 2
    bus = Gio.bus_get_sync(Gio.BusType.SESSION)
    name = "org.gnome.Mutter.RemoteDesktop"
    rd = Gio.DBusProxy.new_sync(bus, 0, None, name, "/org/gnome/Mutter/RemoteDesktop", name)
    path = rd.call_sync("CreateSession", None, 0, -1, None).unpack()[0]
    session = Gio.DBusProxy.new_sync(bus, 0, None, name, path, name + ".Session")
    session.call_sync("Start", None, 0, -1, None)
    time.sleep(0.3)
    from gi.repository import GLib

    def key(sym, down):
        session.call_sync("NotifyKeyboardKeysym", GLib.Variant("(ub)", (sym, down)), 0, -1, None)

    try:
        for combo in combos:
            syms = [keysym(k) for k in combo.split("+")]
            for s in syms:
                key(s, True)
                time.sleep(0.03)
            for s in reversed(syms):
                key(s, False)
                time.sleep(0.03)
            print(f"press-keys: pressed {combo}")
            time.sleep(gap)
    finally:
        session.call_sync("Stop", None, 0, -1, None)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
