# Spike 1 helper benchmark results

- Run: 2026-11-19T21:45:46Z -> 2026-11-19T21:48:22Z (UTC); clip 10 s per encode; queue depth 3 frames
- Machine: `{"cpu": "Apple M3 Pro", "ncpu": "11", "perf_cores": "5", "eff_cores": "6", "mem_bytes": "19327352832", "macos": "26.7.2", "loadavg_start": "6.92 6.53 7.49"}`
- Helper: `ffmpeg version 9.0.2-simple-capture Copyright (c) 2000-2026 the FFmpeg developers`, `gifski 1.34.0`; validator: `ffprobe version 9.0.2 Copyright (c) 2007-2026 the FFmpeg developers`
- Load average at end: 7.57 7.35 7.69 (other heavy jobs may be running: numbers are preliminary)

## Synthetic frame generator (single thread)

- {"res": "1920x1080", "fmt": "I420", "frames_per_s": 805.1614}
- {"res": "1920x1080", "fmt": "Nv12", "frames_per_s": 747.6229}
- {"res": "2560x1440", "fmt": "I420", "frames_per_s": 431.7612}
- {"res": "2560x1440", "fmt": "Nv12", "frames_per_s": 419.2404}

## Pipe transport ceiling (generator -> bounded queue -> stdin -> ffmpeg demux -> null)

- {"res": "1920x1080", "input": "Y4M", "frames": 600.0000, "frames_per_s": 704.0832, "mb_per_s": 2189.9803, "gen_wait_s": 0.0119, "helper_cpu_pct": 29.6243, "exit": "exit 0"}
- {"res": "1920x1080", "input": "NV12", "frames": 600.0000, "frames_per_s": 666.1776, "mb_per_s": 2072.0787, "gen_wait_s": 0.0103, "helper_cpu_pct": 33.8066, "exit": "exit 0"}
- {"res": "2560x1440", "input": "Y4M", "frames": 600.0000, "frames_per_s": 385.9546, "mb_per_s": 2134.1743, "gen_wait_s": 0.0085, "helper_cpu_pct": 35.6242, "exit": "exit 0"}
- {"res": "2560x1440", "input": "NV12", "frames": 600.0000, "frames_per_s": 370.8701, "mb_per_s": 2050.7635, "gen_wait_s": 0.0093, "helper_cpu_pct": 33.1557, "exit": "exit 0"}

## Encode speed

