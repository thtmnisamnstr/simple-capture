# Spike 1 helper benchmark results

- Run: 2026-11-19T21:43:13Z -> 2026-11-19T21:45:45Z (UTC); clip 10 s per encode; queue depth 3 frames
- Machine: `{"cpu": "Apple M3 Pro", "ncpu": "11", "perf_cores": "5", "eff_cores": "6", "mem_bytes": "19327352832", "macos": "26.7.2", "loadavg_start": "6.65 5.83 7.46"}`
- Helper: `ffmpeg version 9.0.2-simple-capture Copyright (c) 2000-2026 the FFmpeg developers`, `gifski 1.34.0`; validator: `ffprobe version 9.0.2 Copyright (c) 2007-2026 the FFmpeg developers`
- Load average at end: 6.92 6.53 7.49 (other heavy jobs may be running: numbers are preliminary)

## Synthetic frame generator (single thread)

- {"res": "1920x1080", "fmt": "I420", "frames_per_s": 839.9624}
- {"res": "1920x1080", "fmt": "Nv12", "frames_per_s": 779.8695}
- {"res": "2560x1440", "fmt": "I420", "frames_per_s": 452.2221}
- {"res": "2560x1440", "fmt": "Nv12", "frames_per_s": 400.8193}

## Pipe transport ceiling (generator -> bounded queue -> stdin -> ffmpeg demux -> null)

- {"res": "1920x1080", "input": "Y4M", "frames": 600.0000, "frames_per_s": 709.8266, "mb_per_s": 2207.8445, "gen_wait_s": 0.0118, "helper_cpu_pct": 29.8329, "exit": "exit 0"}
- {"res": "1920x1080", "input": "NV12", "frames": 600.0000, "frames_per_s": 667.2814, "mb_per_s": 2075.5121, "gen_wait_s": 0.0114, "helper_cpu_pct": 33.3286, "exit": "exit 0"}
- {"res": "2560x1440", "input": "Y4M", "frames": 600.0000, "frames_per_s": 392.0874, "mb_per_s": 2168.0862, "gen_wait_s": 0.0066, "helper_cpu_pct": 35.2295, "exit": "exit 0"}
- {"res": "2560x1440", "input": "NV12", "frames": 600.0000, "frames_per_s": 378.1944, "mb_per_s": 2091.2640, "gen_wait_s": 0.0045, "helper_cpu_pct": 33.0703, "exit": "exit 0"}

## Encode speed

