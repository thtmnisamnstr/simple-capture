Results from /Users/gavin/Documents/Development/simple-capture/spikes/02-preview-transport/results/verify-r3 (ProductName: macOS ProductVersion: 26.7.2 BuildVersion: 25G332, tauri 2.12.7, dpr 2)

| mode | frame px | produced fps | presented fps | drops (id gaps) | latency p50 / p95 / p99 ms | rAF fps | CPU% app | CPU% WebContent | CPU% WebKit GPU | CPU% total | mem MB (app / WC / GPU) | verify ok/fail | snapshot | errors |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| idle | – | 0.0 | – | – | – | 72.0 | 4.2 | 4.0 | 0.7 | 9.0 | 42 / 39 / 106 | 0/0 | video NOT visible, HTML over it |  |
| a-gl-540 | 960×540 | 60.1 | 58.5 | 15 | 10.3 / 17.6 / 22.7 | 71.7 | 16.7 | 15.5 | 7.0 | 39.2 | 225 / 155 / 126 | 19/0 | video visible, HTML over it |  |
| a-2d-540 | 960×540 | 60.0 | 59.7 | 3 | 10.1 / 16.6 / 18.2 | 72.0 | 17.1 | 16.9 | 11.1 | 45.0 | 217 / 132 / 117 | 19/0 | video visible, HTML over it |  |
| a-gl-720 | 1280×720 | 60.1 | 58.8 | 12 | 10.4 / 16.9 / 19.5 | 72.0 | 16.2 | 17.0 | 8.9 | 42.3 | 222 / 199 / 119 | 19/0 | video visible, HTML over it |  |
| a-2d-720 | 1280×720 | 60.0 | 58.9 | 11 | 10.4 / 17.3 / 20.3 | 72.0 | 16.5 | 17.8 | 14.1 | 48.5 | 226 / 186 / 117 | 19/0 | video visible, HTML over it |  |
| a-gl-1080 | 1920×1080 | 60.0 | 59.1 | 8 | 10.9 / 17.1 / 26.0 | 71.7 | 18.5 | 22.9 | 16.3 | 57.7 | 251 / 346 / 120 | 19/0 | video visible, HTML over it |  |
| a-2d-1080 | 1920×1080 | 60.1 | 59.4 | 6 | 11.6 / 18.3 / 23.8 | 71.9 | 17.3 | 23.2 | 25.2 | 65.7 | 242 / 220 / 118 | 19/0 | video visible, HTML over it |  |
| b-gl-540 | 960×540 | 60.1 | 59.5 | 5 | 10.8 / 16.6 / 18.4 | 72.0 | 16.8 | 12.3 | 8.9 | 38.1 | 221 / 219 / 122 | 19/0 | video visible, HTML over it |  |
| b-gl-720 | 1280×720 | 60.1 | 58.5 | 15 | 10.8 / 19.1 / 22.2 | 72.0 | 15.7 | 13.8 | 9.8 | 39.3 | 225 / 277 / 120 | 19/0 | video visible, HTML over it |  |
| b-gl-1080 | 1920×1080 | 60.0 | 59.3 | 7 | 9.6 / 15.6 / 18.3 | 72.0 | 15.0 | 16.8 | 15.3 | 47.2 | 242 / 342 / 120 | 19/0 | video visible, HTML over it |  |
| b-gl-1080-raf | 1920×1080 | 60.1 | 60.1 | 0 | 11.1 / 16.9 / 17.8 | 72.0 | 18.2 | 20.0 | 17.2 | 55.4 | 234 / 247 / 132 | 20/0 | video visible, HTML over it |  |
| c-native-540 | 960×540 | 60.2 | 60.2 | 0 | 0.6 / 1.3 / 1.6 | 72.0 | 14.8 | 4.0 | 0.6 | 19.4 | 214 / 34 / 13 | 0/0 | video visible, HTML over it |  |
| c-native-1080 | 1920×1080 | 60.1 | 60.1 | 0 | 0.7 / 1.2 / 2.5 | 71.8 | 15.6 | 4.2 | 0.8 | 20.5 | 235 / 39 / 107 | 0/0 | video visible, HTML over it |  |
| c-above-1080 | 1920×1080 | 60.1 | 60.1 | 0 | 0.6 / 1.3 / 1.9 | 71.9 | 17.1 | 5.2 | 0.7 | 23.0 | 238 / 37 / 105 | 0/0 | video visible, HTML not over it |  |

Producer timings (ms, p50/p95): encode | GPU wait (render+readback) | copy into Vec | send | recv latency (producer→JS onmessage) | fetch
- idle: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 37.8%
- a-gl-540: 0.29/0.83 | 1.04/5.24 | 0.06/0.17 | 0.03/0.08 | 3.71/8.84 | –; backpressure drops 2 (timeout overrides 0), unfetched 0, late ticks 4; GPU util 48.5%
- a-2d-540: 0.30/0.87 | 1.07/5.30 | 0.08/0.15 | 0.03/0.06 | 4.34/8.78 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 51.6%
- a-gl-720: 0.26/0.61 | 1.03/5.15 | 0.10/0.24 | 0.02/0.07 | 3.94/8.62 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 46.6%
- a-2d-720: 0.30/0.74 | 1.09/6.05 | 0.11/0.26 | 0.03/0.08 | 4.13/8.82 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 49.8%
- a-gl-1080: 0.26/0.60 | 1.11/4.83 | 0.25/0.56 | 0.02/0.07 | 5.10/10.05 | –; backpressure drops 3 (timeout overrides 0), unfetched 0, late ticks 6; GPU util 43.4%
- a-2d-1080: 0.25/0.49 | 1.21/6.28 | 0.25/0.45 | 0.02/0.04 | 5.45/10.72 | –; backpressure drops 1 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 48.0%
- b-gl-540: 0.37/0.88 | 1.04/5.66 | 0.07/0.20 | 0.00/0.02 | 3.08/8.13 | 16.00/22.00; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 1; GPU util 49.1%
- b-gl-720: 0.29/0.74 | 1.02/6.24 | 0.11/0.27 | 0.00/0.02 | 3.48/8.54 | 16.00/23.00; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 50.7%
- b-gl-1080: 0.21/0.48 | 1.08/6.59 | 0.20/0.35 | 0.00/0.01 | 3.16/9.05 | 16.00/23.00; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 49.7%
- b-gl-1080-raf: 0.27/0.48 | 1.23/5.96 | 0.32/0.45 | 0.00/0.01 | 4.75/9.13 | 10.00/21.00; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 44.8%
- c-native-540: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 45.9%
- c-native-1080: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 2; GPU util 45.3%
- c-above-1080: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 37.2%
