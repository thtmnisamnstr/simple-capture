# Spike 1 helper benchmark results

- Run: 2026-11-19T21:07:16Z -> 2026-11-19T21:10:12Z (UTC); clip 10 s per encode; queue depth 3 frames
- Machine: `{"cpu": "Apple M3 Pro", "ncpu": "11", "perf_cores": "5", "eff_cores": "6", "mem_bytes": "19327352832", "macos": "26.7.2", "loadavg_start": "18.30 18.47 25.57"}`
- Helper: `ffmpeg version 9.0.2-simple-capture Copyright (c) 2000-2026 the FFmpeg developers`, `gifski 1.34.0`; validator: `ffprobe version 9.0.2 Copyright (c) 2007-2026 the FFmpeg developers`
- Load average at end: 11.39 15.20 22.87 (other heavy jobs may be running: numbers are preliminary)

## Synthetic frame generator (single thread)

- {"res": "1920x1080", "fmt": "I420", "frames_per_s": 617.6356}
- {"res": "1920x1080", "fmt": "Nv12", "frames_per_s": 699.8530}
- {"res": "2560x1440", "fmt": "I420", "frames_per_s": 403.6160}
- {"res": "2560x1440", "fmt": "Nv12", "frames_per_s": 379.4719}

## Pipe transport ceiling (generator -> bounded queue -> stdin -> ffmpeg demux -> null)

- {"res": "1920x1080", "input": "Y4M", "frames": 600.0000, "frames_per_s": 657.4885, "mb_per_s": 2045.0521, "gen_wait_s": 0.0093, "helper_cpu_pct": 36.9766, "exit": "exit 0"}
- {"res": "1920x1080", "input": "NV12", "frames": 600.0000, "frames_per_s": 641.4574, "mb_per_s": 1995.1891, "gen_wait_s": 0.0089, "helper_cpu_pct": 35.8110, "exit": "exit 0"}
- {"res": "2560x1440", "input": "Y4M", "frames": 600.0000, "frames_per_s": 344.1955, "mb_per_s": 1903.2636, "gen_wait_s": 0.0088, "helper_cpu_pct": 36.2091, "exit": "exit 0"}
- {"res": "2560x1440", "input": "NV12", "frames": 600.0000, "frames_per_s": 267.6714, "mb_per_s": 1480.1158, "gen_wait_s": 0.0046, "helper_cpu_pct": 32.6040, "exit": "exit 0"}

## Encode speed

