#!/usr/bin/env python3
"""Screenshots of one window, with its Breeze frame and shadow, from a session that
tools/linux-dev/kde-headless.sh started (KWin's ScreenShot2 interface; the session lifts KWin's
permission checks).
  kde-shot.py list                         the windows, as JSON lines
  kde-shot.py activate CAPTION             gives the window titled CAPTION the focus
  kde-shot.py CAPTION OUT.png              the window titled CAPTION, else the first whose title
                                           contains it
Reads KWin's script output from $SC_KWIN_LOG (default /run/user/UID/sc-kde/kwin.log).
"""
import json
import os
import struct
import sys
import tempfile
import time
import zlib

import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402

BUS = Gio.bus_get_sync(Gio.BusType.SESSION)
LOG = os.environ.get("SC_KWIN_LOG", f"/run/user/{os.getuid()}/sc-kde/kwin.log")


def call(path, iface, method, sig, args, reply="()"):
    return BUS.call_sync("org.kde.KWin", path, iface, method,
                         GLib.Variant(sig, args) if sig else None,
                         GLib.VariantType(reply), 0, 10000, None).unpack()


def windows():
    marker = f"SCWIN{os.getpid()}{int(time.time() * 1000)}"
    js = ("for (const w of workspace.windowList()) { if (!w.normalWindow && !w.dialog) continue;"
          f" print('{marker} ' + JSON.stringify({{id: w.internalId.toString(), caption: w.caption,"
          " x: w.x, y: w.y, w: w.width, h: w.height})); }")
    with tempfile.NamedTemporaryFile("w", suffix=".js", delete=False) as f:
        f.write(js)
    start = os.path.getsize(LOG)
    (sid,) = call("/Scripting", "org.kde.kwin.Scripting", "loadScript", "(ss)", (f.name, marker), "(i)")
    call(f"/Scripting/Script{sid}", "org.kde.kwin.Script", "run", None, None)
    time.sleep(0.5)
    call("/Scripting", "org.kde.kwin.Scripting", "unloadScript", "(s)", (marker,), "(b)")
    os.unlink(f.name)
    with open(LOG, errors="replace") as log:
        log.seek(start)
        return [json.loads(line.split(marker + " ", 1)[1]) for line in log if marker + " " in line]


def png(path, w, h, rows):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    raw = b"".join(b"\0" + r for r in rows)
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
                + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def shot(window_id, out):
    r, w = os.pipe()
    fds = Gio.UnixFDList.new()
    idx = fds.append(w)
    os.close(w)
    opts = {"include-decoration": GLib.Variant("b", True), "include-shadow": GLib.Variant("b", True),
            "native-resolution": GLib.Variant("b", True)}
    res, _ = BUS.call_with_unix_fd_list_sync(
        "org.kde.KWin", "/org/kde/KWin/ScreenShot2", "org.kde.KWin.ScreenShot2", "CaptureWindow",
        GLib.Variant("(sa{sv}h)", (window_id, opts, idx)), GLib.VariantType("(a{sv})"), 0, 10000,
        fds, None)
    meta = res.unpack()[0]
    for fd in fds.steal_fds():
        os.close(fd)
    data = bytearray()
    while chunk := os.read(r, 1 << 20):
        data += chunk
    os.close(r)
    width, height, stride, fmt = meta["width"], meta["height"], meta["stride"], meta["format"]
    if fmt not in (5, 6):
        raise SystemExit(f"kde-shot: unexpected QImage format {fmt}")
    rows = []
    for y in range(height):
        row = bytearray(data[y * stride:y * stride + width * 4])
        b, g, rr, a = row[0::4], row[1::4], row[2::4], row[3::4]
        if fmt == 6:
            for i, alpha in enumerate(a):
                if 0 < alpha < 255:
                    b[i], g[i], rr[i] = (min(255, c * 255 // alpha) for c in (b[i], g[i], rr[i]))
        row[0::4], row[1::4], row[2::4], row[3::4] = rr, g, b, a
        rows.append(bytes(row))
    png(out, width, height, rows)
    return width, height


def run_script(js):
    marker = f"SCJS{os.getpid()}{int(time.time() * 1000)}"
    with tempfile.NamedTemporaryFile("w", suffix=".js", delete=False) as f:
        f.write(js)
    (sid,) = call("/Scripting", "org.kde.kwin.Scripting", "loadScript", "(ss)", (f.name, marker), "(i)")
    call(f"/Scripting/Script{sid}", "org.kde.kwin.Script", "run", None, None)
    time.sleep(0.3)
    call("/Scripting", "org.kde.kwin.Scripting", "unloadScript", "(s)", (marker,), "(b)")
    os.unlink(f.name)


def main():
    if sys.argv[1:2] == ["activate"]:
        caption = json.dumps(sys.argv[2])
        run_script("for (const w of workspace.windowList()) { if (w.caption === " + caption +
                   ") { workspace.activeWindow = w; break; } }")
        return
    if sys.argv[1:] == ["list"]:
        for w in windows():
            print(json.dumps(w))
        return
    caption, out = sys.argv[1], sys.argv[2]
    found = windows()
    match = [w for w in found if w["caption"] == caption] or [w for w in found if caption in w["caption"]]
    if not match:
        raise SystemExit(f"kde-shot: no window with {caption!r} in its caption")
    print(f"kde-shot: {out} {shot(match[0]['id'], out)}")


if __name__ == "__main__":
    main()
