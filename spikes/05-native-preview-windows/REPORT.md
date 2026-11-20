# Spike 5: native preview on Windows (WebView2)
Status: **feasible with caveats**. The design is settled and compiles for Windows, but the compositing has not yet been checked on real hardware. Written 2026-11-19.
Machine: M3 Pro, macOS 26.7.2, Rust 1.95. No Windows machine exists, so nothing here was run on Windows.

## Question
The owner approved native preview only if it also works on Windows and Linux. Can the macOS design (ADR-007, spike 2 mode C: wgpu under a transparent webview, HTML on top) be done on Windows with Tauri v2 / wry / WebView2? This note covers Windows only.

## Short answer
Yes, very likely, and **without** WebView2 composition hosting or a wry fork. ADR-007's line "would need WebView2 composition hosting (not exposed by wry)" is wrong.

- A fully transparent windowed WebView2 shows the host window's own layers beneath it.
- Windows has a public slot for our content under the child windows: a DirectComposition target bound to the window with `topmost = FALSE`.
- wgpu 30 can render straight into a visual in that tree.

## How the layers stack (read, not run)
Microsoft's [CreateTargetForHwnd docs](https://learn.microsoft.com/en-us/windows/win32/api/dcomp/nf-dcomp-idcompositiondevice-createtargetforhwnd) describe four layers per window, bottom to top:

1. GDI and DXGI `Present` content of the window itself. tao paints the window background here in `WM_ERASEBKGND`.
2. The optional **non-topmost** DirectComposition tree. **Our preview visual goes here.**
3. Child windows. The wry container HWND and the cross-process WebView2 HWND live here.
4. The optional topmost DirectComposition tree.

Each window allows at most one topmost and one non-topmost target. Nothing in tao, wry, Tauri or webview2-com creates one (grep of their sources), so the slot is free.

### WebView2 side
- [ICoreWebView2Controller2](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller2): only alpha 0 or 255 is allowed for the default background. With a transparent default background, "WebView will render hosting app content as the background".
- The `WEBVIEW2_DEFAULT_BACKGROUND_COLOR` environment variable avoids the initial white flash.

### wry 0.57.3 (pinned by Tauri 2.12.7)
- **Windowed controller only.** It creates a `WS_CHILD|WS_CLIPCHILDREN` container HWND, then calls `CreateCoreWebView2Controller(WithOptions)` (`src/webview2/mod.rs:264-306, 406-458`). There is no CompositionController anywhere.
- **Transparency.** Transparent, or a background colour with alpha 0, goes to `SetDefaultBackgroundColor` with alpha 0 (`:134-149, :491-501, :2010-2030`).