| job | group | input | frames | wall s | enc fps | x realtime | helper CPU % | helper CPU s | peak RSS MB | size MB | Mbps | gen wait s | pipe write s | progress updates | load before |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| x264_medium_crf20_aacat | best | Y4M | 300 | 2.83 | 105.9 | 3.53 | 469 | 13.3 | 702 | 1.69 | 1.35 | 1.46 | 2.32 | 6 | 17.07 18.22 25.43 |
| x264_slow_crf20_aacat | best | Y4M | 300 | 2.86 | 104.8 | 3.49 | 509 | 14.6 | 768 | 1.60 | 1.28 | 1.40 | 2.15 | 6 | 17.06 18.20 25.38 |
| x264_medium_crf20_aac | best | Y4M | 300 | 1.79 | 167.2 | 5.57 | 622 | 11.2 | 703 | 1.51 | 1.21 | 0.92 | 1.45 | 4 | 16.82 18.13 25.32 |
| x265_medium_crf24_aacat | best | Y4M | 300 | 3.90 | 76.9 | 2.56 | 536 | 20.9 | 690 | 1.22 | 0.98 | 2.93 | 3.53 | 8 | 16.82 18.13 25.32 |
| svtav1_p8_crf32_aacat | best | Y4M | 300 | 1.88 | 159.6 | 5.32 | 658 | 12.4 | 980 | 1.27 | 1.02 | 0.68 | 1.33 | 4 | 16.75 18.09 25.26 |
| svtav1_p6_crf32_aacat | best | Y4M | 300 | 3.70 | 81.1 | 2.70 | 674 | 24.9 | 1244 | 1.04 | 0.84 | 2.13 | 2.72 | 8 | 16.21 17.96 25.17 |
| vp9_good_cpu4_crf32_opus | best | Y4M | 300 | 6.35 | 47.3 | 1.58 | 569 | 36.1 | 378 | 1.30 | 1.04 | 4.87 | 5.46 | 13 | 15.07 17.69 25.04 |
| vp9_good_cpu2_crf32_opus | best | Y4M | 300 | 6.21 | 48.3 | 1.61 | 646 | 40.2 | 378 | 1.24 | 0.99 | 4.93 | 5.51 | 13 | 14.91 17.61 24.97 |
| vp8_good_cpu4_vorbis | best | Y4M | 300 | 3.58 | 83.8 | 2.79 | 526 | 18.8 | 150 | 5.43 | 4.35 | 2.91 | 3.44 | 8 | 14.59 17.50 24.88 |
| h264_vt_q65_aacat | fast | Y4M | 300 | 1.63 | 184.6 | 6.15 | 29 | 0.5 | 128 | 4.74 | 3.79 | 1.06 | 1.47 | 4 | 13.83 17.30 24.77 |
| hevc_vt_q65_aacat | fast | Y4M | 300 | 1.70 | 176.0 | 5.87 | 28 | 0.5 | 135 | 5.20 | 4.16 | 1.12 | 1.54 | 4 | 13.04 17.07 24.65 |
| x264_medium_crf20_t4 | proxy4 | Y4M | 300 | 2.46 | 121.7 | 4.06 | 370 | 9.1 | 468 | 1.43 | 1.14 | 1.47 | 2.01 | 5 | 13.92 17.19 24.64 |
| x264_slow_crf20_t4 | proxy4 | Y4M | 300 | 3.00 | 99.9 | 3.33 | 324 | 9.7 | 536 | 1.33 | 1.07 | 1.78 | 2.25 | 6 | 13.92 17.19 24.64 |
| x265_medium_crf24_t4 | proxy4 | Y4M | 300 | 3.35 | 89.5 | 2.98 | 384 | 12.9 | 620 | 1.01 | 0.81 | 2.49 | 2.95 | 7 | 13.28 17.00 24.53 |
| svtav1_p8_crf32_lp1 | proxy4 | Y4M | 300 | 6.80 | 44.1 | 1.47 | 102 | 6.9 | 561 | 1.06 | 0.85 | 5.33 | 5.87 | 14 | 12.70 16.82 24.42 |
| svtav1_p8_crf32_lp2 | proxy4 | Y4M | 300 | 2.93 | 102.4 | 3.41 | 303 | 8.9 | 662 | 1.06 | 0.85 | 2.08 | 2.50 | 6 | 12.08 16.62 24.31 |
| vp9_good_cpu4_crf32_t4 | proxy4 | Y4M | 300 | 6.01 | 49.9 | 1.66 | 359 | 21.6 | 344 | 1.27 | 1.01 | 4.81 | 5.24 | 12 | 11.59 16.45 24.20 |
| x264_medium_crf20 | input | Y4M | 300 | 1.77 | 169.6 | 5.65 | 601 | 10.6 | 700 | 1.48 | 1.18 | 0.85 | 1.34 | 4 | 11.15 16.27 24.09 |
| x264_medium_crf20_nv12 | input | NV12 | 300 | 1.87 | 160.0 | 5.33 | 589 | 11.0 | 697 | 1.48 | 1.18 | 1.00 | 1.56 | 4 | 10.89 16.14 24.00 |
| h264_vt_q65 | input | Y4M | 300 | 1.97 | 152.1 | 5.07 | 31 | 0.6 | 127 | 4.53 | 3.62 | 0.86 | 1.81 | 4 | 10.89 16.14 24.00 |
| h264_vt_q65_nv12 | input | NV12 | 300 | 1.71 | 175.9 | 5.86 | 38 | 0.6 | 128 | 4.53 | 3.62 | 0.79 | 1.52 | 4 | 13.46 16.58 24.11 |
| x264_medium_crf20_1440p | 1440p | Y4M | 300 | 4.05 | 74.1 | 2.47 | 529 | 21.4 | 1202 | 2.38 | 1.90 | 2.51 | 3.48 | 8 | 13.46 16.58 24.11 |
| x265_medium_crf24_1440p | 1440p | Y4M | 300 | 5.03 | 59.6 | 1.99 | 666 | 33.5 | 990 | 1.66 | 1.33 | 3.49 | 4.50 | 10 | 14.07 16.65 24.09 |
| svtav1_p8_crf32_1440p | 1440p | Y4M | 300 | 2.97 | 101.0 | 3.37 | 649 | 19.3 | 1285 | 1.48 | 1.18 | 0.97 | 2.11 | 6 | 12.83 16.30 23.88 |
| h264_vt_q65_1440p | 1440p | Y4M | 300 | 2.58 | 116.1 | 3.87 | 28 | 0.7 | 209 | 7.93 | 6.34 | 1.52 | 2.31 | 6 | 12.83 16.30 23.88 |
| hevc_vt_q65_1440p_nv12 | 1440p | NV12 | 300 | 5.03 | 59.7 | 1.99 | 24 | 1.2 | 212 | 9.50 | 7.60 | 3.21 | 4.38 | 10 | 12.52 16.18 23.79 |
| gif_ffmpeg_palette | gif | Y4M | 150 | 15.81 | 9.5 | 0.63 | 100 | 15.9 | 307 | 36.71 | 29.37 | 0.86 | 0.95 | 30 | 14.57 16.48 23.81 |
| gif_ffmpeg_palette_untagged | demo | Y4M | 150 | 13.70 | 10.9 | 0.73 | 104 | 14.3 | 405 | 36.68 | 29.34 | 0.50 | 0.56 | 27 | 13.02 16.06 23.53 |
| gif_gifski_q90 | gif | Y4M | 150 | 4.26 | 35.2 | 2.35 | 632 | 26.9 | 212 | 26.18 | 20.94 | 3.40 | 3.53 | 0 | 11.47 15.56 23.22 |
| gif_gifski_q70 | gif | Y4M | 150 | 3.26 | 46.1 | 3.07 | 702 | 22.9 | 203 | 17.68 | 14.15 | 2.66 | 2.81 | 0 | 10.76 15.28 23.03 |

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
| x264_q_fifo | ffmpeg | Fifo | q-on-stdin | exit 0 | 0.501 | false | 140 | false | 752135 | 139 | true | true |
| x264_sigterm | ffmpeg | Stdin | SIGTERM | exit 255 | 0.242 | false | 91 | false | 556138 | 90 | true | true |
| x264_sigkill | ffmpeg | Stdin | SIGKILL | signal 9 | 0.015 | false | 90 | false | 262192 | unreadable: [mov,mp4,m4a,3gp,3g2,mj2 @ 0x9f6c18000] moov atom not found | false | true |
| x264_eof | ffmpeg | Stdin | close-input (EOF) | exit 0 | 0.266 | false | 91 | false | 732728 | 91 | true | true |
| x264_hung_sigterm_then_kill | ffmpeg | Stdin | SIGTERM to hung helper -> SIGKILL | signal 9 | 1.527 | true | 90 | false | 262192 | unreadable: [mov,mp4,m4a,3gp,3g2,mj2 @ 0x8dd020000] moov atom not found | false | true |
| gifski_sigterm | gifski | Stdin | SIGTERM | signal 15 | 0.005 | false | 45 | false | 3200451 | 18 | true | true |
| gifski_eof | gifski | Stdin | close-input (EOF) | exit 0 | 0.625 | false | 46 | false | 8080368 | 46 | true | true |

