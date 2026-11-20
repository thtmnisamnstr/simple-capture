Results from /Users/gavin/Documents/Development/simple-capture/spikes/02-preview-transport/results/verify-r2 (ProductName: macOS ProductVersion: 26.7.2 BuildVersion: 25G332, tauri 2.12.7, dpr 2)

| mode | frame px | produced fps | presented fps | drops (id gaps) | latency p50 / p95 / p99 ms | rAF fps | CPU% app | CPU% WebContent | CPU% WebKit GPU | CPU% total | mem MB (app / WC / GPU) | verify ok/fail | snapshot | errors |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| idle | – | 0.0 | – | – | – | 72.0 | 5.0 | 4.6 | 0.7 | 10.3 | 24 / 39 / 110 | 0/0 | video NOT visible, HTML not over it |  |
| a-gl-540 | 960×540 | 60.2 | 59.5 | 4 | 10.2 / 16.1 / 18.0 | 72.0 | 16.5 | 15.4 | 7.9 | 39.8 | 222 / 128 / 120 | 19/0 | video visible, HTML over it |  |
| a-2d-540 | 960×540 | 60.2 | 59.8 | 1 | 10.6 / 16.5 / 18.1 | 72.0 | 16.6 | 16.5 | 10.4 | 43.4 | 221 / 132 / 117 | 19/0 | video visible, HTML over it |  |
| a-gl-720 | 1280×720 | 60.1 | 59.3 | 7 | 10.7 / 17.8 / 21.2 | 72.0 | 15.4 | 16.9 | 9.4 | 41.7 | 226 / 188 / 124 | 19/0 | video visible, HTML over it |  |
| a-2d-720 | 1280×720 | 60.1 | 59.1 | 9 | 10.5 / 16.8 / 17.9 | 72.0 | 16.5 | 18.1 | 15.4 | 50.1 | 225 / 189 / 117 | 19/0 | video visible, HTML over it |  |
| a-gl-1080 | 1920×1080 | 60.0 | 59.7 | 3 | 11.9 / 17.6 / 18.8 | 72.0 | 17.8 | 22.0 | 17.2 | 57.1 | 243 / 347 / 123 | 19/0 | video visible, HTML over it |  |
| a-2d-1080 | 1920×1080 | 60.0 | 58.9 | 10 | 15.7 / 23.8 / 25.1 | 71.9 | 18.7 | 24.4 | 27.3 | 70.5 | 251 / 227 / 118 | 19/0 | video visible, HTML over it |  |
| b-gl-540 | 960×540 | 60.1 | 59.6 | 4 | 10.7 / 16.4 / 17.7 | 72.0 | 17.6 | 13.5 | 10.1 | 41.3 | 220 / 124 / 120 | 19/0 | video visible, HTML over it |  |
| b-gl-720 | 1280×720 | 59.9 | 59.3 | 6 | 10.3 / 16.3 / 17.5 | 71.9 | 15.4 | 13.6 | 10.1 | 39.1 | 224 / 185 / 120 | 19/0 | video visible, HTML over it |  |
| b-gl-1080 | 1920×1080 | 60.1 | 58.6 | 14 | 11.6 / 19.1 / 30.2 | 71.9 | 14.3 | 16.0 | 14.9 | 45.2 | 243 / 350 / 120 | 19/0 | video visible, HTML over it |  |
| b-gl-1080-raf | 1920×1080 | 59.8 | 59.6 | 2 | 10.6 / 17.1 / 21.8 | 71.5 | 14.8 | 16.7 | 15.2 | 46.7 | 242 / 349 / 120 | 19/0 | video visible, HTML over it |  |
| c-native-540 | 960×540 | 60.1 | 60.1 | 0 | 0.6 / 1.6 / 3.1 | 71.8 | 15.3 | 3.9 | 0.8 | 20.0 | 217 / 38 / 106 | 0/0 | video visible, HTML over it |  |
| c-native-1080 | 1920×1080 | 60.1 | 60.1 | 0 | 0.6 / 1.3 / 2.1 | 71.9 | 15.4 | 4.2 | 0.8 | 20.4 | 238 / 39 / 107 | 0/0 | video visible, HTML over it |  |
| c-above-1080 | 1920×1080 | 59.9 | 59.9 | 0 | 0.5 / 0.9 / 2.6 | 72.0 | 11.9 | 3.1 | 0.6 | 15.6 | 238 / 36 / 14 | 0/0 | video visible, HTML not over it |  |

Producer timings (ms, p50/p95): encode | GPU wait (render+readback) | copy into Vec | send | recv latency (producer→JS onmessage) | fetch
- idle: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 59.0%
- a-gl-540: 0.32/0.73 | 1.00/4.71 | 0.06/0.16 | 0.03/0.09 | 3.57/8.05 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 1; GPU util 50.0%
- a-2d-540: 0.33/0.77 | 1.00/4.95 | 0.06/0.16 | 0.03/0.07 | 3.78/7.80 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 47.9%
- a-gl-720: 0.25/0.65 | 0.96/4.86 | 0.10/0.23 | 0.02/0.06 | 3.99/8.54 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 1; GPU util 45.5%
- a-2d-720: 0.29/0.70 | 1.09/5.42 | 0.11/0.26 | 0.03/0.06 | 4.20/8.53 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 47.4%
- a-gl-1080: 0.25/0.57 | 2.42/7.93 | 0.26/0.45 | 0.02/0.04 | 6.50/11.76 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 1; GPU util 58.7%
- a-2d-1080: 0.26/0.47 | 3.31/8.76 | 0.30/0.71 | 0.02/0.05 | 7.92/13.55 | –; backpressure drops 1 (timeout overrides 0), unfetched 0, late ticks 2; GPU util 67.5%
- b-gl-540: 0.41/0.81 | 1.04/5.15 | 0.08/0.20 | 0.01/0.01 | 3.13/7.43 | 16.00/22.00; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 48.0%
- b-gl-720: 0.25/0.67 | 1.02/4.35 | 0.11/0.26 | 0.00/0.01 | 3.13/6.66 | 16.00/22.00; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 1; GPU util 50.8%
- b-gl-1080: 0.20/0.39 | 1.18/5.22 | 0.18/0.39 | 0.00/0.01 | 4.44/10.17 | 16.00/22.00; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 1; GPU util 48.4%
- b-gl-1080-raf: 0.20/0.37 | 1.08/4.82 | 0.19/0.35 | 0.00/0.01 | 4.19/8.43 | 11.00/19.00; backpressure drops 0 (timeout overrides 0), unfetched 1, late ticks 2; GPU util 45.9%
- c-native-540: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 2; GPU util 44.4%
- c-native-1080: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 53.5%
- c-above-1080: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 1; GPU util 44.8%
