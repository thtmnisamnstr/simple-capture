# Spike 2 report: preview transport (Tauri v2)
Status: pass, keep Tauri. Written 2026-11-19 from `results/verify-*`.
Machine: M3 Pro, macOS 26.7.2, DPR 2, Tauri 2.12.7. No M1, WebKitGTK or WebView2 data.

## Question
Can the editor preview reach ≥30 fps at 1080p inside Tauri, and with which transport?

## Method
Each mode runs 10 s at 60 fps after 3 s of warmup (app defaults), and `bench.sh` runs every mode in sequence. Modes:
- A: push over a Tauri Channel, drawn with WebGL2 or 2D canvas
- B: pull through a custom URI protocol (latest frame wins), drawn with WebGL2; also a rAF-paced variant
- C: wgpu into a native CAMetalLayer NSView under a transparent WKWebView, plus C-above (the view placed above the webview)

Sizes are 540, 720 and 1080. Measured: presented fps, frame-id gaps, producer→draw latency, CPU of the app / WebContent / WebKit GPU process, and memory. A window snapshot checks whether the video and the HTML are visible.

## Evidence
- Quiet: `results/verify-r1..r3`. Constrained (CPU hogs): `results/verify-con1..2`. Earlier: `default`, `default-r2`, `constrained`, `experiments/`, `verify-fixcheck`.
- `evidence/*.png` committed; the result PNGs are gitignored.

## Results (median of r1–r3, range in brackets)
| Mode | fps | CPU % total | WC mem |
|---|---|---|---|
| A gl 540 | 59.3 | 39.8 | 128 MB |
| A gl 1080 | 59.1 | 57.1 | 346 MB |
| A 2d 1080 | 59.4 | 70.2 | |
| B gl 540 | 59.5 | 38.1 | 124 MB |
| B gl 1080 | 58.8 | 47.2 [45.2–52.5] | 342 MB |
| B gl 1080 rAF | 59.9 | 54.6 | |
| C native 540 | 60.1 | 20.0 | |
| C native 1080 | 60.1 | 20.4 | 39 MB |
| C above 1080 | 60.1 | 21.4 | |

- Latency p50/p95: A/B about 10–12 / 16–18 ms; C 0.6 / 1.3 ms. On C this measures the producer→present path only, so its 0 drops hold by construction.
- WebKit GPU-process CPU, 2D vs WebGL2: 1.3× at 540, 1.7× at 720 and 1080.
- Snapshot: HTML composites over the native layer in every C mode except C-above.
- The constrained runs show *lower* CPU than the quiet ones, so the hogs did not constrain anything. Treat them as uninformative.

## Verdict
Pass, keep Tauri (ADR-004). Native surface on macOS, URI-pull webview frames as the fallback (ADR-007). Svelte 5 worked (ADR-005). `macOSPrivateApi` is not needed (ADR-026).

## Caveats / open
- Photon latency, resize/scroll sync, WebKitGTK and WebView2 untested.
- The native path relies on wry's private `drawsBackground` key.
- Naming: `tauri.conf.json` still says "Preview Transport Spike" / `com.simplecapture.spike.preview`. It is deliberately not renamed again. The release build embeds no Info.plist and asks for no permission. Never run `pnpm tauri dev`: it embeds an Info.plist with that name.
- `bench.sh` skips the window snapshot by default. `SC_SNAPSHOT=1` runs the binary through `tools/macos-dev-host/run.sh` so the snapshot uses the Simple Capture Dev grant (checked at close-out on `c-native-540`). The `"tauri"` package script refuses to run.
