#!/usr/bin/env python3
"""Validation only. swap_analyze.py DIR -> DIR/analysis.json; swap_analyze.py --summary ROOT -> ROOT/summary.json"""
import array
import json
import math
import subprocess
import sys
from pathlib import Path


def run(args):
    return subprocess.run(args, capture_output=True, text=True)


def probe(path):
    r = run(["ffprobe", "-v", "error", "-show_entries",
             "stream=codec_name,codec_tag_string,width,height,pix_fmt,color_space,color_transfer,color_primaries,nb_frames,duration,sample_rate,channels:format=duration,size",
             "-of", "json", str(path)])
    info = json.loads(r.stdout or "{}")
    info["probe_stderr"] = r.stderr.strip()[:300] or None
    dec = run(["ffmpeg", "-v", "error", "-i", str(path), "-f", "null", "-"])
    lines = [l for l in dec.stderr.splitlines() if l.strip()]
    mux = [l for l in lines if "non monotonically increasing dts" in l or "Last message repeated" in l]
    errs = [l for l in lines if l not in mux]
    info["decode_error_lines"] = len(errs)
    info["decode_error_sample"] = errs[:3]
    info["null_muxer_dts_warnings"] = len(mux)
    cnt = run(["ffprobe", "-v", "error", "-count_frames", "-select_streams", "0", "-show_entries", "stream=nb_read_frames", "-of", "csv=p=0", str(path)])
    info["ffprobe_decode_stderr_lines"] = len([l for l in cnt.stderr.splitlines() if l.strip()])
    try:
        info["decoded_frames"] = int(cnt.stdout.strip().split(",")[0])
    except ValueError:
        info["decoded_frames"] = None
    return info


