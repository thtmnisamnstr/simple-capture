# Codec matrix

Status: M0 draft, 2026-11-19; M1 results added 2026-11-20 (§1 macOS rows, §2.7, §3, §4). This file answers two questions: which API or encoder does each media job on each OS, and which export combinations we offer with which settings. It is fed by the M0 spikes and research notes; every non-obvious claim names its source.

Sources, short names used below:

| Short name | Where |
|---|---|
| S1 | Spike 1, export helpers: `spikes/01-helpers/` (results in `results/latest.md`, quiet-machine verification run `run-20261119T214545Z` plus two sibling runs for medians) and `third_party/` |
| S2 | Spike 2, preview transport: `spikes/02-preview-transport/` (results in `results/verify-*`) |
| S3 | Spike 3, macOS pipeline: `spikes/03-macos-pipeline/` (results in `out/bench-quiet*`) |
| S4 | Spike 4, Skia text: `spikes/04-skia-text/` |
| R-rec, R-dec, R-edit, R-mit, R-kap, R-eco | `docs/research/cap-recording.md`, `cap-render-decode.md`, `cap-editor-export.md`, `cap-mit-crates.md`, `kap.md`, `ecosystem.md` |
| M1:<package> | The M1 package's report (2026-11-19/20), e.g. M1:export = wp-export. Not stored in the repo; docs/STATUS.md §2 is the in-repo record of every M1 number with its source. |

Important caveats:

