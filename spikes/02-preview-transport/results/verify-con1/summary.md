Results from /Users/gavin/Documents/Development/simple-capture/spikes/02-preview-transport/results/verify-con1 (ProductName: macOS ProductVersion: 26.7.2 BuildVersion: 25G332, tauri 2.12.7, dpr 2)

| mode | frame px | produced fps | presented fps | drops (id gaps) | latency p50 / p95 / p99 ms | rAF fps | CPU% app | CPU% WebContent | CPU% WebKit GPU | CPU% total | mem MB (app / WC / GPU) | verify ok/fail | snapshot | errors |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| idle | – | 0.0 | – | – | – | 72.0 | 1.4 | 1.5 | 0.2 | 3.1 | 44 / 34 / 13 | 0/0 | video NOT visible, HTML over it |  |
| a-gl-540 | 960×540 | 60.0 | 59.1 | 8 | 9.2 / 15.5 / 17.1 | 71.7 | 10.8 | 9.7 | 4.2 | 24.7 | 225 / 124 / 122 | 19/0 | video visible, HTML over it |  |
| a-2d-540 | 960×540 | 59.6 | 58.7 | 9 | 9.0 / 15.8 / 21.1 | 72.0 | 11.2 | 10.3 | 7.1 | 28.6 | 223 / 109 / 125 | 19/0 | video visible, HTML over it |  |
| a-gl-720 | 1280×720 | 59.9 | 59.0 | 8 | 9.8 / 16.0 / 19.9 | 71.8 | 13.4 | 12.9 | 6.7 | 33.0 | 225 / 286 / 128 | 19/0 | video visible, HTML over it |  |
| a-2d-720 | 1280×720 | 60.0 | 59.6 | 4 | 10.9 / 17.4 / 18.0 | 71.8 | 11.3 | 12.0 | 10.3 | 33.5 | 221 / 189 / 116 | 19/0 | video visible, HTML over it |  |
| a-gl-1080 | 1920×1080 | 60.0 | 59.8 | 2 | 10.5 / 17.2 / 18.0 | 72.0 | 15.4 | 17.3 | 13.0 | 45.8 | 243 / 346 / 120 | 19/0 | video visible, HTML over it |  |
| a-2d-1080 | 1920×1080 | 60.0 | 59.0 | 9 | 11.3 / 17.8 / 22.7 | 72.0 | 15.6 | 18.4 | 18.6 | 52.6 | 243 / 229 / 122 | 19/0 | video visible, HTML over it |  |
| b-gl-540 | 960×540 | 60.1 | 59.2 | 9 | 8.6 / 15.2 / 16.4 | 71.9 | 9.4 | 6.7 | 3.8 | 19.9 | 219 / 88 / 118 | 19/0 | video visible, HTML over it |  |
| b-gl-720 | 1280×720 | 60.0 | 59.0 | 8 | 9.4 / 16.0 / 18.3 | 71.7 | 11.2 | 9.6 | 6.3 | 27.1 | 228 / 192 / 124 | 19/0 | video visible, HTML over it |  |
| b-gl-1080 | 1920×1080 | 59.7 | 58.3 | 12 | 11.3 / 17.4 / 21.6 | 71.5 | 14.7 | 15.0 | 13.0 | 42.7 | 242 / 247 / 124 | 19/0 | video visible, HTML over it |  |
| b-gl-1080-raf | 1920×1080 | 60.0 | 59.6 | 3 | 11.3 / 18.9 / 32.6 | 71.8 | 15.7 | 15.6 | 13.9 | 45.2 | 234 / 239 / 124 | 19/0 | video visible, HTML over it |  |
| c-native-540 | 960×540 | 60.1 | 60.1 | 0 | 0.4 / 0.6 / 0.7 | 72.0 | 6.1 | 1.5 | 0.4 | 8.0 | 218 / 54 / 111 | 0/0 | video visible, HTML over it |  |
| c-native-1080 | 1920×1080 | 60.0 | 60.0 | 0 | 0.4 / 0.6 / 1.4 | 71.8 | 6.5 | 1.7 | 0.3 | 8.4 | 236 / 39 / 107 | 0/0 | video visible, HTML over it |  |
| c-above-1080 | 1920×1080 | 60.1 | 60.1 | 0 | 0.4 / 0.7 / 1.2 | 72.0 | 7.3 | 1.9 | 0.4 | 9.5 | 237 / 39 / 113 | 0/0 | video visible, HTML not over it |  |

Producer timings (ms, p50/p95): encode | GPU wait (render+readback) | copy into Vec | send | recv latency (producer→JS onmessage) | fetch
- idle: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 34.4%
- a-gl-540: 0.35/0.52 | 0.86/4.67 | 0.07/0.12 | 0.03/0.08 | 2.56/6.73 | –; backpressure drops 1 (timeout overrides 0), unfetched 0, late ticks 5; GPU util 41.8%
- a-2d-540: 0.37/0.60 | 0.90/4.87 | 0.07/0.12 | 0.03/0.08 | 1.64/6.56 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 3; GPU util 44.1%
- a-gl-720: 0.37/0.80 | 0.89/5.29 | 0.17/0.33 | 0.04/0.15 | 3.14/8.06 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 2; GPU util 46.1%
- a-2d-720: 0.36/0.55 | 0.77/3.99 | 0.16/0.25 | 0.04/0.10 | 3.07/6.54 | –; backpressure drops 1 (timeout overrides 0), unfetched 0, late ticks 1; GPU util 45.7%
- a-gl-1080: 0.36/0.57 | 1.56/5.51 | 0.39/0.56 | 0.04/0.10 | 5.08/8.82 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 61.9%
- a-2d-1080: 0.37/0.64 | 2.53/6.95 | 0.39/0.57 | 0.04/0.14 | 6.07/10.77 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 3; GPU util 70.3%
- b-gl-540: 0.35/0.47 | 0.79/4.29 | 0.07/0.11 | 0.01/0.01 | 2.18/5.78 | 17.00/21.00; backpressure drops 0 (timeout overrides 0), unfetched 2, late ticks 3; GPU util 39.0%
- b-gl-720: 0.36/0.56 | 0.84/3.86 | 0.16/0.24 | 0.01/0.01 | 2.54/6.30 | 17.00/21.00; backpressure drops 0 (timeout overrides 0), unfetched 1, late ticks 4; GPU util 38.0%
- b-gl-1080: 0.36/0.65 | 0.94/4.62 | 0.39/0.56 | 0.01/0.01 | 3.88/8.56 | 16.00/21.00; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 4; GPU util 40.7%
- b-gl-1080-raf: 0.35/0.63 | 1.01/5.00 | 0.39/0.59 | 0.01/0.01 | 3.99/10.54 | 10.00/19.00; backpressure drops 0 (timeout overrides 0), unfetched 3, late ticks 6; GPU util 47.1%
- c-native-540: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 50.9%
- c-native-1080: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 3; GPU util 63.0%
- c-above-1080: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 1; GPU util 41.4%
