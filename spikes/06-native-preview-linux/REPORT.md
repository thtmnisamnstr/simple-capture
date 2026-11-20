# Spike 6: native preview on Linux (WebKitGTK, X11 + Wayland)
Status: partially feasible. Written 2026-11-19 from `spikes/06-native-preview-linux/results/`.
Machine: M3 Pro host, Docker linux/arm64. Ubuntu 24.04, WebKitGTK 2.52.6, GTK 3.24.41, Mesa 25.2.8 (llvmpipe/lavapipe, **software only**), weston 13.0.0, Xvfb 21.1.12. No real GPU and no real Linux desktop.

## Question
Can ADR-007's macOS design work on Linux under Tauri v2 / wry / WebKitGTK, on both X11 and Wayland? That design puts wgpu in a native surface under a transparent webview so the HTML composites on top.

## Method
- A small tao 0.37.3 + wry 0.57.3 program. These are the versions Tauri 2.12.7 resolves (spike 02 Cargo.lock).
- wgpu 30 on Vulkan renders spike 2's synthetic 1080p composite.
- The page is served from a custom `sc://` protocol: transparent body, opaque top bar, translucent red "HTML OVER VIDEO" panel with a white border, translucent timeline.
- Run under Xvfb (X11) and `weston --backend=headless --renderer=pixman --debug` (Wayland; kiosk-shell, plus desktop-shell for one run).
- Screenshots from `import` (X11) and `weston-screenshooter` (Wayland). I judged every result by reading the screenshots.
- `./run.sh build`, `./run.sh x11|wayland <mode> [tag]`. Env knobs: `SC_SECONDS`, `SC_MARGIN`, `SC_SHELL`, `SC_HIDE_WEB`, `SC_URL`, `WEBKIT_DISABLE_*`.

Modes:
| mode | what | X11 | Wayland |
|---|---|---|---|
| `subsurface` | wgpu → `wl_subsurface` of GTK's toplevel `wl_surface`: `place_below(parent)`, `set_desync`, empty input region. tao window and webview transparent | n/a | **PASS** |
| `x11child` | wgpu → native X11 child window (`GtkDrawingArea` + `gdk_window_ensure_native`) under the webview | **FAIL**: HTML on opaque black, video not visible (visible only with the webview hidden) | n/a |
| `glarea` | `GtkOverlay` { `GtkGLArea` (bottom), webview (overlay) }; frames via wgpu readback → `glTexSubImage2D`. Same layout as wry `examples/gtk_opengl.rs` | **PASS** with the default renderer; **FAIL** (HTML vanishes) with `WEBKIT_DISABLE_DMABUF_RENDERER=1` or `WEBKIT_DISABLE_COMPOSITING_MODE=1` | **PASS**, also with both variables |
| `cairo` | `GtkDrawingArea` + cairo under the webview | PASS default; FAIL with DMA-BUF disabled (opaque black webview background) | PASS |
| `pull` | fallback: custom-protocol pull → WebGL2 canvas (spike 2 mode B) | **PASS** (also with DMA-BUF disabled) | **PASS** |

webkit://gpu on both backends: Renderer "DMABuf (Supported buffers: Shared Memory)", buffer format shm AR24, render process "Surfaceless", threaded GPU rendering. GL_EXT_memory_object_fd is present in the UI-process GL context.

Wayland positioning check: `SC_SHELL=desktop SC_MARGIN=60` put the subsurface at logical (86,120), 1160×680. The video sat exactly in the margins under GTK's client-side title bar.

