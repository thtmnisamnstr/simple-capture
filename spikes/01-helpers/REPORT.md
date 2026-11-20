# Spike 1 report: export helpers (FFmpeg + gifski sidecars)
Status: pass, with corrections. Written 2026-11-19 from files on disk plus re-measurements in the consistency pass; the verifier's own output was not saved.
Machine: M3 Pro (5P+6E), 18 GB, macOS 26.7.2. Nothing was measured on the base M1 8 GB. Treat every speed as an upper bound. The 4-thread proxy is noisy.

## Question
Can we ship pinned, network-free FFmpeg and gifski sidecars, feed them frames over a pipe, get correctly tagged and valid files, and cancel cleanly?

## Method
- Helpers built from SHA-256-pinned sources (`third_party/ffmpeg/build-macos.sh`, `third_party/gifski/build.sh`). Binary hashes are in `results/run-*/binaries.txt`.
- `bench.sh` runs a std-only Rust harness that pipes synthetic 1080p30 / 1440p30 / 960x540@15 frames, as Y4M or NV12, through a 3-frame bounded queue.
- Every output is validated with Homebrew ffprobe/ffmpeg (reference tool only): codec, tag, size, fps, frame count, duration, pix_fmt, the four colour fields and colour-patch error.
- Cancel tests: q on a FIFO, SIGTERM, SIGKILL, EOF, a hung helper escalated SIGTERM→1.5 s→SIGKILL, and SIGKILL of the parent.
- Probes: missing helper, encoder not compiled in, VideoToolbox size limits.

## Evidence
All saved runs use a 10 s clip. `results.md` and `results.json` are committed; `out/`, `cancel/` and `logs/` are gitignored.

| Run | Load average at end | Use |
|---|---|---|
| `results/run-20261119T210716Z` | 11.4 | under load, not used |
| `results/run-20261119T211353Z` | 6.5 | not used |
| `results/run-20261119T214043Z`, `T214312Z`, `T214545Z` (= `latest.md`) | 6.7–7.6 | verification runs, medians |

## Results (1080p30, 10 s clip, median of the 3 verification runs)
| Job | fps | Notes |
|---|---|---|
| x264 medium CRF 20 | 196.8 (193.2 / 196.8 / 198.6) | RSS ~708 MB |
| x264 slow | 142.4 | |
| x265 medium CRF 24 | 120.1 | |
| SVT-AV1 p8 CRF 32 | 189.1 | ~1.0 GB RSS |
| SVT-AV1 p6 | 95.8 | 1.17–1.29 GB RSS |
| VP9 cpu-used 4 | 64.3 | |
| VP8 | 115.3 | BT.601 by design (ADR-029) |
| VT H.264 q65 | 190.6 | ~30% CPU |
| VT HEVC q65 | 176.7 | |
| gifski q90 / q70 (960x540@15) | 40.9 / 57.2 | |
| FFmpeg palette GIF | 14.2 (12.8–16.3) | single-threaded |
| Pipe ceiling | ~704 fps | |

- 4-thread proxy over the 5 saved runs: x264 108.8–146.0, x265 83.0–95.8, SVT lp2 87.3–102.4, lp1 38.5–44.2, VP9 41.9–49.9.
- Validation: all 29 outputs PASS in each verification run. The untagged palette GIF is an expected-fail demo (patch error 32). VP8 is tagged smpte170m by design.
- Cancel (all 5 runs agree):
  - ffmpeg SIGTERM: exits in 0.22 s with a playable partial. q / EOF: 0.26–0.31 s.
  - ffmpeg SIGKILL, or the escalated hung helper (1.53–1.55 s): no moov atom.
  - gifski SIGTERM: exits in ~5 ms; ffprobe counts 18–20 frames. Re-check this pass: the file is truncated (no 0x3B trailer, last frame fails LZW decode).
  - Parent killed: the helper exits on EOF in ~0.23 s.
  - No leftover processes in any case.

## VideoToolbox size limits (re-measured this pass, `-allow_sw 0`)
- h264_videotoolbox fails (-12903) when either dimension is over 4096: 4098x2160, 4112x2160, 4352x2160, 5120x2160, 5120x2880, 6016x3384, 7680x2160, 8192x4320, 2160x4098, 2160x4352, 1920x4320.
- It opens at 3840x2160, 4096x2160, 4096x2304, 4096x4096 and 2160x4096.
- With `-allow_sw 1` it opens at 5120x2880 and 7680x2160 (software). That probably explains spike 3's slow AVAssetWriter H.264 above 4096 (inference).
- hevc_videotoolbox opens at every size up to 8192x4320.

## 30 s clip (re-measured this pass, 2 runs, all outputs PASS)
- x264: 142.7 / 197.7 fps
- SVT-AV1 p8: 181.8 / 185.0 fps (the earlier "153.6" had no source and is withdrawn)
- VT H.264: 207.3 / 208.4 fps

## Verdict
Pass. Feeds ADR-013, 014, 015, 016, 017, 029 and CODEC_MATRIX §2.

## Open
- No M1 run.
- Small and High tiers not encoded.
- No PNG or AV1 decoder in the helper.
- gifski x86_64 not built.
- gifski notices incomplete and gifsicle provenance unreviewed (LICENSES §3.2).
- No GPG check.
