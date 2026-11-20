"""M1 end-to-end run (plan §8.2 wp-e2e-cli). Started by scripts/e2e/run.sh inside the live lock
with the fixture apps up; every recording goes through tools/macos-dev-host/run.sh. Standard
library only; frames and audio are decoded with the ffmpeg/ffprobe helpers (never $PATH)."""

import array
import json
import math
import os
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import threading
import time
import zlib

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
RUNSH = os.path.join(ROOT, "tools", "macos-dev-host", "run.sh")
CLI = os.environ["SC_E2E_CLI"]
DEVTOOLS = os.environ["SC_DEVTOOLS"]
FFMPEG = os.environ["SC_FFMPEG"]
FFPROBE = os.environ["SC_FFPROBE"]
KEEP = os.environ.get("SC_E2E_KEEP") == "1"
FIXTURE = json.load(open(os.environ["SC_FIXTURE"]))
APPS = {a["title"]: a for a in FIXTURE["apps"]}
# Everything the dev host's child touches (binary, cwd, projects) stays outside ~/Documents, so
# no recording depends on a Documents-folder TCC answer.
WORK = tempfile.mkdtemp(prefix="sc-e2e-")
SPAWNED = []
RESULTS = []


class Step:
    def __init__(self, n, title):
        self.n, self.title = n, title
        self.fails, self.warns, self.numbers, self.skipped = [], [], {}, None

    def check(self, ok, what):
        if not ok:
            self.fails.append(what)
        return ok

    def warn(self, what):
        self.warns.append(what)

    def num(self, key, value):
        self.numbers[key] = value


def log(msg):
    print(msg, flush=True)


def run(cmd, timeout=600, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, **kw)


def cli_json(*args):
    r = run([CLI, "--json", *args])
    if r.returncode != 0:
        raise RuntimeError(f"{args}: exit {r.returncode}: {r.stderr.strip()}")
    return json.loads(r.stdout.strip().splitlines()[-1])


class Rec:
    pass


def record(name, args, actions=(), term_at=None, stop_line_at=None, timeout_s=90):
    out = os.path.join(WORK, name)
    os.makedirs(out, exist_ok=True)
    projects = os.path.join(out, "projects")
    pid_file = os.path.join(out, "child.pid")
    env = dict(os.environ, SC_DEV_PID_FILE=pid_file)
    fifo = None
    if stop_line_at is not None:
        fifo = os.path.join(out, "stdin.fifo")
        os.mkfifo(fifo)
        env["SC_DEV_STDIN"] = fifo
    cmd = [RUNSH, CLI, "record", *args, "--events", "--projects-dir", projects]
    log(f"  record {name}: {' '.join(args)}")
    p = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                         env=env, cwd=WORK, start_new_session=True)
    rec = Rec()
    rec.events, rec.stdout_lines, rec.err = [], [], []
    rec.t0 = None

    def read_out():
        for line in p.stdout:
            line = line.rstrip("\n")
            rec.stdout_lines.append(line)
            if line.startswith("{"):
                try:
                    ev = json.loads(line)
                except ValueError:
                    continue
                ev["_t"] = time.monotonic()
                rec.events.append(ev)
                if ev.get("ev") == "started":
                    rec.t0 = ev["_t"]

    def read_err():
        for line in p.stderr:
            rec.err.append(line.rstrip("\n"))

    writer = {}

    def open_fifo():
        writer["f"] = open(fifo, "w")

    threads = [threading.Thread(target=read_out, daemon=True),
               threading.Thread(target=read_err, daemon=True)]
    if fifo:
        threads.append(threading.Thread(target=open_fifo, daemon=True))
    for t in threads:
        t.start()
    todo = list(actions)
    if term_at is not None:
        todo.append((term_at, lambda: p.send_signal(signal.SIGTERM)))
    if stop_line_at is not None:
        def send_stop():
            f = writer.get("f")
            if f:
                f.write("stop\n")
                f.flush()
        todo.append((stop_line_at, send_stop))
    todo.sort(key=lambda a: a[0])
    peak_kb, next_rss, deadline = 0, 0.0, time.monotonic() + timeout_s
    child = None
    while p.poll() is None:
        now = time.monotonic()
        if now > deadline:
            rec.err.append("e2e: timeout, killing the recording")
            if child is not None:
                try:
                    os.kill(child, signal.SIGKILL)
                except OSError:
                    pass
            os.killpg(p.pid, signal.SIGKILL)
            break
        if rec.t0 is not None:
            while todo and now - rec.t0 >= todo[0][0]:
                todo.pop(0)[1]()
        if child is None:
            try:
                child = int(open(pid_file).read().strip())
            except (OSError, ValueError):
                pass
        if child is not None and now >= next_rss:
            next_rss = now + 0.2
            r = subprocess.run(["ps", "-o", "rss=", "-p", str(child)], capture_output=True, text=True)
            if r.stdout.strip().isdigit():
                peak_kb = max(peak_kb, int(r.stdout.strip()))
        time.sleep(0.02)
    p.wait()
    for t in threads[:2]:
        t.join(timeout=5)
    if writer.get("f"):
        try:
            writer["f"].close()
        except OSError:
            pass
    rec.rc = p.returncode
    rec.peak_rss_mb = round(peak_kb / 1024, 1)
    rec.dir = next((l for l in reversed(rec.stdout_lines) if l and not l.startswith("{")), None)
    rec.project, rec.journal = None, []
    if rec.dir and os.path.isfile(os.path.join(rec.dir, "project.json")):
        rec.project = json.load(open(os.path.join(rec.dir, "project.json")))
        for line in open(os.path.join(rec.dir, "journal.jsonl")):
            try:
                rec.journal.append(json.loads(line))
            except ValueError:
                pass
    if rec.rc != 0:
        log("    stderr: " + " | ".join(rec.err[-8:]))
    return rec


