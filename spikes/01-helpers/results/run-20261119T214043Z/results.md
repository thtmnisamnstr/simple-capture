# Spike 1 helper benchmark results

- Run: 2026-11-19T21:40:44Z -> 2026-11-19T21:43:12Z (UTC); clip 10 s per encode; queue depth 3 frames
- Machine: `{"cpu": "Apple M3 Pro", "ncpu": "11", "perf_cores": "5", "eff_cores": "6", "mem_bytes": "19327352832", "macos": "26.7.2", "loadavg_start": "4.79 4.56 7.37"}`
- Helper: `ffmpeg version 9.0.2-simple-capture Copyright (c) 2000-2026 the FFmpeg developers`, `gifski 1.34.0`; validator: `ffprobe version 9.0.2 Copyright (c) 2007-2026 the FFmpeg developers`
- Load average at end: 6.65 5.83 7.46 (other heavy jobs may be running: numbers are preliminary)

## Synthetic frame generator (single thread)

- {"res": "1920x1080", "fmt": "I420", "frames_per_s": 849.5183}
- {"res": "1920x1080", "fmt": "Nv12", "frames_per_s": 810.4436}
- {"res": "2560x1440", "fmt": "I420", "frames_per_s": 465.0931}
- {"res": "2560x1440", "fmt": "Nv12", "frames_per_s": 420.2511}

## Pipe transport ceiling (generator -> bounded queue -> stdin -> ffmpeg demux -> null)

- {"res": "1920x1080", "input": "Y4M", "frames": 600.0000, "frames_per_s": 698.9791, "mb_per_s": 2174.1045, "gen_wait_s": 0.0157, "helper_cpu_pct": 33.0353, "exit": "exit 0"}
- {"res": "1920x1080", "input": "NV12", "frames": 600.0000, "frames_per_s": 682.2178, "mb_per_s": 2121.9703, "gen_wait_s": 0.0083, "helper_cpu_pct": 34.2133, "exit": "exit 0"}
- {"res": "2560x1440", "input": "Y4M", "frames": 600.0000, "frames_per_s": 394.1470, "mb_per_s": 2179.4752, "gen_wait_s": 0.0089, "helper_cpu_pct": 35.5426, "exit": "exit 0"}
- {"res": "2560x1440", "input": "NV12", "frames": 600.0000, "frames_per_s": 380.2947, "mb_per_s": 2102.8774, "gen_wait_s": 0.0092, "helper_cpu_pct": 33.1905, "exit": "exit 0"}

## Encode speed

