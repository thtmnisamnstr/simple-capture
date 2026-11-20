Results from /Users/gavin/Documents/Development/simple-capture/spikes/02-preview-transport/results/constrained (ProductName: macOS ProductVersion: 26.7.2 BuildVersion: 25G332, tauri 2.12.7, dpr 2)

| mode | frame px | produced fps | presented fps | drops (id gaps) | latency p50 / p95 / p99 ms | rAF fps | CPU% app | CPU% WebContent | CPU% WebKit GPU | CPU% total | mem MB (app / WC / GPU) | verify ok/fail | snapshot | errors |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| idle | – | 0.0 | – | – | – | 71.9 | 2.6 | 2.7 | 0.4 | 5.7 | 42 / 39 / 111 | 0/0 | video NOT visible, HTML over it |  |
| a-gl-540 | 960×540 | 60.0 | 59.4 | 6 | 11.2 / 17.2 / 18.4 | 72.0 | 10.8 | 10.0 | 4.8 | 25.6 | 219 / 99 / 124 | 19/0 | video visible, HTML over it |  |
| a-2d-540 | 960×540 | 60.1 | 59.9 | 1 | 8.6 / 14.7 / 15.3 | 72.0 | 8.8 | 8.6 | 6.9 | 24.4 | 213 / 93 / 118 | 19/0 | video visible, HTML over it |  |
| a-gl-720 | 1280×720 | 60.0 | 59.7 | 3 | 8.8 / 15.1 / 16.1 | 72.0 | 11.0 | 11.2 | 5.6 | 27.8 | 222 / 127 / 123 | 19/0 | video visible, HTML over it |  |
| a-2d-720 | 1280×720 | 60.0 | 59.9 | 1 | 9.9 / 16.1 / 17.2 | 72.0 | 10.3 | 11.2 | 10.5 | 32.0 | 225 / 128 / 116 | 19/0 | video visible, HTML over it |  |
| a-gl-1080 | 1920×1080 | 59.9 | 59.5 | 4 | 9.5 / 16.1 / 17.6 | 71.9 | 14.4 | 17.0 | 11.6 | 42.9 | 243 / 234 / 118 | 19/0 | video visible, HTML over it |  |
| a-2d-1080 | 1920×1080 | 60.0 | 59.5 | 5 | 9.3 / 15.5 / 16.7 | 72.0 | 13.8 | 16.6 | 18.1 | 48.5 | 235 / 218 / 118 | 19/0 | video visible, HTML over it |  |
| b-gl-540 | 960×540 | 60.1 | 59.9 | 1 | 8.1 / 14.8 / 15.5 | 72.0 | 8.7 | 6.6 | 3.6 | 18.9 | 219 / 87 / 121 | 19/0 | video visible, HTML over it |  |
| b-gl-720 | 1280×720 | 60.1 | 59.8 | 2 | 9.4 / 15.8 / 17.3 | 72.0 | 10.1 | 8.9 | 6.1 | 25.1 | 226 / 122 / 123 | 19/0 | video visible, HTML over it |  |
| b-gl-1080 | 1920×1080 | 60.0 | 59.5 | 4 | 9.7 / 16.5 / 17.5 | 72.0 | 13.7 | 14.1 | 11.9 | 39.7 | 235 / 221 / 122 | 19/0 | video visible, HTML over it |  |
| b-gl-1080-raf | 1920×1080 | 60.1 | 59.8 | 1 | 10.3 / 17.0 / 18.7 | 71.8 | 13.8 | 14.3 | 11.7 | 39.9 | 243 / 221 / 122 | 19/0 | video visible, HTML over it |  |
| c-native-540 | 960×540 | 60.1 | 60.1 | 0 | 0.4 / 0.5 / 0.6 | 72.0 | 6.5 | 1.7 | 0.2 | 8.4 | 218 / 34 / 12 | 0/0 | video visible, HTML over it |  |
| c-native-1080 | 1920×1080 | 60.1 | 60.1 | 0 | 0.4 / 0.6 / 1.0 | 72.0 | 7.1 | 1.9 | 0.3 | 9.2 | 238 / 51 / 110 | 0/0 | video visible, HTML over it |  |
| c-above-1080 | 1920×1080 | 60.1 | 60.1 | 0 | 0.4 / 0.5 / 0.6 | 72.0 | 7.1 | 2.0 | 0.3 | 9.3 | 236 / 37 / 105 | 0/0 | video visible, HTML not over it |  |

Producer timings (ms, p50/p95): encode | GPU wait (render+readback) | copy into Vec | send | recv latency (producer→JS onmessage) | fetch
- idle: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 38.4%
- a-gl-540: 0.36/0.50 | 0.85/4.46 | 0.08/0.14 | 0.03/0.09 | 2.72/6.74 | –; backpressure drops 1 (timeout overrides 0), unfetched 0, late ticks 2; GPU util 39.8%
- a-2d-540: 0.36/0.53 | 0.74/3.72 | 0.07/0.10 | 0.04/0.07 | 2.13/5.26 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 37.5%
- a-gl-720: 0.36/0.45 | 0.79/3.98 | 0.16/0.20 | 0.04/0.08 | 2.49/5.94 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 38.7%
- a-2d-720: 0.35/0.46 | 0.77/4.25 | 0.16/0.20 | 0.04/0.08 | 2.68/5.86 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 37.1%
- a-gl-1080: 0.34/0.42 | 0.85/4.34 | 0.38/0.45 | 0.04/0.08 | 3.90/7.34 | –; backpressure drops 1 (timeout overrides 0), unfetched 0, late ticks 3; GPU util 43.1%
- a-2d-1080: 0.36/0.44 | 1.04/4.28 | 0.38/0.45 | 0.05/0.08 | 3.71/7.18 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 46.3%
- b-gl-540: 0.36/0.46 | 0.77/4.22 | 0.07/0.10 | 0.01/0.01 | 1.85/4.88 | 17.00/21.00; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 37.4%
- b-gl-720: 0.35/0.45 | 0.81/4.25 | 0.16/0.21 | 0.01/0.01 | 2.18/5.61 | 17.00/21.00; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 38.3%
- b-gl-1080: 0.34/0.43 | 1.03/3.95 | 0.39/0.46 | 0.01/0.01 | 3.35/6.26 | 17.00/20.00; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 43.1%
- b-gl-1080-raf: 0.33/0.42 | 1.03/5.04 | 0.38/0.49 | 0.01/0.01 | 3.96/7.58 | 12.00/16.00; backpressure drops 0 (timeout overrides 0), unfetched 4, late ticks 3; GPU util 44.9%
- c-native-540: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 47.0%
- c-native-1080: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 47.1%
- c-above-1080: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 55.0%