## Parent crash (spike process SIGKILLed while feeding the helper)

`{"helper_exited_on_its_own": true, "seconds_until_helper_gone": 0.2577, "leftover_partial_exists": true, "leftover_partial_bytes": 1519768.0000, "leftover_partial_frames": "314", "leftover_partial_playable": true}`

## Robustness / capability probes

- `{"case": "missing helper binary", "handled": true, "result": "helper not found: /nonexistent/simple-capture/ffmpeg (is the app bundle complete?)"}`
- `{"case": "encoder not compiled in (libaom-av1)", "handled": true, "result": "exit 8; writer: Some(\"Broken pipe (os error 32)\"); stderr: Error opening output files: Encoder not found"}`
- `{"case": "h264_videotoolbox at 4096x2304 (probe encode, 6 frames)", "encoded": true, "result": ""}`
- `{"case": "h264_videotoolbox at 4096x4096 (probe encode, 6 frames)", "encoded": true, "result": ""}`
- `{"case": "h264_videotoolbox at 8192x4320 (probe encode, 6 frames)", "encoded": false, "result": "[h264_videotoolbox @ 0xbd2c2c000] Cannot create compression session: -12903 | [h264_videotoolbox @ 0xbd2c2c000] Try -allow_sw 1. The hardware encoder may be busy, or not supported. | [vost#0:0/h264_videotoolbox @ 0xbd3034000] [enc:h264_videotoolbox @ 0xbd3008930] Error while opening encoder - maybe incorrect parameters such as bit_rate, rate, width or height. | [vf#0:0 @ 0xbd3038000] Error sending frames to consumers: Generic error in an external library | [vf#0:0 @ 0xbd3038000] Task finished with error code: -542398533 (Generic error in an external library) | [vf#0:0 @ 0xbd3038000] Terminating thread with return code -542398533 (Generic error in an external library) | [vost#0:0/h264_videotoolbox @ 0xbd3034000] [enc:h264_videotoolbox @ 0xbd3008930] Could not open encoder before EOF | [vost#0:0/h264_videotoolbox @ 0xbd3034000] Task finished with error code: -22 (Invalid argument) | [vost#0:0/h264_videotoolbox @ 0xbd3034000] Terminating thread with return code -22 (Invalid argument) | [out#0/null @ 0xbd300c780] Nothing was written into output file, because at least one of its streams received no packets."}`
- `{"case": "hevc_videotoolbox at 8192x4320 (probe encode, 6 frames)", "encoded": true, "result": ""}`
- `{"case": "hevc_videotoolbox at 7680x4320 (probe encode, 6 frames)", "encoded": true, "result": ""}`
- `{"case": "h264_videotoolbox at 1918x1080 (probe encode, 6 frames)", "encoded": true, "result": ""}`
