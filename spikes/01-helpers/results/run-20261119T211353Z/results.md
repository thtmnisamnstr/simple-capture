# Spike 1 helper benchmark results

- Run: 2026-11-19T21:13:54Z -> 2026-11-19T21:16:16Z (UTC); clip 10 s per encode; queue depth 3 frames
- Machine: `{"cpu": "Apple M3 Pro", "ncpu": "11", "perf_cores": "5", "eff_cores": "6", "mem_bytes": "19327352832", "macos": "26.7.2", "loadavg_start": "2.62 9.15 18.53"}`
- Helper: `ffmpeg version 9.0.2-simple-capture Copyright (c) 2000-2026 the FFmpeg developers`, `gifski 1.34.0`; validator: `ffprobe version 9.0.2 Copyright (c) 2007-2026 the FFmpeg developers`
- Load average at end: 6.51 8.50 16.81 (other heavy jobs may be running: numbers are preliminary)

## Synthetic frame generator (single thread)

- {"res": "1920x1080", "fmt": "I420", "frames_per_s": 853.2635}
- {"res": "1920x1080", "fmt": "Nv12", "frames_per_s": 786.0695}
- {"res": "2560x1440", "fmt": "I420", "frames_per_s": 424.0038}
- {"res": "2560x1440", "fmt": "Nv12", "frames_per_s": 440.3929}

## Pipe transport ceiling (generator -> bounded queue -> stdin -> ffmpeg demux -> null)

- {"res": "1920x1080", "input": "Y4M", "frames": 600.0000, "frames_per_s": 654.9716, "mb_per_s": 2037.2238, "gen_wait_s": 0.0604, "helper_cpu_pct": 31.1025, "exit": "exit 0"}
- {"res": "1920x1080", "input": "NV12", "frames": 600.0000, "frames_per_s": 692.7863, "mb_per_s": 2154.8426, "gen_wait_s": 0.0118, "helper_cpu_pct": 33.0484, "exit": "exit 0"}
- {"res": "2560x1440", "input": "Y4M", "frames": 600.0000, "frames_per_s": 403.4442, "mb_per_s": 2230.8849, "gen_wait_s": 0.0096, "helper_cpu_pct": 34.6951, "exit": "exit 0"}
- {"res": "2560x1440", "input": "NV12", "frames": 600.0000, "frames_per_s": 388.3505, "mb_per_s": 2147.4229, "gen_wait_s": 0.0078, "helper_cpu_pct": 33.2645, "exit": "exit 0"}

## Encode speed