| job | group | input | frames | wall s | enc fps | x realtime | helper CPU % | helper CPU s | peak RSS MB | size MB | Mbps | gen wait s | pipe write s | progress updates | load before |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| x264_medium_crf20_aacat | best | Y4M | 300 | 1.51 | 198.6 | 6.62 | 684 | 10.3 | 708 | 1.69 | 1.35 | 0.76 | 1.23 | 3 | 6.16 6.38 7.43 |
| x264_slow_crf20_aacat | best | Y4M | 300 | 2.28 | 131.4 | 4.38 | 582 | 13.3 | 779 | 1.60 | 1.28 | 1.21 | 1.74 | 5 | 6.16 6.38 7.43 |
| x264_medium_crf20_aac | best | Y4M | 300 | 1.55 | 193.2 | 6.44 | 678 | 10.5 | 703 | 1.51 | 1.21 | 0.78 | 1.26 | 4 | 6.47 6.44 7.44 |
| x265_medium_crf24_aacat | best | Y4M | 300 | 2.64 | 113.8 | 3.79 | 731 | 19.3 | 717 | 1.22 | 0.98 | 1.80 | 2.33 | 6 | 6.47 6.44 7.44 |
| svtav1_p8_crf32_aacat | best | Y4M | 300 | 1.61 | 186.4 | 6.21 | 710 | 11.4 | 981 | 1.27 | 1.02 | 0.65 | 1.28 | 4 | 7.07 6.56 7.48 |
| svtav1_p6_crf32_aacat | best | Y4M | 300 | 3.18 | 94.2 | 3.14 | 744 | 23.7 | 1286 | 1.04 | 0.84 | 1.65 | 2.25 | 7 | 7.07 6.56 7.48 |
| vp9_good_cpu4_crf32_opus | best | Y4M | 300 | 5.53 | 54.3 | 1.81 | 630 | 34.8 | 378 | 1.30 | 1.04 | 4.16 | 4.75 | 11 | 9.55 7.08 7.66 |
| vp9_good_cpu2_crf32_opus | best | Y4M | 300 | 6.01 | 50.0 | 1.67 | 665 | 39.9 | 378 | 1.24 | 0.99 | 4.64 | 5.25 | 12 | 10.79 7.38 7.76 |
| vp8_good_cpu4_vorbis | best | Y4M | 300 | 2.60 | 115.3 | 3.84 | 512 | 13.3 | 150 | 5.43 | 4.35 | 1.92 | 2.46 | 6 | 10.82 7.50 7.80 |
| h264_vt_q65_aacat | fast | Y4M | 300 | 1.56 | 191.7 | 6.39 | 31 | 0.5 | 132 | 4.74 | 3.79 | 0.99 | 1.41 | 4 | 10.82 7.50 7.80 |
| hevc_vt_q65_aacat | fast | Y4M | 300 | 1.76 | 170.5 | 5.68 | 34 | 0.6 | 135 | 5.20 | 4.16 | 1.13 | 1.58 | 4 | 10.11 7.41 7.77 |
| x264_medium_crf20_t4 | proxy4 | Y4M | 300 | 2.58 | 116.2 | 3.87 | 362 | 9.3 | 466 | 1.43 | 1.14 | 1.55 | 2.12 | 6 | 9.86 7.40 7.76 |
| x264_slow_crf20_t4 | proxy4 | Y4M | 300 | 4.30 | 69.8 | 2.33 | 295 | 12.7 | 526 | 1.33 | 1.07 | 2.82 | 3.48 | 9 | 9.86 7.40 7.76 |
| x265_medium_crf24_t4 | proxy4 | Y4M | 300 | 3.43 | 87.4 | 2.91 | 383 | 13.1 | 622 | 1.01 | 0.81 | 2.53 | 3.00 | 7 | 10.27 7.53 7.80 |
| svtav1_p8_crf32_lp1 | proxy4 | Y4M | 300 | 7.80 | 38.5 | 1.28 | 99 | 7.7 | 499 | 1.06 | 0.85 | 6.06 | 6.72 | 16 | 9.93 7.50 7.79 |
| svtav1_p8_crf32_lp2 | proxy4 | Y4M | 300 | 3.32 | 90.5 | 3.02 | 284 | 9.4 | 668 | 1.06 | 0.85 | 2.41 | 2.87 | 7 | 9.27 7.44 7.76 |
| vp9_good_cpu4_crf32_t4 | proxy4 | Y4M | 300 | 7.16 | 41.9 | 1.40 | 342 | 24.5 | 338 | 1.27 | 1.01 | 5.80 | 6.31 | 15 | 8.93 7.40 7.75 |
| x264_medium_crf20 | input | Y4M | 300 | 1.90 | 157.7 | 5.26 | 585 | 11.1 | 691 | 1.48 | 1.18 | 0.95 | 1.44 | 4 | 8.62 7.36 7.73 |
| x264_medium_crf20_nv12 | input | NV12 | 300 | 1.59 | 189.0 | 6.30 | 666 | 10.6 | 704 | 1.48 | 1.18 | 0.80 | 1.28 | 4 | 8.49 7.35 7.73 |
| h264_vt_q65 | input | Y4M | 300 | 1.57 | 191.2 | 6.37 | 28 | 0.4 | 130 | 4.53 | 3.62 | 0.95 | 1.41 | 4 | 8.49 7.35 7.73 |
| h264_vt_q65_nv12 | input | NV12 | 300 | 1.56 | 192.4 | 6.41 | 42 | 0.6 | 131 | 4.53 | 3.62 | 0.95 | 1.39 | 4 | 8.49 7.35 7.73 |
| x264_medium_crf20_1440p | 1440p | Y4M | 300 | 3.44 | 87.2 | 2.91 | 565 | 19.4 | 1184 | 2.38 | 1.90 | 1.65 | 2.60 | 7 | 8.05 7.28 7.70 |
| x265_medium_crf24_1440p | 1440p | Y4M | 300 | 4.80 | 62.5 | 2.08 | 706 | 33.9 | 1006 | 1.66 | 1.33 | 3.17 | 4.26 | 10 | 8.52 7.39 7.74 |
| svtav1_p8_crf32_1440p | 1440p | Y4M | 300 | 4.03 | 74.4 | 2.48 | 524 | 21.1 | 1262 | 1.48 | 1.18 | 1.18 | 2.33 | 8 | 9.04 7.52 7.78 |
| h264_vt_q65_1440p | 1440p | Y4M | 300 | 2.62 | 114.4 | 3.81 | 27 | 0.7 | 204 | 7.93 | 6.34 | 1.57 | 2.34 | 6 | 9.04 7.54 7.79 |
| hevc_vt_q65_1440p_nv12 | 1440p | NV12 | 300 | 2.81 | 106.7 | 3.56 | 39 | 1.1 | 212 | 9.00 | 7.20 | 1.71 | 2.52 | 6 | 8.79 7.51 7.77 |
| gif_ffmpeg_palette | gif | Y4M | 150 | 10.56 | 14.2 | 0.95 | 108 | 11.4 | 428 | 36.71 | 29.37 | 0.49 | 0.56 | 20 | 8.49 7.47 7.76 |
| gif_ffmpeg_palette_untagged | demo | Y4M | 150 | 9.61 | 15.6 | 1.04 | 107 | 10.3 | 449 | 36.68 | 29.34 | 0.39 | 0.44 | 19 | 8.23 7.45 7.75 |
| gif_gifski_q90 | gif | Y4M | 150 | 3.61 | 41.5 | 2.77 | 710 | 25.7 | 208 | 26.18 | 20.94 | 2.83 | 3.05 | 0 | 7.18 7.25 7.67 |
| gif_gifski_q70 | gif | Y4M | 150 | 2.61 | 57.4 | 3.83 | 835 | 21.8 | 212 | 17.68 | 14.15 | 2.04 | 2.19 | 0 | 7.65 7.35 7.70 |