- **Each spike has a REPORT.md** (saved at close-out from the consistency pass's drafts). The verifiers' corrected numbers were never saved to disk, so in the 2026-11-19 consistency pass every number below was re-read from the saved results files (S1: medians of the three verification runs `run-20261119T214043Z`, `T214312Z`, `T214545Z`; S3: `out/bench-quiet{,2,3}/summary.json`, committed; only the recordings are gitignored). Claims with no saved source were either re-measured in that pass (marked "re-measured") or removed.
- **All measurements are from an Apple M3 Pro (5P + 6E cores, 18 GB, macOS 26.7.2).** No base M1 8 GB was available. The "4-thread proxy" removes the M3 Pro's core-count advantage but not its faster cores, and the verifier found it noisy (up to 1.7x spread between runs, S1). VideoToolbox speed depends on the chip's media engine, so an M1 must be measured directly. Treat every speed figure as an upper bound for an M1.
- "Status" legend: **verified** = run and checked on this Mac in M0; **researched** = read in docs or source, never run; **planned** = the spec's choice, with no M0 evidence either way.

## 1. Media jobs per OS

### macOS (first-class)

| Job | API / encoder | Status | Limits and notes |
|---|---|---|---|
| Screen + system audio | ScreenCaptureKit `SCStream` through `objc2-screen-capture-kit` 0.3.2 | **verified** (S3) | Captured native 7680x2160 px (3840x1080 pt at scale 2). Pixel format 420v, BT.709 YCbCr matrix set explicitly, queueDepth 6, cursor on. Frames arrive only on change: 0.86 fps idle, 28.95 fps active at a 30 cap, 55.3 at a 60 cap (single window). Full display at 60 fps requested delivered only 44.5 fps. `excludesCurrentProcessAudio` works (own sound at -120 dB). Hiding our own windows works **per window**; whole-app exclusion **fails** under the dev host (S3). SCStream microphone output needs macOS 15+ (R-eco). |
| Camera | AVFoundation `AVCaptureSession` (`objc2-av-foundation`) | **verified** (S3) | FaceTime HD 1080p30 420v: capture drops 0 and late frames 0 in all three quiet runs; writer "not ready" drops 0, 0 and **6** (`bench-quiet3`). Continuity Camera reports as `External`, not `ContinuityCamera` (S3 verification). |
| Mic | AVFoundation `AVCaptureSession` (not AVAudioEngine) | **verified** (S3) | Host-time timestamps straight into the writer. Clock offset to host time 0.15-1.18 ms. Mic A/V sync not verified (the mic did not hear the test sounds). |
| Raw-track record encode | `AVAssetWriter`, QuickTime `.mov`, `movieFragmentInterval` 1 s; VideoToolbox **HEVC Main, hvc1** for screen and camera; AAC LC for mic (mono 48 kHz) and system audio (stereo 48 kHz) | **verified** (S3) | SIGKILL at 6 s loses at most 0.86 s and every file still decodes (S3). Keyframe interval set to 1 s, but gaps reach 1.65-2.78 s when SCK sends no frames on an idle screen (S3 verification). HEVC encoder headroom, frames/s: 7680x2160 46.3, 5120x2880 52.3, 3840x2160 90.3, 3024x1964 122.9, 1920x1080 315.4. So: 30 fps at native size, 60 fps only at 4K or below, after a real encode test. H.264 through AVAssetWriter (hardware not required): 7680x2160 19.6-31.8 fps and 5120x2880 18.5-36.6 fps (unstable), 4K 54.4, 1080p 204 (S3). It does open above 4096 px, unlike FFmpeg's hardware-only `h264_videotoolbox -allow_sw 0` (S1); the low, noisy speed suggests a software encoder there (inference, see ADR-009). **M1:** `fragment_interval_s` 1.0 loses 1.003–1.076 s per track on a live SIGKILL (a whole unwritten fragment plus encoder latency; M1:kill-recovery-test, owner question in ADR-008). Default HEVC rate 0.05 bits per pixel; H.264 for raw tracks only when both sides ≤ 4096 (M1:mac-av). Probe v1 on this Mac: HEVC 3024x1964 117.9 fps, 1512x982 386.8 fps (M1:export). 30-minute soak at 3024x1964 30 fps: 0 writer not-ready on every track (M1:soak). |
| Editor decode | `AVAssetReader` producing IOSurface-backed NV12 (420v) | **verified** (S3) | 7680x2160 HEVC: 302 fps decode, 344 fps with import and preview render; seek median 54 ms, max 142 ms (three runs). Camera 1080p: 1599 fps, seek 11 ms. AVAssetReader reads a SIGKILLed fragmented `.mov` completely. Readers cannot seek: recreate on a keyframe (R-dec). |
| Frames into wgpu | `CVMetalTextureCache` makes R8 (Y) and RG8 (CbCr) Metal textures from the IOSurface; `wgpu-hal` wraps them; WGSL BT.709 limited-range shader | **verified** (S3) | Zero-copy: IOSurface ids match on both planes, the same Metal texture object, 39 µs per 7680x2160 frame. PSNR against an ffmpeg reference conversion 79.3 dB (screen). Keep the source CVPixelBuffer alive until GPU work finishes (R-dec). |
| Preview to UI | `CAMetalLayer` NSView under a transparent WKWebView (wgpu FIFO surface); fallback raw frames into WebGL2 | **verified** (S2) | Native path 60.1 fps of 60 at 20% total CPU; WebGL2 fallback 58.5-59.9 fps. Send webview frames at the preview's device-pixel size (S2). |
| Export encode + mux | Bundled static GPL FFmpeg 9.0.2 helper (`third_party/ffmpeg/build-macos.sh`) | **verified** (S1) | arm64 built and benchmarked; x86_64 cross-built and only smoke-tested under Rosetta. See section 2. |
| GIF | Bundled gifski 1.34.0 helper (Y4M on stdin) | **verified** (S1) | arm64 only; gifski x86_64 not built yet. FFmpeg palette mode as the alternative. |

### Windows (M6)

| Job | API / encoder | Status | Limits and notes |
|---|---|---|---|
| Screen + system audio | Windows.Graphics.Capture + WASAPI loopback; process loopback for "only this app" | **researched** (R-eco, R-mit) | Process loopback and `IsBorderRequired` need build 20348+ (Windows 11 in practice). Removing the yellow border needs the `graphicsCaptureWithoutBorder` package-manifest capability; whether an unpackaged MSI/NSIS app can get it is unverified (R-eco). Choose `windows-capture` (MIT) or vendored `scap-direct3d` in the Windows spike (R-mit). |
| Camera / mic | Media Foundation with DirectShow fallback / WASAPI | **researched** (R-mit) | Candidate: vendor and adapt Cap's MIT `cap-camera-windows`, `-mediafoundation`, `-directshow` (R-mit). |
| Raw-track record encode | Media Foundation hardware MFT, zero-copy D3D11 | **planned** | Hardware MFTs are async-only (`MF_TRANSFORM_ASYNC_UNLOCK`, `MFT_ENUM_FLAG_ASYNCMFT`, event-driven) (R-eco). Set the GOP explicitly and tag BT.709 explicitly; Cap does neither on this path (R-rec, unverified). |
| Editor decode | Media Foundation Source Reader with DXVA, NV12 out | **researched** (R-dec) | Cap falls back to FFmpeg D3D11VA, then software. |
| Frames into wgpu | Shared D3D11 texture opened on D3D12, wrapped by wgpu; CPU upload fallback | **researched** (R-dec) | Cap's D3D11 zero-copy appears unused in its editor at the audited commit; CPU upload may be the realistic v1 path (R-dec). |
| Preview to UI | Raw frames into WebGL2 (webview) | **planned** (S2) | A native child swap chain cannot sit under HTML in WebView2 without visual hosting, which wry does not expose (S2). |
| Export encode + mux | Pinned FFmpeg-Builds (BtbN) `gpl` release | **researched** (R-eco) | BtbN's stated floor is Windows 10 22H2, newer than the spec's 2004 (R-eco). See docs/LICENSES.md about networking in these builds. |
| GIF | gifski built from the same pinned crate with cargo in CI | **planned** | Not built for Windows in M0. |

### Linux (M6)

| Job | API / encoder | Status | Limits and notes |
|---|---|---|---|
| Screen + system audio | xdg-desktop-portal ScreenCast + PipeWire; system audio from the default sink monitor (`stream.capture.sink=true`) | **researched** (R-eco) | Request `persist_mode=2`; the restore token is single-use, so save the new one after every session (R-eco). System audio "All" only in v1 (spec). |
| Camera / mic | V4L2 or portal camera / PipeWire | **planned** | |
| Raw-track record encode | FFmpeg VAAPI/NVENC through the LGPL libraries in-process; x264 through the helper if no hardware encoder | **planned** | Probe each hardware encoder with a real encode and a decode-back check (R-rec). |
| Editor decode | FFmpeg LGPL libraries + VAAPI | **planned** | Cap has no VAAPI decode path to learn from (R-dec). |
| Frames into wgpu | dmabuf import, or CPU upload | **planned** | |
| Preview to UI | Raw frames into WebGL2 at 960x540 or smaller | **planned** (S2) | WebKitGTK is untested. Gate the decision on a real-hardware bench at the end of M0/M1 (S2). |
| Export encode + mux | Pinned FFmpeg-Builds `gpl` release | **researched** (R-eco) | Same notes as Windows. |
| GIF | gifski, cargo build in CI | **planned** | |

## 2. Export

### 2.1 How every export runs (all OSes)

Verified on macOS in S1:

- The renderer applies the EDL, size and fps. Frames go to the helper on stdin as **Y4M** (default) or raw **NV12**; mixed audio goes as a temporary 48 kHz WAV. Recipes never use `-ss`, `-to`, `-r`, `-s` or scale filters on video formats (R-kap). A 3-frame bounded queue gives backpressure (S1).
- Pipe throughput is not the bottleneck: about 704 fps Y4M / 667 fps NV12 at 1080p (about 2.1 GB/s), and 392 / 378 fps at 1440p (S1).
- **Colour (pitfall 5):** frames are tagged inside the filter graph with `setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv`, **and** the output carries the same `-color_*` options. With output options alone, FFmpeg 9 leaves primaries and transfer "unknown" for libx264, h264_videotoolbox, libsvtav1 and libvpx-vp9 (S1, confirmed with Homebrew ffmpeg too). Every output passed with decoded colour-patch error ≤ 3/255, also checked through Apple's decoder via QuickLook (S1).
- MP4 always gets `-movflags +faststart` (R-kap) and an explicit `-pix_fmt yuv420p`.
- Even dimensions for all 4:2:0 video; GIF may be odd (spec, R-kap). h264_videotoolbox did accept 1918x1080 in the probe, but we do not rely on that (S1).
- Cancel: SIGTERM, 1.5 s grace, then SIGKILL. Write to `<name>.part`, rename on success, delete on cancel or failure, sweep stale `.part` files at launch. A SIGTERMed ffmpeg finalizes a playable file in about 0.23 s; a SIGKILLed one has no moov atom; a SIGTERMed gifski leaves a **truncated** GIF: ffprobe counts 18-20 frames in all five saved runs, but re-measured in the consistency pass the file has no trailer byte (0x3B) and its last frame fails LZW decode. If the app dies, the helper sees EOF and exits by itself in about 0.23 s (S1).
- Progress: frames written to the helper's stdin, plus `-nostats -progress` for the final flush and end. gifski prints no progress at all (S1, R-kap).

### 2.2 Video encoders

Speed: 1080p30, 10 s synthetic clip, Y4M input, **median of 3 quiet-machine runs** from the S1 verification (load average still about 6-7 on 11 cores). "4-thread" is `-threads 4`, or `lp` for SVT-AV1, which ignores `-threads`. The proxy ranges were partly taken from verifier runs that were not saved; over the five saved runs they are: x264 108.8-146.0, x265 83.0-95.8, SVT-AV1 lp=2 87.3-102.4 and lp=1 38.5-44.2, VP9 41.9-49.9. Speeds were measured at the bench settings in the "Measured at" column, **not** at the proposed tiers. The tiers are spike 1's proposal: only Balanced is close to what was measured, and **no tier has been checked visually on real screen recordings**.

| Codec | Mode | Exact helper encoder and fixed args | Small / Balanced / High | Measured at | M3 Pro fps (x realtime) | 4-thread proxy fps | 1440p fps | Notes |
|---|---|---|---|---|---|---|---|---|
| H.264 | Best | `libx264 -preset medium -profile:v high -pix_fmt yuv420p` | CRF 26 / 22 / 18 | CRF 20 | 196.8 (6.56x) | 124.4 (108.8-144.9) | 106.8 | `-preset slow` costs about 28%: 142.4 fps, proxy 95.2. Peak RSS 708 MB. |
| H.264 | Fast (macOS) | `h264_videotoolbox -allow_sw 0` | `-q:v` 50 / 60 / 70 | q 65 | 190.6 (6.35x), 32% CPU | n/a (hardware) | 114.5 | 30 s clip: 207.3 and 208.4 fps (re-measured). **Fails to open with `-allow_sw 0` when either dimension exceeds 4096**: the saved runs probed only 8192x4320; re-measured: 4098x2160, 4112x2160, 4352x2160, 5120x2160, 5120x2880, 6016x3384, 7680x2160, 2160x4098, 2160x4352 and 1920x4320 all give -12903, while 4096x2160, 4096x2304, 4096x4096 and 2160x4096 open. With `-allow_sw 1` it opens at 5120x2880 and 7680x2160 (software). So 5K/6K sources need a downscale, HEVC or x264 for "Fast". |
| H.264 | Fast (Win/Linux) | `h264_nvenc`, `h264_qsv`, `h264_amf`, `h264_mf` / `h264_nvenc`, `h264_vaapi` | TBD in M6 | – | not measured | – | – | researched only; offer only what the probe opens. |
| HEVC | Best | `libx265 -preset medium -pix_fmt yuv420p -tag:v hvc1` | CRF 28 / 25 / 22 | CRF 24 | 120.1 (4.00x) | 85.7 (51.2-89.4) | 71.7 | Always `-tag:v hvc1` or QuickTime will not play it. |
| HEVC | Fast (macOS) | `hevc_videotoolbox -allow_sw 0 -tag:v hvc1` | `-q:v` 50 / 60 / 70 | q 65 | 176.7 (5.89x), 32% CPU | n/a | 106.0 (NV12 input) | Opens at 8192x4320 and 7680x4320 (S1). |
| HEVC | Fast (Win/Linux) | `hevc_nvenc`, `hevc_qsv`, `hevc_amf`, `hevc_mf` / `hevc_nvenc`, `hevc_vaapi` | TBD in M6 | – | not measured | – | – | researched only. |
| AV1 | Best | `libsvtav1 -preset 8 -pix_fmt yuv420p` | CRF 38 / 32 / 26 | CRF 32 | 189.1 (6.30x) on 10 s; 181.8 and 185.0 on 30 s (re-measured) | lp=2: 90.5; lp=1: 39.5 (medians of the 3 runs) | 114.7 | Preset 6: 95.8 fps, 1.17-1.29 GB RSS, too heavy for an 8 GB M1 by default. An earlier "153.6 fps on 30 s" had no saved source and did not reproduce. |
| AV1 | Fast | none on macOS (no AV1 hardware encoder through VideoToolbox here); `av1_nvenc`, `av1_qsv`, `av1_amf`, `av1_vaapi` on capable PCs | – | – | – | – | – | Hide AV1 Fast unless the probe opens one (S1). |
| VP9 | Best | `libvpx-vp9 -deadline good -cpu-used 4 -row-mt 1 -tile-columns 2 -b:v 0 -pix_fmt yuv420p` | CRF 38 / 33 / 28 | CRF 32 | 64.3 (2.14x) | 42.6 (1.44x) | not run | `-cpu-used 2`: 56.7 fps. Slowest Best option; may end up near realtime on an M1 (S1). |
| VP9 | Fast | none on macOS. Possible `vp9_qsv` (Windows) / `vp9_vaapi` (Linux) | – | – | – | – | – | Unverified whether the BtbN builds include them. |
| VP8 | Best only | `libvpx -deadline good -cpu-used 4` with `-crf` and a required `-b:v` cap | CRF/cap 20/4M, 10/8M, 6/16M | CRF 10, 8M | 115.3 (3.84x) | not run | not run | **Converted to the BT.601 matrix and tagged smpte170m**: the VP8 bitstream cannot signal BT.709 and FFmpeg ignores container tags for it (S1). This is an exception to Pitfall 5 and is **Proposed, needs owner OK** (ADR-029). Legacy option. |
| ProRes 422 (MOV, nice-to-have) | Best / Fast | `prores_ks` / `prores_videotoolbox` | – | – | not measured | – | – | Compiled into the helper, never benchmarked. |

### 2.3 Audio encoders

| Codec | Exact helper encoder | Small / Balanced / High | Notes |
|---|---|---|---|
| AAC LC | `aac_at` (macOS AudioToolbox); `aac` (FFmpeg native) elsewhere; `aac_mf` on Windows is possible but not checked | 128k / 160k / 256k | aac_at vs native aac made no measurable speed difference (S1). |
| Opus | `libopus` | 96k / 128k / 192k | |
| Vorbis | `libvorbis` (never FFmpeg's experimental native `vorbis`) | `-q:a` 3 / 5 / 7 | Our build patches libvorbis' configure on Darwin (S1). |
| FLAC | `flac` | lossless (compression level only) | Compiled in, not exercised in the bench. |
| ALAC | `alac` | lossless | Compiled in, not exercised in the bench. |

### 2.4 Container, codec and compatibility

Defaults are listed first in each cell. The compatibility columns are **expectations from general platform knowledge, not tested in M0**; the M5 playback test from the definition of done decides the final labels, and the UI marks anything not "yes" as "less compatible". S1 validated only that each file is well formed (ffprobe: codec, tag, size, 30/1 fps, 300 frames, duration, yuv420p, BT.709 tags).

| Container | Video | Audio | ffprobe-validated in S1 | QuickTime | Chrome | Firefox | VLC |
|---|---|---|---|---|---|---|---|
| MP4 | H.264 (avc1) | AAC | yes (x264 and VT) | yes | yes | yes | yes |
| MP4 | HEVC (hvc1) | AAC | yes (x265 and VT) | yes | varies (needs OS/hardware decode) | varies by OS | yes |
| MP4 | AV1 (av01) | AAC | yes (SVT-AV1) | only on Macs with AV1 decode (M3 and later); not on M1/M2 | yes | yes | yes |
| MP4 | any of the above | Opus | no | unlikely | yes | yes | yes |
| MP4 | any of the above | FLAC | no | uncertain | yes | yes | yes |
| MP4 | any of the above | ALAC | no | yes | unlikely | unlikely | yes |
| WebM | VP9 | Opus | yes (container duration 10.008 s) | no | yes | yes | yes |
| WebM | AV1 | Opus | no (AV1 tested only in MP4) | no | yes | yes | yes |
| WebM | VP8 | Vorbis | yes (BT.601 by design) | no | yes | yes | yes |
| WebM | VP9/AV1/VP8 | Vorbis / Opus (swapped) | no | no | yes | yes | yes |
| GIF | gifski or palette | none | yes | Quick Look/Preview animate it; QuickTime Player and VLC are not targets | yes | yes | – |

Helper gaps that affect this table (S1): the shipped ffmpeg has **no AV1 decoder** (it cannot decode its own AV1 output, so decode-back probes for AV1 need another decoder or must be skipped), and the **PNG decoder is silently missing** because zlib is disabled. Both need a decision before M5.

### 2.5 GIF

| Mode | Exact helper and args | Small / Balanced / High | M3 Pro speed (960x540, 15 fps, 150 frames) | Notes |
|---|---|---|---|---|
| Best (default) | `gifski --fps N --quality Q --repeat R --y4m-color-override bt709 -o OUT -` (Y4M on stdin) | quality 60 / 80 / 90 | q90: 40.9 fps, 26.2 MB; q70: 57.2 fps, 17.7 MB | Uses all cores (about 710-835% CPU). `--motion-quality` and `--lossy-quality` exist but were not exercised. Without the colour override gifski assumes BT.601 for frames up to 720x480 (R-eco). |
| Palette (spec calls it "Fast") | `ffmpeg ... -filter_complex "[0:v]setparams=...bt709...,split[a][b];[a]palettegen=max_colors=C:stats_mode=full[p];[b][p]paletteuse=dither=sierra2_4a[out]" -loop L` | colors 64 / 128 / 256 | 14.2 fps median (12.8-16.3; about realtime), about 108% CPU, 36.7 MB | Adapted from Kap with attribution (docs/THIRD_PARTY.md). Single-threaded and about 2.9x slower than gifski q90, so present it as "colour and dither control", not as faster (S1). Without the BT.709 tag swscale uses BT.601 (patch error 32 vs ≤ 10). The single-pass `split` graph buffers every frame in ffmpeg: about 1.9 GB for 60 s at 960x540 15 fps, too much for an 8 GB M1, so M5 should use two passes or cap the length (R-kap). |

Loop settings differ per tool: gifski `--repeat` (-1 none, 0 forever, N); ffmpeg GIF `-loop` (-1 once, 0 forever, N = repeats, so "play 3 times" is `-loop 2`) (R-eco, R-kap).

### 2.6 FPS per format

Each format remembers its own last explicit choice; the effective fps is the largest list value not above min(preferred, recording fps), compared with a tolerance so 59.94 counts as 60 (R-kap). Text animations and camera moves render at the export fps.

- **Video (MP4, WebM):** 15, 24, 25, 30, 50, 60. Default 30.
- **GIF:** 5, 8, 10, 12, **15 (default)**, 20, 25, 30, 50. GIF frame delays are whole hundredths of a second, so fps that don't divide 100 are approximate. Our frame timing alternates delays so the total stays exact (S1 measured 15 fps as 50 frames of 6 cs and 100 frames of 7 cs = exactly 10.00 s):

| GIF fps | Ideal delay | Delays written | Exact? |
|---|---|---|---|
| 5 | 20 cs | 20 | yes |
| 8 | 12.5 cs | 12, 13 alternating | average only |
| 10 | 10 cs | 10 | yes |
| 12 | 8.33 cs | 8, 8, 9 | average only |
| 15 | 6.67 cs | 6, 7, 7 | average only (verified) |
| 20 | 5 cs | 5 | yes |
| 25 | 4 cs | 4 | yes |
| 30 | 3.33 cs | 3, 3, 4 | average only; label "about 30" |
| 50 | 2 cs | 2 | yes; the maximum |

Browsers treat delays below 2 cs as 10 cs, so anything above 50 fps would play slower; that is why the list stops at 50.

### 2.7 Quick export (M1, measured)

The M1 quick export is the only export: `libx264 -preset medium -profile:v high -pix_fmt yuv420p -crf 22` with `setparams` BT.709 limited tags plus output tags, `aac_at` 160k (`aac` off macOS), `+faststart`, `-an` when there is no audio. The compositor renders each frame on the GPU (Metal here) from CPU-uploaded NV12 and pipes Y4M I420 to the helper. The long side is capped at 3840 (Q6); the reference size is never upscaled.

| Case | Wall | Encode rate | × realtime | Source |
|---|---|---|---|---|
| 1920x1080, 30 s at 30 fps | 5.20 s | 173.1 fps | 5.77× | M1:export, `cargo test --release --test export_bench -- --ignored` |
| 3840x2160, 30 s at 30 fps | 13.85 s | 65.0 fps | 2.17× | same |
| 7680x2160, 10 s at 30 fps (bypasses the job; the quick export caps at 3840) | 11.18 s | 26.8 fps | 0.89× (slower than realtime) | same |
| 3024x1964 reference export of a real 10 s display recording | 4.07–4.3 s | – | about 2.3–2.5× | M1:e2e, step 1, two runs (`target/e2e/report-20261119-232627.json`, `…232905.json`) |

- The bench frames are synthetic and flat, so real screen content will encode slower; treat the bench rates as upper bounds (M1:export). All on the M3 Pro; nothing on an M1.
- Compositor share of the frame time, Metal, whole `render()` including background upload and readback (M1:render): 1080p 6.3–9.7 ms, 2160p 17.4–18.7 ms. The background is re-uploaded every frame (about 33 MB at 4K), a known later optimisation.
- Validation (`ffprobe_check`): codec, size, fps, frame count, duration within one frame, audio length within one AAC frame plus one video frame, colour tags bt709/bt709/bt709/tv. All 20 aspect × background exports in e2e passed, and colour patches decode within 3/255 (M1:export, M1:e2e).
- The x264 7680x2160 rate is the reason for the 3840 cap (ADR-036, Q6).

## 3. What the capability probe must check

Run once per machine and cache the result keyed by OS version, helper version and GPU. Hide anything that fails.

1. **Each helper encoder actually opens and encodes** a few frames, not just appears in `-encoders` (pitfall 6). Pass `-allow_sw 0` for VideoToolbox so a software fallback counts as failure (S1).
2. **Size limits at the requested output size.** h264_videotoolbox with `-allow_sw 0` fails when either dimension exceeds 4096 on the M3 Pro (S1, re-measured); the M1 limit is unknown. Probe at the actual export size, or at least at the next size class, and fall back to x264 or HEVC.
3. **Each container/codec/audio pair muxes**, using a tiny test encode, then ffprobe the result: codec, tag (avc1/hvc1/av01), pix_fmt, the four colour fields, faststart for MP4.
4. **Decode-back** where the helper can decode the codec. AV1 cannot be decoded by our helper (no AV1 decoder), so skip or use a native decoder for that check.
5. **`aac_at` presence** (macOS only), otherwise native `aac`.
6. **Hardware Fast encoders on Windows/Linux** (NVENC/QSV/AMF/MF/VAAPI, including AV1 and VP9 variants). Run risky ones in a child process, and mark an encoder "in use" while it runs so a crash quarantines it on next launch (R-rec).
7. **Native recording headroom:** an HEVC (and H.264) encode at the display's native size and the chosen fps. Offer 60 fps only when it keeps up; S3 measured 46 fps HEVC at 7680x2160 on the M3 Pro, which rules out 60 fps there. **M1 probe v1 (the probe-size rule, ADR-009):** at launch, HEVC at 60 fps for every connected display's native and 1× size; 60 fps is offered for a source only if its display reached ≥ 78 fps (1.3 × 60). M3 Pro: 3024x1964 117.9 fps, 1512x982 386.8 fps; `h264_videotoolbox -allow_sw 0` opens at 3840 and 4096 x2160, fails at 4098 (M1:export). Delivered with `--fps 60` on a changing test screen: 50.8–52.5 fps (M1:e2e).
8. **Native decode paths:** AVAssetReader for HEVC/H.264 on macOS; MF DXVA on Windows; VAAPI on Linux.
9. **gifski** is present and is version 1.32 or later (needed for Y4M on stdin; we ship 1.34.0) (R-eco).
10. **Helper integrity:** binary present, version string matches the pinned build (`9.0.2-simple-capture`), with a clear error if missing (S1 tested the missing-helper message).

## 4. Open items

- Run S1 and S3 on a real base M1 8 GB: x264/x265/VP9 speed, VideoToolbox throughput, HEVC headroom at the M1's native sizes. Also the M1 quick export and probe v1 (§2.7); every M1 number is from the M3 Pro.
- Fragment interval for raw tracks (1 s loses up to about 1.08 s on a SIGKILL; ADR-008 owner question).
- Quick export speed on real (not synthetic) screen content.
- Decide the 5K/6K Fast path for H.264 (downscale, HEVC, or x264).
- VP8 colour exception (ADR-029) needs the owner's OK.
- The S3 evidence behind section 1 (summaries, analyses, logs) is un-ignored under `out/`; only the recordings are gitignored (STATUS §1).
- Visual check of all quality tiers on real screen recordings; only synthetic content was encoded.
- Playback test in QuickTime, Chrome, Firefox and VLC for every offered combination (section 2.4 is unverified).
- Decide on the helper's PNG decoder (enable zlib or drop it) and whether to add an AV1 decoder (`libdav1d`, BSD-2-Clause) for decode-back probes.
- gifski x86_64, and all Windows/Linux helper builds, are not built yet.