def probe(path):
    r = run([FFPROBE, "-v", "error", "-count_frames", "-show_entries",
             "stream=codec_type,codec_name,width,height,nb_read_frames,duration,sample_rate,channels",
             "-show_entries", "format=duration", "-of", "json", path])
    if r.returncode != 0:
        return None
    return json.loads(r.stdout)


def frame_times(path):
    r = run([FFPROBE, "-v", "error", "-select_streams", "v:0", "-show_entries",
             "frame=best_effort_timestamp_time", "-of", "csv=p=0", path])
    out = []
    for line in r.stdout.split():
        try:
            out.append(float(line.strip(",")))
        except ValueError:
            pass
    return out


def frames_rgb(path, width=320, start=None, dur=None):
    pr = probe(path)
    v = next(s for s in pr["streams"] if s["codec_type"] == "video")
    h = max(2, int(round(width * v["height"] / v["width"] / 2)) * 2)
    cmd = [FFMPEG, "-v", "error"]
    if start is not None:
        cmd += ["-ss", f"{start:.3f}"]
    cmd += ["-i", path]
    if dur is not None:
        cmd += ["-t", f"{dur:.3f}"]
    cmd += ["-vf", f"scale={width}:{h}:flags=area", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"]
    data = subprocess.run(cmd, capture_output=True, timeout=600).stdout
    n = width * h * 3
    return width, h, [data[i:i + n] for i in range(0, len(data) - n + 1, n)]


def red(r, g, b):
    return r > 150 and g < 90 and b < 90


def magenta(r, g, b):
    return r > 150 and g < 100 and b > 150


def frac(frame, w, h, rect, pred):
    x0, y0, x1, y1 = (max(0, int(rect[0])), max(0, int(rect[1])),
                      min(w, int(rect[2])), min(h, int(rect[3])))
    if x1 <= x0 or y1 <= y0:
        return 0.0
    hit = tot = 0
    for y in range(y0, y1):
        row = frame[(y * w + x0) * 3:(y * w + x1) * 3]
        for i in range(0, len(row), 3):
            tot += 1
            if pred(row[i], row[i + 1], row[i + 2]):
                hit += 1
    return hit / tot if tot else 0.0


def pt_rect_in_frame(rect_pt, disp_pt, w, h):
    sx, sy = w / disp_pt[0], h / disp_pt[1]
    x, y, rw, rh = rect_pt
    return (x * sx, y * sy, (x + rw) * sx, (y + rh) * sy)


def pcm(path, rate=16000, start=None, dur=None):
    cmd = [FFMPEG, "-v", "error"]
    if start is not None:
        cmd += ["-ss", f"{start:.3f}"]
    cmd += ["-i", path]
    if dur is not None:
        cmd += ["-t", f"{dur:.3f}"]
    cmd += ["-ac", "1", "-ar", str(rate), "-f", "s16le", "-"]
    a = array.array("h")
    a.frombytes(subprocess.run(cmd, capture_output=True, timeout=120).stdout)
    return a


def goertzel(samples, rate, f):
    k = 2.0 * math.cos(2.0 * math.pi * f / rate)
    s1 = s2 = 0.0
    for x in samples:
        s1, s2 = x + k * s1 - s2, s1
    return s1 * s1 + s2 * s2 - k * s1 * s2


def rec_duration_ns(p):
    r = p["recording"]
    return r["stop_host_ns"] - r["start_host_ns"]


def track_path(rec, t):
    return os.path.join(rec.dir, t["file"])


def common_checks(st, rec, export=True, expect_reason=None):
    if not st.check(rec.rc == 0, f"record exit {rec.rc}"):
        return False
    p = rec.project
    if not st.check(p is not None, "no project.json"):
        return False
    st.check(p["state"] == "finalized", f"state {p['state']}")
    dur = rec_duration_ns(p)
    segs = p["recording"]["screen"]["segments"]
    tile = bool(segs) and segs[0]["start_rec_ns"] == 0 and segs[-1].get("end_rec_ns") == dur
    tile = tile and all(
        a.get("end_rec_ns") == b["start_rec_ns"]
        or (a.get("end_reason") == "source_lost" and a.get("end_rec_ns", 1 << 62) < b["start_rec_ns"])
        for a, b in zip(segs, segs[1:]))
    st.check(tile, "segments do not tile [0, duration) (only a lost source may leave a gap): "
             + str([(s["start_rec_ns"], s.get("end_rec_ns")) for s in segs]) + f" dur {dur}")
    fps = p["recording"]["screen"]["fps_requested"]
    frame_ns = 1e9 / fps
    by_seg = {s["id"]: s for s in segs}
    has_audio = False
    for t in p["recording"]["tracks"]:
        path = track_path(rec, t)
        pr = probe(path)
        if not st.check(pr is not None and pr.get("streams"), f"{t['file']} does not decode"):
            continue
        s0 = pr["streams"][0]
        if t["kind"] in ("screen", "camera"):
            st.check(int(s0.get("nb_read_frames") or 0) > 0, f"{t['file']}: no frames")
            size = t["video"]["size_px"]
            st.check((s0.get("width"), s0.get("height")) == (size["w"], size["h"]),
                     f"{t['file']}: {s0.get('width')}x{s0.get('height')} vs descriptor {size}")
            if t["kind"] == "screen":
                fp = by_seg[t["segment_id"]]["frame_px"]
                st.check((fp["w"], fp["h"]) == (size["w"], size["h"]),
                         f"{t['file']}: segment frame_px {fp} vs track {size}")
        else:
            has_audio = True
            d_ns = float(pr["format"]["duration"]) * 1e9
            start = t.get("first_sample_offset_ns") or 0
            st.num(f"{t['id']}_len_s", round(d_ns / 1e9, 3))
            st.num(f"{t['id']}_offset_ms", round(start / 1e6, 1))
            st.check(abs(d_ns - dur) <= frame_ns,
                     f"{t['file']}: length {d_ns / 1e9:.3f} s vs recording {dur / 1e9:.3f} s "
                     f"(offset {start / 1e6:.1f} ms), more than one frame apart")
    if expect_reason:
        stop = [e for e in rec.journal if e.get("ev") == "stop"]
        st.check(stop and stop[-1]["reason"] == expect_reason,
                 f"stop reason {stop[-1]['reason'] if stop else None}, want {expect_reason}")
    if export:
        export_check(st, rec, "480", has_audio)
    return not st.fails


def export_check(st, rec, size, audio, tag=None):
    name = os.path.basename(os.path.dirname(os.path.dirname(rec.dir)))
    out = os.path.join(WORK, f"{name}-{tag or size}.mp4")
    t0 = time.monotonic()
    r = run([CLI, "export", rec.dir, "--out", out, "--size", size])
    took = time.monotonic() - t0
    if not st.check(r.returncode == 0,
                    f"export {tag or size}: exit {r.returncode}: {r.stderr.strip()[-300:]}"):
        return None
    pr = probe(out)
    v = next((s for s in (pr or {}).get("streams", []) if s["codec_type"] == "video"), None)
    if not st.check(v is not None, f"export {tag or size}: no video stream"):
        return None
    w, h = v["width"], v["height"]
    ratio = rec.project["output"]["canvas"]["ratio"]
    want = ratio["num"] / ratio["den"]
    st.check(abs(w / h - want) / want < 0.02,
             f"export {tag or size}: {w}x{h} vs canvas ratio {want:.4f}")
    if size != "reference":
        longest = max(max(s["frame_px"]["w"], s["frame_px"]["h"])
                      for s in rec.project["recording"]["screen"]["segments"])
        st.check(min(w, h) == int(size) or (min(w, h) < int(size) and max(w, h) <= longest),
                 f"export {size}: {w}x{h}; sources are at most {longest} px long")
    dur_s = rec_duration_ns(rec.project) / 1e9
    fps = rec.project["recording"]["screen"]["fps_requested"]
    vargs = [CLI, "--json", "validate", out, "--size", f"{w}x{h}", "--fps", str(fps),
             "--duration", f"{dur_s:.3f}"]
    if audio:
        vargs.append("--audio")
    vr = run(vargs)
    st.check(vr.returncode == 0, f"validate {tag or size}: {vr.stderr.strip()[-300:]}")
    return {"path": out, "w": w, "h": h, "export_s": round(took, 2)}


def spawn_window(*args):
    p = subprocess.Popen([DEVTOOLS, "testapps", *args], stdout=subprocess.PIPE, text=True, cwd=WORK)
    ready = json.loads(p.stdout.readline())
    SPAWNED.append(p)
    threading.Thread(target=lambda: [None for _ in p.stdout], daemon=True).start()
    return p, ready


def journal_events(rec, kind):
    return [e for e in rec.journal if e.get("ev") == kind]


def switch_numbers(st, rec):
    p = rec.project
    start = p["recording"]["start_host_ns"]
    segs = p["recording"]["screen"]["segments"]
    tracks = {t["id"]: t for t in p["recording"]["tracks"]}
    reqs = [e for e in rec.events if e.get("ev") == "switch_request"]
    closes = {e["seg"]: e for e in journal_events(rec, "segment_close")}
    rows = []
    for i, (a, b) in enumerate(zip(segs, segs[1:])):
        req = reqs[i]["host_ns"] if i < len(reqs) else None
        cut = start + b["start_rec_ns"]
        ta, tb = tracks[a["track_id"]], tracks[b["track_id"]]
        fa, fb = frame_times(track_path(rec, ta)), frame_times(track_path(rec, tb))
        gap_ms = None
        if fa and fb:
            last_a = ta["first_sample_offset_ns"] / 1e9 + fa[-1]
            first_b = tb["first_sample_offset_ns"] / 1e9 + fb[0]
            gap_ms = round((first_b - last_a) * 1000, 1)
        row = {
            "from": a["source"]["kind"], "to": b["source"]["kind"],
            "request_to_cut_ms": round((cut - req) / 1e6, 1) if req else None,
            "trailing_dropped": closes.get(a["id"], {}).get("trailing_dropped"),
            "recorded_gap_ms": gap_ms,
        }
        rows.append(row)
        if row["request_to_cut_ms"] is not None and row["request_to_cut_ms"] > 500:
            st.warn(f"switch {i + 1} {row['from']}→{row['to']}: {row['request_to_cut_ms']} ms "
                    "request→cut, over the 500 ms target")
    return rows


def main_display():
    ds = cli_json("displays")
    return [d for d in ds if d.get("is_main")][0], ds


MAIN, DISPLAYS = main_display()
DISP_PT = (MAIN["frame_pt"]["w"], MAIN["frame_pt"]["h"])
SCALE = MAIN["scale"]
RED_APP = APPS["SC-TEST-RED"]
RED_RECT = RED_APP["rect_pt"]


def region_in_red(w, h):
    return f"region:main:{RED_RECT[0] + 20:g},{RED_RECT[1] + 20:g},{w:g},{h:g}"


def step1(st):
    rec = record("s1-display-av", ["--source", "display:main", "--duration", "10", "--mic", "default",
                                   "--system-audio", "all", "--camera", "default"], timeout_s=120)
    if not common_checks(st, rec, export=False):
        return
    kinds = sorted(t["kind"] for t in rec.project["recording"]["tracks"])
    st.check(kinds == ["camera", "mic", "screen", "system_audio"], f"tracks {kinds}")
    ex = export_check(st, rec, "reference", True)
    if ex:
        fp = rec.project["recording"]["screen"]["segments"][0]["frame_px"]
        k = min(1.0, 3840 / max(fp["w"], fp["h"]))
        st.num("reference_export", f"{ex['w']}x{ex['h']} in {ex['export_s']} s")
        st.check(abs(ex["w"] - fp["w"] * k) <= 2 and abs(ex["h"] - fp["h"] * k) <= 2,
                 f"reference export {ex['w']}x{ex['h']} vs frame {fp['w']}x{fp['h']} (cap 3840)")


def step2(st):
    rec = record("s2-window", ["--source", f"window:{RED_APP['window_id']}", "--duration", "8"])
    if not common_checks(st, rec):
        return
    seg = rec.project["recording"]["screen"]["segments"][0]
    want = (round(RED_RECT[2] * SCALE), round(RED_RECT[3] * SCALE))
    st.check((seg["frame_px"]["w"], seg["frame_px"]["h"]) == want,
             f"frame_px {seg['frame_px']} vs {want}")
    w, h, fr = frames_rgb(track_path(rec, rec.project["recording"]["tracks"][0]), 160,
                          start=3, dur=0.2)
    f = frac(fr[0], w, h, (0, 0, w, h), red) if fr else 0
    st.num("red_fraction", round(f, 3))
    st.check(f > 0.9, f"window frame is {f:.2f} red")


def step3(st):
    green = APPS["SC-TEST-GREEN"]
    rec = record("s3-app", ["--source", f"app-pid:{green['pid']}", "--duration", "8"],
                 actions=[(3.0, lambda: os.kill(green["pid"], signal.SIGUSR2))])
    if not common_checks(st, rec):
        return
    path = track_path(rec, rec.project["recording"]["tracks"][0])
    w, h, early = frames_rgb(path, 320, start=1.0, dur=0.1)
    _, _, late = frames_rgb(path, 320, start=6.0, dur=0.1)
    if not st.check(early and late, "no decoded frames"):
        return
    sec = pt_rect_in_frame(green["second_rect_pt"], DISP_PT, w, h)
    redr = pt_rect_in_frame(RED_RECT, DISP_PT, w, h)
    m0, m1 = frac(early[0], w, h, sec, magenta), frac(late[0], w, h, sec, magenta)
    r1 = frac(late[0], w, h, redr, red)
    st.num("second_window_magenta_before_after", [round(m0, 3), round(m1, 3)])
    st.num("other_app_red_fraction", round(r1, 3))
    st.check(m1 > 0.8, f"second window not in the recording after USR2 ({m1:.2f})")
    st.check(r1 < 0.05, f"SC-TEST-RED (another app) shows in the app recording ({r1:.2f})")


def step4(st):
    for tag, (w_pt, h_pt) in (("pt", (617, 283.5)), ("px", (1121 / SCALE, 641 / SCALE))):
        rec = record(f"s4-region-{tag}", ["--source", region_in_red(w_pt, h_pt), "--duration", "3"])
        if not common_checks(st, rec):
            continue
        fp = rec.project["recording"]["screen"]["segments"][0]["frame_px"]
        want = (int(w_pt * SCALE) // 2 * 2, int(h_pt * SCALE) // 2 * 2)
        st.num(f"region_{tag}_frame_px", f"{fp['w']}x{fp['h']}")
        st.check(abs(fp["w"] - want[0]) <= 2 and abs(fp["h"] - want[1]) <= 2
                 and fp["w"] % 2 == 0 and fp["h"] % 2 == 0,
                 f"region {tag}: frame {fp} vs about {want} (even)")
        ww, hh, fr = frames_rgb(track_path(rec, rec.project["recording"]["tracks"][0]), 160,
                                start=1.5, dur=0.1)
        f = frac(fr[0], ww, hh, (0, 0, ww, hh), red) if fr else 0
        st.check(f > 0.9, f"region {tag}: frame is {f:.2f} red")


def step5(st):
    rec = record("s5-switch-cycle", [
        "--source", "display:main", "--switch", f"3=window:{RED_APP['window_id']}",
        "--switch", f"6={region_in_red(400, 300)}", "--switch", "9=display:main",
        "--system-audio", "all", "--duration", "12"], timeout_s=120)
    if not common_checks(st, rec):
        return
    kinds = [s["source"]["kind"] for s in rec.project["recording"]["screen"]["segments"]]
    st.check(kinds == ["display", "window", "region", "display"], f"segment sources {kinds}")
    st.num("switches", switch_numbers(st, rec))
    st.num("peak_rss_mb", rec.peak_rss_mb)


def step6(st):
    t440 = APPS["SC-TONE-440"]
    _, t660 = spawn_window("tone", "--hz", "660", "--window", "--title", "SC-TONE-660W",
                           "--pos", "1100,850", "--size", "200x120", "--color", "yellow")
    time.sleep(0.5)
    rec = record("s6-app-audio", ["--source", f"window:{t440['window_id']}",
                                  "--system-audio", "app",
                                  "--switch", f"4=window:{t660['window_id']}", "--duration", "8"])
    if not common_checks(st, rec):
        return
    sa = [t for t in rec.project["recording"]["tracks"] if t["kind"] == "system_audio"]
    if not st.check(sa, "no system_audio track"):
        return
    segs = rec.project["recording"]["screen"]["segments"]
    if not st.check(len(segs) == 2, f"{len(segs)} segments"):
        return
    cut = segs[1]["start_rec_ns"] / 1e9
    off = (sa[0].get("first_sample_offset_ns") or 0) / 1e9
    end = rec_duration_ns(rec.project) / 1e9
    spans = {"440": (0.5, cut - 0.4), "660": (cut + 0.6, end - 0.3)}
    out = {}
    for want, (a, b) in spans.items():
        s = pcm(track_path(rec, sa[0]), start=max(0.0, a - off), dur=min(1.5, b - a))
        p440, p660 = goertzel(s, 16000, 440), goertzel(s, 16000, 660)
        ratio = (p440 / p660 if p660 else 1e12) if want == "440" else (p660 / p440 if p440 else 1e12)
        out[want] = round(10 * math.log10(max(ratio, 1e-12)), 1)
        st.check(ratio > 10, f"span {a:.1f}-{b:.1f} s: {want} Hz is only {out[want]} dB over "
                 "the other tone")
    st.num("tone_dominance_db", out)


def step7(st):
    _, lost = spawn_window("window", "--title", "SC-TEST-LOST", "--color", "red",
                           "--size", "300x110", "--pos", "380,850", "--floating", "--animate",
                           "--close-on-signal", "USR1")
    time.sleep(0.5)
    rec = record("s7-source-lost", ["--source", f"window:{lost['window_id']}",
                                    "--system-audio", "all", "--switch", "7=display:main",
                                    "--duration", "10"],
                 actions=[(4.0, lambda: os.kill(lost["pid"], signal.SIGUSR1))])
    if not common_checks(st, rec):
        return
    segs = rec.project["recording"]["screen"]["segments"]
    if not st.check(len(segs) == 2, f"{len(segs)} segments, want 2"):
        return
    st.check(segs[0]["end_reason"] == "source_lost", f"s0 end_reason {segs[0]['end_reason']}")
    st.check(segs[1]["id"] == "s1" and segs[1]["source"]["kind"] == "display",
             f"s1 {segs[1]['source']}")
    lost_ev = journal_events(rec, "source_lost")
    start = rec.project["recording"]["start_host_ns"]
    if st.check(lost_ev, "no source_lost journal event"):
        st.num("source_lost_at_s", round((lost_ev[0]["host_ns"] - start) / 1e9, 2))
    tracks = rec.project["recording"]["tracks"]
    s0t = next(t for t in tracks if t.get("segment_id") == "s0")
    ft = frame_times(track_path(rec, s0t))
    st.num("s0_last_frame_s", round(ft[-1], 2) if ft else None)
    st.num("s1_start_s", round(segs[1]["start_rec_ns"] / 1e9, 2))
    st.check(abs(segs[1]["start_rec_ns"] / 1e9 - 7.0) < 1.0, "s1 does not open near the 7 s switch")
    sa = next(t for t in tracks if t["kind"] == "system_audio")
    st.check(not sa["health"]["gaps"], f"system audio gaps {sa['health']['gaps']}")


def step8(st):
    _, mini = spawn_window("window", "--title", "SC-TEST-MIN", "--color", "blue",
                           "--size", "320x240", "--pos", "720,700", "--minimized")
    st.num("minimized_as", mini.get("minimized"))
    stat = APPS["SC-STATIC"]
    rec = record("s8-static-minimized", ["--source", "display:main",
                                         "--switch", f"2=window:{stat['window_id']}",
                                         "--switch", f"5=window:{mini['window_id']}",
                                         "--duration", "9"])
    if not common_checks(st, rec):
        return
    segs = rec.project["recording"]["screen"]["segments"]
    st.check([s["source"]["kind"] for s in segs] == ["display", "window"],
             f"segments {[s['source'].get('window_title', s['source']['kind']) for s in segs]}")
    if len(segs) >= 2:
        st.check(segs[1]["source"].get("window_title") == "SC-STATIC", f"s1 {segs[1]['source']}")
        st.num("static_switch", switch_numbers(st, rec)[0])
    failed = journal_events(rec, "switch_failed")
    st.check(any(f.get("next") == "keep_old" for f in failed), f"switch_failed {failed}")
    notices = [e for e in rec.events if e.get("ev") == "notice" and e.get("notice") == "switch_failed"]
    st.check(notices, "no switch_failed notice event")


def step9(st):
    for res in ("native", "1x"):
        rec = record(f"s9-fps60-{res}", ["--source", "display:main", "--fps", "60",
                                         "--resolution", res, "--duration", "4"])
        if not common_checks(st, rec):
            continue
        t = rec.project["recording"]["tracks"][0]
        seg = rec.project["recording"]["screen"]["segments"][0]
        rate = t["health"]["frames"] / (t["duration_ns"] / 1e9)
        st.num(f"fps60_{res}", {"nominal_fps": t["video"]["nominal_fps"],
                                "delivered_fps": round(rate, 1),
                                "frame_px": f"{seg['frame_px']['w']}x{seg['frame_px']['h']}"})
        st.check(t["video"]["nominal_fps"] == 60, f"{res}: nominal fps {t['video']['nominal_fps']}")
        k = SCALE if res == "native" else 1
        want = (round(DISP_PT[0] * k), round(DISP_PT[1] * k))
        st.check((seg["frame_px"]["w"], seg["frame_px"]["h"]) == want,
                 f"{res}: frame {seg['frame_px']} vs {want}")


def step10(st):
    rec = record("s10-1x-nocursor", ["--source", "display:main", "--resolution", "1x",
                                     "--no-cursor", "--duration", "3"])
    if not common_checks(st, rec):
        return
    seg = rec.project["recording"]["screen"]["segments"][0]
    fpt = seg["source"]["frame_pt"]
    st.check((seg["frame_px"]["w"], seg["frame_px"]["h"]) == (round(fpt["w"]), round(fpt["h"])),
             f"frame {seg['frame_px']} vs frame_pt {fpt}")
    st.check(rec.project["recording"]["screen"]["cursor"] is False, "project cursor is not false")
    cfg = [l for l in open(os.path.join(rec.dir, "session.log")) if " INFO config {" in l]
    st.check(cfg and '"cursor":false' in cfg[0], f"config log: {cfg[:1]}")


def step11(st):
    others = [d for d in DISPLAYS if not d.get("is_main")]
    if not others:
        st.skipped = "one display"
        return
    d = others[0]
    rec = record("s11-display2", ["--source", f"display:{d['id']}", "--duration", "3"])
    if common_checks(st, rec):
        fp = rec.project["recording"]["screen"]["segments"][0]["frame_px"]
        st.check((fp["w"], fp["h"]) == (d["size_px"]["w"], d["size_px"]["h"]),
                 f"display 2 frame {fp}")
    rec = record("s11-region2", ["--source", f"region:{d['id']}:40,40,400,300", "--duration", "3"])
    common_checks(st, rec)


def step12(st):
    rec = record("s12-own-window", ["--source", "display:main", "--own-test-window", "2=4",
                                    "--duration", "8"])
    if not common_checks(st, rec):
        return
    shown = [e for e in rec.events if e.get("ev") == "own_window" and e.get("state") == "shown"]
    if not st.check(shown, "own test window never shown"):
        return
    x, y, w_pt, h_pt = shown[0]["cocoa_frame_pt"]
    rect_pt = (x, DISP_PT[1] - y - h_pt, w_pt, h_pt)
    t = rec.project["recording"]["tracks"][0]
    w, h, frames = frames_rgb(track_path(rec, t), 240)
    rect = pt_rect_in_frame(rect_pt, DISP_PT, w, h)
    worst = max((frac(f, w, h, rect, magenta) for f in frames), default=1.0)
    st.num("own_window_frames_checked", len(frames))
    st.num("own_window_max_magenta_fraction", round(worst, 4))
    st.check(len(frames) > 30 and worst < 0.02, f"own test window colour in a frame ({worst:.3f})")


def write_png(path, rgb, size=64):
    raw = b"".join(b"\x00" + bytes(rgb) * size for _ in range(size))

    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)

    png = (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 2, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))
    open(path, "wb").write(png)