| job | group | input | frames | wall s | enc fps | x realtime | helper CPU % | helper CPU s | peak RSS MB | size MB | Mbps | gen wait s | pipe write s | progress updates | load before |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| x264_medium_crf20_aacat | best | Y4M | 300 | 2.44 | 122.8 | 4.09 | 492 | 12.0 | 704 | 1.69 | 1.35 | 1.43 | 1.95 | 5 | 2.60 8.93 18.34 |
| x264_slow_crf20_aacat | best | Y4M | 300 | 2.03 | 148.1 | 4.94 | 631 | 12.8 | 781 | 1.60 | 1.28 | 1.06 | 1.55 | 4 | 2.60 8.93 18.34 |
| x264_medium_crf20_aac | best | Y4M | 300 | 1.55 | 193.0 | 6.43 | 677 | 10.5 | 703 | 1.51 | 1.21 | 0.80 | 1.27 | 4 | 3.11 8.93 18.29 |
| x265_medium_crf24_aacat | best | Y4M | 300 | 2.51 | 119.7 | 3.99 | 774 | 19.4 | 714 | 1.22 | 0.98 | 1.64 | 2.17 | 5 | 3.11 8.93 18.29 |
| svtav1_p8_crf32_aacat | best | Y4M | 300 | 1.57 | 191.4 | 6.38 | 726 | 11.4 | 884 | 1.27 | 1.02 | 0.68 | 1.21 | 4 | 3.98 9.01 18.26 |
| svtav1_p6_crf32_aacat | best | Y4M | 300 | 2.98 | 100.5 | 3.35 | 781 | 23.3 | 1388 | 1.04 | 0.84 | 1.77 | 2.21 | 6 | 3.98 9.01 18.26 |
| vp9_good_cpu4_crf32_opus | best | Y4M | 300 | 4.58 | 65.5 | 2.18 | 703 | 32.2 | 378 | 1.30 | 1.04 | 3.41 | 3.95 | 10 | 4.39 9.01 18.21 |
| vp9_good_cpu2_crf32_opus | best | Y4M | 300 | 5.27 | 57.0 | 1.90 | 708 | 37.3 | 378 | 1.24 | 0.99 | 4.06 | 4.63 | 11 | 5.07 9.08 18.18 |
| vp8_good_cpu4_vorbis | best | Y4M | 300 | 2.66 | 112.6 | 3.75 | 524 | 14.0 | 150 | 5.43 | 4.35 | 1.98 | 2.55 | 6 | 5.31 9.06 18.12 |
| h264_vt_q65_aacat | fast | Y4M | 300 | 1.62 | 185.3 | 6.18 | 34 | 0.6 | 132 | 4.74 | 3.79 | 1.03 | 1.46 | 4 | 6.24 9.19 18.11 |
| hevc_vt_q65_aacat | fast | Y4M | 300 | 1.69 | 177.9 | 5.93 | 31 | 0.5 | 132 | 5.20 | 4.16 | 1.09 | 1.52 | 4 | 6.24 9.19 18.11 |
| x264_medium_crf20_t4 | proxy4 | Y4M | 300 | 2.06 | 146.0 | 4.87 | 374 | 7.7 | 473 | 1.43 | 1.14 | 1.17 | 1.61 | 5 | 6.14 9.12 18.03 |
| x264_slow_crf20_t4 | proxy4 | Y4M | 300 | 3.07 | 97.8 | 3.26 | 323 | 9.9 | 534 | 1.33 | 1.07 | 1.85 | 2.33 | 7 | 6.14 9.12 18.03 |
| x265_medium_crf24_t4 | proxy4 | Y4M | 300 | 3.13 | 95.8 | 3.19 | 389 | 12.2 | 621 | 1.01 | 0.81 | 2.32 | 2.76 | 7 | 6.21 9.08 17.97 |
| svtav1_p8_crf32_lp1 | proxy4 | Y4M | 300 | 6.79 | 44.2 | 1.47 | 102 | 6.9 | 510 | 1.06 | 0.85 | 5.34 | 5.91 | 14 | 6.28 9.05 17.90 |
| svtav1_p8_crf32_lp2 | proxy4 | Y4M | 300 | 2.99 | 100.3 | 3.34 | 298 | 8.9 | 662 | 1.06 | 0.85 | 2.14 | 2.57 | 6 | 6.09 8.97 17.82 |
| vp9_good_cpu4_crf32_t4 | proxy4 | Y4M | 300 | 6.03 | 49.8 | 1.66 | 358 | 21.6 | 344 | 1.27 | 1.01 | 4.81 | 5.26 | 12 | 5.69 8.83 17.72 |
| x264_medium_crf20 | input | Y4M | 300 | 1.50 | 199.9 | 6.66 | 669 | 10.0 | 700 | 1.48 | 1.18 | 0.77 | 1.21 | 3 | 5.63 8.77 17.65 |
| x264_medium_crf20_nv12 | input | NV12 | 300 | 1.55 | 193.9 | 6.46 | 650 | 10.1 | 705 | 1.48 | 1.18 | 0.78 | 1.25 | 4 | 6.30 8.86 17.63 |
| h264_vt_q65 | input | Y4M | 300 | 1.58 | 189.6 | 6.32 | 30 | 0.5 | 126 | 4.53 | 3.62 | 0.98 | 1.42 | 4 | 6.30 8.86 17.63 |
| h264_vt_q65_nv12 | input | NV12 | 300 | 1.57 | 191.3 | 6.38 | 57 | 0.9 | 128 | 4.53 | 3.62 | 0.96 | 1.40 | 4 | 5.96 8.74 17.54 |
| x264_medium_crf20_1440p | 1440p | Y4M | 300 | 2.77 | 108.5 | 3.62 | 647 | 17.9 | 1198 | 2.38 | 1.90 | 1.39 | 2.17 | 6 | 5.96 8.74 17.54 |
| x265_medium_crf24_1440p | 1440p | Y4M | 300 | 3.88 | 77.4 | 2.58 | 825 | 32.0 | 1097 | 1.66 | 1.33 | 2.44 | 3.43 | 8 | 7.56 9.03 17.59 |
| svtav1_p8_crf32_1440p | 1440p | Y4M | 300 | 2.44 | 123.0 | 4.10 | 752 | 18.4 | 1454 | 1.48 | 1.18 | 1.00 | 1.90 | 5 | 8.32 9.16 17.58 |
| h264_vt_q65_1440p | 1440p | Y4M | 300 | 2.60 | 115.2 | 3.84 | 29 | 0.7 | 198 | 7.93 | 6.34 | 1.56 | 2.32 | 6 | 8.05 9.09 17.51 |
| hevc_vt_q65_1440p_nv12 | 1440p | NV12 | 300 | 2.81 | 106.6 | 3.55 | 48 | 1.3 | 216 | 9.00 | 7.20 | 1.72 | 2.51 | 6 | 8.05 9.09 17.51 |
| gif_ffmpeg_palette | gif | Y4M | 150 | 9.24 | 16.2 | 1.08 | 109 | 10.1 | 455 | 36.71 | 29.37 | 0.43 | 0.49 | 18 | 8.13 9.09 17.46 |
| gif_ffmpeg_palette_untagged | demo | Y4M | 150 | 8.95 | 16.8 | 1.12 | 109 | 9.8 | 455 | 36.68 | 29.34 | 0.39 | 0.45 | 17 | 7.21 8.84 17.22 |
| gif_gifski_q90 | gif | Y4M | 150 | 3.65 | 41.1 | 2.74 | 708 | 25.8 | 212 | 26.18 | 20.94 | 2.92 | 3.10 | 0 | 6.71 8.68 17.07 |
| gif_gifski_q70 | gif | Y4M | 150 | 2.68 | 56.0 | 3.74 | 803 | 21.5 | 196 | 17.68 | 14.15 | 2.09 | 2.26 | 0 | 6.57 8.62 17.00 |

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
| x264_q_fifo | ffmpeg | Fifo | q-on-stdin | exit 0 | 0.220 | false | 103 | false | 610632 | 102 | true | true |
| x264_sigterm | ffmpeg | Stdin | SIGTERM | exit 255 | 0.228 | false | 91 | false | 556138 | 90 | true | true |
| x264_sigkill | ffmpeg | Stdin | SIGKILL | signal 9 | 0.008 | false | 90 | false | 262192 | unreadable: [mov,mp4,m4a,3gp,3g2,mj2 @ 0xafcc38000] moov atom not found | false | true |
| x264_eof | ffmpeg | Stdin | close-input (EOF) | exit 0 | 0.228 | false | 92 | false | 737794 | 92 | true | true |
| x264_hung_sigterm_then_kill | ffmpeg | Stdin | SIGTERM to hung helper -> SIGKILL | signal 9 | 1.533 | true | 90 | false | 262192 | unreadable: [mov,mp4,m4a,3gp,3g2,mj2 @ 0xad6c38000] moov atom not found | false | true |
| gifski_sigterm | gifski | Stdin | SIGTERM | signal 15 | 0.006 | false | 45 | false | 3535934 | 20 | true | true |
| gifski_eof | gifski | Stdin | close-input (EOF) | exit 0 | 0.595 | false | 46 | false | 8080368 | 46 | true | true |