## Software numbers (llvmpipe; NOT representative of real hardware)
Medians over t = 3–9 s. CPU is % of one core over a 3 s window.
| run | producer/present /s | GTK draws /s | JS drawn /s | WebKitWebProcess CPU | app CPU |
|---|---|---|---|---|---|
| x11 glarea | 60 | 60 | n/a | 118% | 529% |
| x11 cairo | 59 | 59 | n/a | 136% | 333% |
| x11 x11child | 60 presented | n/a | n/a | 128% | 330% |
| x11 pull | 56 | n/a | 35 | 402% | 428% |
| wayland subsurface | 43 (60 → 25 drift in run 1) | n/a | n/a | 116% | 329% |
| wayland subsurface, DMA-BUF off | 57 | n/a | n/a | 4% | 390% |
| wayland subsurface, desktop-shell + margin | 59 | n/a | n/a | 82% | 389% |
| wayland glarea | 60 | 60 | n/a | 118% | 630% |
| wayland cairo | 60 | 36 | n/a | 104% | 408% |
| wayland pull | 48 | n/a | 18 | 359% | 370% |

App CPU is dominated by the 1080p llvmpipe composite plus readback.

## Findings
1. **Wayland can do what macOS does.** A wgpu surface on a `wl_subsurface` placed below a transparent toplevel lets the compositor blend the HTML over the video. It needs no copies and no GTK repaint per frame. About 60 lines: `wayland-client` with `wayland-backend/client_system` on GDK's `wl_display`, plus `gdk_wayland_window_get_wl_surface`.
2. **X11 cannot put a native window under the HTML.** X11 does not alpha-blend child windows inside a toplevel, so the transparent webview shows black over the native window (observed).
3. **X11 can composite inside GTK.** GtkOverlay with a GtkGLArea under the webview works with WebKit's default DMA-BUF renderer (shm fallback here). Wry ships this layout as an example. It breaks when WebKit's legacy paths are forced, which Tauri documents as NVIDIA workarounds.
4. **Pull mode works everywhere I tried**, including the legacy WebKit paths, and costs about 3× the web-process CPU of the native modes (software).
5. **Tauri integration surface:** `Window::gtk_window()`, `default_vbox()` (tauri 2.12.7 src/window/mod.rs:1790/1804) and `PlatformWebview::inner() -> webkit2gtk::WebView` (src/webview/mod.rs:177). Tauri packs the webview into `default_vbox` (tauri-runtime-wry lib.rs:5181). wry sets transparency with `set_background_color(RGBA 0)` (wry src/webkitgtk/mod.rs:293). tao gives transparent windows an RGBA visual and `app_paintable` (tao window.rs:141-154). wry's `build_as_child` is X11-only, so Wayland must use `build_gtk`.

## Recommendation
- **Wayland:** subsurface below a transparent toplevel. A `PreviewSink` implementation keeps `set_position` and the surface size in sync with the UI's preview rectangle, and rebuilds the subsurface on remap.
- **X11:** GtkOverlay with GtkGLArea under the webview. Ship it only after adding a zero-copy Vulkan→GL hand-off (`VK_KHR_external_memory_fd` → `GL_EXT_memory_object_fd` in the GLX context, or a dma-buf) and measuring it on real hardware. Until then, pull mode is the X11 default.
- **Runtime rule:** if `WEBKIT_DISABLE_DMABUF_RENDERER` or `WEBKIT_DISABLE_COMPOSITING_MODE` is set, or the webkit renderer is not DMABuf, X11 uses pull mode.
- **Fallback (all OSes):** pull mode, as in ADR-007.
- Owner condition "only if it also works on Windows and Linux": Linux meets it on Wayland, and on X11 only conditionally (above). Windows was not part of this spike.

## Must test on real Linux hardware
- GNOME/Mutter, KDE/KWin and Sway on Intel/AMD/NVIDIA: stacking, translucency, input pass-through.
- Resize, maximize, hide/show, monitor moves, HiDPI and fractional scale.
- Xorg with and without a compositing WM.
- NVIDIA with the Tauri-documented variables.
- Zero-copy X11 import, then CPU, latency and pacing at 1080p/4K against pull mode.
- Running inside a real Tauri app (moving the webview from `default_vbox` into a GtkOverlay).

## Caveats
- Everything ran on software rendering in arm64 Docker. Weston pixman with shm buffers is not a production compositor.
- The first subsurface run drifted from 60 to about 25 presents/s; later runs held about 60. Not investigated.
- Resize/scroll sync is untested. GTK3 destroys the toplevel `wl_surface` on hide, so the subsurface must be recreated.
- GTK4 graphics offload (GtkGraphicsOffload, 4.14+/4.16 black-background) is the toolkit-native version of this idea, but Tauri/wry are on GTK3.

