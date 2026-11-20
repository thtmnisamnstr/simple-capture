"""Checks the logs and project folders of one app/driver/run.sh run; writes report.json."""

import glob
import json
import os
import subprocess
import sys

RUN = sys.argv[1]
FFMPEG = os.environ.get("SC_FFMPEG", "")
FILTERED = os.environ.get("SC_APP_DRIVER_FILTERED") == "1"
results = []


def check(flow, name, ok, detail=None):
    results.append({"flow": flow, "check": name, "ok": bool(ok), "detail": detail})
    print(f"{'PASS' if ok else 'FAIL'} {flow}: {name}" + (f"  {detail}" if detail is not None else ""))


def lines(flow):
    path = os.path.join(RUN, f"{flow}.log")
    if not os.path.exists(path):
        return None
    out = []
    for raw in open(path, errors="replace"):
        raw = raw.strip()
        if raw.startswith("{"):
            try:
                out.append(json.loads(raw))
            except ValueError:
                pass
    return out


def steps(log, prefix=None):
    return [l for l in log if "step" in l and (prefix is None or l["line"].startswith(prefix))]


def events(log, name):
    return [l["payload"] for l in log if l.get("event") == name]


def rc(flow):
    try:
        return int(open(os.path.join(RUN, f"{flow}.rc")).read().strip())
    except (OSError, ValueError):
        return None


def projects():
    out = {}
    for pj in glob.glob(os.path.join(RUN, "projects", "*", "project.json")):
        d = os.path.dirname(pj)
        p = json.load(open(pj))
        journal = []
        jp = os.path.join(d, "journal.jsonl")
        if os.path.exists(jp):
            for raw in open(jp, errors="replace"):
                try:
                    journal.append(json.loads(raw))
                except ValueError:
                    pass
        out[p.get("title")] = {"dir": d, "project": p, "journal": journal}
    return out


def stop_reason(pr):
    stops = [e for e in pr["journal"] if e.get("ev") == "stop"]
    return stops[-1].get("reason") if stops else None


def segments(pr):
    return pr["project"]["recording"]["screen"]["segments"]


def media_ok(pr):
    opens = [e for e in pr["journal"] if e.get("ev") == "segment_open"]
    if not opens:
        return False, ["no segment_open"]
    bad = []
    for e in opens:
        p = os.path.join(pr["dir"], e["file"])
        if not os.path.exists(p) or os.path.getsize(p) == 0:
            bad.append(e["file"])
    return not bad, bad


def basic(flow, log, allow_fail=()):
    if log is None:
        if not FILTERED:
            check(flow, "ran", False, "no log")
        return False
    failed = [s for s in steps(log) if not s["ok"] and not any(s["line"].startswith(a) for a in allow_fail)]
    check(flow, "every step ok and exit 0", not failed and rc(flow) == 0,
          {"rc": rc(flow), "failed": [(s["line"][:80], s.get("error")) for s in failed]})
    return True


def finalized(flow, pr, title, state="finalized", reason=None):
    if pr is None:
        check(flow, f"project {title!r} exists", False)
        return
    st = pr["project"]["state"]
    check(flow, f"{title!r} state {state}", st == state, {"state": st, "dir": os.path.basename(pr["dir"])})
    if reason:
        check(flow, f"{title!r} stop reason {reason}", stop_reason(pr) == reason, stop_reason(pr))
    ok, bad = media_ok(pr)
    check(flow, f"{title!r} segment files written", ok, {"segments": len(segments(pr)), "missing_or_empty": bad})


def ui_lines(step):
    return step.get("result") or []