## Parent crash (spike process SIGKILLed while feeding the helper)

`{"helper_exited_on_its_own": true, "seconds_until_helper_gone": 0.2319, "leftover_partial_exists": true, "leftover_partial_bytes": 1577641.0000, "leftover_partial_frames": "333", "leftover_partial_playable": true}`

## Robustness / capability probes

- `{"case": "missing helper binary", "handled": true, "result": "helper not found: /nonexistent/simple-capture/ffmpeg (is the app bundle complete?)"}`
- `{"case": "encoder not compiled in (libaom-av1)", "handled": true, "result": "exit 8; writer: Some(\"Broken pipe (os error 32)\"); stderr: Error opening output files: Encoder not found"}`
- `{"case": "h264_videotoolbox at 4096x2304 (probe encode, 6 frames)", "encoded": true, "result": ""}`
- `{"case": "h264_videotoolbox at 4096x4096 (probe encode, 6 frames)", "encoded": true, "result": ""}`
- `{"case": "h264_videotoolbox at 8192x4320 (probe encode, 6 frames)", "encoded": false, "result": "[h264_videotoolbox @ 0x8d4c30000] Cannot create compression session: -12903 | [h264_videotoolbox @ 0x8d4c30000] Try -allow_sw 1. The hardware encoder may be busy, or not supported. | [vost#0:0/h264_videotoolbox @ 0x8d5030000] [enc:h264_videotoolbox @ 0x8d5008930] Error while opening encoder - maybe incorrect parameters such as bit_rate, rate, width or height. | [vf#0:0 @ 0x8d5028000] Error sending frames to consumers: Generic error in an external library | [vf#0:0 @ 0x8d5028000] Task finished with error code: -542398533 (Generic error in an external library) | [vf#0:0 @ 0x8d5028000] Terminating thread with return code -542398533 (Generic error in an external library) | [vost#0:0/h264_videotoolbox @ 0x8d5030000] [enc:h264_videotoolbox @ 0x8d5008930] Could not open encoder before EOF | [vost#0:0/h264_videotoolbox @ 0x8d5030000] Task finished with error code: -22 (Invalid argument) | [vost#0:0/h264_videotoolbox @ 0x8d5030000] Terminating thread with return code -22 (Invalid argument) | [out#0/null @ 0x8d500c780] Nothing was written into output file, because at least one of its streams received no packets."}`
- `{"case": "hevc_videotoolbox at 8192x4320 (probe encode, 6 frames)", "encoded": true, "result": ""}`
- `{"case": "hevc_videotoolbox at 7680x4320 (probe encode, 6 frames)", "encoded": true, "result": ""}`
- `{"case": "h264_videotoolbox at 1918x1080 (probe encode, 6 frames)", "encoded": true, "result": ""}`