def pcm(path):
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", str(path), "-ac", "1", "-ar", "48000", "-f", "f32le", "-"], capture_output=True).stdout
    a = array.array("f")
    a.frombytes(raw[: len(raw) // 4 * 4])
    return a


def goertzel_db(x, start, n, hz, sr=48000):
    k = 2 * math.cos(2 * math.pi * hz / sr)
    s1 = s2 = 0.0
    for i in range(start, start + n):
        s0 = x[i] + k * s1 - s2
        s2, s1 = s1, s0
    p = s1 * s1 + s2 * s2 - k * s1 * s2
    return 10 * math.log10(p / (n * n) + 1e-20)


def tone_track(x, hz, win=480):
    return [goertzel_db(x, i, win, hz) for i in range(0, len(x) - win, win)]


def tone_report(levels, win_s, switch_rel, lo, hi):
    idx = [i for i in range(len(levels)) if lo + 0.3 <= i * win_s <= hi - 0.3]
    if not idx:
        return None
    vals = sorted(levels[i] for i in idx)
    med = vals[len(vals) // 2]
    drops = [i for i in idx if levels[i] < med - 12]
    runs = []
    for i in drops:
        if runs and i == runs[-1][1] + 1:
            runs[-1][1] = i
        else:
            runs.append([i, i])
    near = []
    for s in switch_rel:
        seg = [levels[i] for i in idx if abs(i * win_s - s) <= 0.5]
        near.append({"switch_rel_s": round(s, 3), "min_db_within_0p5s": round(min(seg), 1) if seg else None})
    return {
        "median_db": round(med, 1),
        "checked_from_to_rel_s": [round(lo + 0.3, 2), round(hi - 0.3, 2)],
        "windows_checked": len(idx),
        "windows_below_median_minus_12db": len(drops),
        "dropout_runs": [{"from_rel_s": round(a * win_s, 3), "to_rel_s": round((b + 1) * win_s, 3)} for a, b in runs][:10],
        "around_switches": near,
    }


def analyze(d):
    d = Path(d)
    pj = json.loads((d / "project.json").read_text())
    res = {"project": d.name, "strategy": pj.get("strategy"), "audio_mode": pj.get("audio_mode"), "update_order": pj.get("update_order"),
           "permission_host": pj.get("permission_host"), "errors": pj.get("errors"), "stream_errors": pj.get("stream_errors")}
    segs = []
    for s in pj.get("screen_segments", []):
        f = d / s["file"]
        p = probe(f) if f.exists() else None
        st = (p.get("streams") or [{}])[0] if p else {}
        intended = (s["end_host_s"] - s["start_host_s"]) if s.get("end_host_s") and s.get("start_host_s") else None
        w = s.get("writer") or {}
        segs.append({
            "file": s["file"], "source": s["source"]["kind"], "pixels": s["pixels"],
            "codec": st.get("codec_name"), "tag": st.get("codec_tag_string"), "size": [st.get("width"), st.get("height")],
            "color": [st.get("color_primaries"), st.get("color_transfer"), st.get("color_space")],
            "ffprobe_duration_s": float(p["format"]["duration"]) if p and p.get("format", {}).get("duration") else None,
            "intended_duration_s": intended,
            "nb_frames": st.get("nb_frames"), "decoded_frames": p.get("decoded_frames") if p else None,
            "frames_written_by_app": s.get("frames_written"), "held_last_frame_s": w.get("held_last_frame_s"),
            "decode_error_lines": p.get("decode_error_lines") if p else None, "decode_error_sample": p.get("decode_error_sample") if p else None,
            "ffprobe_decode_stderr_lines": p.get("ffprobe_decode_stderr_lines") if p else None, "null_muxer_dts_warnings": p.get("null_muxer_dts_warnings") if p else None,
            "fps": s.get("fps"), "late_or_missing_frames": s.get("late_or_missing_frames"), "content_classes": s.get("content_classes"),
            "encoder_not_ready_drops": w.get("encoder_not_ready_drops"), "append_failures": w.get("append_failures"),
            "size_matches_source": st.get("width") == s["pixels"][0] and st.get("height") == s["pixels"][1],
        })
    res["segments"] = segs
    t_start = pj["recording_start_host_s"]
    switch_host = [m["request_host_s"] for m in pj.get("switch_metrics", [])]
    keys = ["from", "to", "api_ms", "gap_last_old_to_first_new_ms", "request_to_first_new_pts_ms", "request_to_first_new_arrival_ms",
            "old_content_frames_arriving_after_request", "old_content_frames_with_pts_after_request", "old_content_frames_after_api_done_not_in_old_segment",
            "frames_in_old_segment_not_old_content", "frames_in_new_segment_not_new_content", "transition_frames_dropped", "transition_dropped_kinds",
            "frame_sequence_around_switch", "frame_geometry_around_switch"]
    res["switches"] = [{k: m.get(k) for k in keys} for m in pj.get("switch_metrics", [])]
    res["switch_steps"] = [[{k: s.get(k) for k in ("step", "ms", "error")} for s in sw["steps"]] for sw in pj.get("switches", [])]
    tracks = {t["kind"]: t for t in pj.get("tracks", [])}
    audio = {}
    for kind in ["system_audio", "mic"]:
        t = tracks.get(kind)
        if not t:
            continue
        f = d / t["file"]
        p = probe(f)
        st = (p.get("streams") or [{}])[0]
        cont = pj.get(f"{kind}_continuity", {})
        a = {"codec": st.get("codec_name"), "sample_rate": st.get("sample_rate"), "channels": st.get("channels"),
             "ffprobe_duration_s": float(p["format"]["duration"]) if p.get("format", {}).get("duration") else None,
             "decode_error_lines": p.get("decode_error_lines"),
             "first_pts_rel_to_start_s": t["first_pts_host_s"] - t_start if t.get("first_pts_host_s") else None,
             "continuity_from_buffers": {k: v for k, v in cont.items() if k != "gaps"},
             "buffer_gaps": cont.get("gaps")}
        if kind == "system_audio" and t.get("first_pts_host_s"):
            x = pcm(f)
            rel = [h - t["first_pts_host_s"] for h in switch_host]
            lo, hi = 0.0, len(x) / 48000
            for h in pj.get("helpers", []):
                if h.get("tone_started_host_s"):
                    lo = max(lo, h["tone_started_host_s"] - t["first_pts_host_s"])
            a["tone_440_window_owner"] = tone_report(tone_track(x, 440), 0.01, rel, lo, hi)
            a["tone_660_windowless_process"] = tone_report(tone_track(x, 660), 0.01, rel, lo, hi)
            a["switch_rel_to_file_s"] = [round(r, 3) for r in rel]
        audio[kind] = a
    res["audio"] = audio
    res["cpu_pct_of_one_core_total"] = pj.get("cpu_pct_of_one_core_total")
    res["cpu_per_segment"] = [c["cpu_pct_of_one_core"] for c in pj.get("cpu_per_segment", [])]
    res["sck_frame_status"] = pj.get("sck_frame_status")
    res["self_rss_max_mb"] = pj.get("self_rss_max_mb")
    (d / "analysis.json").write_text(json.dumps(res, indent=1))
    return res


def r1(v):
    return round(v, 1) if isinstance(v, (int, float)) else v


def summary(root):
    root = Path(root)
    rows = []
    for a in sorted(root.glob("*/analysis.json")):
        r = json.loads(a.read_text())
        sa = r["audio"].get("system_audio", {})
        rows.append({
            "run": r["project"], "strategy": r["strategy"], "audio_mode": r["audio_mode"], "order": r["update_order"],
            "switch_gap_ms": [r1(s["gap_last_old_to_first_new_ms"]) for s in r["switches"]],
            "api_ms": [r1(s["api_ms"]) for s in r["switches"]],
            "req_to_first_new_arrival_ms": [r1(s["request_to_first_new_arrival_ms"]) for s in r["switches"]],
            "old_content_after_request": [s["old_content_frames_arriving_after_request"] for s in r["switches"]],
            "old_content_after_api_done": [s["old_content_frames_after_api_done_not_in_old_segment"] for s in r["switches"]],
            "wrong_content_in_segments": [s["frames_in_old_segment_not_old_content"] + s["frames_in_new_segment_not_new_content"] for s in r["switches"]],
            "transition_dropped": [s["transition_frames_dropped"] for s in r["switches"]],
            "segments": [{"src": s["source"], "size": s["size"], "codec": s["codec"], "dur": r1(s["ffprobe_duration_s"]), "intended": r1(s["intended_duration_s"]),
                          "fps": r1(s["fps"]), "late": s["late_or_missing_frames"], "dec_err": s["decode_error_lines"],
                          "enc_drops": s["encoder_not_ready_drops"]} for s in r["segments"]],
            "sys_audio_buffer_gaps_ms": [r1(g["gap_ms"]) for g in (sa.get("buffer_gaps") or [])],
            "sys_audio_dur": r1(sa.get("ffprobe_duration_s")),
            "tone440_dropouts": (sa.get("tone_440_window_owner") or {}).get("dropout_runs"),
            "tone660_dropouts": (sa.get("tone_660_windowless_process") or {}).get("dropout_runs"),
            "mic_buffer_gaps": r["audio"].get("mic", {}).get("continuity_from_buffers", {}).get("gaps_over_1ms"),
            "cpu_total": r["cpu_pct_of_one_core_total"],
            "errors": (r["errors"] or []) + (r["stream_errors"] or []),
        })
    (root / "summary.json").write_text(json.dumps(rows, indent=1))
    print(json.dumps(rows, indent=1))


if __name__ == "__main__":
    if sys.argv[1] == "--summary":
        summary(sys.argv[2])
    else:
        print(json.dumps(analyze(sys.argv[1]), indent=1))
