#!/usr/bin/env python3
"""Validation only: uses Homebrew ffprobe/ffmpeg to inspect a spike project folder."""
import array
import json
import math
import subprocess
import sys
from pathlib import Path


def run(args):
    return subprocess.run(args, capture_output=True, text=True).stdout


def probe(path):
    if not path.exists():
        return None
    out = run(["ffprobe", "-v", "error", "-show_entries",
               "stream=codec_name,codec_tag_string,profile,width,height,pix_fmt,color_range,color_space,color_transfer,color_primaries,nb_frames,duration,sample_rate,channels,bit_rate:format=duration,size,bit_rate",
               "-of", "json", str(path)])
    info = json.loads(out or "{}")
    keys = run(["ffprobe", "-v", "error", "-select_streams", "v", "-show_entries", "packet=pts_time,flags", "-of", "csv=p=0", str(path)])
    kf = [float(l.split(",")[0]) for l in keys.splitlines() if "K" in l.split(",")[-1]]
    if kf:
        gaps = [b - a for a, b in zip(kf, kf[1:])]
        info["keyframes"] = {"count": len(kf), "max_gap_s": max(gaps) if gaps else None}
    return info


def pcm(path):
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", str(path), "-ac", "1", "-ar", "48000", "-f", "f32le", "-"], capture_output=True).stdout
    a = array.array("f")
    a.frombytes(raw[: len(raw) // 4 * 4])
    return a


def envelope(samples, win=480):
    env = []
    for i in range(0, len(samples) - win, win):
        s = sum(x * x for x in samples[i:i + win]) / win
        env.append(10 * math.log10(s + 1e-12))
    return env


def onset_near(env, t_rel, before=0.3, after=0.8, win_s=0.01):
    lo = max(0, int((t_rel - before) / win_s))
    hi = min(len(env), int((t_rel + after) / win_s))
    if hi <= lo:
        return None
    floor_seg = env[max(0, lo - 100):lo] or env[lo:lo + 5]
    floor = sorted(floor_seg)[len(floor_seg) // 2]
    peak = max(env[lo:hi])
    for i in range(lo, hi):
        if env[i] > floor + 20 and env[i] > -60:
            return {"onset_rel_s": i * win_s, "floor_db": round(floor, 1), "peak_db": round(peak, 1)}
    return {"onset_rel_s": None, "floor_db": round(floor, 1), "peak_db": round(peak, 1)}


def video_flash(path, crop, t_rel):
    vf = "signalstats,metadata=print:key=lavfi.signalstats.YAVG"
    if crop:
        x, y, w, h = [int(v) // 2 * 2 for v in crop]
        vf = f"crop={w}:{h}:{x}:{y}," + vf
    out = subprocess.run(["ffmpeg", "-v", "info", "-nostats", "-i", str(path), "-vf", vf, "-f", "null", "-"], capture_output=True, text=True).stderr
    pts, frames = None, []
    for line in out.splitlines():
        if "pts_time:" in line:
            pts = float(line.split("pts_time:")[1].split()[0])
        elif "YAVG=" in line and pts is not None:
            frames.append((pts, float(line.split("YAVG=")[1])))
    for p, y in frames:
        if p >= t_rel - 0.5 and y > 200:
            return {"flash_frame_rel_s": p, "yavg": y}
    return {"flash_frame_rel_s": None, "max_yavg": max((y for _, y in frames), default=None)}


def main(d):
    d = Path(d)
    pj = json.loads((d / "project.json").read_text())
    res = {"project": str(d), "status": pj.get("status"), "files": {}}
    for f in ["screen.mov", "camera.mov", "mic.mov", "system_audio.mov"]:
        res["files"][f] = probe(d / f)
    tracks = {t["kind"]: t for t in pj.get("tracks", [])}
    ev = {e["event"]: e for e in pj.get("events", [])}
    scale = pj.get("display", {}).get("point_pixel_scale", 2.0)
    checks = {}
    flash = ev.get("flash_and_glass_sound")
    ping = ev.get("self_sound_ping")
    for kind in ["system_audio", "mic"]:
        t = tracks.get(kind)
        if not t or not (d / t["file"]).exists():
            continue
        env = envelope(pcm(d / t["file"]))
        first = t["first_pts_host_s"]
        c = {}
        if flash:
            o = onset_near(env, flash["host_s"] - first)
            if o and o["onset_rel_s"] is not None:
                o["onset_host_s"] = first + o["onset_rel_s"]
                o["onset_minus_event_ms"] = round((o["onset_host_s"] - flash["host_s"]) * 1e3, 1)
            c["other_process_glass_sound"] = o
        if ping:
            o = onset_near(env, ping["host_s"] - first)
            if o and o["onset_rel_s"] is not None:
                o["onset_host_s"] = first + o["onset_rel_s"]
                o["onset_minus_event_ms"] = round((o["onset_host_s"] - ping["host_s"]) * 1e3, 1)
            c["own_process_ping_sound"] = o
        checks[kind] = c
    st = tracks.get("screen")
    if flash and st and (d / "screen.mov").exists():
        win = ev.get("activity_window")
        crop = None
        if win and "single window" not in pj.get("filter", ""):
            r = win["rect_points_top_left"]
            crop = [r[0] * scale, r[1] * scale, r[2] * scale, r[3] * scale]
        v = video_flash(d / "screen.mov", crop, flash["host_s"] - st["first_pts_host_s"])
        if v.get("flash_frame_rel_s") is not None:
            v["flash_host_s"] = st["first_pts_host_s"] + v["flash_frame_rel_s"]
            v["flash_minus_event_ms"] = round((v["flash_host_s"] - flash["host_s"]) * 1e3, 1)
            sa = checks.get("system_audio", {}).get("other_process_glass_sound") or {}
            if sa.get("onset_host_s"):
                v["system_audio_onset_minus_video_flash_ms"] = round((sa["onset_host_s"] - v["flash_host_s"]) * 1e3, 1)
            mic = checks.get("mic", {}).get("other_process_glass_sound") or {}
            if mic.get("onset_host_s"):
                v["mic_onset_minus_video_flash_ms"] = round((mic["onset_host_s"] - v["flash_host_s"]) * 1e3, 1)
        checks["screen_flash"] = v
    res["sync_checks"] = checks
    kj = d / "kill.json"
    if kj.exists():
        k = json.loads(kj.read_text())
        kill = {"killed_after_s": k["killed_after_s"], "tracks": {}}
        for f in ["screen.mov", "camera.mov", "mic.mov", "system_audio.mov"]:
            side = d / f.replace(".mov", ".start.json")
            if not (d / f).exists():
                continue
            pk = run(["ffprobe", "-v", "error", "-select_streams", "0", "-show_entries", "packet=pts_time,duration_time", "-of", "csv=p=0", str(d / f)]).split()
            last = pk[-1].split(",") if pk else None
            end = float(last[0]) + float(last[1] or 0) if last else 0.0
            t = {"readable_packets": len(pk), "readable_media_s": round(end, 3), "bytes": (d / f).stat().st_size}
            if side.exists():
                first = json.loads(side.read_text())["first_pts_host_s"]
                t["first_pts_host_s"] = first
                t["covered_until_host_s"] = first + end
                t["lost_before_kill_s"] = round(k["killed_at_host_s"] - (first + end), 3)
            dec = subprocess.run(["ffmpeg", "-v", "error", "-i", str(d / f), "-f", "null", "-"], capture_output=True, text=True).stderr
            t["ffmpeg_full_decode_errors"] = len([l for l in dec.splitlines() if "rror" in l and "monotonic" not in l])
            kill["tracks"][f] = t
        res["kill_test"] = kill
    print(json.dumps(res, indent=1))
    (d / "analysis.json").write_text(json.dumps(res, indent=1))


def summary(root):
    root = Path(root)
    out = {}
    def load(p):
        p = root / p
        return json.loads(p.read_text()) if p.exists() else None
    m = load("main/project.json")
    if m:
        tr = {t["kind"]: t for t in m["tracks"]}
        out["main_capture"] = {
            "display_pixels": m["display"]["pixels"],
            "screen_frames": tr.get("screen", {}).get("appended"),
            "screen_delivered_fps_busy_desktop": round(m["screen_delivery"]["active_phase"]["fps"], 2),
            "encoder_not_ready_drops": {k: v.get("encoder_not_ready_drops") for k, v in tr.items()},
            "camera_capture_dropped": tr.get("camera", {}).get("capture_dropped"),
            "camera": m.get("camera_delivery"),
            "file_mbps": {k: round(v["bytes"] * 8 / m["duration_s"] / 1e6, 3) for k, v in tr.items()},
            "file_bytes": {k: v["bytes"] for k, v in tr.items()},
            "cpu_pct_of_one_core": {c["process"]: round(c["cpu_pct_of_one_core"], 2) for c in m["cpu"] if c["cpu_s"] > 0},
            "rss_max_mb": round(m["self_rss_max_mb"], 1),
            "clocks": m["clocks"],
        }
    a = load("main/analysis.json")
    if a:
        out["main_sync_checks"] = a["sync_checks"]
    for f in ["screen", "camera"]:
        dj = load(f"main/{f}.decode.json")
        if dj:
            out[f"decode_{f}"] = {"decode_only_fps": round(dj["decode_only"]["fps"], 1), "decode_import_render_fps": round(dj["decode_import_render_preview"]["fps"], 1),
                                 "import_us_median": dj["decode_import_render_preview"]["import_us"]["median"], "seek_ms": dj["random_seek_to_rendered_frame_ms"],
                                 "zero_copy": dj["zero_copy_proof"]["textures_alias_pixel_buffer_iosurface"], "open_ms": round(dj["decode_only"]["open_ms"], 1)}
    for name in ["main", "noexcl", "exclapp"]:
        dj = load(f"{name}/screen.decode.json")
        if dj and dj.get("test_window_region"):
            out[f"exclusion_{name}"] = dj["test_window_region"]
    for name in ["window30", "window60", "display30", "display60"]:
        p = load(f"{name}/project.json")
        if p:
            s = p["screen_delivery"]
            out[f"delivery_{name}"] = {"idle_fps": round(s["idle_phase_1"]["fps"], 2), "active_fps": round(s["active_phase"]["fps"], 2), "active_p50_ms": round(s["active_phase"]["interval_ms_p50"], 2),
                                      "late_or_missing": s["active_phase_late_or_missing_frames"], "status_counts": s["status_counts"], "stop_extension_gap_s": s["stop_extension_appended_gap_s"],
                                      "encoder_drops": p["tracks"][0]["encoder_not_ready_drops"], "dims": p["tracks"][0].get("dims"),
                                      "cpu_pct_of_one_core": {c["process"]: round(c["cpu_pct_of_one_core"], 2) for c in p["cpu"] if c["cpu_s"] > 0},
                                      "screen_mbps": round(p["tracks"][0]["bytes"] * 8 / p["duration_s"] / 1e6, 3)}
    k = load("kill/analysis.json")
    if k:
        out["kill_test"] = k.get("kill_test")
    for name in ["encbench/encbench.json", "encbench_ecore/encbench.json"]:
        e = load(name)
        if e:
            out[name.split("/")[0]] = [{"size": r["size"], "codec": r["codec"], "fps": round(r.get("encode_fps", 0), 1), "x30": round(r.get("headroom_vs_30fps", 0), 2), "x60": round(r.get("headroom_vs_60fps", 0), 2), "err": r.get("error")} for r in e["results"]]
    psnr = root / "psnr.txt"
    if psnr.exists():
        out["wgpu_vs_ffmpeg_psnr"] = psnr.read_text().strip().splitlines()
    (root / "summary.json").write_text(json.dumps(out, indent=1))
    print(json.dumps(out, indent=1))


if __name__ == "__main__":
    if sys.argv[1] == "--summary":
        summary(sys.argv[2])
    else:
        main(sys.argv[1])
