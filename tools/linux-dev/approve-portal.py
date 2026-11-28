#!/usr/bin/env python3
"""Approves this app's GNOME portal dialogs in dev and test runs (CLAUDE.md: consent dialogs).

Waits for xdg-desktop-portal-gnome's dialog, then acts through the accessibility bus (AT-SPI):
  approve-portal.py share [--window]   pick the first screen (or window) and press Share
  approve-portal.py shortcuts          accept the global-shortcuts dialog
  approve-portal.py dump               print the portal's accessibility tree
Options: --timeout S (default 60).
"""

import sys
import time

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402

PORTAL_APPS = ("xdg-desktop-portal-gnome", "gnome-control-center-global-shortcuts-provider", "gnome-shell")


def apps():
    desktop = Atspi.get_desktop(0)
    for i in range(desktop.get_child_count()):
        app = desktop.get_child_at_index(i)
        if app is not None and (app.get_name() or "") in PORTAL_APPS:
            yield app


def walk(node, depth=0):
    yield node, depth
    try:
        n = node.get_child_count()
    except Exception:
        return
    for i in range(n):
        child = node.get_child_at_index(i)
        if child is not None:
            yield from walk(child, depth + 1)


def role(node):
    try:
        return node.get_role_name()
    except Exception:
        return "?"


def name(node):
    try:
        return node.get_name() or ""
    except Exception:
        return ""


def states(node):
    try:
        return node.get_state_set()
    except Exception:
        return None


def showing(node):
    s = states(node)
    return s is not None and s.contains(Atspi.StateType.SHOWING)


def enabled(node):
    s = states(node)
    return s is not None and s.contains(Atspi.StateType.SENSITIVE)


def press(node):
    try:
        action = node.get_action_iface()
    except Exception:
        action = None
    if action is None:
        return False
    for i in range(action.get_n_actions()):
        if action.get_action_name(i) in ("click", "press", "activate", "toggle", "select"):
            return action.do_action(i)
    return action.get_n_actions() > 0 and action.do_action(0)


def dialogs():
    for app in apps():
        for node, _ in walk(app):
            if role(node) in ("dialog", "frame", "window") and showing(node):
                yield node


def buttons(dialog, label):
    return [n for n, _ in walk(dialog) if role(n) in ("push button", "button", "toggle button") and name(n) == label]


def dump():
    for app in apps():
        for node, depth in walk(app):
            if depth > 14:
                continue
            print("  " * depth + f"[{role(node)}] {name(node)!r}{'' if showing(node) else ' (hidden)'}")


def share(window, timeout):
    """GNOME 50: a "Share Screen" frame with "Window" and "Display" tabs, one toggle button per
    screen or window, and Share. "Remember this selection" is left as it is (on), which is what
    makes the portal return a restore token."""
    tab_name = "Window" if window else "Display"
    deadline = time.time() + timeout
    while time.time() < deadline:
        for app in apps():
            for d, _ in walk(app):
                if role(d) != "frame" or name(d) != "Share Screen" or not showing(d):
                    continue
                for t, _ in walk(d):
                    if role(t) == "page tab" and name(t) == tab_name:
                        press(t)
                time.sleep(0.6)
                page = next((n for n, _ in walk(d) if role(n) == "panel" and name(n) == tab_name), None)
                items = [n for n, _ in walk(page)] if page else []
                choice = next((n for n in items if role(n) == "toggle button" and showing(n)), None)
                if choice is None:
                    continue
                press(choice)
                time.sleep(0.3)
                for b in buttons(d, "Share"):
                    if enabled(b) and press(b):
                        print(f"approve-portal: shared {name(choice)!r}")
                        return 0
        time.sleep(0.3)
    print("approve-portal: no share dialog found", file=sys.stderr)
    return 1


def shortcuts(timeout):
    deadline = time.time() + timeout
    while time.time() < deadline:
        for d in dialogs():
            for label in ("Add", "Allow", "Confirm", "Done", "Save"):
                for b in buttons(d, label):
                    if enabled(b) and press(b):
                        print(f"approve-portal: pressed {label}")
                        return 0
        time.sleep(0.3)
    print("approve-portal: no shortcuts dialog found", file=sys.stderr)
    return 1


def main(argv):
    timeout = 60.0
    if "--timeout" in argv:
        timeout = float(argv[argv.index("--timeout") + 1])
    cmd = argv[1] if len(argv) > 1 else "dump"
    if cmd == "dump":
        dump()
        return 0
    if cmd == "share":
        return share("--window" in argv, timeout)
    if cmd == "shortcuts":
        return shortcuts(timeout)
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