### Upstream precedent
- [wry `examples/wgpu.rs`](https://docs.rs/crate/wry/latest/source/examples/wgpu.rs) draws wgpu on the parent HWND under a transparent child webview.
- [wry #1331](https://github.com/tauri-apps/wry/issues/1331) reported that it did not blend on Windows 11. The maintainer traced it to `WS_CLIPCHILDREN` on the parent. Commit fb7ef9f ("clip children, fixes #1331") disabled it and closed the issue. I found no later user confirmation.

### tao 0.37.3 (Tauri's windowing)
Never sets `WS_CLIPCHILDREN` on top-level windows (`window_state.rs:239-292`), so the clipping problem from #1331 does not apply.

### wgpu 30
- `SurfaceTargetUnsafe::CompositionVisual` renders into a caller-owned visual. wgpu-hal calls `SetContent`; the caller must `Commit` (`wgpu-hal dx12/mod.rs:1538-1625`).
- wgpu's own `Dx12SwapchainKind::DxgiFromVisual` uses `CreateTargetForHwnd(hwnd, false)`, the same layer-2 approach (`dcomp.rs:95-115`).
- Visual targets support premultiplied alpha; HWND targets are opaque only.

### Tauri wiring
- Use `WebviewWindowBuilder::background_color(Color(r,g,b,0))`. It sets both layers: the window gets an opaque GDI fill (alpha ignored), and the webview becomes transparent (`webview_window.rs:1237-1252`).
- Do **not** use `.transparent(true)`. It makes tauri-runtime-wry paint the window with softbuffer (`lib.rs:4008-4022, 4143-4148`).
- `WebviewWindow::hwnd()` gives the window handle.

## Recommended Windows design ("dcomp")
1. Build the window with `background_color(Color(24,24,28,0))`. The HTML stays opaque everywhere except a transparent preview hole, the same as macOS.
2. On the main thread (`run_on_main_thread`):
   - `DCompositionCreateDevice2(None)`
   - `CreateTargetForHwnd(hwnd, FALSE)`
   - a root visual with a child visual added under it
   - `SetRoot`, then `Commit`
3. Create the wgpu surface with `CompositionVisual(child.as_raw())` on DX12, Bgra8Unorm, opaque alpha, Fifo.
4. The UI reports the preview rect (CSS px × devicePixelRatio = client px). For each report: `SetOffsetX/Y`, then `surface.configure` if the size changed, then `Commit`.
   - For smooth live resize, keep the swapchain at the maximum size and change only offset, clip or scale per frame. Resize the buffers once the layout settles.
5. Input: the WebView2 covers the whole window, so HTML gets all input, the same as macOS.
6. Both platforms sit behind the existing `PreviewSink` trait. The Windows-specific code is about 100 lines.

## Prototype: `spikes/05-native-preview-windows`
Built on tao 0.37.3, wry 0.57.3, wgpu 30.0.2 and windows 0.62.2, the same versions as spike 2's lock. The page has a grid of opaque panels and a transparent hole. Over the hole sit an opaque green badge, a 50% blue strip and a label. JS reports the hole rect over IPC.

Modes (first argument):

| mode | how | layer |
|---|---|---|
| `dcomp` (default) | DirectComposition visual, non-topmost target on the top-level window | 2 |
| `child-below` | Child HWND with a DXGI flip swapchain, placed at the bottom of the sibling z-order (no `WS_CLIPSIBLINGS`) | 3, under the webview |
| `parent` | Whole-window DXGI swapchain, preview drawn in a viewport (the wry example layout; no GDI background) | 1 |
| `child-above` | Child HWND above the webview (equivalent to macOS C-above) | 3, over the webview |

The renderer draws the left half pure magenta and the right half as an animated bar. Fifo present runs on a render thread and prints `[fps]` once a second.

On Windows, after `SC_PROBE_AT` seconds (default 4), the probe:
- captures the client area with screen `BitBlt(CAPTUREBLT)` and with `PrintWindow(PW_CLIENTONLY|PW_RENDERFULLCONTENT)`
- writes `probe-<mode>-screen.bmp` and `probe-<mode>-printwindow.bmp`
- prints PASS/FAIL for five points: video with no HTML, opaque HTML badge over the video, 50% blue over the video (expect about 128,0,255), opaque sidebar, stage margin.

Environment variables: `SC_STRESS=1` animates the hole rect; `SC_SECONDS=N` exits after N seconds; `SC_OUT_DIR` sets where the BMPs go. In parent or child modes, `WGPU_DX12_PRESENTATION_SYSTEM=Visual` switches wgpu to its own DirectComposition swapchain.

### What ran
| command | result |
|---|---|
| `rustup target add x86_64-pc-windows-msvc` | installed |
| `cargo check --target x86_64-pc-windows-msvc` | clean: 0 errors, 0 warnings; the Windows module was compiled |
| `cargo clippy --target x86_64-pc-windows-msvc` | clean after one collapsible-if fix |
| `cargo tree -i windows@0.62.2` | one `windows` crate shared with tao, wry, webview2-com and wgpu-hal |
| `cargo build` + run `parent` on macOS (`SC_STRESS=1`, 7 s) | Metal / Apple M3 Pro; `[fps]` 131–149; 120 rect messages and 434 frames presented by the probe point. Shows the IPC → rect → surface flow works. Not visually checked; the fps figure is not a performance result. |

Linking for Windows is not possible on this machine, and nothing was composited on Windows.

## Fallbacks, in order
1. **`parent` or `child-below`.** Same "HTML over video" result if layer 2 misbehaves. `parent` is the layout the wry maintainer fixed for #1331 and costs a full-window swapchain.
2. **`child-above` (C-above).** Always composites, because it is a plain child window. But HTML cannot draw over the video: crop handles, selection boxes and caption editing would have to be drawn by wgpu or by native windows. Input in that rectangle goes to the native child, and routing it to the webview is untested.
3. **Spike-2 mode B (URI-pull + WebGL2)** on every OS. Measured on macOS only: 58.8 fps at 1080 and 59.5 fps at 540 (of 60). Not measured on WebView2.

## Must be tested on real Windows hardware
- **Each mode on Windows 11 and Windows 10 22H2.** dcomp, child-below and parent should print verdict=PASS; child-above should PASS with magenta under the badge and strip. Check the window and the BMPs by eye too.
- **`SC_STRESS=1` and live resize:** maximize, minimize/restore, snap, and drag between monitors with different DPI (100/150/200%). Watch for lag or gaps between the hole and the video.
- **Frame pacing:** `[fps]` on 60 Hz and high-refresh displays.
- **CPU:** app plus msedgewebview2 processes, compared with mode B on the same machine.
- **GPU matrix:** Intel, AMD, NVIDIA, and hybrid laptops.
- **Degraded paths:** RDP, a VM without a GPU, WebView2 with hardware acceleration off, HDR on.
- **Capture:** our own Windows Graphics Capture recorder and the Snipping Tool should capture the composited preview. This also tells us whether the BitBlt probe can be trusted, since a hardware overlay plane (MPO) might hide the video from GDI capture.
- **Record the WebView2 runtime version** used for the tests.

## Risks
- The central claim rests on Microsoft's layer docs and wry #1331, not on a Windows observation. The docs also say "child windows may clip the visual tree", which leaves room for a surprise.
- **Two compositor commits** (ours and the WebView2 GPU process's) mean a frame or so of drift during layout changes is likely.
- **Single slot.** Each window allows only one non-topmost DirectComposition target. If a future wry or Tauri claims it, or switches to CompositionController, this breaks. Re-check on every upgrade.
- **Probe accuracy.** A hardware overlay plane can make the BitBlt probe report a false FAIL.

## Linux
Not covered here. wry only has `examples/gtk_opengl.rs` (a GtkOverlay with a GLArea), and wgpu cannot target a GtkGLArea directly. Linux needs its own spike before the owner's condition is met.

## Decision input for ADR-007
- Change the Windows line from "start with webview frames; native needs composition hosting" to "native via a non-topmost DirectComposition visual under the transparent windowed WebView2, pending the hardware checks above; mode B stays the fallback".
- Keep native preview conditional until both the Windows hardware run and a Linux spike pass.

---

## Independent verification (2026-11-19)

An adversarial verifier re-read the sources and re-ran the builds and measurements. Verdict: **partially confirmed**. Where this section and the text above disagree, this section wins.

### Discrepancies found

- The documented hardware test 'parent/child modes with WGPU_DX12_PRESENTATION_SYSTEM=Visual switches wgpu to its own DirectComposition swapchain' does not work as written. src/main.rs builds the instance with `..wgpu::InstanceDescriptor::new_without_display_handle()`, which sets `backend_options: Default::default()` (wgpu-types-30.0.2/src/instance.rs:67-75). The env var is only read through `Dx12SwapchainKind::from_env/with_env` (backend.rs:701-722) or `new_without_display_handle_from_env()` (instance.rs:87-89), and the spike calls neither. wgpu-hal takes `desc.backend_options.dx12.presentation_system` as given (dx12/instance.rs:118, 142-148). On Windows that run would quietly test DxgiFromHwnd again.
- The Microsoft doc is quoted selectively. The investigator cites 'child windows may clip the visual tree' only as a small risk, but the very next sentence reads 'The topmost parameter determines whether child windows clip the visual tree.' Read literally, a topmost=FALSE tree is the one child windows clip. The wry container HWND covers the whole client area, has no background brush, and its pixels sit in the parent's redirection surface, so the dcomp visual could be clipped completely and the hole would show tao's GDI fill. The doc is ambiguous rather than supportive, so 'very likely yes' and 'the stacking is the same as macOS' overstate the evidence.
- 'ADR-007's line ... is wrong' is not established. At most it may be wrong, pending a Windows run. Nothing has been composited on Windows.
- wry #1331 is evidence for layer 1 (a swapchain on the parent HWND with WS_CLIPCHILDREN off), not for layer 2 (the non-topmost DComp tree). The fix commit fb7ef9f only adds `with_clip_children(false)` to examples/wgpu.rs (6 lines), and no user ever confirmed it works. It is weak support for the recommended 'dcomp' mode.
- 'parent mode = the wry-example layout' is only approximate. The wry example also sets the winit window to `attributes.transparent = true` and uses `build_as_child` with 200x200 bounds (examples/wgpu.rs:161-190). The spike uses an opaque tao window and a full-window `build()`.
- 'The Windows-specific code is about 100 lines' is an unmeasured estimate. win.rs is 288 lines, including the probe.
- Avoiding `.transparent(true)` because of softbuffer is a preference, not a proven blocker. softbuffer paints layer 1, which sits below the DComp tree anyway. The source claim itself (tauri-runtime-wry lib.rs:4008-4022, 4143-4148) is accurate.
- Not re-verified by me: the substance of the construct.net blog beyond the WebFetch summary (it composited a DComp visual OVER WebView2 with premultiplied alpha; topmost is not stated), and the internals of FabianLars/tauri-v2-wgpu (confirmed only that it is archived and is a port of the wry wgpu example).

### Corrected summary

Question: can the macOS native preview design (wgpu under a transparent webview, HTML on top) work on Windows with Tauri v2 / wry 0.57.3 / WebView2?

Verdict: plausible but unproven. Nothing has been composited on Windows. Treat it as a hypothesis with three candidate layouts for one hardware run to decide, not as "very likely".

What the sources support (all re-read):
- WebView2 with a DefaultBackgroundColor alpha of 0 'will render hosting app content as the background'. Only alpha 0 or 255 is allowed. WEBVIEW2_DEFAULT_BACKGROUND_COLOR avoids the white flash. (MS ICoreWebView2Controller2 docs)
- wry 0.57.3 uses only the windowed controller. It creates a WS_CHILD|WS_CLIPCHILDREN container HWND with no background brush and passes alpha 0 when transparent or when the background colour's alpha is 0. It never uses CompositionController. A Microsoft maintainer confirms Tauri's windowed child-HWND structure (WebView2Feedback #4019).
- tao 0.37.3 never sets WS_CLIPCHILDREN, and its WM_ERASEBKGND fills with an RGB-only GDI brush. Tauri's background_color(Color(r,g,b,0)) sets both the window and the webview, and hwnd() is public. transparent(true) turns on softbuffer painting.
- No crate in the stack creates a DirectComposition target, so the window's non-topmost slot is free. wgpu 30 can render into a caller-owned IDCompositionVisual. The caller must Commit after configure, and wgpu's own DxgiFromVisual uses CreateTargetForHwnd(hwnd, FALSE).
- MS CreateTargetForHwnd docs give the four layers (HDC/Present content, non-topmost tree, child windows, topmost tree). They also say window clipping rules apply and 'The topmost parameter determines whether child windows clip the visual tree'. Read literally, that means a topmost=FALSE visual may be clipped by the full-client wry container HWND. So it is open whether the 'dcomp' layout shows through at all.
- Upstream precedent is thin. wry's examples/wgpu.rs draws on the parent HWND (layer 1) under a transparent child webview, with winit transparent=true and clip_children(false). wry #1331 was closed by commit fb7ef9f, which only added clip_children(false) to the example, and no user confirmed the fix. This supports the 'parent' layout at least as much as 'dcomp'.

Prototype (spikes/05-native-preview-windows, versions identical to spike 2's lock):
- Four modes: dcomp, child-below, parent, child-above. A Windows-only BitBlt/PrintWindow pixel probe checks 5 points.
- Re-verified: cargo check and clippy for x86_64-pc-windows-msvc are clean (0 warnings). The host clippy and build are clean. A single windows 0.62.2 crate is shared across the stack.
- macOS parent-mode run: Metal / Apple M3 Pro, [fps] 139-144, 132 rect updates and 424 frames at the probe point. This shows the IPC → rect → surface plumbing works. It is not visually verified and not a performance figure; fps above 120 under Fifo is unexplained.
- Linking for Windows was not possible.
- Known bug in the test plan: the WGPU_DX12_PRESENTATION_SYSTEM=Visual variant is ineffective. The spike builds InstanceDescriptor::new_without_display_handle() without with_env(), so before that test is meaningful the spike must use new_without_display_handle_from_env() or set backend_options.dx12.presentation_system.

Recommended next step: run all four modes on Windows 11 and Windows 10 and let the probe, plus eyeballing the window, pick between dcomp, parent and child-below. Do not pre-commit to dcomp. Then run the stress, resize and DPI, GPU-vendor, RDP/VM/no-GPU, and capture (WGC / Snipping Tool, which also tests whether the probe is affected by MPO) checks, and record the WebView2 runtime version.

Fallbacks:
- child-above (C-above): HTML cannot draw over the video, and input routing is untested.
- Spike-2 mode B URI-pull + WebGL2: measured on macOS only, 58.8 fps at 1080 and 59.5 fps at 540; not measured on WebView2.

Do not change ADR-007's 'needs composition hosting' line until the hardware run passes. Linux is not covered and needs its own spike before the owner's Windows-and-Linux condition can be met.
