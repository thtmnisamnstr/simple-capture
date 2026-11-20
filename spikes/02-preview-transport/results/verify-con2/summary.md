Results from /Users/gavin/Documents/Development/simple-capture/spikes/02-preview-transport/results/verify-con2 (ProductName: macOS ProductVersion: 26.7.2 BuildVersion: 25G332, tauri 2.12.7, dpr 2)

| mode | frame px | produced fps | presented fps | drops (id gaps) | latency p50 / p95 / p99 ms | rAF fps | CPU% app | CPU% WebContent | CPU% WebKit GPU | CPU% total | mem MB (app / WC / GPU) | verify ok/fail | snapshot | errors |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| idle | – | 0.0 | – | – | – | 71.8 | 2.2 | 1.9 | 0.4 | 4.4 | 42 / 39 / 123 | 0/0 | video NOT visible, HTML over it |  |
| a-gl-540 | 960×540 | 59.9 | 59.4 | 5 | 9.5 / 16.1 / 17.5 | 71.9 | 10.7 | 9.8 | 4.6 | 25.1 | 219 / 129 / 124 | 19/0 | video visible, HTML over it |  |
| a-2d-540 | 960×540 | 59.8 | 59.4 | 4 | 9.2 / 15.9 / 21.7 | 71.5 | 10.0 | 9.7 | 7.3 | 27.0 | 221 / 130 / 117 | 19/0 | video visible, HTML over it |  |
| a-gl-720 | 1280×720 | 60.1 | 59.1 | 9 | 10.1 / 16.8 / 17.7 | 72.0 | 12.4 | 12.4 | 6.7 | 31.4 | 224 / 194 / 127 | 19/0 | video visible, HTML over it |  |
| a-2d-720 | 1280×720 | 59.9 | 59.2 | 6 | 10.8 / 16.6 / 17.6 | 71.8 | 12.5 | 12.7 | 10.9 | 36.1 | 225 / 193 / 123 | 19/0 | video visible, HTML over it |  |
| a-gl-1080 | 1920×1080 | 60.1 | 59.0 | 10 | 10.4 / 16.9 / 17.9 | 71.9 | 16.3 | 18.2 | 13.7 | 48.2 | 242 / 243 / 127 | 19/0 | video visible, HTML over it |  |
| a-2d-1080 | 1920×1080 | 60.1 | 58.9 | 11 | 11.4 / 17.7 / 22.9 | 71.9 | 15.2 | 18.0 | 18.8 | 52.0 | 234 / 220 / 118 | 19/0 | video visible, HTML over it |  |
| b-gl-540 | 960×540 | 60.1 | 59.0 | 11 | 8.5 / 15.5 / 18.0 | 71.9 | 9.8 | 7.2 | 4.1 | 21.1 | 216 / 133 / 123 | 19/0 | video visible, HTML over it |  |
| b-gl-720 | 1280×720 | 59.8 | 58.8 | 9 | 10.7 / 17.3 / 25.2 | 71.5 | 11.5 | 9.8 | 7.1 | 28.5 | 228 / 243 / 122 | 19/0 | video visible, HTML over it |  |
| b-gl-1080 | 1920×1080 | 60.1 | 58.7 | 13 | 10.5 / 16.8 / 21.8 | 71.9 | 14.6 | 14.6 | 12.9 | 42.1 | 250 / 350 / 127 | 19/0 | video visible, HTML over it |  |
| b-gl-1080-raf | 1920×1080 | 60.1 | 59.8 | 2 | 11.3 / 17.6 / 27.6 | 71.9 | 15.3 | 15.3 | 13.8 | 44.4 | 242 / 289 / 121 | 19/0 | video visible, HTML over it |  |
| c-native-540 | 960×540 | 60.1 | 60.1 | 0 | 0.4 / 0.5 / 1.4 | 72.0 | 6.4 | 1.7 | 0.3 | 8.3 | 217 / 38 / 110 | 0/0 | video visible, HTML over it |  |
| c-native-1080 | 1920×1080 | 59.9 | 59.9 | 0 | 0.4 / 0.6 / 1.1 | 71.9 | 6.6 | 1.6 | 0.4 | 8.6 | 237 / 39 / 110 | 0/0 | video visible, HTML over it |  |
| c-above-1080 | 1920×1080 | 59.8 | 59.8 | 0 | 0.4 / 0.9 / 6.6 | 71.4 | 7.1 | 1.9 | 0.4 | 9.3 | 236 / 39 / 111 | 0/0 | video visible, HTML not over it |  |

Producer timings (ms, p50/p95): encode | GPU wait (render+readback) | copy into Vec | send | recv latency (producer→JS onmessage) | fetch
- idle: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 36.2%
- a-gl-540: 0.35/0.52 | 0.84/3.73 | 0.07/0.12 | 0.03/0.12 | 2.49/6.17 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 2; GPU util 41.3%
- a-2d-540: 0.36/0.51 | 0.82/4.40 | 0.07/0.12 | 0.03/0.08 | 2.37/6.46 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 3; GPU util 41.1%
- a-gl-720: 0.35/0.55 | 0.87/4.34 | 0.16/0.23 | 0.04/0.09 | 3.24/7.27 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 1; GPU util 45.8%
- a-2d-720: 0.35/0.59 | 0.93/4.64 | 0.16/0.22 | 0.04/0.10 | 3.10/7.82 | –; backpressure drops 1 (timeout overrides 0), unfetched 0, late ticks 3; GPU util 42.2%
- a-gl-1080: 0.34/0.50 | 1.02/4.93 | 0.38/0.53 | 0.04/0.09 | 4.28/8.47 | –; backpressure drops 1 (timeout overrides 0), unfetched 0, late ticks 3; GPU util 48.5%
- a-2d-1080: 0.35/0.50 | 1.05/5.90 | 0.38/0.52 | 0.04/0.10 | 4.43/9.54 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 2; GPU util 48.4%
- b-gl-540: 0.35/0.48 | 0.89/5.28 | 0.07/0.12 | 0.01/0.01 | 2.08/6.70 | 16.00/22.00; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 2; GPU util 46.2%
- b-gl-720: 0.35/0.63 | 0.89/5.02 | 0.16/0.23 | 0.00/0.01 | 2.57/7.30 | 16.00/22.00; backpressure drops 0 (timeout overrides 0), unfetched 2, late ticks 3; GPU util 42.9%
- b-gl-1080: 0.34/0.53 | 1.05/4.91 | 0.38/0.56 | 0.01/0.01 | 3.69/8.72 | 17.00/21.00; backpressure drops 0 (timeout overrides 0), unfetched 3, late ticks 3; GPU util 45.7%
- b-gl-1080-raf: 0.34/0.51 | 1.02/5.42 | 0.37/0.57 | 0.00/0.01 | 3.71/8.77 | 9.00/20.00; backpressure drops 0 (timeout overrides 0), unfetched 1, late ticks 5; GPU util 45.6%
- c-native-540: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 40.5%
- c-native-1080: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 2; GPU util 40.1%
- c-above-1080: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 10; GPU util 40.6%