Sources: https://v2.tauri.app/develop/debug/linux-graphics/, https://docs.rs/crate/wry/0.56.0/source/README.md, https://docs.rs/crate/wry/latest/source/examples/wgpu.rs, https://blog.gtk.org/2024/04/, https://docs.gtk.org/gtk4/property.GraphicsOffload.black-background.html; local crate sources cited above.

---

## Independent verification (2026-11-19)

An adversarial verifier re-read the sources and re-ran the builds and measurements. Verdict: **partially confirmed**. Where this section and the text above disagree, this section wins.

### Discrepancies found

- GL_EXT_memory_object_fd is claimed to be in the 'UI-process GL extension list' on both backends. The saved webkit://gpu dumps show it in the UI process only on Wayland: that section of results/wayland-webkitgpu.log reports API 'OpenGL (libepoxy)', Native interface EGL, GL 4.5 Core, and lists the extension. On X11 (results/x11-webkitgpu.log:56-66) the UI process reports 'API: Not available' and no GL_EXTENSIONS at all. There the extension appears only in the 'Hardware Acceleration Information (Render process)' section (lines 68-89), which is a surfaceless OpenGL ES 3.2 context in the web process. So the X11 zero-copy plan (import into GTK3's GLX GLArea context via GL_EXT_memory_object_fd) has no evidence behind it: the GLX context's extensions were never queried.
- The x11child failure is explained as 'X11 does not alpha-blend child windows inside a toplevel'. That is the investigator's inference; no run tests it. xwininfo (results/x11-x11child.log) shows the native wgpu child 0x20001f (1280x800) as the only full-size child of the toplevel. The webview is not its own X window. The observed fact is narrower: under Xvfb with no compositing manager, GTK3's ensure_native child plus webview showed the HTML over opaque black. The broader claim that a native surface under the HTML is unavailable on X11 in general was not tested with a compositing WM or ARGB child visuals.
- Several medians in the software-numbers table (t = 3-9 s) differ slightly from the saved logs, in each case by 1-6. Producer rate: x11 pull is 58 in the log (claimed 56), wayland subsurface run 1 is 42 (claimed 43), wayland subsurface with DMA-BUF off is 59 (claimed 57), the desktop-shell margin run is 60 (claimed 59), wayland pull is 54 (claimed 48). x11 cairo is 60/60 (claimed 59/59), and wayland cairo GTK draws are 37 (claimed 36).
- Wayland pull at '18 drawn fps' is not stable. 18 is the median of all JS samples in the original log (12 35 27 29 12 23 31 24 18 12 5 5 8). My re-run gave a median of 32 (18 to 37) and WebKitWebProcess at 435%, against the claimed 359%. The '18 on Wayland' figure should be reported as a range across runs.
- 'WebKitWebProcess about 116-136% in the native modes' leaves out two saved runs below that range: wayland cairo at 104% and wayland subsurface desktop-shell+margin at 82%.
- The x11child wgpu surface was configured with alpha mode PreMultiplied (Opaque was not offered: alpha=[PreMultiplied, Inherit]). The write-up does not mention this. It does not change the observed result.
- The Tauri linux-graphics page puts the nvidia_drm.modeset=1 kernel parameter first in its list of workarounds. It also does not present the variables as NVIDIA-only, although it names NVIDIA as the most common cause. The investigator's 'NVIDIA workarounds' framing is a simplification. The order of the three variables and the 'last resort' status of WEBKIT_DISABLE_COMPOSITING_MODE are correct.
- Claims about GTK3 recreating the wl_surface on hide/show, desync resize mismatch, input pass-through via the empty input region, and HiDPI behaviour are read or inferred only. None was exercised. The investigator mostly labels these as untested, which is correct.

### Corrected summary