| job | group | input | frames | wall s | enc fps | x realtime | helper CPU % | helper CPU s | peak RSS MB | size MB | Mbps | gen wait s | pipe write s | progress updates | load before |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| x264_medium_crf20_aacat | best | Y4M | 300 | 1.55 | 193.2 | 6.44 | 664 | 10.3 | 700 | 1.69 | 1.35 | 0.80 | 1.27 | 4 | 4.97 4.60 7.37 |
| x264_slow_crf20_aacat | best | Y4M | 300 | 2.06 | 145.3 | 4.84 | 622 | 12.8 | 781 | 1.60 | 1.28 | 1.05 | 1.52 | 5 | 5.05 4.62 7.36 |
| x264_medium_crf20_aac | best | Y4M | 300 | 1.76 | 170.6 | 5.69 | 616 | 10.8 | 699 | 1.51 | 1.21 | 0.93 | 1.42 | 4 | 5.05 4.62 7.36 |
| x265_medium_crf24_aacat | best | Y4M | 300 | 2.50 | 120.1 | 4.00 | 763 | 19.1 | 715 | 1.22 | 0.98 | 1.67 | 2.21 | 5 | 5.12 4.64 7.35 |
| svtav1_p8_crf32_aacat | best | Y4M | 300 | 1.59 | 189.1 | 6.30 | 725 | 11.5 | 968 | 1.27 | 1.02 | 0.66 | 1.27 | 4 | 4.95 4.62 7.32 |
| svtav1_p6_crf32_aacat | best | Y4M | 300 | 3.13 | 95.8 | 3.19 | 752 | 23.5 | 1168 | 1.04 | 0.84 | 1.78 | 2.28 | 7 | 4.95 4.62 7.32 |
| vp9_good_cpu4_crf32_opus | best | Y4M | 300 | 4.66 | 64.3 | 2.14 | 690 | 32.2 | 378 | 1.30 | 1.04 | 3.51 | 4.04 | 10 | 5.76 4.79 7.37 |
| vp9_good_cpu2_crf32_opus | best | Y4M | 300 | 5.29 | 56.7 | 1.89 | 707 | 37.4 | 378 | 1.24 | 0.99 | 4.08 | 4.64 | 11 | 6.82 5.02 7.44 |
| vp8_good_cpu4_vorbis | best | Y4M | 300 | 3.35 | 89.5 | 2.98 | 599 | 20.1 | 150 | 5.43 | 4.35 | 2.53 | 3.23 | 7 | 7.55 5.21 7.49 |
| h264_vt_q65_aacat | fast | Y4M | 300 | 1.62 | 185.3 | 6.18 | 33 | 0.5 | 134 | 4.74 | 3.79 | 1.03 | 1.46 | 4 | 8.07 5.35 7.53 |
| hevc_vt_q65_aacat | fast | Y4M | 300 | 1.70 | 176.7 | 5.89 | 32 | 0.5 | 132 | 5.20 | 4.16 | 1.08 | 1.53 | 4 | 8.07 5.35 7.53 |
| x264_medium_crf20_t4 | proxy4 | Y4M | 300 | 2.76 | 108.8 | 3.63 | 343 | 9.5 | 466 | 1.43 | 1.14 | 1.66 | 2.25 | 6 | 7.90 5.36 7.52 |
| x264_slow_crf20_t4 | proxy4 | Y4M | 300 | 3.42 | 87.8 | 2.93 | 305 | 10.4 | 526 | 1.33 | 1.07 | 2.09 | 2.62 | 7 | 8.31 5.49 7.55 |
| x265_medium_crf24_t4 | proxy4 | Y4M | 300 | 3.49 | 85.9 | 2.86 | 380 | 13.3 | 614 | 1.01 | 0.81 | 2.54 | 3.07 | 7 | 8.31 5.49 7.55 |
| svtav1_p8_crf32_lp1 | proxy4 | Y4M | 300 | 7.39 | 40.6 | 1.35 | 102 | 7.5 | 510 | 1.06 | 0.85 | 5.82 | 6.44 | 15 | 8.37 5.55 7.56 |
| svtav1_p8_crf32_lp2 | proxy4 | Y4M | 300 | 3.14 | 95.7 | 3.19 | 293 | 9.2 | 668 | 1.06 | 0.85 | 2.24 | 2.71 | 7 | 8.22 5.61 7.56 |
| vp9_good_cpu4_crf32_t4 | proxy4 | Y4M | 300 | 6.60 | 45.5 | 1.52 | 347 | 22.9 | 344 | 1.27 | 1.01 | 5.29 | 5.82 | 14 | 8.60 5.73 7.59 |
| x264_medium_crf20 | input | Y4M | 300 | 1.67 | 179.4 | 5.98 | 636 | 10.6 | 692 | 1.48 | 1.18 | 0.88 | 1.37 | 4 | 8.80 5.82 7.61 |
| x264_medium_crf20_nv12 | input | NV12 | 300 | 1.70 | 176.6 | 5.89 | 632 | 10.7 | 705 | 1.48 | 1.18 | 0.89 | 1.39 | 4 | 8.49 5.81 7.59 |
| h264_vt_q65 | input | Y4M | 300 | 1.65 | 182.2 | 6.07 | 27 | 0.4 | 123 | 4.53 | 3.62 | 1.05 | 1.48 | 4 | 8.49 5.81 7.59 |
| h264_vt_q65_nv12 | input | NV12 | 300 | 1.57 | 191.0 | 6.37 | 35 | 0.5 | 128 | 4.53 | 3.62 | 0.97 | 1.41 | 4 | 8.45 5.84 7.59 |
| x264_medium_crf20_1440p | 1440p | Y4M | 300 | 2.74 | 109.3 | 3.64 | 658 | 18.1 | 1202 | 2.38 | 1.90 | 1.33 | 2.19 | 6 | 8.45 5.84 7.59 |
| x265_medium_crf24_1440p | 1440p | Y4M | 300 | 4.18 | 71.7 | 2.39 | 773 | 32.3 | 1098 | 1.66 | 1.33 | 2.67 | 3.67 | 9 | 8.50 5.90 7.60 |
| svtav1_p8_crf32_1440p | 1440p | Y4M | 300 | 2.62 | 114.7 | 3.82 | 706 | 18.5 | 1349 | 1.48 | 1.18 | 0.78 | 1.88 | 6 | 8.62 5.96 7.61 |
| h264_vt_q65_1440p | 1440p | Y4M | 300 | 2.62 | 114.5 | 3.82 | 26 | 0.7 | 204 | 7.93 | 6.34 | 1.60 | 2.34 | 6 | 8.73 6.03 7.63 |
| hevc_vt_q65_1440p_nv12 | 1440p | NV12 | 300 | 2.83 | 106.0 | 3.53 | 42 | 1.2 | 211 | 9.00 | 7.20 | 1.73 | 2.52 | 6 | 8.73 6.03 7.63 |
| gif_ffmpeg_palette | gif | Y4M | 150 | 9.23 | 16.3 | 1.08 | 109 | 10.0 | 455 | 36.71 | 29.37 | 0.41 | 0.47 | 18 | 8.03 5.93 7.58 |
| gif_ffmpeg_palette_untagged | demo | Y4M | 150 | 9.50 | 15.8 | 1.05 | 108 | 10.3 | 459 | 36.68 | 29.34 | 0.41 | 0.47 | 18 | 7.43 5.90 7.54 |
| gif_gifski_q90 | gif | Y4M | 150 | 3.67 | 40.9 | 2.73 | 705 | 25.8 | 213 | 26.18 | 20.94 | 2.91 | 3.10 | 0 | 6.67 5.79 7.48 |
| gif_gifski_q70 | gif | Y4M | 150 | 2.64 | 56.9 | 3.79 | 820 | 21.6 | 194 | 17.68 | 14.15 | 2.08 | 2.23 | 0 | 6.37 5.74 7.46 |

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
| x264_q_fifo | ffmpeg | Fifo | q-on-stdin | exit 0 | 0.213 | false | 102 | false | 612421 | 101 | true | true |
| x264_sigterm | ffmpeg | Stdin | SIGTERM | exit 255 | 0.229 | false | 91 | false | 561442 | 90 | true | true |
| x264_sigkill | ffmpeg | Stdin | SIGKILL | signal 9 | 0.010 | false | 90 | false | 262192 | unreadable: [mov,mp4,m4a,3gp,3g2,mj2 @ 0x732c18000] moov atom not found | false | true |
| x264_eof | ffmpeg | Stdin | close-input (EOF) | exit 0 | 0.267 | false | 91 | false | 732728 | 91 | true | true |
| x264_hung_sigterm_then_kill | ffmpeg | Stdin | SIGTERM to hung helper -> SIGKILL | signal 9 | 1.547 | true | 90 | false | 262192 | unreadable: [mov,mp4,m4a,3gp,3g2,mj2 @ 0x70ec18000] moov atom not found | false | true |
| gifski_sigterm | gifski | Stdin | SIGTERM | signal 15 | 0.005 | false | 45 | false | 3535934 | 20 | true | true |
| gifski_eof | gifski | Stdin | close-input (EOF) | exit 0 | 0.685 | false | 46 | false | 8080368 | 46 | true | true |