| job | group | input | frames | wall s | enc fps | x realtime | helper CPU % | helper CPU s | peak RSS MB | size MB | Mbps | gen wait s | pipe write s | progress updates | load before |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| x264_medium_crf20_aacat | best | Y4M | 300 | 1.52 | 196.8 | 6.56 | 668 | 10.2 | 708 | 1.69 | 1.35 | 0.77 | 1.22 | 3 | 6.27 5.77 7.43 |
| x264_slow_crf20_aacat | best | Y4M | 300 | 2.11 | 142.4 | 4.75 | 610 | 12.8 | 780 | 1.60 | 1.28 | 1.08 | 1.57 | 5 | 6.65 5.86 7.45 |
| x264_medium_crf20_aac | best | Y4M | 300 | 1.56 | 192.9 | 6.43 | 665 | 10.3 | 703 | 1.51 | 1.21 | 0.80 | 1.26 | 4 | 6.65 5.86 7.45 |
| x265_medium_crf24_aacat | best | Y4M | 300 | 2.50 | 120.1 | 4.00 | 770 | 19.2 | 713 | 1.22 | 0.98 | 1.66 | 2.20 | 5 | 6.52 5.84 7.43 |
| svtav1_p8_crf32_aacat | best | Y4M | 300 | 1.57 | 191.0 | 6.37 | 728 | 11.4 | 979 | 1.27 | 1.02 | 0.62 | 1.22 | 4 | 6.52 5.84 7.43 |
| svtav1_p6_crf32_aacat | best | Y4M | 300 | 3.09 | 97.2 | 3.24 | 753 | 23.2 | 1166 | 1.04 | 0.84 | 1.73 | 2.20 | 7 | 8.80 6.33 7.60 |
| vp9_good_cpu4_crf32_opus | best | Y4M | 300 | 4.64 | 64.6 | 2.15 | 695 | 32.3 | 378 | 1.30 | 1.04 | 3.54 | 4.07 | 10 | 8.50 6.30 7.58 |
| vp9_good_cpu2_crf32_opus | best | Y4M | 300 | 5.26 | 57.0 | 1.90 | 712 | 37.5 | 378 | 1.24 | 0.99 | 4.05 | 4.61 | 11 | 8.06 6.25 7.55 |
| vp8_good_cpu4_vorbis | best | Y4M | 300 | 2.60 | 115.6 | 3.85 | 498 | 12.9 | 150 | 5.43 | 4.35 | 1.94 | 2.47 | 6 | 8.45 6.36 7.58 |
| h264_vt_q65_aacat | fast | Y4M | 300 | 1.57 | 190.6 | 6.35 | 32 | 0.5 | 134 | 4.74 | 3.79 | 0.98 | 1.41 | 4 | 8.45 6.36 7.58 |
| hevc_vt_q65_aacat | fast | Y4M | 300 | 1.69 | 177.4 | 5.91 | 32 | 0.5 | 132 | 5.20 | 4.16 | 1.08 | 1.52 | 4 | 7.85 6.27 7.55 |
| x264_medium_crf20_t4 | proxy4 | Y4M | 300 | 2.35 | 127.7 | 4.26 | 348 | 8.2 | 467 | 1.43 | 1.14 | 1.43 | 1.90 | 5 | 7.71 6.27 7.54 |
| x264_slow_crf20_t4 | proxy4 | Y4M | 300 | 3.01 | 99.7 | 3.32 | 325 | 9.8 | 536 | 1.33 | 1.07 | 1.77 | 2.24 | 6 | 7.71 6.27 7.54 |
| x265_medium_crf24_t4 | proxy4 | Y4M | 300 | 3.61 | 83.0 | 2.77 | 376 | 13.6 | 617 | 1.01 | 0.81 | 2.67 | 3.15 | 8 | 7.65 6.28 7.53 |
| svtav1_p8_crf32_lp1 | proxy4 | Y4M | 300 | 7.60 | 39.5 | 1.32 | 101 | 7.7 | 507 | 1.06 | 0.85 | 5.97 | 6.63 | 16 | 7.92 6.36 7.55 |
| svtav1_p8_crf32_lp2 | proxy4 | Y4M | 300 | 3.43 | 87.3 | 2.91 | 289 | 9.9 | 584 | 1.06 | 0.85 | 2.38 | 2.86 | 7 | 7.68 6.33 7.54 |
| vp9_good_cpu4_crf32_t4 | proxy4 | Y4M | 300 | 6.96 | 43.1 | 1.44 | 348 | 24.3 | 344 | 1.27 | 1.01 | 5.60 | 6.14 | 14 | 7.39 6.29 7.52 |
| x264_medium_crf20 | input | Y4M | 300 | 1.61 | 185.9 | 6.20 | 674 | 10.9 | 700 | 1.48 | 1.18 | 0.79 | 1.31 | 4 | 6.87 6.22 7.48 |
| x264_medium_crf20_nv12 | input | NV12 | 300 | 1.81 | 165.4 | 5.51 | 632 | 11.5 | 704 | 1.48 | 1.18 | 0.83 | 1.38 | 4 | 6.87 6.22 7.48 |
| h264_vt_q65 | input | Y4M | 300 | 1.59 | 188.7 | 6.29 | 27 | 0.4 | 126 | 4.53 | 3.62 | 1.00 | 1.43 | 4 | 6.80 6.21 7.47 |
| h264_vt_q65_nv12 | input | NV12 | 300 | 1.57 | 190.8 | 6.36 | 40 | 0.6 | 131 | 4.53 | 3.62 | 0.96 | 1.41 | 4 | 6.80 6.21 7.47 |
| x264_medium_crf20_1440p | 1440p | Y4M | 300 | 2.81 | 106.8 | 3.56 | 662 | 18.6 | 1215 | 2.38 | 1.90 | 1.34 | 2.25 | 6 | 6.65 6.19 7.45 |
| x265_medium_crf24_1440p | 1440p | Y4M | 300 | 4.06 | 73.9 | 2.46 | 813 | 33.0 | 1097 | 1.66 | 1.33 | 2.59 | 3.60 | 8 | 6.65 6.19 7.45 |
| svtav1_p8_crf32_1440p | 1440p | Y4M | 300 | 2.55 | 117.9 | 3.93 | 731 | 18.6 | 1367 | 1.48 | 1.18 | 1.08 | 1.90 | 5 | 6.92 6.26 7.47 |
| h264_vt_q65_1440p | 1440p | Y4M | 300 | 2.60 | 115.5 | 3.85 | 27 | 0.7 | 204 | 7.93 | 6.34 | 1.58 | 2.31 | 6 | 9.09 6.72 7.62 |
| hevc_vt_q65_1440p_nv12 | 1440p | NV12 | 300 | 2.84 | 105.8 | 3.53 | 39 | 1.1 | 217 | 9.00 | 7.20 | 1.72 | 2.54 | 6 | 9.48 6.84 7.66 |
| gif_ffmpeg_palette | gif | Y4M | 150 | 11.76 | 12.8 | 0.85 | 106 | 12.5 | 403 | 36.71 | 29.37 | 0.51 | 0.58 | 23 | 9.04 6.79 7.64 |
| gif_ffmpeg_palette_untagged | demo | Y4M | 150 | 11.16 | 13.4 | 0.90 | 108 | 12.0 | 456 | 36.68 | 29.34 | 0.46 | 0.52 | 22 | 8.01 6.67 7.58 |
| gif_gifski_q90 | gif | Y4M | 150 | 3.85 | 39.0 | 2.60 | 675 | 26.0 | 212 | 26.18 | 20.94 | 3.06 | 3.27 | 0 | 7.46 6.60 7.54 |
| gif_gifski_q70 | gif | Y4M | 150 | 2.62 | 57.2 | 3.82 | 828 | 21.7 | 210 | 17.68 | 14.15 | 2.06 | 2.23 | 0 | 7.11 6.54 7.52 |

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
| x264_q_fifo | ffmpeg | Fifo | q-on-stdin | exit 0 | 0.243 | false | 107 | false | 631606 | 106 | true | true |
| x264_sigterm | ffmpeg | Stdin | SIGTERM | exit 255 | 0.235 | false | 91 | false | 557909 | 90 | true | true |
| x264_sigkill | ffmpeg | Stdin | SIGKILL | signal 9 | 0.010 | false | 90 | false | 262192 | unreadable: [mov,mp4,m4a,3gp,3g2,mj2 @ 0xa6ac18000] moov atom not found | false | true |
| x264_eof | ffmpeg | Stdin | close-input (EOF) | exit 0 | 0.223 | false | 91 | false | 732728 | 91 | true | true |
| x264_hung_sigterm_then_kill | ffmpeg | Stdin | SIGTERM to hung helper -> SIGKILL | signal 9 | 1.531 | true | 90 | false | 262192 | unreadable: [mov,mp4,m4a,3gp,3g2,mj2 @ 0xb97020000] moov atom not found | false | true |
| gifski_sigterm | gifski | Stdin | SIGTERM | signal 15 | 0.005 | false | 45 | false | 3535934 | 20 | true | true |
| gifski_eof | gifski | Stdin | close-input (EOF) | exit 0 | 0.619 | false | 46 | false | 8080368 | 46 | true | true |