Verdict: partially feasible. These results were checked by re-running the spike and are all on software rendering only: arm64 Docker, Ubuntu 24.04, WebKitGTK 2.52.6, GTK 3.24.41, Mesa 25.2.8 llvmpipe/lavapipe, weston 13 headless with pixman, Xvfb 21.1.12, tao 0.37.3 / wry 0.57.3 (the versions Tauri 2.12.7 resolves) and wgpu 30.0.2 on Vulkan.

Observed (re-run by the verifier and matching the investigator's screenshots):
- **Wayland subsurface: PASS.** wgpu presents to a wl_subsurface of GTK's toplevel wl_surface, placed below the parent, set to desync, with an empty input region, under a transparent tao window and webview. The HTML blends over the Vulkan frames. Placement was also correct in a decorated desktop-shell window with 60 px margins. It also passed with WEBKIT_DISABLE_DMABUF_RENDERER=1 and with WEBKIT_DISABLE_COMPOSITING_MODE=1.
- **X11 native child window (GtkDrawingArea + ensure_native) under the webview: FAIL under Xvfb without a compositing manager.** The HTML appears on opaque black and the video is hidden. With the webview hidden the video is visible. Why it fails was not established, and the setup was not tried with a compositing WM.
- **GtkOverlay { GtkGLArea under the webview }, frames via CPU readback and glTexSubImage2D: PASS on X11 and Wayland** with WebKit's default renderer ('DMABuf (Supported buffers: Shared Memory)'). On X11 the HTML disappears with either WEBKIT_DISABLE_DMABUF_RENDERER=1 or WEBKIT_DISABLE_COMPOSITING_MODE=1. On Wayland it still works with both.
- **Cairo DrawingArea under the webview: PASS on X11 and Wayland**, except on X11 with the DMA-BUF renderer disabled, where the webview background turns opaque black.
- **Pull fallback (custom protocol into a WebGL2 canvas): renders on both**, and also on X11 with the DMA-BUF renderer disabled.

Recommendation, labelled as inference:
- **Wayland:** subsurface below a transparent toplevel.
- **X11:** GtkOverlay with GtkGLArea, but only with the DMA-BUF renderer active. A zero-copy Vulkan-to-GL hand-off is unbuilt and unverified. In particular, GL_EXT_memory_object_fd was seen only in WebKit's render-process GLES context on X11 and in the UI-process EGL context on Wayland. GTK3's GLX context on X11 was never queried. Until that is proven on real hardware, X11 should default to pull mode, and so should any session where the WebKit disable variables are set.
- **Integration:** Tauri exposes gtk_window(), default_vbox() and webview inner() -> webkit2gtk::WebView. Moving the webview into a GtkOverlay inside a real Tauri app was not tested; wry's gtk_opengl example builds that layout directly.

Software numbers (NOT representative of real hardware; medians over t = 3-9 s):
- Native modes mostly held about 60 producer or present /s. The first Wayland subsurface run drifted to 25-42/s; the verifier's re-run held 60.
- Pull drew about 35-37 fps on X11 and 18-32 fps on Wayland, varying between runs.
- WebKitWebProcess CPU: about 82-136% in the native, GLArea and cairo modes with the default renderer; 359-435% in pull mode; 3-5% with the DMA-BUF renderer or compositing disabled.
- App process: 330-630%, dominated by the llvmpipe composite and readback.

Untested, and needs real Linux hardware:
- Mutter, KWin and Sway on Intel, AMD and NVIDIA GPUs.
- Resize, hide/show (wl_surface recreation), HiDPI and fractional scaling, and input pass-through.
- Xorg with a compositing WM.
- NVIDIA with the Tauri-documented variables (Tauri's linux-graphics page lists the nvidia_drm.modeset=1 kernel parameter first, then __NV_DISABLE_EXPLICIT_SYNC=1, WEBKIT_DISABLE_DMABUF_RENDERER=1 and, as a last resort, WEBKIT_DISABLE_COMPOSITING_MODE=1).
- The X11 zero-copy import.
- Running the same code inside a real Tauri app.

Windows was out of scope.
