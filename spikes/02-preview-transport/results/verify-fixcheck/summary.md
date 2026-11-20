Results from /Users/gavin/Documents/Development/simple-capture/spikes/02-preview-transport/results/verify-fixcheck (ProductName: macOS ProductVersion: 26.7.2 BuildVersion: 25G332, tauri 2.12.7, dpr 2)

| mode | frame px | produced fps | presented fps | drops (id gaps) | latency p50 / p95 / p99 ms | rAF fps | CPU% app | CPU% WebContent | CPU% WebKit GPU | CPU% total | mem MB (app / WC / GPU) | verify ok/fail | snapshot | errors |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| idle | – | 0.0 | – | – | – | 72.0 | 5.1 | 4.1 | 0.9 | 10.2 | 42 / 36 / 20 | 0/0 | video NOT visible, HTML over it |  |
| c-above-1080 | 1920×1080 | 60.2 | 60.2 | 0 | 0.6 / 1.2 / 3.8 | 72.0 | 15.3 | 4.1 | 0.8 | 20.1 | 237 / 39 / 107 | 0/0 | video visible, HTML not over it |  |

Producer timings (ms, p50/p95): encode | GPU wait (render+readback) | copy into Vec | send | recv latency (producer→JS onmessage) | fetch
- idle: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 0; GPU util 46.5%
- c-above-1080: – | – | – | – | – | –; backpressure drops 0 (timeout overrides 0), unfetched 0, late ticks 2; GPU util 41.2%
