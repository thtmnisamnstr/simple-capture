Results from /Users/gavin/Documents/Development/simple-capture/spikes/02-preview-transport/results/default-r2 (ProductName: macOS ProductVersion: 26.7.2 BuildVersion: 25G332, tauri 2.12.7, dpr 2)

| mode | frame px | produced fps | presented fps | drops (id gaps) | latency p50 / p95 / p99 ms | rAF fps | CPU% app | CPU% WebContent | CPU% WebKit GPU | CPU% total | mem MB (app / WC / GPU) | verify ok/fail | snapshot | errors |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| idle | – | 0.0 | – | – | – | 72.0 | 5.6 | 4.8 | 0.7 | 11.1 | 43 / 51 / 111 | 0/0 | video NOT visible, HTML over it |  |
| a-gl-540 | 960×540 | 60.1 | 59.8 | 2 | 10.6 / 16.9 / 17.6 | 72.0 | 15.1 | 15.1 | 6.5 | 36.7 | 222 / 94 / 118 | 19/0 | video visible, HTML over it |  |
| a-2d-540 | 960×540 | 60.1 | 59.9 | 1 | 10.6 / 16.4 / 17.0 | 72.0 | 16.1 | 17.1 | 12.6 | 46.0 | 218 / 95 / 119 | 19/0 | video visible, HTML over it |  |
| a-gl-720 | 1280×720 | 60.1 | 59.7 | 3 | 15.6 / 22.8 / 23.9 | 72.0 | 14.5 | 17.0 | 9.4 | 41.1 | 226 / 313 / 120 | 19/0 | video visible, HTML over it |  |
| a-2d-720 | 1280×720 | 60.0 | 59.6 | 4 | 17.4 / 23.4 / 24.2 | 72.0 | 15.3 | 19.0 | 18.3 | 52.7 | 222 / 131 / 115 | 19/0 | video visible, HTML over it |  |
| a-gl-1080 | 1920×1080 | 60.0 | 59.7 | 3 | 11.4 / 17.3 / 18.1 | 72.0 | 16.8 | 22.2 | 16.0 | 54.9 | 235 / 226 / 119 | 19/0 | video visible, HTML over it |  |
| a-2d-1080 | 1920×1080 | 60.1 | 59.6 | 4 | 17.5 / 22.8 / 24.0 | 72.0 | 17.5 | 24.2 | 30.3 | 72.1 | 235 / 218 / 117 | 19/0 | video visible, HTML over it |  |
| b-gl-540 | 960×540 | 60.0 | 59.0 | 10 | 14.7 / 23.0 / 23.8 | 72.0 | 16.1 | 13.3 | 8.8 | 38.2 | 220 / 87 / 120 | 19/0 | video visible, HTML over it |  |
| b-gl-720 | 1280×720 | 60.1 | 60.0 | 0 | 10.5 / 15.8 / 16.7 | 72.0 | 15.0 | 15.0 | 10.6 | 40.6 | 222 / 120 / 121 | 20/0 | video visible, HTML over it |  |
| b-gl-1080 | 1920×1080 | 60.1 | 59.6 | 4 | 11.4 / 17.3 / 18.1 | 72.0 | 16.4 | 19.7 | 16.7 | 52.9 | 235 / 221 / 118 | 19/0 | video visible, HTML over it |  |
| b-gl-1080-raf | 1920×1080 | 60.0 | 59.9 | 1 | 17.1 / 24.0 / 24.5 | 72.0 | 16.4 | 20.6 | 16.1 | 53.1 | 235 / 229 / 119 | 19/0 | video visible, HTML over it |  |
| c-native-540 | 960×540 | 59.8 | 59.8 | 0 | 0.7 / 1.4 / 2.6 | 72.0 | 16.5 | 6.1 | 0.7 | 23.2 | 218 / 37 / 105 | 0/0 | video visible, HTML over it |  |
| c-native-1080 | 1920×1080 | 60.0 | 60.0 | 0 | 0.7 / 1.3 / 1.8 | 71.9 | 16.3 | 5.8 | 0.7 | 22.8 | 235 / 37 / 105 | 0/0 | video visible, HTML over it |  |
| c-above-1080 | 1920×1080 | 60.1 | 60.1 | 0 | 0.7 / 1.3 / 1.9 | 71.9 | 16.2 | 5.8 | 0.6 | 22.6 | 235 / 37 / 106 | 0/0 | video visible, HTML not over it |  |

Producer timings (ms, p50/p95): encode | GPU wait (render+readback) | copy into Vec | send | recv latency (producer→JS onmessage) | fetch
- idle: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 57.0%
- a-gl-540: 0.28/0.59 | 1.28/4.93 | 0.05/0.13 | 0.02/0.04 | 4.02/7.49 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 58.5%
- a-2d-540: 0.32/0.82 | 4.79/9.84 | 0.07/0.26 | 0.03/0.06 | 6.83/11.86 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 72.5%
- a-gl-720: 0.25/0.43 | 4.73/9.28 | 0.13/0.22 | 0.02/0.04 | 7.36/11.93 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 73.4%
- a-2d-720: 0.29/0.52 | 4.82/9.67 | 0.13/0.25 | 0.02/0.04 | 7.49/12.23 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 74.2%
- a-gl-1080: 0.24/0.47 | 4.67/9.38 | 0.24/0.41 | 0.02/0.03 | 8.10/13.00 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 72.8%
- a-2d-1080: 0.27/0.45 | 5.07/9.38 | 0.29/0.47 | 0.02/0.04 | 7.99/12.79 | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 77.5%
- b-gl-540: 0.29/0.79 | 4.59/8.81 | 0.08/0.31 | 0.00/0.01 | 6.54/11.26 | 15.00/26.00; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 73.3%
- b-gl-720: 0.26/0.66 | 4.25/9.08 | 0.12/0.36 | 0.00/0.01 | 6.02/10.95 | 15.00/26.00; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 72.7%
- b-gl-1080: 0.25/0.49 | 4.98/9.36 | 0.32/0.45 | 0.00/0.01 | 7.64/12.10 | 15.00/25.00; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 72.8%
- b-gl-1080-raf: 0.23/0.41 | 4.07/9.60 | 0.25/0.42 | 0.00/0.00 | 8.50/13.82 | 6.00/18.00; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 1; GPU util 75.0%
- c-native-540: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 1; GPU util 72.1%
- c-native-1080: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 74.1%
- c-above-1080: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 72.8%
