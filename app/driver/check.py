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
    expected = any(not s["ok"] for s in steps(log))
    check(flow, "every step ok (or failed as expected) and the matching exit status", not failed and rc(flow) == (1 if expected else 0),
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
    check("01-onboarding", "thirteen windows, IDs > 0, onboarding shown",
          len(rects) == 13 and all(r["id"] > 0 for r in rects) and vis.get("onboarding"), vis)
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
    mw = menu["result"].get("menu_window") or {}
    mr = mw.get("rect_pt") or {}
    menus = [{"rect_pt": [mr["x"], mr["y"], mr["w"], mr["h"]]}] if mw.get("visible") and mr else []
    check("05-exclusion", "tray menu window shown while recording", bool(menus), mw)
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
            compare("05-exclusion", what, gray_frames(path, crop, fps), absent, present, fps, 0.0)

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

log = lines("12-preview")
if basic("12-preview", log):
    st = [x["result"] for x in steps(log, "cmd preview_state")]
    rects = [{r["label"]: r for r in x["result"]} for x in steps(log, "log-window-rects")]
    if len(st) >= 4 and len(rects) >= 2:
        a, b, c, d = st[0], st[1], st[2], st[3]
        win = rects[0].get("stage")
        check("12-preview", "camera opened for the preview", bool(a.get("camera_open")), a.get("camera_open"))
        check("12-preview", "preview window shown with live frames", a.get("shown") and a.get("frame", 0) > 0
              and bool(win and win["visible"]), {"state": a, "window": win})
        fps = b.get("frame", 0) - a.get("frame", 0)
        check("12-preview", "preview frames keep coming (>= 15 in 1 s)", fps >= 15, fps)
        if sys.platform == "darwin" and os.environ.get("SC_PREVIEW") != "webview":
            check("12-preview", "drawn by the native Metal layer (ADR-007)", a.get("native") is True, a.get("native"))
        frames = [x["frame_pt"] for x in DISPLAYS]
        r = (win or {}).get("rect_pt") or {}
        check("12-preview", "preview window placed on a display", bool(r) and any(
            r["x"] >= f["x"] and r["y"] >= f["y"] and r["x"] + r["w"] <= f["x"] + f["w"] and r["y"] + r["h"] <= f["y"] + f["h"]
            for f in frames), {"rect": r, "displays": frames})
        k = (c.get("key") or {}).get("center_norm")
        check("12-preview", "a live camera edit reaches the preview", k == {"x": 0.3, "y": 0.3}, k)
        win2 = rects[1].get("stage")
        check("12-preview", "9:16 frame: preview still shown", bool(d.get("shown")) and bool(win2 and win2["visible"]), d)
        if len(st) >= 5:
            e = st[4]
            uis = steps(log, "ui-log")
            page = next((json.loads(l.split("stage page ", 1)[1]) for l in (ui_lines(uis[0]) if uis else [])
                         if "stage page " in l), None)
            check("12-preview", "after a resize: new frames, page 720 pt wide", e.get("frame", 0) > d.get("frame", 0)
                  and abs(((e.get("page") or {}).get("w") or 0) - 720) <= 1, {"before": d.get("frame"), "after": e})
            if not page:
                check("12-preview", "the Preview page reports its layout", False, uis[:1])
            else:
                if e.get("native"):
                    want = {"w": round(page["vw"] * page["dpr"]), "h": round(page["vh"] * page["dpr"])}
                    check("12-preview", "after a resize: the native drawable is the page viewport in pixels",
                          e.get("drawable") == want, {"drawable": e.get("drawable"), "viewport_px": want})
                cv, cam, fr, pc = e.get("canvas") or {}, e.get("camera_px"), page.get("frame"), page.get("cam")
                if cam and fr and pc and cv.get("w"):
                    k = cv["w"] / fr[2]
                    got = [(pc[0] - fr[0]) * k, (pc[1] - fr[1]) * k, pc[2] * k, pc[3] * k]
                    check("12-preview", "the page's camera outline is where the camera was drawn (2 px), after a panel edit",
                          all(abs(a - b) <= 2 for a, b in zip(got, cam)), {"drawn_px": cam, "outline_px": got})
                else:
                    check("12-preview", "camera drawn and outlined", False, {"canvas": cv, "camera_px": cam, "page": page})
    else:
        check("12-preview", "preview steps logged", False, {"states": len(st), "rects": len(rects)})

log = lines("14-preview-capture")
if basic("14-preview-capture", log):
    pr = P.get("drv preview capture")
    finalized("14-preview-capture", pr, "drv preview capture", reason="user")
    if pr:
        keys = [k for o in pr["project"].get("camera_layout", {}).get("overlays", []) for k in o.get("keyframes", [])]
        check("14-preview-capture", "keyframes: the start plus the moves", len(keys) >= 3 and keys[0]["t_rec_ns"] == 0
              and keys[-1]["center_norm"] == {"x": 0.2, "y": 0.3}, [(k["t_rec_ns"], k["center_norm"]) for k in keys])
        tracks = {t["kind"] for t in pr["project"]["recording"]["tracks"]}
        check("14-preview-capture", "camera track recorded", "camera" in tracks, sorted(tracks))
    rects = steps(log, "log-window-rects")
    shown_t = steps(log, "cmd set_settings")[1]["rec_ns"] / 1e9
    move_t = steps(log, "cmd camera_key_live")[0]["rec_ns"] / 1e9
    if len(rects) >= 2 and DISPLAYS:
        b0 = {r["label"]: r for r in rects[0]["result"]}.get("bar", {}).get("rect_pt") or {}
        b1 = {r["label"]: r for r in rects[1]["result"]}.get("bar", {}).get("rect_pt") or {}
        f = DISPLAYS[0]["frame_pt"]
        check("14-preview-capture", "control bar at the bottom centre", bool(b0) and abs(b0["x"] + b0["w"] / 2 - (f["x"] + f["w"] / 2)) <= 2
              and abs(b0["y"] - (f["y"] + f["h"] - b0["h"] - 96)) <= 2, {"bar": b0, "display": f})
        check("14-preview-capture", "control bar moves by its grip", bool(b0 and b1) and abs(b1["x"] - (b0["x"] - 40)) <= 1
              and abs(b1["y"] - (b0["y"] - 20)) <= 1, {"before": b0, "after": b1})
    if FFMPEG and DISPLAYS and pr and segments(pr) and rects:
        win = {r["label"]: r for r in rects[0]["result"]}.get("stage")
        check("14-preview-capture", "preview visible during the recording", bool(win and win["visible"]), win)
        if win and win["visible"]:
            seg = segments(pr)[0]
            path = os.path.join(pr["dir"], [e["file"] for e in pr["journal"] if e.get("ev") == "segment_open"][0])
            disp = next((x for x in DISPLAYS if x["id"] == seg["source"].get("display_id")), DISPLAYS[0])
            rp = win["rect_pt"]
            crop = crop_for([rp["x"], rp["y"], rp["w"], rp["h"]], disp, seg)
            fps = pr["project"]["recording"]["screen"]["fps_requested"]
            if crop:
                compare("14-preview-capture", "preview window", gray_frames(path, crop, fps),
                        (0.6, shown_t - 0.2), (shown_t + 0.6, move_t - 0.2), fps)
            else:
                check("14-preview-capture", "preview rect inside the frame", False, rp)

log = lines("16-soak")
if log is not None and basic("16-soak", log):
    pr = P.get("drv soak")
    finalized("16-soak", pr, "drv soak", reason="user")
    if pr:
        rec = pr["project"]["recording"]
        dur_s = (rec["stop_host_ns"] - rec["start_host_ns"]) / 1e9 if rec.get("stop_host_ns") else 0
        check("16-soak", "about 30 minutes recorded", 1790 <= dur_s <= 1830, round(dur_s, 1))
        tracks = {t["id"]: t for t in rec["tracks"]}
        kinds = sorted({t["kind"] for t in rec["tracks"]})
        check("16-soak", "screen, camera, mic and system audio tracks", all(k in kinds for k in
              ("screen", "camera", "mic", "system_audio")), kinds)
        for tid, t in sorted(tracks.items()):
            h = t.get("health") or {}
            n, d = h.get("frames", 0), h.get("dropped", 0)
            pct = 100.0 * d / max(1, n + d)
            check("16-soak", f"{tid}: under 0.5% dropped, no writer stalls", pct < 0.5 and h.get("writer_not_ready", 0) == 0,
                  {"frames": n, "dropped": d, "pct": round(pct, 3), "writer_not_ready": h.get("writer_not_ready"),
                   "gaps": len(h.get("gaps") or [])})
        ps = steps(log, "cmd preview_state")
        st = [x["result"] for x in ps]
        if len(st) >= 2:
            cam = (tracks.get("camera") or {}).get("health", {}).get("frames", 0)
            cam_fps = cam / max(1.0, dur_s)
            rates = []
            for a, b in zip(ps, ps[1:]):
                dt = ((b.get("rec_ns") or 0) - (a.get("rec_ns") or 0)) / 1e9
                if dt > 0:
                    rates.append((b["result"].get("frame", 0) - a["result"].get("frame", 0)) / dt)
            check("16-soak", "the Preview never stalled (each interval >= 75% of the camera's rate)",
                  bool(rates) and cam_fps > 0 and min(rates) >= 0.75 * cam_fps,
                  {"camera_fps": round(cam_fps, 1), "preview_fps": [round(r, 1) for r in rates]})
            check("16-soak", "drawn natively throughout", all(x.get("native") for x in st) or sys.platform != "darwin",
                  [x.get("native") for x in st])
        es = [x["result"] for x in steps(log, 'cmd editor {"action":"state"}')]
        if es:
            check("16-soak", "the 30-minute take opens in the editor within 2 s",
                  (es[0].get("first_frame_ms") or 99999) < 2000, es[0].get("first_frame_ms"))
        ex = [x for x in events(log, "export-state") if x.get("state") in ("done", "failed")]
        check("16-soak", "a 1080p export of the whole take finishes", bool(ex) and ex[-1]["state"] == "done",
              [x.get("state") for x in ex])
        if ex and ex[-1]["state"] == "done" and FFMPEG:
            probe = os.path.join(os.path.dirname(FFMPEG), "sc-ffprobe")
            r = subprocess.run([probe, "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0",
                                ex[-1].get("path", "")], capture_output=True, text=True)
            try:
                d = float(r.stdout.strip())
            except ValueError:
                d = 0.0
            check("16-soak", "the export is as long as the take", abs(d - dur_s) < 2.0, {"export_s": d, "take_s": dur_s})
        ps_file = os.path.join(RUN, "soak-ps.txt")
        if os.path.exists(ps_file):
            rows = [l.split() for l in open(ps_file) if l.strip() and l.split()[0].isdigit()]
            rows = rows[: int(dur_s // 15)]
            rss = [int(r[0]) / 1024 for r in rows]
            cpu = [float(r[1]) for r in rows]
            check("16-soak", "the app's memory and CPU were sampled", len(rss) > 10 and min(rss) > 30,
                  {"samples": len(rss), "min_mb": round(min(rss), 1) if rss else None})
            if len(rss) > 10 and min(rss) > 30:
                q = len(rss) // 4
                early, late = sum(rss[q:2 * q]) / q, sum(rss[-q:]) / q
                check("16-soak", "memory steady while recording (last quarter within 150 MB of the second)",
                      late - early < 150, {"early_mb": round(early), "late_mb": round(late), "max_mb": round(max(rss)),
                                           "cpu_avg": round(sum(cpu) / len(cpu), 1), "cpu_max": round(max(cpu), 1)})

log = lines("17-editor")
if basic("17-editor", log):
    pr = P.get("drv editor")
    finalized("17-editor", pr, "drv editor", reason="user")
    states = [x["result"] for x in steps(log, 'cmd editor {"action":"state"}')]
    check("17-editor", "the editor opened the new recording", bool(states) and pr is not None
          and os.path.basename(states[0].get("project_dir", "")) == os.path.basename(pr["dir"]),
          states[0].get("project_dir") if states else None)
    if states and pr:
        rec_len = states[0]["rec_len_ns"]
        doc = pr["project"].get("edl", {})
        clips = doc.get("clips", [])
        def tl(c):
            return (c["src_out_rec_ns"] - c["src_in_rec_ns"]) * c["speed"]["den"] // c["speed"]["num"]
        total = sum(tl(c) for c in clips)
        want = rec_len - 2_500_000_000
        check("17-editor", "edits autosaved to project.json (3 clips, 2x, muted, mic -6 dB)",
              len(clips) == 3 and clips[1]["speed"] == {"num": 2, "den": 1} and clips[2]["muted"]
              and pr["project"].get("audio_mix", {}).get("mic", {}).get("gain_db") == -6,
              {"clips": [(c["src_in_rec_ns"], c["src_out_rec_ns"], c["speed"], c["muted"]) for c in clips],
               "audio_mix": pr["project"].get("audio_mix")})
        check("17-editor", "timeline length = recording - 2.5 s", abs(total - want) <= 2_000_000,
              {"timeline_s": total / 1e9, "want_s": want / 1e9})
        last = states[-1]
        check("17-editor", "playback moved the playhead about 2.5 s", 3.0e9 <= last["playhead_tl_ns"] <= 4.6e9,
              {"playhead_s": last["playhead_tl_ns"] / 1e9})
        check("17-editor", "the preview drew frames", last.get("frames_presented", 0) >= 20,
              last.get("frames_presented"))
        check("17-editor", "first frame within 2 s of opening", (last.get("first_frame_ms") or 99999) < 2000,
              last.get("first_frame_ms"))
        ex = [x for x in events(log, "export-state") if x.get("state") == "done"]
        check("17-editor", "export done", bool(ex), [x.get("state") for x in events(log, "export-state")])
        if ex and FFMPEG:
            probe = os.path.join(os.path.dirname(FFMPEG), "sc-ffprobe")
            out = ex[0].get("path", "")
            dur = None
            if os.path.exists(probe) and os.path.exists(out):
                r = subprocess.run([probe, "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", out],
                                   capture_output=True, text=True)
                try:
                    dur = float(r.stdout.strip())
                except ValueError:
                    dur = None
            check("17-editor", "exported length matches the edit (within 2 frames)",
                  dur is not None and abs(dur - total / 1e9) <= 2 / 30 + 0.03,
                  {"export_s": dur, "timeline_s": total / 1e9})
        if ex and FFMPEG:
            out = ex[0].get("path", "")

            def yellow_at(t_s):
                r = subprocess.run([FFMPEG, "-v", "error", "-ss", str(t_s), "-i", out, "-frames:v", "1",
                                    "-vf", "scale=320:-2", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"],
                                   capture_output=True)
                px = r.stdout
                return sum(1 for i in range(0, len(px) - 2, 3) if px[i] > 200 and px[i + 1] > 190 and px[i + 2] < 90)

            during, after = yellow_at(2.0), yellow_at(5.0)
            check("17-editor", "the text item is in the export (2.0 s) and gone after it (5.0 s)",
                  during > 150 and after < during / 10, {"yellow_px_2s": during, "yellow_px_5s": after})
            pj = pr["project"].get("text", [])
            check("17-editor", "text item autosaved", len(pj) == 1 and pj[0].get("content") == "DRIVER TEXT",
                  [t.get("content") for t in pj])
        done = [x.get("path", "") for x in events(log, "export-state") if x.get("state") == "done"]
        probe_bin = os.path.join(os.path.dirname(FFMPEG), "sc-ffprobe") if FFMPEG else ""

        def codec_of(path):
            if not (probe_bin and os.path.exists(probe_bin) and os.path.exists(path)):
                return None
            r = subprocess.run([probe_bin, "-v", "error", "-show_entries", "stream=codec_name,width,height",
                                "-of", "json", path], capture_output=True, text=True)
            try:
                return json.loads(r.stdout).get("streams", [])
            except ValueError:
                return None

        gif = [p for p in done if p.endswith(".gif")]
        webm = [p for p in done if p.endswith(".webm")]
        g = codec_of(gif[0]) if gif else None
        w = codec_of(webm[0]) if webm else None
        check("17-editor", "export sheet path: GIF (240p) and WebM (VP9 + Opus) exported",
              bool(g) and g[0].get("codec_name") == "gif" and g[0].get("height") == 240
              and bool(w) and sorted(x.get("codec_name") for x in w) == ["opus", "vp9"],
              {"gif": g, "webm": w})
        est = [s["result"] for s in steps(log, 'cmd editor {"action":"export_estimate"')]
        check("17-editor", "estimate before export", bool(est) and est[0].get("canvas", {}).get("h") == 240
              and est[0].get("bytes_high", 0) > est[0].get("bytes_low", 0) > 0, est[:1])
        ui = [l for l in log if l.get("event") == "editor-frame"]
        check("17-editor", "editor-frame says native on macOS", sys.platform != "darwin" or (ui and ui[-1]["payload"].get("native")),
              ui[-1]["payload"] if ui else None)

log = lines("27-tracks")
if basic("27-tracks", log):
    pr = P.get("drv tracks")
    finalized("27-tracks", pr, "drv tracks", reason="user")
    states = [x["result"] for x in steps(log, 'cmd editor {"action":"state"}')]
    check("27-tracks", "three editor states", len(states) == 3, len(states))
    if len(states) == 3:
        def spans(clips):
            out, t = [], 0
            for c in clips:
                n = (c["src_out_rec_ns"] - c["src_in_rec_ns"]) * c["speed"]["den"] // c["speed"]["num"]
                out.append((t, t + n, c["src_in_rec_ns"], bool(c.get("hidden"))))
                t += n
            return out
        first, mid, last = states
        cam = (mid["doc"].get("camera_edl") or {}).get("clips", [])
        cs = spans(cam)
        check("27-tracks", "the camera has a 2 s gap, then its clip from 3 s",
              len(cs) == 2 and cs[0][3] and abs(cs[0][1] - 2e9) < 2e7 and not cs[1][3] and abs(cs[1][2] - 3e9) < 2e7,
              cs)
        check("27-tracks", "the screen track did not move",
              spans(mid["doc"]["edl"]["clips"]) == spans(first["doc"]["edl"]["clips"]),
              {"before": spans(first["doc"]["edl"]["clips"]), "after": spans(mid["doc"]["edl"]["clips"])})
        check("27-tracks", "linking again drops the camera's own clips",
              last["doc"].get("camera_edl") is None and pr is not None and "camera_edl" not in pr["project"],
              last["doc"].get("camera_edl"))
        ex = [x for x in events(log, "export-state") if x.get("state") == "done"]
        check("27-tracks", "export done", bool(ex), [x.get("state") for x in events(log, "export-state")])
        if ex and FFMPEG:
            probe = os.path.join(os.path.dirname(FFMPEG), "sc-ffprobe")
            out = ex[0].get("path", "")
            total = spans(mid["doc"]["edl"]["clips"])[-1][1] / 1e9
            dur = None
            if os.path.exists(probe) and os.path.exists(out):
                r = subprocess.run([probe, "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", out],
                                   capture_output=True, text=True)
                try:
                    dur = float(r.stdout.strip())
                except ValueError:
                    dur = None
            check("27-tracks", "the export is as long as the screen track", dur is not None and abs(dur - total) <= 0.1,
                  {"export_s": dur, "timeline_s": total})

log = lines("28-iphone")
if log is not None and basic("28-iphone", log):
    pr = P.get("drv iphone")
    finalized("28-iphone", pr, "drv iphone", reason="user")
    if pr:
        segs = segments(pr)
        src = segs[0].get("source", {}) if segs else {}
        check("28-iphone", "the main picture is the plugged-in device", src.get("kind") == "camera", src)
        size = segs[0].get("content_px", {}) if segs else {}
        check("28-iphone", "recorded at the phone's own shape (portrait or landscape, not 16:9)",
              bool(size) and abs(size.get("w", 16) / max(1, size.get("h", 9)) - 16 / 9) > 0.05, size)
        tracks = [t.get("kind") for t in pr["project"]["recording"].get("tracks", [])]
        check("28-iphone", "the Mac's camera recorded in the corner too", "camera" in tracks, tracks)
        check("28-iphone", "the phone's sound recorded as its own track", "device_audio" in tracks, tracks)
        ex = [x for x in events(log, "export-state") if x.get("state") == "done"]
        check("28-iphone", "export done", bool(ex), [x.get("state") for x in events(log, "export-state")])

log = lines("29-sections")
if log is not None and basic("29-sections", log):
    pr = P.get("drv sections")
    finalized("29-sections", pr, "drv sections", reason="user")
    if pr:
        segs = segments(pr)
        framing = (pr["project"].get("output", {}).get("segment_framing") or {})
        check("29-sections", "two sections; the second keeps its own Fill, left edge",
              len(segs) == 2 and framing.get(segs[1]["id"], {}).get("fit") == "cover"
              and framing.get(segs[1]["id"], {}).get("focus_norm", {}).get("x") == 0.0
              and segs[0]["id"] not in framing,
              {"segments": [s["id"] for s in segs], "framing": framing})
        ex = [x for x in events(log, "export-state") if x.get("state") == "done"]
        check("29-sections", "export done", bool(ex), [x.get("state") for x in events(log, "export-state")])

log = lines("18-diagnostics")
if basic("18-diagnostics", log):
    done = events(log, "diagnostics-done")
    d = done[-1] if done else {}
    check("18-diagnostics", "self-test from the app: no failures, report written",
          bool(done) and d.get("failed") == 0 and d.get("passed", 0) >= 6 and bool(d.get("report")),
          {k: d.get(k) for k in ("passed", "failed", "skipped", "report")})
    lines_seen = events(log, "diagnostics-line")
    check("18-diagnostics", "self-test lines streamed to the window", len(lines_seen) >= 15, len(lines_seen))
    about = [s["result"] for s in steps(log, 'cmd app {"action":"about"}')]
    lic = (about[0] or {}).get("licenses", []) if about else []
    check("18-diagnostics", "About lists the app, helpers, fonts and libraries",
          len(lic) >= 5 and any(e.get("files") for e in lic if e.get("title") == "simple-capture"),
          [(e.get("title"), len(e.get("files", []))) for e in lic])
    rep = d.get("report")
    if rep:
        for ext in (".txt", ".json"):
            try:
                os.remove(os.path.splitext(rep)[0] + ext)
            except OSError:
                pass

log = lines("19-library")
if basic("19-library", log):
    res = [s["result"] for s in steps(log, 'cmd app {"action":"library"}')]
    items = (res[-1] or {}).get("items", []) if res else []
    check("19-library", "lists the recordings, newest first",
          len(items) >= 1 and all(i.get("bytes", 0) > 0 for i in items)
          and [i["created_at"] for i in items] == sorted((i["created_at"] for i in items), reverse=True),
          [(i.get("title"), i.get("state")) for i in items])
    posters = [os.path.join(i["dir"], "thumbs", "poster-v1.png") for i in items if i.get("state") != "recording"]
    def png_ok(p):
        try:
            head = open(p, "rb").read(24)
        except OSError:
            return False
        w, h = int.from_bytes(head[16:20], "big"), int.from_bytes(head[20:24], "big")
        return head[:8] == b"\x89PNG\r\n\x1a\n" and w >= 200 and h >= 100

    made = [p for p in posters if png_ok(p)]
    check("19-library", "posters rendered for the visible recordings", posters and len(made) == len(posters),
          {"posters": len(posters), "made": len(made)})

log = lines("20-pause")
if basic("20-pause", log):
    pr = P.get("drv pause")
    proj = pr["project"] if pr else {}
    pauses = proj.get("recording", {}).get("pauses", [])
    clips = proj.get("edl", {}).get("clips", [])
    span = (pauses[0]["end_host_ns"] - pauses[0]["start_host_ns"]) / 1e9 if pauses else 0
    check("20-pause", "one pause of about 1.5 s, cut from the timeline (two clips)",
          len(pauses) == 1 and 1.3 <= span <= 1.9 and len(clips) == 2, {"pauses": pauses, "clips": clips})
    st = [s["result"] for s in steps(log, 'cmd app {"action":"rec_status"}')]
    check("20-pause", "status says paused and the timer stopped",
          bool(st) and st[0].get("paused") is True and 1.8e9 <= st[0].get("elapsed_ns", 0) <= 3.5e9, st)
    ev = events(log, "recording-paused")
    check("20-pause", "paused and resumed events", [e.get("paused") for e in ev] == [True, False], ev)
    lv = [l for l in log if l.get("event") == "mic-level"]
    check("20-pause", "mic level reaches the bar while recording (about 15/s)", len(lv) >= 40, len(lv))

log = lines("21-camera")
if basic("21-camera", log):
    pr = P.get("drv camera")
    finalized("21-camera", pr, "drv camera", reason="user")
    if pr:
        segs = segments(pr)
        kinds = [s["source"].get("kind") for s in segs]
        check("21-camera", "camera, display, camera: one recording", kinds == ["camera", "display", "camera"], kinds)
        cams = [s for s in segs if s["source"].get("kind") == "camera"]
        check("21-camera", "camera segments carry the camera's own frame size",
              all(s["frame_px"]["w"] >= 320 and s["frame_px"]["h"] >= 240 for s in cams), [s["frame_px"] for s in cams])
        tracks = {t["kind"] for t in pr["project"]["recording"]["tracks"]}
        check("21-camera", "no overlay camera track; the mic is recorded", "camera" not in tracks and "mic" in tracks,
              sorted(tracks))
        failed = [e for e in pr["journal"] if e.get("ev") == "switch_failed"]
        check("21-camera", "both switches worked", not failed, failed)

log = lines("22-silence")
if basic("22-silence", log):
    pr = P.get("drv silence")
    finalized("22-silence", pr, "drv silence", reason="user")
    st = [x["result"] for x in steps(log, 'cmd editor {"action":"state"}')]
    found = [x["result"] for x in steps(log) if x["line"].startswith('cmd editor {"action":"silences"')]
    if st and len(found) == 2 and pr:
        rec = st[0]["rec_len_ns"]
        loud, auto = found
        check("22-silence", "a -5 dB threshold finds the whole quiet take as one silence",
              loud["ranges"] == [[0, loud["duration_ns"]]] and loud["removed_ns"] == loud["duration_ns"],
              {k: loud[k] for k in ("ranges", "removed_ns", "duration_ns")})
        check("22-silence", "the suggested threshold sits in -60..-20 dB and is used without one",
              -60 <= auto["suggested_db"] <= -20 and auto["threshold_db"] == auto["suggested_db"],
              {k: auto[k] for k in ("suggested_db", "threshold_db")})
        clips = pr["project"].get("edl", {}).get("clips", [])
        total = sum((c["src_out_rec_ns"] - c["src_in_rec_ns"]) * c["speed"]["den"] // c["speed"]["num"] for c in clips)
        check("22-silence", "one delete_ranges edit cut two seconds in two places",
              len(clips) == 2 and abs(total - (rec - 2_000_000_000)) <= 2_000_000,
              {"clips": [(c["src_in_rec_ns"], c["src_out_rec_ns"]) for c in clips], "rec": rec})
        blurs = pr["project"].get("redactions", [])
        check("22-silence", "the blur box is saved with the project", [b.get("id") for b in blurs] == ["drv-blur"], blurs)
        ex = [x for x in events(log, "export-state") if x.get("state") == "done"]
        check("22-silence", "the export with the blur box finished", bool(ex), [x.get("state") for x in events(log, "export-state")])
        if ex and FFMPEG:
            r = subprocess.run([FFMPEG, "-v", "error", "-ss", "0.5", "-i", ex[0].get("path", ""), "-frames:v", "1",
                                "-vf", "scale=160:-2", "-f", "rawvideo", "-pix_fmt", "gray", "-"], capture_output=True)
            px = r.stdout
            mean = sum(px) / len(px) if px else 255
            check("22-silence", "the screen is black under the box (export at 0.5 s)", px and mean < 20 and max(px) < 40,
                  {"mean": mean, "max": max(px) if px else None})

log = lines("23-shortcuts")
if basic("23-shortcuts", log, allow_fail=('cmd app {"action":"shortcut_set","id":"record","keys":"KeyR"}',)):
    res = [x for x in steps(log) if x["line"].startswith("cmd app")]
    rows = lambda r: {x["id"]: x for x in (r.get("result") or [])}
    first = rows(res[0])
    check("23-shortcuts", "five actions with their defaults, all registered",
          len(first) == 5 and all(x["keys"] == x["default"] and x["error"] is None for x in first.values()),
          {k: (v["keys"], v["error"]) for k, v in first.items()})
    set_pause = rows(res[1])["pause"]
    check("23-shortcuts", "a new shortcut registers", set_pause["keys"] == "Control+Alt+KeyP" and set_pause["error"] is None,
          set_pause)
    clash = rows(res[2])["switch"]
    check("23-shortcuts", "a clash with another action is reported", "Also used for" in (clash["error"] or ""), clash)
    bare = res[3]
    check("23-shortcuts", "a key without ⌘, ⌃ or ⌥ is refused", not bare["ok"] and "so typing" in (bare.get("error") or ""),
          bare.get("error"))
    after = rows(res[6])
    check("23-shortcuts", "restoring the key clears the clash", after["switch"]["error"] is None, after["switch"])
    st = [x["result"] for x in steps(log, "cmd get_settings")]
    check("23-shortcuts", "a cleared shortcut is saved as none", bool(st) and st[-1]["shortcuts"].get("pause") == "",
          st[-1]["shortcuts"] if st else None)

log = lines("24-long")
if log is not None and basic("24-long", log):
    st = [x["result"] for x in steps(log, 'cmd editor {"action":"state"}')]
    if len(st) == 2:
        opened, after = st
        check("24-long", "the 10-minute recording opened with its full length",
              opened.get("rec_len_ns", 0) >= 590_000_000_000, opened.get("rec_len_ns"))
        check("24-long", "first frame within 2 s of opening (definition of done)",
              (opened.get("first_frame_ms") or 99999) < 2000, opened.get("first_frame_ms"))
        check("24-long", "a seek near the end drew new frames",
              after.get("frames_presented", 0) > opened.get("frames_presented", 0),
              {"before": opened.get("frames_presented"), "after": after.get("frames_presented")})

log = lines("25-preview-rate")
if basic("25-preview-rate", log):
    ps = steps(log, "cmd preview_state")
    pr = P.get("drv preview rate")
    cam = next((t for t in (pr or {}).get("project", {}).get("recording", {}).get("tracks", []) if t["kind"] == "camera"), None)
    if len(ps) == 2 and cam:
        dt = (ps[1]["rec_ns"] - ps[0]["rec_ns"]) / 1e9
        rate = (ps[1]["result"].get("frame", 0) - ps[0]["result"].get("frame", 0)) / dt
        rec = pr["project"]["recording"]
        cam_fps = cam["health"]["frames"] / ((rec["stop_host_ns"] - rec["start_host_ns"]) / 1e9)
        check("25-preview-rate", "the Preview shows at least 90% of the camera's frames while recording",
              rate >= 0.9 * cam_fps, {"preview_fps": round(rate, 1), "camera_fps": round(cam_fps, 1)})

json.dump({"run": RUN, "projects": {k: os.path.basename(v["dir"]) for k, v in P.items()}, "results": results},
          open(os.path.join(RUN, "report.json"), "w"), indent=2)
failed = [r for r in results if not r["ok"]]
print(f"\n{len(results) - len(failed)}/{len(results)} checks passed")
sys.exit(1 if failed else 0)