## Parent crash (spike process SIGKILLed while feeding the helper)

`{"helper_exited_on_its_own": true, "seconds_until_helper_gone": 0.2183, "leftover_partial_exists": true, "leftover_partial_bytes": 1598524.0000, "leftover_partial_frames": "340", "leftover_partial_playable": true}`

## Robustness / capability probes

- `{"case": "missing helper binary", "handled": true, "result": "helper not found: /nonexistent/simple-capture/ffmpeg (is the app bundle complete?)"}`
- `{"case": "encoder not compiled in (libaom-av1)", "handled": true, "result": "exit 8; writer: Some(\"Broken pipe (os error 32)\"); stderr: Error opening output files: Encoder not found"}`
- `{"case": "h264_videotoolbox at 4096x2304 (probe encode, 6 frames)", "encoded": true, "result": ""}`
- `{"case": "h264_videotoolbox at 4096x4096 (probe encode, 6 frames)", "encoded": true, "result": ""}`
- `{"case": "h264_videotoolbox at 8192x4320 (probe encode, 6 frames)", "encoded": false, "result": "[h264_videotoolbox @ 0xb65038000] Cannot create compression session: -12903 | [h264_videotoolbox @ 0xb65038000] Try -allow_sw 1. The hardware encoder may be busy, or not supported. | [vost#0:0/h264_videotoolbox @ 0xb64c38000] [enc:h264_videotoolbox @ 0xb6503c000] Error while opening encoder - maybe incorrect parameters such as bit_rate, rate, width or height. | [vf#0:0 @ 0xb65040000] Error sending frames to consumers: Generic error in an external library | [vf#0:0 @ 0xb65040000] Task finished with error code: -542398533 (Generic error in an external library) | [vf#0:0 @ 0xb65040000] Terminating thread with return code -542398533 (Generic error in an external library) | [vost#0:0/h264_videotoolbox @ 0xb64c38000] [enc:h264_videotoolbox @ 0xb6503c000] Could not open encoder before EOF | [vost#0:0/h264_videotoolbox @ 0xb64c38000] Task finished with error code: -22 (Invalid argument) | [vost#0:0/h264_videotoolbox @ 0xb64c38000] Terminating thread with return code -22 (Invalid argument) | [out#0/null @ 0xb65020480] Nothing was written into output file, because at least one of its streams received no packets."}`
- `{"case": "hevc_videotoolbox at 8192x4320 (probe encode, 6 frames)", "encoded": true, "result": ""}`
- `{"case": "hevc_videotoolbox at 7680x4320 (probe encode, 6 frames)", "encoded": true, "result": ""}`
- `{"case": "h264_videotoolbox at 1918x1080 (probe encode, 6 frames)", "encoded": true, "result": ""}`
