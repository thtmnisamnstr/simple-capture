Results from /Users/gavin/Documents/Development/simple-capture/spikes/02-preview-transport/results/verify-r1 (ProductName: macOS ProductVersion: 26.7.2 BuildVersion: 25G332, tauri 2.12.7, dpr 2)

| mode | frame px | produced fps | presented fps | drops (id gaps) | latency p50 / p95 / p99 ms | rAF fps | CPU% app | CPU% WebContent | CPU% WebKit GPU | CPU% total | mem MB (app / WC / GPU) | verify ok/fail | snapshot | errors |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| idle | – | 0.0 | – | – | – | 71.9 | 5.0 | 4.3 | 0.8 | 10.1 | 42 / 53 / 112 | 0/0 | video NOT visible, HTML over it |  |
| a-gl-540 | 960×540 | 60.1 | 59.3 | 7 | 10.4 / 16.4 / 17.6 | 72.0 | 16.0 | 15.9 | 8.2 | 40.1 | 223 / 122 / 121 | 19/0 | video visible, HTML over it |  |
| a-2d-540 | 960×540 | 60.0 | 57.4 | 26 | 10.6 / 20.4 / 23.4 | 71.9 | 16.2 | 16.2 | 10.0 | 42.4 | 223 / 133 / 120 | 19/0 | video visible, HTML over it |  |
| a-gl-720 | 1280×720 | 60.0 | 58.2 | 19 | 10.0 / 16.8 / 20.5 | 71.9 | 15.4 | 16.6 | 8.8 | 40.8 | 224 / 193 / 120 | 19/0 | video visible, HTML over it |  |
| a-2d-720 | 1280×720 | 60.1 | 59.9 | 1 | 10.8 / 16.8 / 18.0 | 71.9 | 15.8 | 17.3 | 15.8 | 48.9 | 228 / 185 / 117 | 19/0 | video visible, HTML over it |  |
| a-gl-1080 | 1920×1080 | 60.1 | 58.6 | 14 | 10.5 / 16.9 / 18.2 | 72.0 | 18.2 | 22.3 | 16.5 | 57.1 | 242 / 346 / 121 | 19/0 | video visible, HTML over it |  |
| a-2d-1080 | 1920×1080 | 60.1 | 59.7 | 3 | 11.2 / 17.0 / 26.4 | 71.9 | 18.0 | 24.3 | 27.9 | 70.2 | 234 / 220 / 118 | 19/0 | video visible, HTML over it |  |
| b-gl-540 | 960×540 | 59.9 | 59.2 | 7 | 10.2 / 17.5 / 22.2 | 71.7 | 15.9 | 12.1 | 8.7 | 36.8 | 223 / 90 / 126 | 19/0 | video visible, HTML over it |  |
| b-gl-720 | 1280×720 | 60.0 | 58.5 | 16 | 10.6 / 17.5 / 21.7 | 72.0 | 15.6 | 14.5 | 10.4 | 40.7 | 221 / 185 / 120 | 19/0 | video visible, HTML over it |  |
| b-gl-1080 | 1920×1080 | 60.1 | 58.8 | 12 | 10.5 / 16.8 / 18.6 | 72.0 | 16.5 | 18.9 | 17.1 | 52.5 | 243 / 302 / 121 | 19/0 | video visible, HTML over it |  |
| b-gl-1080-raf | 1920×1080 | 60.1 | 59.9 | 1 | 11.8 / 17.7 / 19.1 | 71.9 | 17.4 | 19.8 | 17.4 | 54.6 | 243 / 249 / 124 | 19/0 | video visible, HTML over it |  |
| c-native-540 | 960×540 | 59.8 | 59.8 | 0 | 0.9 / 1.4 / 2.7 | 72.0 | 18.5 | 4.5 | 0.8 | 23.8 | 222 / 38 / 106 | 0/0 | video visible, HTML over it |  |
| c-native-1080 | 1920×1080 | 60.1 | 60.1 | 0 | 0.5 / 1.3 / 2.1 | 72.0 | 15.4 | 3.9 | 0.7 | 20.0 | 238 / 39 / 110 | 0/0 | video visible, HTML over it |  |
| c-above-1080 | 1920×1080 | 60.1 | 60.1 | 0 | 0.7 / 1.3 / 3.7 | 72.0 | 16.1 | 4.5 | 0.8 | 21.4 | 238 / 39 / 107 | 0/0 | video visible, HTML not over it |  |

Producer timings (ms, p50/p95): encode | GPU wait (render+readback) | copy into Vec | send | recv latency (producer→JS onmessage) | fetch
- idle: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 39.6%
- a-gl-540: 0.27/0.65 | 1.14/6.34 | 0.06/0.13 | 0.02/0.05 | 3.75/8.70 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 48.1%
- a-2d-540: 0.30/0.82 | 1.13/6.37 | 0.07/0.20 | 0.03/0.08 | 4.29/9.93 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 1; GPU util 47.9%
- a-gl-720: 0.25/0.46 | 1.16/6.03 | 0.10/0.23 | 0.02/0.04 | 4.22/9.24 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 48.5%
- a-2d-720: 0.27/0.68 | 1.06/4.63 | 0.11/0.25 | 0.02/0.06 | 3.96/7.93 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 43.2%
- a-gl-1080: 0.26/0.59 | 1.09/5.09 | 0.24/0.48 | 0.02/0.04 | 5.13/9.20 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 48.4%
- a-2d-1080: 0.27/0.53 | 1.25/5.92 | 0.26/0.41 | 0.02/0.04 | 5.64/10.16 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 46.9%
- b-gl-540: 0.28/0.76 | 1.55/7.13 | 0.07/0.18 | 0.00/0.01 | 3.41/8.95 | 16.00/22.00; backpressure drops 0 (timeout overrides 0), unfetched 2, late ticks 2; GPU util 55.7%
- b-gl-720: 0.29/0.83 | 1.05/4.99 | 0.11/0.30 | 0.00/0.01 | 3.75/8.49 | 16.00/21.00; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 2; GPU util 47.5%
- b-gl-1080: 0.23/0.38 | 1.06/4.45 | 0.24/0.45 | 0.00/0.01 | 4.26/8.02 | 17.00/20.00; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 47.9%
- b-gl-1080-raf: 0.26/0.45 | 1.10/4.51 | 0.24/0.44 | 0.00/0.01 | 4.56/8.37 | 10.00/19.00; backpressure drops 0 (timeout overrides 0), unfetched 1, late ticks 1; GPU util 45.0%
- c-native-540: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 1; GPU util 43.8%
- c-native-1080: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 44.6%
- c-above-1080: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 2; GPU util 44.7%
