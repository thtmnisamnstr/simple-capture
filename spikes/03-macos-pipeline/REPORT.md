# Spike 3 report: macOS capture, record, decode
Status: pass, with caveats. Written 2026-11-19 from `out/bench-quiet{,2,3}/summary.json` and `*/analysis.json`. The recordings under `out/` are gitignored; the summaries, analyses, `project.json` files and run logs are committed.
Machine: M3 Pro, 18 GB, macOS 26.7.2, display 7680x2160 px (3840x1080 pt at scale 2). Every run went through `tools/macos-dev-host/run.sh` (39 of 39 `project.json` files record `permission_host = com.simplecapture.dev`). No M1 data.

## Question
Can objc2 + ScreenCaptureKit + AVCaptureSession + AVAssetWriter record screen, camera, mic and system audio into crash-safe tracks on one clock, and can AVAssetReader decode zero-copy into wgpu?

## Method
`bench.sh` runs, in order:
- `main`: 10 s of screen + camera + mic + system audio, with a flash and a sound from a separate process and a sound from our own process
- exclusion tests
- window and display delivery at 30 and 60 fps
- a SIGKILL at about 6 s
- HEVC/H.264 encbench, plus an E-core variant
- AVAssetReader decode into wgpu with PSNR against an ffmpeg reference

`analyze.py` validates with Homebrew ffprobe.

## Results (quiet runs 1 / 2 / 3)
**Main recording**
- Screen frames: 273 / 163 / 178; delivered fps on a busy desktop: 28.4 / 28.7 / 26.1.
- Writer not-ready drops, screen / system audio / camera / mic: 0/0/0/0, 0/0/0/0, **0/0/6/0**. Camera capture drops: 0 in all three; late frames 0.
- Camera: 30.00 fps, 279 frames in each run.
- File Mbps, screen / camera: 12.2 / 13.0, 5.9 / 13.2, 7.5 / 13.0.
- CPU (% of one core), self / replayd / VTEncoderXPCService: 32.4 / 4.0 / 6.8, 29.8 / 4.0 / 5.8, 28.7 / 3.6 / 6.0. RSS max: 117 / 129 / 186 MB.

**Clocks**
- Screen display time minus PTS: at most 0.000001 ms.
- Clock-to-host probes, max |Δ|: SCStream 0.0058 / 0.0045 / 0.0052 ms; camera ≤0.0003 ms; mic 1.18 / 0.48 / 0.43 ms.
- Callback lag after PTS (median): camera 47.0–50.8 ms, mic ~12.1 ms, system audio 51.0–52.7 ms, screen within ±0.6 ms.

**Sync**
- Flash → system-audio onset: 58.0 / 45.8 / 39.3 ms.
- The mic heard neither test sound (onset null in all three runs), so mic sync is unverified.

**Exclusion**
- Per-window filter hides the test window in all three runs.
- With no filter, or with a whole-app filter (the app shows up as `com.simplecapture.dev` under the host), the window is visible.

**Delivery** (active fps / late or missing frames; 0 encoder drops in every case)
- Window 720x480 @30: 28.7 / 29.0 / 29.0
- Window @60: 55.5 / 54.5 / 55.3
- Display 7680x2160 @30: 28.7 / 27.6 / 27.6
- Display @60: 44.2 / 44.5 / 45.0, with 51 / 45 / 49 late or missing
- Idle window: 0.86 fps

**Kill at about 6 s** (readable packets / seconds lost / decode errors)
- screen: 97 / 169 / 114 packets, 0.14 / 0.08 / 0.10 s lost
- camera: ~208 packets, 0.79–0.86 s lost
- mic: 329 packets, 0.74–0.81 s lost
- system audio: 282 packets, 0.11–0.17 s lost
- 0 decode errors in every file

**Keyframe gaps**
- Screen max: 1.05 / 2.78 / 1.65 s in `main` (idle screen sends no frames); about 1.0 s in `kill`.
- Camera ≤1.0 s.

**Encbench, AVAssetWriter fps** (HEVC / H.264)
- 7680x2160: 46.3, 46.0, 46.3 / 25.8, 19.6, 31.8
- 5120x2880: 52.3–52.4 / 18.5, 24.3, 36.6
- 3840x2160: 89.9–90.5 / 54.4
- 3024x1964: 122.6–123.1 / 74.7–74.9
- 1920x1080: 315.3–317.3 / 199.8–204.2
- E-core HEVC: 45.0–45.2, 51.2–51.5, 86.9–87.3, 279.9–293.0

**Decode**
- Screen: 286–306 fps decode-only, 319–344 fps with import and render; import 37–47 µs; seek median 45–60 ms, max 142 ms.
- Camera: ~1600 fps; seek ~10–11 ms.
- Zero-copy confirmed.
- PSNR against ffmpeg: screen 77.2–79.5 dB, camera 71.8–72.1 dB.

## Verdict
Pass for display + single-window capture, the four tracks, crash safety, clocks and zero-copy decode. Feeds ADR-006, 008, 009, 010, 011 and ARCHITECTURE §2–§5, §8, §12.

## Not tested
- Region capture (`sourceRect` vs crop), application capture, "only this app" audio
- Device hot-plug, Mic Mode, permission onboarding, `SCContentSharingPicker` (ADR-028)
- Pause, soak longer than 10 s, mic sync
- Whether the `startCapture` completion lags the first frame by about 240 ms (agent claim, not re-derived from saved fields)