## Validation (ffprobe: ffprobe version 9.0.2 Copyright (c) 2007-2026 the FFmpeg developers)

| job | result | codec | tag | size | r_frame_rate | frames | duration s | pix_fmt | primaries/trc/space/range | audio | color max err | GIF delays (cs: count) | loop |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| x264_medium_crf20_aacat | PASS | h264 | avc1 | 1920x1080 | 30/1 | 300 | 10.000000 | yuv420p | bt709/bt709/bt709/tv | aac 48000Hz 2ch 10.005333s | 1 |  |  |
| x264_slow_crf20_aacat | PASS | h264 | avc1 | 1920x1080 | 30/1 | 300 | 10.000000 | yuv420p | bt709/bt709/bt709/tv | aac 48000Hz 2ch 10.005333s | 1 |  |  |
| x264_medium_crf20_aac | PASS | h264 | avc1 | 1920x1080 | 30/1 | 300 | 10.000000 | yuv420p | bt709/bt709/bt709/tv | aac 48000Hz 2ch 10.000000s | 1 |  |  |
| x265_medium_crf24_aacat | PASS | hevc | hvc1 | 1920x1080 | 30/1 | 300 | 10.000000 | yuv420p | bt709/bt709/bt709/tv | aac 48000Hz 2ch 10.005333s | 1 |  |  |
| svtav1_p8_crf32_aacat | PASS | av1 | av01 | 1920x1080 | 30/1 | 300 | 10.000000 | yuv420p | bt709/bt709/bt709/tv | aac 48000Hz 2ch 10.005333s | 1 |  |  |
| svtav1_p6_crf32_aacat | PASS | av1 | av01 | 1920x1080 | 30/1 | 300 | 10.000000 | yuv420p | bt709/bt709/bt709/tv | aac 48000Hz 2ch 10.005333s | 1 |  |  |
| vp9_good_cpu4_crf32_opus | PASS | vp9 | [0][0][0][0] | 1920x1080 | 30/1 | 300 | 10.008000 (fmt) | yuv420p | bt709/bt709/bt709/tv | opus 48000Hz 2ch N/As | 1 |  |  |
| vp9_good_cpu2_crf32_opus | PASS | vp9 | [0][0][0][0] | 1920x1080 | 30/1 | 300 | 10.008000 (fmt) | yuv420p | bt709/bt709/bt709/tv | opus 48000Hz 2ch N/As | 1 |  |  |
| vp8_good_cpu4_vorbis | PASS | vp8 | [0][0][0][0] | 1920x1080 | 30/1 | 300 | 10.003000 (fmt) | yuv420p | bt709/bt709/smpte170m/tv | vorbis 48000Hz 2ch N/As | 4 |  |  |
| h264_vt_q65_aacat | PASS | h264 | avc1 | 1920x1080 | 30/1 | 300 | 10.000000 | yuv420p | bt709/bt709/bt709/tv | aac 48000Hz 2ch 10.005333s | 2 |  |  |
| hevc_vt_q65_aacat | PASS | hevc | hvc1 | 1920x1080 | 30/1 | 300 | 10.000000 | yuv420p | bt709/bt709/bt709/tv | aac 48000Hz 2ch 10.005333s | 1 |  |  |
| x264_medium_crf20_t4 | PASS | h264 | avc1 | 1920x1080 | 30/1 | 300 | 10.000000 | yuv420p | bt709/bt709/bt709/tv |  | 1 |  |  |
| x264_slow_crf20_t4 | PASS | h264 | avc1 | 1920x1080 | 30/1 | 300 | 10.000000 | yuv420p | bt709/bt709/bt709/tv |  | 1 |  |  |
| x265_medium_crf24_t4 | PASS | hevc | hvc1 | 1920x1080 | 30/1 | 300 | 10.000000 | yuv420p | bt709/bt709/bt709/tv |  | 1 |  |  |
| svtav1_p8_crf32_lp1 | PASS | av1 | av01 | 1920x1080 | 30/1 | 300 | 10.000000 | yuv420p | bt709/bt709/bt709/tv |  | 1 |  |  |
| svtav1_p8_crf32_lp2 | PASS | av1 | av01 | 1920x1080 | 30/1 | 300 | 10.000000 | yuv420p | bt709/bt709/bt709/tv |  | 1 |  |  |
| vp9_good_cpu4_crf32_t4 | PASS | vp9 | [0][0][0][0] | 1920x1080 | 30/1 | 300 | 10.000000 (fmt) | yuv420p | bt709/bt709/bt709/tv |  | 1 |  |  |
| x264_medium_crf20 | PASS | h264 | avc1 | 1920x1080 | 30/1 | 300 | 10.000000 | yuv420p | bt709/bt709/bt709/tv |  | 1 |  |  |
| x264_medium_crf20_nv12 | PASS | h264 | avc1 | 1920x1080 | 30/1 | 300 | 10.000000 | yuv420p | bt709/bt709/bt709/tv |  | 1 |  |  |
| h264_vt_q65 | PASS | h264 | avc1 | 1920x1080 | 30/1 | 300 | 10.000000 | yuv420p | bt709/bt709/bt709/tv |  | 2 |  |  |
| h264_vt_q65_nv12 | PASS | h264 | avc1 | 1920x1080 | 30/1 | 300 | 10.000000 | yuv420p | bt709/bt709/bt709/tv |  | 2 |  |  |
| x264_medium_crf20_1440p | PASS | h264 | avc1 | 2560x1440 | 30/1 | 300 | 10.000000 | yuv420p | bt709/bt709/bt709/tv | aac 48000Hz 2ch 10.005333s | 1 |  |  |
| x265_medium_crf24_1440p | PASS | hevc | hvc1 | 2560x1440 | 30/1 | 300 | 10.000000 | yuv420p | bt709/bt709/bt709/tv | aac 48000Hz 2ch 10.005333s | 1 |  |  |
| svtav1_p8_crf32_1440p | PASS | av1 | av01 | 2560x1440 | 30/1 | 300 | 10.000000 | yuv420p | bt709/bt709/bt709/tv | aac 48000Hz 2ch 10.005333s | 1 |  |  |
| h264_vt_q65_1440p | PASS | h264 | avc1 | 2560x1440 | 30/1 | 300 | 10.000000 | yuv420p | bt709/bt709/bt709/tv | aac 48000Hz 2ch 10.005333s | 3 |  |  |
| hevc_vt_q65_1440p_nv12 | PASS | hevc | hvc1 | 2560x1440 | 30/1 | 300 | 10.000000 | yuv420p | bt709/bt709/bt709/tv | aac 48000Hz 2ch 10.005333s | 1 |  |  |
| gif_ffmpeg_palette | PASS | gif | [0][0][0][0] | 960x540 | 15/1 | 150 | 10.000000 | bgra | unknown/unknown/unknown/unknown |  | 10 | {6: 50, 7: 100} | Some(0) |
| gif_ffmpeg_palette_untagged | EXPECTED FAIL (pitfall demo): color_patches: max |err| 32 (tol 12) red75:[191, 0, 0]->[174, 12, 27] green75:[0, 191, 0]->[15, 223, 5] blue75:[0, 0, 191]->[2, 3, 184] orange:[230, 120, 30]->[219, 115, 36] gray50:[128, 128, 128]->[128, 128, 127] skin:[200, 150, 120]->[195, 146, 122] | gif | [0][0][0][0] | 960x540 | 15/1 | 150 | 10.000000 | bgra | unknown/unknown/unknown/unknown |  | 32 | {6: 50, 7: 100} | Some(0) |
| gif_gifski_q90 | PASS | gif | [0][0][0][0] | 960x540 | 15/1 | 150 | 10.000000 | bgra | unknown/unknown/unknown/unknown |  | 1 | {6: 50, 7: 100} | Some(0) |
| gif_gifski_q70 | PASS | gif | [0][0][0][0] | 960x540 | 15/1 | 150 | 10.000000 | bgra | unknown/unknown/unknown/unknown |  | 2 | {6: 50, 7: 100} | Some(0) |