def step13(st):
    png = os.path.join(WORK, "bg.png")
    write_png(png, (30, 160, 60))
    rec = record("s13-aspects", ["--source", f"window:{RED_APP['window_id']}", "--duration", "3",
                                 "--background", f"image:{png}"])
    if not common_checks(st, rec, export=False):
        return
    pj = os.path.join(rec.dir, "project.json")
    base = json.load(open(pj))
    image_bg = base["output"]["canvas"]["background"]
    st.check(image_bg.get("kind") == "image", f"background {image_bg}")
    blue = [0.1, 0.2, 0.9, 1.0]
    bgs = {
        "solid": {"kind": "solid", "color": blue},
        "linear": {"kind": "linear", "angle_deg": 45.0,
                   "stops": [{"at": 0.0, "color": blue}, {"at": 1.0, "color": [0.9, 0.9, 0.1, 1.0]}]},
        "radial": {"kind": "radial", "center_norm": {"x": 0.5, "y": 0.5}, "radius_norm": 0.8,
                   "stops": [{"at": 0.0, "color": blue}, {"at": 1.0, "color": [0.1, 0.9, 0.9, 1.0]}]},
        "image": image_bg,
    }
    content = base["output"]["canvas"]["ratio"]
    content_r = content["num"] / content["den"]
    ratios = {"original": (content["num"], content["den"]), "16:9": (16, 9), "4:3": (4, 3),
              "1:1": (1, 1), "9:16": (9, 16)}
    done = 0
    for aspect, (num, den) in ratios.items():
        for kind, bg in bgs.items():
            p = json.loads(json.dumps(base))
            p["output"]["canvas"].update({"aspect": aspect, "ratio": {"num": num, "den": den},
                                          "background": bg})
            tmp = pj + ".tmp"
            json.dump(p, open(tmp, "w"))
            os.replace(tmp, pj)
            rec.project = p
            ex = export_check(st, rec, "480", False, tag=f"{aspect.replace(':', 'x')}-{kind}")
            if not ex:
                continue
            done += 1
            w, h, fr = frames_rgb(ex["path"], ex["w"], start=1.5, dur=0.05)
            if not fr:
                st.check(False, f"{aspect}/{kind}: export does not decode")
                continue
            mid = ((h // 2) * w + w // 2) * 3
            c = fr[0][mid:mid + 3]
            st.check(red(*c), f"{aspect}/{kind}: centre pixel {tuple(c)} is not the red window")
            canvas_r = num / den
            if kind == "solid" and abs(canvas_r - content_r) > 0.05:
                x, y = (3, h // 2) if canvas_r > content_r else (w // 2, 3)
                px = fr[0][(y * w + x) * 3:(y * w + x) * 3 + 3]
                want = [round(v * 255) for v in blue[:3]]
                st.check(all(abs(a - b) <= 16 for a, b in zip(px, want)),
                         f"{aspect}/solid: bar pixel {tuple(px)} vs background {want}")
    st.num("exports", done)


def step14(st):
    rec = record("s14-sigterm", ["--source", "display:main"], term_at=3.0)
    common_checks(st, rec, expect_reason="signal")
    rec = record("s14-stdin-stop", ["--source", "display:main"], stop_line_at=3.0)
    common_checks(st, rec, expect_reason="user")


STEPS = [
    (1, "main display 10 s, mic + system audio + camera, reference export", step1),
    (2, "window SC-TEST-RED 8 s", step2),
    (3, "application by app-pid (SC-TEST-GREEN), second window at 3 s", step3),
    (4, "regions 617x283.5 pt and 1121x641 px", step4),
    (5, "display → window → region → display every 3 s, system audio All", step5),
    (6, "system audio App: SC-TONE-440 → SC-TONE-660W", step6),
    (8, "switch to SC-STATIC, then to a minimized window", step8),
    (9, "--fps 60 native and 1x", step9),
    (10, "--resolution 1x --no-cursor", step10),
    (11, "non-main display and region", step11),
    (12, "--own-test-window never recorded", step12),
    (13, "every aspect × background kind (export only)", step13),
    (14, "graceful stop: SIGTERM to run.sh, stop line on stdin", step14),
    (7, "source lost (window closed at 4 s), switch at 7 s", step7),
]


def perms():
    r = run([RUNSH, CLI, "--json", "perms"], timeout=300, cwd=WORK)
    try:
        return json.loads(r.stdout.strip().splitlines()[-1])
    except (ValueError, IndexError):
        return {"error": r.stderr.strip()[-200:]}


def main(argv):
    want = {int(a) for a in argv} if argv else None
    before = perms()
    log(f"e2e: permissions before {before}")
    for n, title, fn in STEPS:
        if want and n not in want:
            continue
        st = Step(n, title)
        log(f"step {n}: {title}")
        t0 = time.monotonic()
        try:
            fn(st)
        except Exception as e:
            st.fails.append(f"exception: {e!r}")
        st.numbers["wall_s"] = round(time.monotonic() - t0, 1)
        RESULTS.append(st)
        if st.skipped:
            log(f"SKIPPED: step {n}: {st.skipped}")
        else:
            log(f"{'PASS' if not st.fails else 'FAIL'} step {n}: {json.dumps(st.numbers)}")
        for w in st.warns:
            log(f"  WARN {w}")
        for f in st.fails:
            log(f"  FAIL {f}")
    after = perms()
    log(f"e2e: permissions after {after}")
    granted = all(after.get(k) == "granted" for k in ("screen", "camera", "microphone"))
    report = {
        "permissions_before": before, "permissions_after": after,
        "steps": [{"n": s.n, "title": s.title, "pass": not s.fails, "skipped": s.skipped,
                   "fails": s.fails, "warns": s.warns, "numbers": s.numbers} for s in RESULTS],
    }
    out_dir = os.path.join(ROOT, "target", "e2e")
    os.makedirs(out_dir, exist_ok=True)
    path = os.path.join(out_dir, time.strftime("report-%Y%m%d-%H%M%S.json"))
    json.dump(report, open(path, "w"), indent=2)
    failed = [s.n for s in RESULTS if s.fails]
    ran = len([s for s in RESULTS if not s.skipped])
    log(f"e2e: ran {ran} steps, {ran - len(failed)} passed, {len(failed)} failed"
        f"{' ' + str(failed) if failed else ''}; report {path}")
    if not granted:
        log("e2e: FAILED: a permission is no longer granted")
    return 0 if not failed and granted else 1


def cleanup():
    for p in SPAWNED:
        p.terminate()
    for p in SPAWNED:
        try:
            p.wait(timeout=3)
        except subprocess.TimeoutExpired:
            p.kill()
    if KEEP:
        log(f"e2e: kept recordings in {WORK}")
    else:
        shutil.rmtree(WORK, ignore_errors=True)


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(130))
    try:
        rc = main(sys.argv[1:])
    finally:
        cleanup()
    sys.exit(rc)
