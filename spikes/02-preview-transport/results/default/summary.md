Results from /Users/gavin/Documents/Development/simple-capture/spikes/02-preview-transport/results/default (ProductName: macOS ProductVersion: 26.7.2 BuildVersion: 25G332, tauri 2.12.7, dpr 2)

| mode | frame px | produced fps | presented fps | drops (id gaps) | latency p50 / p95 / p99 ms | rAF fps | CPU% app | CPU% WebContent | CPU% WebKit GPU | CPU% total | mem MB (app / WC / GPU) | verify ok/fail | snapshot | errors |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| idle | – | 0.0 | – | – | – | 71.9 | 2.0 | 2.5 | 0.4 | 4.9 | 42 / 40 / 107 | 0/0 | video NOT visible, HTML over it |  |
| a-gl-540 | 960×540 | 59.4 | 58.0 | 13 | 10.8 / 17.8 / 39.6 | 70.9 | 13.3 | 14.6 | 5.8 | 33.7 | 220 / 132 / 123 | 19/0 | video visible, HTML over it |  |
| a-2d-540 | 960×540 | 59.4 | 58.4 | 10 | 10.7 / 16.6 / 21.7 | 71.6 | 11.3 | 12.6 | 6.9 | 30.9 | 219 / 99 / 120 | 19/0 | video visible, HTML over it |  |
| a-gl-720 | 1280×720 | 60.0 | 57.7 | 23 | 10.3 / 16.9 / 21.5 | 72.0 | 14.4 | 16.4 | 9.0 | 39.7 | 221 / 134 / 120 | 19/0 | video visible, HTML over it |  |
| a-2d-720 | 1280×720 | 60.1 | 58.6 | 14 | 11.2 / 17.7 / 23.0 | 71.9 | 14.6 | 16.7 | 13.2 | 44.4 | 226 / 128 / 116 | 19/0 | video visible, HTML over it |  |
| a-gl-1080 | 1920×1080 | 60.1 | 58.7 | 13 | 12.3 / 21.6 / 23.2 | 71.8 | 16.6 | 22.5 | 15.8 | 54.9 | 243 / 243 / 120 | 19/0 | video visible, HTML over it |  |
| a-2d-1080 | 1920×1080 | 60.1 | 60.0 | 0 | 11.4 / 17.0 / 17.8 | 72.0 | 17.3 | 23.4 | 27.4 | 68.3 | 243 / 226 / 114 | 20/0 | video visible, HTML over it |  |
| b-gl-540 | 960×540 | 60.1 | 59.9 | 1 | 10.6 / 16.5 / 19.7 | 72.0 | 13.8 | 11.4 | 8.0 | 33.2 | 220 / 93 / 124 | 19/0 | video visible, HTML over it |  |
| b-gl-720 | 1280×720 | 60.0 | 59.1 | 9 | 12.2 / 21.4 / 23.5 | 72.0 | 15.2 | 15.3 | 10.7 | 41.3 | 226 / 130 / 120 | 19/0 | video visible, HTML over it |  |
| b-gl-1080 | 1920×1080 | 60.0 | 59.9 | 1 | 11.1 / 17.2 / 18.0 | 72.0 | 16.6 | 20.7 | 16.8 | 54.1 | 235 / 229 / 119 | 19/0 | video visible, HTML over it |  |
| b-gl-1080-raf | 1920×1080 | 60.1 | 60.0 | 0 | 11.7 / 18.7 / 23.3 | 72.0 | 16.8 | 20.4 | 17.8 | 55.0 | 235 / 223 / 126 | 20/0 | video visible, HTML over it |  |
| c-native-540 | 960×540 | 60.1 | 60.1 | 0 | 0.8 / 1.3 / 1.7 | 72.0 | 17.5 | 5.8 | 0.9 | 24.3 | 217 / 37 / 109 | 0/0 | video visible, HTML over it |  |
| c-native-1080 | 1920×1080 | 60.1 | 60.1 | 0 | 0.9 / 1.5 / 3.0 | 71.9 | 18.8 | 6.2 | 0.7 | 25.8 | 236 / 39 / 106 | 0/0 | video visible, HTML over it |  |
| c-above-1080 | 1920×1080 | 59.8 | 59.8 | 0 | 0.9 / 2.3 / 13.7 | 72.0 | 18.6 | 6.2 | 0.8 | 25.5 | 239 / 39 / 107 | 0/0 | video visible, HTML not over it |  |

Producer timings (ms, p50/p95): encode | GPU wait (render+readback) | copy into Vec | send | recv latency (producer→JS onmessage) | fetch
- idle: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 32.6%
- a-gl-540: 0.23/1.85 | 0.94/4.30 | 0.05/0.29 | 0.02/0.59 | 3.56/11.54 | –; backpressure drops 4 (timeout overrides 0), unfetched 0, late ticks 10; GPU util 37.1%
- a-2d-540: 0.19/1.33 | 0.92/4.40 | 0.05/0.22 | 0.02/0.20 | 3.18/8.86 | –; backpressure drops 1 (timeout overrides 0), unfetched 0, late ticks 10; GPU util 38.5%
- a-gl-720: 0.24/0.37 | 1.09/5.80 | 0.14/0.23 | 0.02/0.04 | 3.89/8.98 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 44.0%
- a-2d-720: 0.26/0.65 | 1.02/5.64 | 0.11/0.24 | 0.02/0.07 | 4.76/10.01 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 4; GPU util 42.4%
- a-gl-1080: 0.24/0.37 | 1.45/9.21 | 0.22/0.42 | 0.02/0.03 | 5.55/13.54 | –; backpressure drops 1 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 61.4%
- a-2d-1080: 0.26/0.46 | 1.28/6.03 | 0.26/0.35 | 0.02/0.03 | 5.32/9.67 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 44.9%
- b-gl-540: 0.27/0.78 | 1.02/5.09 | 0.06/0.16 | 0.00/0.01 | 2.41/7.11 | 16.00/22.00; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 42.0%
- b-gl-720: 0.27/0.66 | 1.58/7.71 | 0.12/0.30 | 0.00/0.01 | 4.14/10.63 | 16.00/23.00; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 2; GPU util 64.3%
- b-gl-1080: 0.26/0.44 | 1.12/5.02 | 0.24/0.44 | 0.00/0.01 | 4.26/7.77 | 17.00/21.00; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 47.9%
- b-gl-1080-raf: 0.25/0.42 | 1.17/5.17 | 0.25/0.46 | 0.00/0.01 | 5.37/9.61 | 12.00/17.00; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 48.5%
- c-native-540: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 38.9%
- c-native-1080: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 1; GPU util 63.7%
- c-above-1080: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 6; GPU util 73.9%