## Cancel tests (1080p30; cancel requested after 90 frames for x264, 45 for gifski; grace 1.5 s then SIGKILL)

| case | tool | transport | method | exit | time to exit s | escalated to SIGKILL | frames sent | pid alive after | partial bytes | partial frames (ffprobe) | partial playable | deleted |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| x264_q_fifo | ffmpeg | Fifo | q-on-stdin | exit 0 | 0.261 | false | 109 | false | 639596 | 108 | true | true |
| x264_sigterm | ffmpeg | Stdin | SIGTERM | exit 255 | 0.221 | false | 91 | false | 554355 | 90 | true | true |
| x264_sigkill | ffmpeg | Stdin | SIGKILL | signal 9 | 0.010 | false | 90 | false | 262192 | unreadable: [mov,mp4,m4a,3gp,3g2,mj2 @ 0xa93020000] moov atom not found | false | true |
| x264_eof | ffmpeg | Stdin | close-input (EOF) | exit 0 | 0.312 | false | 91 | false | 732728 | 91 | true | true |
| x264_hung_sigterm_then_kill | ffmpeg | Stdin | SIGTERM to hung helper -> SIGKILL | signal 9 | 1.549 | true | 90 | false | 262192 | unreadable: [mov,mp4,m4a,3gp,3g2,mj2 @ 0xa29020000] moov atom not found | false | true |
| gifski_sigterm | gifski | Stdin | SIGTERM | signal 15 | 0.005 | false | 45 | false | 3535934 | 20 | true | true |
| gifski_eof | gifski | Stdin | close-input (EOF) | exit 0 | 0.603 | false | 46 | false | 8080368 | 46 | true | true |