def gray_frames(path, crop, fps):
    x, y, w, h = crop
    sw, sh = max(2, w // 4), max(2, h // 4)
    vf = f"fps={fps},crop={w}:{h}:{x}:{y},scale={sw}:{sh},format=gray"
    out = subprocess.run([FFMPEG, "-v", "error", "-i", path, "-vf", vf, "-f", "rawvideo", "-"],
                         capture_output=True, check=False).stdout
    n = sw * sh
    return [out[i:i + n] for i in range(0, len(out) - n + 1, n)]


def mad(a, b):
    return sum(abs(p - q) for p, q in zip(a, b)) / max(1, len(a))


def crop_for(rect, disp, seg):
    fw, fh = seg["frame_px"]["w"], seg["frame_px"]["h"]
    f = disp["frame_pt"]
    sx, sy = fw / f["w"], fh / f["h"]
    x0, y0 = max(0, int((rect[0] - f["x"]) * sx)), max(0, int((rect[1] - f["y"]) * sy))
    x1, y1 = min(fw, int((rect[0] + rect[2] - f["x"]) * sx)), min(fh, int((rect[1] + rect[3] - f["y"]) * sy))
    w, h = (x1 - x0) // 8 * 8, (y1 - y0) // 8 * 8
    return (x0, y0, w, h) if w >= 16 and h >= 16 else None


def compare(flow, what, frames, absent, present, fps=10, grace_s=0.0):
    """Frames while `what` was on screen against frames while it was not. Frames that differ
    within `grace_s` of it appearing are counted and reported but tolerated."""

    def pick(span):
        return [(i / fps, f) for i, f in enumerate(frames) if span[0] <= i / fps <= span[1]]

    a, p = pick(absent), pick(present)
    if len(a) < 2 or not p:
        check(flow, f"{what} excluded", False, {"absent_frames": len(a), "present_frames": len(p), "total": len(frames)})
        return
    ref = a[0][1]
    noise = max(mad(ref, f) for _, f in a[1:])
    diffs = [(t, mad(ref, f)) for t, f in p]
    limit = max(4.0, 2 * noise + 2)
    showing = [round(t - present[0], 3) for t, d in diffs if d > limit]
    late = [t for t in showing if t > grace_s]
    check(flow, f"{what} excluded" + (f" (after {grace_s:.2f} s)" if grace_s else ""), not late,
          {"fps": fps, "frames_compared": len(p), "frames_showing": len(showing), "showing_at_s": showing,
           "max_diff": round(max(d for _, d in diffs), 2), "noise": round(noise, 2), "limit": round(limit, 2)})


P = projects()
DISPLAYS = []

log = lines("01-onboarding")
if basic("01-onboarding", log):
    perms = steps(log, "cmd get_permissions")[0]["result"]
    check("01-onboarding", "permission statuses reported", all(perms.get(k) for k in ("screen", "camera", "mic")), perms)
    DISPLAYS = steps(log, "cmd list_displays")[0]["result"]
    rects = steps(log, "log-window-rects")[0]["result"]
    vis = {r["label"]: r["visible"] for r in rects}
    check("01-onboarding", "nine windows, IDs > 0, onboarding shown",
          len(rects) == 9 and all(r["id"] > 0 for r in rects) and vis.get("onboarding"), vis)
    ui = ui_lines(steps(log, "ui-log")[0])
    text = next((l for l in ui if "onboarding " in l), "")
    check("01-onboarding", "onboarding renders the three statuses",
          all(w in text for w in ("Screen", "Camera", "Microphone")), text[:300])
    check("01-onboarding", "no CSP or script errors", not [l for l in ui if "] csp:" in l or "] error:" in l], ui)
    caps = steps(log, "cmd get_capabilities")[0]["result"]
    check("01-onboarding", "probe cache written", os.path.exists(os.path.join(RUN, "data", "probe-v1.json")),
          caps.get("displays"))

log = lines("02-still-csp")
if basic("02-still-csp", log):
    uis = steps(log, "ui-log")
    first, second = ui_lines(uis[0]), ui_lines(uis[1])
    check("02-still-csp", "no CSP or script errors at launch",
          not [l for l in first if "] csp:" in l or "] error:" in l], first)
    still = next((l for l in second if "still " in l), "")
    check("02-still-csp", "sc://still PNG loads in the webview (16:9 = 960x540)",
          "still ok sc://still/" in still and still.endswith("960x540"), still)
    csp = [l for l in second if "] csp:" in l]
    check("02-still-csp", "CSP active: inline script blocked and reported",
          any("script-src" in l for l in csp) and any("inline ran=false" in l for l in second), second)

log = lines("03-sources")
if basic("03-sources", log):
    for title, kind in (("drv display", "display"), ("drv window", "window"),
                        ("drv application", "application"), ("drv region", "region")):
        pr = P.get(title)
        finalized("03-sources", pr, title, reason="user")
        if pr:
            segs = segments(pr)
            check("03-sources", f"{title!r} source kind {kind}", bool(segs) and segs[0]["source"].get("kind") == kind,
                  [s["source"].get("kind") for s in segs])
    pr = P.get("drv display")
    if pr:
        kinds = sorted(str(t.get("kind")) for t in pr["project"]["recording"]["tracks"])
        check("03-sources", "display run has camera, mic and system audio tracks",
              {"camera", "mic", "system_audio"} <= set(kinds), kinds)
    pr = P.get("drv application")
    if pr and segments(pr):
        s = segments(pr)[0]
        c = s["source"].get("crop_pt")
        fw, fh = s["frame_px"]["w"], s["frame_px"]["h"]
        check("03-sources", "application cropped to its windows (ADR-039): frame matches crop_pt aspect",
              bool(c) and c["w"] > 0 and c["h"] > 0 and abs(fw / fh - c["w"] / c["h"]) < 0.01,
              {"crop_pt": c, "frame_px": s["frame_px"]})
    pr = P.get("drv region")
    if pr and segments(pr):
        s = segments(pr)[0]
        fw, fh = s["frame_px"]["w"], s["frame_px"]["h"]
        check("03-sources", "region at 1x: frame px about 617x283.5", abs(fw - 617) <= 1 and abs(fh - 283.5) <= 1.5,
              s["frame_px"])

log = lines("04-switch")
if basic("04-switch", log):
    pr = P.get("drv switch")
    finalized("04-switch", pr, "drv switch", reason="user")
    if pr:
        segs = segments(pr)
        got = [(s["source"].get("kind"), s["via"], s["start_reason"]) for s in segs]
        want = ["display", "window", "window", "display", "region", "window", "display"]
        check("04-switch", "segments follow bar, tray, hotkey+pick, region, hotkey+pick, cmd",
              [g[0] for g in got] == want, got)
        tiles = all(segs[i]["end_rec_ns"] is not None and segs[i]["end_rec_ns"] <= segs[i + 1]["start_rec_ns"]
                    for i in range(len(segs) - 1))
        check("04-switch", "segments ordered, non-overlapping", tiles,
              [(s["start_rec_ns"], s["end_rec_ns"]) for s in segs])
        sw = [e for e in pr["journal"] if e.get("ev") == "switch_begin"]
        failed = [e for e in pr["journal"] if e.get("ev") == "switch_failed"]
        check("04-switch", "six switches, none failed", len(sw) == 6 and not failed,
              {"switch_begin": len(sw), "switch_failed": failed})
    shown = [w for st in steps(log, "log-window-rects") for w in (st.get("result") or [])
             if w["label"] in ("region", "pick") and w["visible"]]
    frames = [d["frame_pt"] for d in DISPLAYS]
    check("04-switch", "region and pick overlays seen while shown", {w["label"] for w in shown} == {"region", "pick"},
          [w["label"] for w in shown])
    for w in shown:
        r = w["rect_pt"]
        check("04-switch", f"{w['label']} overlay covers its display exactly",
              any(all(abs(r[k] - f[k]) <= 1 for k in ("x", "y", "w", "h")) for f in frames), {"rect": r, "displays": frames})

log = lines("05-exclusion")
if basic("05-exclusion", log) and FFMPEG and DISPLAYS:
    pr = P.get("drv exclusion")
    finalized("05-exclusion", pr, "drv exclusion", reason="user")
    rect_steps = steps(log, "log-window-rects")
    menu = steps(log, "tray open-menu")[0]
    hide_t = steps(log, "eval bar")[0]["rec_ns"] / 1e9
    show_t = steps(log, "show bar")[0]["rec_ns"] / 1e9
    stop_t = max(s["rec_ns"] for s in steps(log) if s.get("rec_ns") is not None) / 1e9
    opened, closed = menu["result"]["opened_rec_ns"] / 1e9, menu["result"]["closed_rec_ns"] / 1e9
    menus = [w for w in menu["result"]["other_windows"] if w["rect_pt"][1] > 20 and w["rect_pt"][3] > 40]
    check("05-exclusion", "tray menu window seen on screen", bool(menus), menu["result"]["other_windows"])
    if pr and segments(pr):
        seg = segments(pr)[0]
        path = os.path.join(pr["dir"], [e["file"] for e in pr["journal"] if e.get("ev") == "segment_open"][0])
        disp = next((d for d in DISPLAYS if d["id"] == seg["source"].get("display_id")), DISPLAYS[0])
        targets = []
        if menus:
            targets.append(("tray menu", menus[0]["rect_pt"], (opened - 1.5, opened - 0.2), (opened, closed)))
        r2 = {r["label"]: r for r in rect_steps[1]["result"]}
        for label in ("bar", "switcher", "panel"):
            rp = r2[label]["rect_pt"]
            if r2[label]["visible"] and rp:
                targets.append((label, [rp["x"], rp["y"], rp["w"], rp["h"]],
                                (hide_t + 0.6, opened - 0.3), (show_t + 0.4, stop_t - 0.3)))
            else:
                check("05-exclusion", f"{label} visible during the recording", False, r2[label])
        fps = pr["project"]["recording"]["screen"]["fps_requested"]
        for what, rect, absent, present in targets:
            crop = crop_for(rect, disp, seg)
            if not crop:
                check("05-exclusion", f"{what} rect inside the frame", False, rect)
                continue
            # A menu's window exists only once it tracks; the refresh it triggers takes ~150 ms.
            grace = 0.25 if what == "tray menu" else 0.0
            compare("05-exclusion", what, gray_frames(path, crop, fps), absent, present, fps, grace)

log = lines("06-export")
if basic("06-export", log):
    pr = P.get("drv export")
    finalized("06-export", pr, "drv export", reason="user")
    prog = events(log, "export-progress")
    states = events(log, "export-state")
    done = [s for s in states if s.get("state") == "done"]
    check("06-export", "progress events", len(prog) >= 2, {"progress_events": len(prog), "last": prog[-1] if prog else None})
    check("06-export", "first export done", bool(done), [s.get("state") for s in states])
    if done:
        out = done[0].get("path", "")
        dec = None
        if FFMPEG and os.path.exists(out):
            dec = subprocess.run([FFMPEG, "-v", "error", "-i", out, "-f", "null", "-"], capture_output=True, text=True)
        check("06-export", "export file decodes", dec is not None and dec.returncode == 0 and not dec.stderr.strip(),
              {"file": os.path.basename(out), "bytes": os.path.getsize(out) if os.path.exists(out) else 0})
    check("06-export", "second export cancelled", any(s.get("state") in ("canceled", "cancelled") for s in states),
          [s.get("state") for s in states])
    if pr:
        parts = glob.glob(os.path.join(pr["dir"], "**", "*.part"), recursive=True)
        check("06-export", "no .part after cancel", not parts, parts)

log = lines("07-sleep")
if basic("07-sleep", log):
    finalized("07-sleep", P.get("drv sleep"), "drv sleep", reason="sleep")
    notices = [n.get("kind") for n in events(log, "notice")]
    check("07-sleep", "stopped_for_sleep notice", "stopped_for_sleep" in notices, notices)

log = lines("08-quit-recording")
if basic("08-quit-recording", log):
    pr = P.get("drv quit")
    finalized("08-quit-recording", pr, "drv quit", reason="quit")
    rel = lines("08b-relaunch")
    if basic("08b-relaunch", rel) and pr:
        rep = steps(rel, "cmd recovery_report")[0]["result"]
        mine = [r for r in rep if os.path.basename(r["dir"]) == os.path.basename(pr["dir"])]
        check("08b-relaunch", "quit project not recovered on relaunch",
              all(r["outcome"] == "already_final" for r in mine), mine)
        state = json.load(open(os.path.join(pr["dir"], "project.json")))["state"]
        check("08b-relaunch", "state still finalized", state == "finalized", state)

log = lines("09-sigterm")
if log is not None:
    check("09-sigterm", "app exited 0 after SIGTERM", rc("09-sigterm") == 0, rc("09-sigterm"))
    finalized("09-sigterm", P.get("drv sigterm"), "drv sigterm", reason="signal")

log = lines("10-kill")
if log is not None:
    pr = P.get("drv kill")
    check("10-kill", "app was killed", rc("10-kill") not in (0, None), rc("10-kill"))
    rel = lines("10b-relaunch")
    if basic("10b-relaunch", rel) and pr:
        rep = steps(rel, "cmd recovery_report")[0]["result"]
        mine = [r for r in rep if os.path.basename(r["dir"]) == os.path.basename(pr["dir"])]
        check("10b-relaunch", "killed project recovered at launch", [r["outcome"] for r in mine] == ["recovered"], mine)
        state = json.load(open(os.path.join(pr["dir"], "project.json")))["state"]
        check("10b-relaunch", "state recovered", state == "recovered", state)
        launch = events(rel, "launch-tasks-done")
        check("10b-relaunch", "launch-tasks-done event carries the report",
              bool(launch) and any(r.get("outcome") == "recovered" for r in launch[0].get("recovery") or []), launch[:1])
        ui = ui_lines(steps(rel, "ui-log")[0])
        panel = next((l for l in ui if "] log: panel " in l), "")
        check("10b-relaunch", "panel shows the recovery notice", "recovered" in panel, panel[:300])

log = lines("11-quit-export")
if basic("11-quit-export", log):
    pr = P.get("drv quit export")
    finalized("11-quit-export", pr, "drv quit export", reason="user")
    if pr:
        parts = glob.glob(os.path.join(pr["dir"], "**", "*.part"), recursive=True)
        check("11-quit-export", "no .part after quit", not parts, parts)
    states = [s.get("state") for s in events(log, "export-state")]
    check("11-quit-export", "export was running at quit", bool(states) and "done" not in states, states)
    after = os.path.join(RUN, "ffmpeg-after.txt")
    left = open(after).read().strip() if os.path.exists(after) else ""
    check("11-quit-export", "no ffmpeg process left", not left, left)

json.dump({"run": RUN, "projects": {k: os.path.basename(v["dir"]) for k, v in P.items()}, "results": results},
          open(os.path.join(RUN, "report.json"), "w"), indent=2)
failed = [r for r in results if not r["ok"]]
log = lines("12-bubble")
if basic("12-bubble", log):
    st = [s["result"] for s in steps(log, "cmd bubble_state")]
    rects = [{r["label"]: r for r in s["result"]} for s in steps(log, "log-window-rects")]
    if len(st) >= 3 and len(rects) >= 2:
        a, b, c = st[0], st[1], st[2]
        check("12-bubble", "camera opened for the bubble", bool(a.get("camera_open")), a.get("camera_open"))
        check("12-bubble", "bubble shown with live frames", a.get("shown") and a.get("frame", 0) > 0, a)
        fps = b.get("frame", 0) - a.get("frame", 0)
        check("12-bubble", "bubble frames keep coming (>= 15 in 1 s)", fps >= 15, fps)
        win, g = rects[0].get("bubble"), (a.get("geom") or {}).get("window")
        check("12-bubble", "bubble window placed at its geometry",
              bool(win and win["visible"] and g) and all(abs(win["rect_pt"][k] - g[k]) <= 1 for k in "xywh"),
              {"window": win, "geom": g})
        k0, k1 = (a.get("key") or {}).get("center_norm"), (c.get("key") or {}).get("center_norm")
        check("12-bubble", "drag moved the camera left and up", bool(k0 and k1) and k1["x"] < k0["x"] and k1["y"] < k0["y"],
              {"before": k0, "after": k1})
        win2, g2 = rects[1].get("bubble"), (c.get("geom") or {}).get("window")
        check("12-bubble", "bubble window followed the drag",
              bool(win2 and g2) and all(abs(win2["rect_pt"][k] - g2[k]) <= 1 for k in "xy"), {"window": win2, "geom": g2})
        if len(st) >= 5 and len(rects) >= 3:
            d, e = st[3], st[4]
            stage = rects[2].get("stage")
            check("12-bubble", "9:16 frame: stage shown, bubble floats", bool(d.get("stage")) and bool(stage and stage["visible"])
                  and not ((d.get("geom") or {}).get("maps_output", True)), {"state": d.get("stage"), "window": stage, "geom": d.get("geom")})
            sfps = e.get("stage_frame", 0) - d.get("stage_frame", 0)
            check("12-bubble", "stage frames keep coming (>= 15 in 1 s)", d.get("stage_frame", 0) > 0 and sfps >= 15, sfps)
    else:
        check("12-bubble", "bubble steps logged", False, {"states": len(st), "rects": len(rects)})

log = lines("14-bubble-capture")
if basic("14-bubble-capture", log):
    pr = P.get("drv bubble capture")
    finalized("14-bubble-capture", pr, "drv bubble capture", reason="user")
    if pr:
        keys = [k for o in pr["project"].get("camera_layout", {}).get("overlays", []) for k in o.get("keyframes", [])]
        check("14-bubble-capture", "keyframes: the start plus the drag", len(keys) >= 3 and keys[0]["t_rec_ns"] == 0
              and keys[-1]["center_norm"]["x"] < keys[0]["center_norm"]["x"], [(k["t_rec_ns"], k["center_norm"]) for k in keys])
        tracks = {t["kind"] for t in pr["project"]["recording"]["tracks"]}
        check("14-bubble-capture", "camera track recorded", "camera" in tracks, sorted(tracks))
    rects = steps(log, "log-window-rects")
    shown_t = steps(log, "cmd set_settings")[1]["rec_ns"] / 1e9
    drag_t = steps(log, "cmd bubble_drag")[0]["rec_ns"] / 1e9
    if FFMPEG and DISPLAYS and pr and segments(pr) and rects:
        bub = {r["label"]: r for r in rects[0]["result"]}.get("bubble")
        check("14-bubble-capture", "bubble visible during the recording", bool(bub and bub["visible"]), bub)
        if bub and bub["visible"]:
            seg = segments(pr)[0]
            path = os.path.join(pr["dir"], [e["file"] for e in pr["journal"] if e.get("ev") == "segment_open"][0])
            disp = next((d for d in DISPLAYS if d["id"] == seg["source"].get("display_id")), DISPLAYS[0])
            rp = bub["rect_pt"]
            crop = crop_for([rp["x"], rp["y"], rp["w"], rp["h"]], disp, seg)
            fps = pr["project"]["recording"]["screen"]["fps_requested"]
            if crop:
                compare("14-bubble-capture", "bubble", gray_frames(path, crop, fps),
                        (0.6, shown_t - 0.2), (shown_t + 0.4, drag_t - 0.2), fps)
            else:
                check("14-bubble-capture", "bubble rect inside the frame", False, rp)

print(f"\n{len(results) - len(failed)}/{len(results)} checks passed")
sys.exit(1 if failed else 0)