## Parent crash (spike process SIGKILLed while feeding the helper)

`{"helper_exited_on_its_own": true, "seconds_until_helper_gone": 0.2259, "leftover_partial_exists": true, "leftover_partial_bytes": 1611423.0000, "leftover_partial_frames": "344", "leftover_partial_playable": true}`

## Robustness / capability probes

- `{"case": "missing helper binary", "handled": true, "result": "helper not found: /nonexistent/simple-capture/ffmpeg (is the app bundle complete?)"}`
- `{"case": "encoder not compiled in (libaom-av1)", "handled": true, "result": "exit 8; writer: Some(\"Broken pipe (os error 32)\"); stderr: Error opening output files: Encoder not found"}`
- `{"case": "h264_videotoolbox at 4096x2304 (probe encode, 6 frames)", "encoded": true, "result": ""}`
- `{"case": "h264_videotoolbox at 4096x4096 (probe encode, 6 frames)", "encoded": true, "result": ""}`
- `{"case": "h264_videotoolbox at 8192x4320 (probe encode, 6 frames)", "encoded": false, "result": "[h264_videotoolbox @ 0xb31034000] Cannot create compression session: -12903 | [h264_videotoolbox @ 0xb31034000] Try -allow_sw 1. The hardware encoder may be busy, or not supported. | [vost#0:0/h264_videotoolbox @ 0xb31048000] [enc:h264_videotoolbox @ 0xb31038000] Error while opening encoder - maybe incorrect parameters such as bit_rate, rate, width or height. | [vf#0:0 @ 0xb30c34000] Error sending frames to consumers: Generic error in an external library | [vf#0:0 @ 0xb30c34000] Task finished with error code: -542398533 (Generic error in an external library) | [vf#0:0 @ 0xb30c34000] Terminating thread with return code -542398533 (Generic error in an external library) | [vost#0:0/h264_videotoolbox @ 0xb31048000] [enc:h264_videotoolbox @ 0xb31038000] Could not open encoder before EOF | [vost#0:0/h264_videotoolbox @ 0xb31048000] Task finished with error code: -22 (Invalid argument) | [vost#0:0/h264_videotoolbox @ 0xb31048000] Terminating thread with return code -22 (Invalid argument) | [out#0/null @ 0xb30c18480] Nothing was written into output file, because at least one of its streams received no packets."}`
- `{"case": "hevc_videotoolbox at 8192x4320 (probe encode, 6 frames)", "encoded": true, "result": ""}`
- `{"case": "hevc_videotoolbox at 7680x4320 (probe encode, 6 frames)", "encoded": true, "result": ""}`
- `{"case": "h264_videotoolbox at 1918x1080 (probe encode, 6 frames)", "encoded": true, "result": ""}`