## Parent crash (spike process SIGKILLed while feeding the helper)

`{"helper_exited_on_its_own": true, "seconds_until_helper_gone": 0.2322, "leftover_partial_exists": true, "leftover_partial_bytes": 1601925.0000, "leftover_partial_frames": "341", "leftover_partial_playable": true}`

## Robustness / capability probes

- `{"case": "missing helper binary", "handled": true, "result": "helper not found: /nonexistent/simple-capture/ffmpeg (is the app bundle complete?)"}`
- `{"case": "encoder not compiled in (libaom-av1)", "handled": true, "result": "exit 8; writer: Some(\"Broken pipe (os error 32)\"); stderr: Error opening output files: Encoder not found"}`
- `{"case": "h264_videotoolbox at 4096x2304 (probe encode, 6 frames)", "encoded": true, "result": ""}`
- `{"case": "h264_videotoolbox at 4096x4096 (probe encode, 6 frames)", "encoded": true, "result": ""}`
- `{"case": "h264_videotoolbox at 8192x4320 (probe encode, 6 frames)", "encoded": false, "result": "[h264_videotoolbox @ 0x751034000] Cannot create compression session: -12903 | [h264_videotoolbox @ 0x751034000] Try -allow_sw 1. The hardware encoder may be busy, or not supported. | [vost#0:0/h264_videotoolbox @ 0x751048000] [enc:h264_videotoolbox @ 0x751038000] Error while opening encoder - maybe incorrect parameters such as bit_rate, rate, width or height. | [vf#0:0 @ 0x750c34000] Error sending frames to consumers: Generic error in an external library | [vf#0:0 @ 0x750c34000] Task finished with error code: -542398533 (Generic error in an external library) | [vf#0:0 @ 0x750c34000] Terminating thread with return code -542398533 (Generic error in an external library) | [vost#0:0/h264_videotoolbox @ 0x751048000] [enc:h264_videotoolbox @ 0x751038000] Could not open encoder before EOF | [vost#0:0/h264_videotoolbox @ 0x751048000] Task finished with error code: -22 (Invalid argument) | [vost#0:0/h264_videotoolbox @ 0x751048000] Terminating thread with return code -22 (Invalid argument) | [out#0/null @ 0x750c18480] Nothing was written into output file, because at least one of its streams received no packets."}`
- `{"case": "hevc_videotoolbox at 8192x4320 (probe encode, 6 frames)", "encoded": true, "result": ""}`
- `{"case": "hevc_videotoolbox at 7680x4320 (probe encode, 6 frames)", "encoded": true, "result": ""}`
- `{"case": "h264_videotoolbox at 1918x1080 (probe encode, 6 frames)", "encoded": true, "result": ""}`
