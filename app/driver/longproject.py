"""Writes a finalized 10-minute recording (3024x1964 HEVC test pattern at 30 fps with 1 s
keyframes, and a mono AAC tone) into a projects folder, for timing the editor on long takes
without recording the screen. Usage: longproject.py FFMPEG PROJECTS_DIR SECONDS"""
import json
import os
import subprocess
import sys
import uuid

ffmpeg, root, secs = sys.argv[1], sys.argv[2], int(sys.argv[3])
ns = secs * 1_000_000_000
d = os.path.join(root, "2026-11-21 12.00.00 drv long.sc")
os.makedirs(os.path.join(d, "tracks"), exist_ok=True)
frag = ["-movflags", "+frag_keyframe+empty_moov+default_base_moof"]
subprocess.run([ffmpeg, "-v", "error", "-y", "-f", "lavfi", "-i", f"testsrc2=size=3024x1964:rate=30:duration={secs}",
                "-c:v", "hevc_videotoolbox", "-b:v", "6M", "-g", "30", "-tag:v", "hvc1", "-pix_fmt", "nv12",
                *frag, os.path.join(d, "tracks", "screen-000.mov")], check=True)
subprocess.run([ffmpeg, "-v", "error", "-y", "-f", "lavfi", "-i", f"sine=frequency=440:sample_rate=48000:duration={secs}",
                "-c:a", "aac", "-b:a", "128k", "-ac", "1", *frag, os.path.join(d, "tracks", "mic.mov")], check=True)
start = 1_000_000_000_000
color = {"primaries": "bt709", "transfer": "bt709", "matrix": "bt709", "range": "limited"}
health = lambda n: {"frames": n, "dropped": 0, "writer_not_ready": 0, "gaps": []}
project = {
    "schema_version": 1, "id": str(uuid.uuid4()), "created_at": "2026-11-21T12:00:00-08:00",
    "app_version": "0.1.0", "state": "finalized", "title": "drv long",
    "recording": {
        "clock": "mach_host", "start_host_ns": start, "stop_host_ns": start + ns, "pauses": [],
        "screen": {"fps_requested": 30, "resolution": "native", "cursor": True, "click_highlight": False,
                   "segments": [{"id": "s0", "track_id": "screen-0", "start_rec_ns": 0, "end_rec_ns": ns,
                                 "start_reason": "record_start", "end_reason": "record_stop", "via": "own_chooser",
                                 "source": {"kind": "display", "display_id": "1", "display_name": "Test",
                                            "scale": 2.0, "frame_pt": {"x": 0.0, "y": 0.0, "w": 1512.0, "h": 982.0}},
                                 "frame_px": {"w": 3024, "h": 1964},
                                 "content_px": {"x": 0, "y": 0, "w": 3024, "h": 1964}}]},
        "tracks": [
            {"id": "mic", "kind": "mic", "file": "tracks/mic.mov", "container": "mov_fragmented", "codec": "aac",
             "first_sample_offset_ns": 0, "duration_ns": ns, "audio": {"sample_rate": 48000, "channels": 1},
             "health": health(secs * 48000), "fragment_interval_s": 1.0},
            {"id": "screen-0", "kind": "screen", "segment_id": "s0", "file": "tracks/screen-000.mov",
             "container": "mov_fragmented", "codec": "hevc", "first_sample_offset_ns": 0, "duration_ns": ns,
             "video": {"size_px": {"w": 3024, "h": 1964}, "nominal_fps": 30, "vfr": True,
                       "keyframe_interval_s": 1.0, "color": color},
             "health": health(secs * 30), "fragment_interval_s": 1.0},
        ],
        "system_audio": "off",
    },
    "camera_layout": {"overlays": []},
    "edl": {"clips": [{"id": str(uuid.uuid4()), "src_in_rec_ns": 0, "src_out_rec_ns": ns,
                       "speed": {"num": 1, "den": 1}, "muted": False}]},
    "audio_mix": {},
    "text": [],
    "output": {"canvas": {"aspect": "original", "ratio": {"num": 756, "den": 491}, "fit": "contain",
                          "background": {"kind": "solid", "color": [0.07, 0.07, 0.08, 1.0]}}},
    "export_presets": {"last_format": "mp4", "per_format": {}, "named": []},
    "fonts": [],
}
json.dump(project, open(os.path.join(d, "project.json"), "w"), indent=2)
print(d)
