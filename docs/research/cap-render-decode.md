# Cap research: decoding, rendering, and getting the preview on screen

Scope: how Cap (CapSoftware/Cap @880ec89, read-only clone in `reference/Cap`) decodes recorded tracks for the editor, turns them into GPU textures, converts YUV to RGB, composites the camera overlay and text, and puts the editor preview on screen in its GPUI desktop app. The last section turns this into lessons for our Tauri-vs-GPUI decision (SPEC "Tech stack", "Architecture → Preview transport").

Method: I read the source and READMEs. No git history is available (depth-1 clone), so everything about why Cap made a choice comes from code comments and `apps/desktop-gpui/README.md`. I ran no benchmarks for this note. The only performance numbers below are Cap's own published figures, attributed to them, from their hardware (unspecified). None of them are from this machine (M3 Pro), and none says anything about a base M1 8 GB.

Licensing: everything described here is AGPL-3.0 (see the license table at the end). This note describes ideas in my own words and cites file paths. It contains no code from those crates, and nothing was adapted, so no `docs/THIRD_PARTY.md` entry is needed.

---

## 1. Big picture

Cap's editor has four stages, and they are the same for preview and export:

1. **Decode** (`crates/video-decode`, wrapped by `crates/rendering/src/decoder/`). One async "decoder handle" per track (screen, camera), each backed by a dedicated OS thread that owns the native decoder and a frame cache. Callers ask for "the frame on screen at time t" and get a `DecodedFrame` back over a oneshot channel.
2. **Layer prepare** (`crates/rendering/src/layers/*.rs`). Each layer turns its decoded frame into an RGBA wgpu texture: zero-copy import where possible, then a compute-shader YUV→RGBA conversion. If the frame's storage is the same as last time, the upload is skipped.
3. **Composite** (`crates/rendering/src/lib.rs`: `ProjectUniforms`, `RendererLayers::render`). One render pass per layer, drawn in order onto ping-pong `Rgba8Unorm` targets: background, display (screen), cursor, camera, then overlay tracks (images, text, shapes, masks), keyboard, and captions. All placement math happens on the CPU in `ProjectUniforms`. The shaders only receive rectangles, radii, and flags.
4. **Output** (`crates/rendering/src/frame_pipeline.rs`). Three ways out: (a) macOS preview: blit into a ring of BGRA IOSurface-backed `CVPixelBuffer`s for the UI to display with no copy; (b) every other preview: pipelined GPU→CPU readback of padded RGBA rows; (c) export: RGBA→NV12 compute pass, then readback, or on macOS an NV12 IOSurface handed straight to VideoToolbox.

The renderer owns its own `wgpu::Device` (wgpu 25 on a vendored `wgpu-hal`). Moving the shell from Tauri to GPUI did not change this: `apps/desktop-gpui/README.md` ("The editor stack") says GPUI on macOS draws through Metal directly and exposes no wgpu device, so the app has two GPU contexts that share pixels through IOSurfaces. The Tauri app had the same two-context shape.

---

## 2. Decode strategy per OS

Selection lives in `crates/rendering/src/decoder/mod.rs` (`spawn_decoder`). Each attempt gets a 30 s initialization timeout before the code falls back.

| OS | Primary | Fallback(s) | Notes |
|---|---|---|---|
| macOS | AVAssetReader (`video-decode/src/avassetreader.rs`, pool wrapper `rendering/src/decoder/avassetreader.rs`) | FFmpeg with the VideoToolbox hwaccel, then FFmpeg software | FFmpeg is also used directly for segmented/fragmented recordings (a directory of fragments) and for sources whose chroma is not 4:2:0. A runtime watchdog swaps a running AVAssetReader for FFmpeg if it misses three playback rounds in a row without producing a frame. |
| Windows | Media Foundation Source Reader (`video-decode/src/media_foundation.rs`, wrapper `rendering/src/decoder/media_foundation.rs`) | FFmpeg with D3D11VA, then software | The MF reader is created with a DXGI device manager, hardware transforms enabled and advanced video processing on, and asked for NV12. A capability probe checks for D3D11 H.264/HEVC decoder profiles and maximum dimensions before choosing hardware. |
| Linux | FFmpeg software (`video-decode/src/ffmpeg.rs`) | none | Hardware decode is CUDA/NVDEC only, enabled by an environment variable and aimed at their render-farm workers (`rendering/src/linux_gpu.rs`). There is **no VAAPI decode path.** Software threading is 4 threads, rising to 8 only for 4K and larger, because more frame threads made seeks slower on their measurements (comment in `configure_software_threading`). |

Details worth taking:

- **AVAssetReader cannot seek in place.** A seek cancels the reader and builds a new one with a time range that starts at the target. Cap starts that range *exactly* on the keyframe's stream tick. A comment in `video-decode/src/avassetreader.rs` explains why: a start rounded to just below a keyframe makes the reader decode the whole previous GOP and hand back its last frame. The track output is configured not to copy sample data, so frames arrive as IOSurface-backed `CVPixelBuffer`s.
- **The keyframe index comes from the container, not from decoding.** `KeyframeIndex::build` parses the MP4 `moov` sync-sample table when that is safe: no composition offsets (`ctts`), and not a fragmented file. Otherwise it scans packets with libavformat. The index also caches fps, duration, and dimensions, so opening a decoder does not have to probe the file again. Note that even the macOS path links FFmpeg (libavformat) for this probing.
- **Pixel format requested from AVAssetReader is always `420v`** (bi-planar, video range). Any source whose chroma is not 4:2:0 is routed to FFmpeg so no chroma is lost.
- **Windows output-surface lifetime.** MF decodes into a texture-array slice and reuses that slice as soon as the sample is released. So the decoder first copies the slice into its own texture (`CopySubresourceRegion`). In the code path the editor actually uses, it then reads back to the CPU through a pipeline of staging textures: the copy for sample N+1 is queued while sample N is being mapped (`read_nv12`, `CpuReadbackPool`). See §4 for the zero-copy path that exists alongside it.

### Frame request semantics (the subtle part)

`rendering/src/decoder/mod.rs` defines which sample answers a request at time *t*:

- The request is mapped to a frame index, floor(t·fps), with a little float slack so that f32 times computed from frame numbers do not round down to the previous frame.
- If *t* lands past the middle of a frame cell, the handle also asks for the next index and waits up to 250 ms for it, because a VFR sample filed under the next index may already have been on screen. Exports wait without a limit, so their output never depends on machine load. Every consumer (editor, preview, export) goes through this one function, so they all pick the same sample.
- **VFR "pts holes"** (stretches with no samples, which are common in screen capture because ScreenCaptureKit only delivers frames when something changes) are recorded the first time the decoder steps over one. A request that falls inside a hole is answered with the frame before it (the "held" frame), and that held frame is kept even after it leaves the cache.

This matches SPEC pitfall #1 (screen capture is VFR). Our playback engine needs the same "newest sample at or before t, with hole tracking" rule from day one.

## 3. Frame caching and prefetch

macOS reader pool (`rendering/src/decoder/avassetreader.rs`, `decoder/multi_position.rs`):

- Per-track frame cache: up to 90 frames **and** 256 MiB of decoded data. Without the byte cap, high-resolution frames would pin more than 1 GiB. During sequential playback it drops to 8 frames, because a linear reader never revisits older frames and each kept frame pins a decoder IOSurface (about 9 MB for a 3K screen, per the comment).
- **Multi-reader pool for scrubbing.** One reader at first. When a non-sequential request would force an active reader to reposition, more readers are created on demand, up to 5 (7 for videos of 10 minutes or more, 8 for 30 minutes or more). A reader is reused if it sits within a threshold behind the target (5–10 s, scaled with duration, and 0.5 s while scrubbing). A scrub detector treats jumps of more than 5 frames within 100 ms as scrubbing. A process-wide semaphore caps native readers at 16, because AVAssetReader starts decoding eagerly. Idle readers keep their state for 5 s, or less when another track is waiting for a slot.
- Health tracking recreates a reader after repeated empty reads or errors.

Playback prefetch (`crates/editor/src/playback.rs`): the prefetch buffer starts at 10 frames and grows to 90, limited to 128 MiB (192 MiB in a temporary "boost"), fed by 4 parallel decode tasks, with a small render-side cache of 4 frames / 64 MiB.

Memory reality check, from Cap's own README (their machine, not ours): an editor that was cycling play/pause/seek reached about 1.39 GB RSS and settled at about 1.1 GB; a paused editor sat at 345 MB. That is a warning for our 8 GB M1 target. We should keep decoded frames as IOSurface-backed buffers rather than CPU copies, cap every cache by bytes, and set smaller defaults than Cap's.

## 4. Zero-copy texture import

### macOS: IOSurface → Metal → wgpu (`rendering/src/iosurface_texture.rs`)

- `DecodedFrame` can hold the decoder's `CVPixelBuffer` itself instead of bytes (`rendering/src/decoder/mod.rs`). CPU bytes are made only if some CPU consumer asks for them. When that happens, the IOSurface is released so the cache does not account for the frame twice.
- Import makes **one Metal texture per plane** directly from the IOSurface with Metal's "new texture from IOSurface, plane N" call: Y as `R8Unorm` at full size, CbCr as `Rg8Unorm` at half size. It does not go through `CVMetalTextureCache`. The Metal texture is then wrapped as a wgpu texture through `wgpu-hal`'s Metal `texture_from_raw` and `create_texture_from_hal`. This needs `wgpu-hal`/`wgpu-core` as direct dependencies and unsafe code, and it ties you to a specific wgpu version.
- **Lifetime hazard:** the Metal textures alias the IOSurface without retaining the pixel buffer, and the decoder's pool recycles buffers once they are released. So the YUV converter holds each source buffer until enough later conversions have been submitted that the GPU work reading it must have finished. This covers up to three earlier sources, because a transition frame converts twice (`RetainedImageBuf` in `rendering/src/yuv_converter.rs`; regression test `crates/rendering/tests/iosurface-source-lifetime.rs`). We need the same rule, or we will see frames from a later moment in the video.
- Output works the same way in reverse: `RgbaToBgraSurfaceConverter` (`frame_pipeline.rs`) keeps a ring of 8–12 BGRA IOSurfaces imported as wgpu render targets. Export does the same with NV12 IOSurfaces for VideoToolbox.

### Windows: D3D11 shared texture → D3D12 → wgpu (`rendering/src/d3d_texture.rs`)

- wgpu runs on DX12. The import opens an NT shared handle on the D3D12 device (`OpenSharedHandle`) and wraps the resulting `ID3D12Resource` with `wgpu-hal`'s DX12 `texture_from_raw`. Textures meant for sharing are created with `D3D11_RESOURCE_MISC_SHARED_NTHANDLE`. `adapter_luid.rs` makes sure both APIs are on the same adapter.
- NV12 is **split into separate R8 (Y) and RG8 (UV) textures by a small D3D11 compute shader** before sharing (`Nv12PlaneConverter` in `video-decode/src/media_foundation.rs`). wgpu then imports two simple single-plane formats rather than a multi-planar NV12 resource.
- Caveat from reading the code: at this commit the MF decoder's own textures are created without the shared-handle flag, and the editor's MF thread calls the CPU-readback method (`read_nv12`), not the texture method. I found no call site that builds a D3D11-backed `DecodedFrame`. So the D3D11→wgpu import exists and the display/camera layers check for it, but the current Windows editor seems to use CPU NV12 upload. I can't tell from a shallow clone whether this was a deliberate retreat (for stability, for example) or a path not yet wired up. Either way, it is a hint that cross-API sharing on Windows is the hard part.

### Linux

CPU upload. For CUDA (render-farm only), NVDEC frames are copied device-to-device into a Vulkan buffer shared with CUDA and then into the plane textures, so nothing touches system memory. No dmabuf/VAAPI path.

## 5. YUV→RGB conversion and color

- **GPU:** compute shaders `nv12_to_rgba.wgsl` and `yuv420p_to_rgba.wgsl` write an RGBA storage texture. The matrix coefficients are pipeline-overridable constants, so there is one compiled pipeline per matrix, built lazily (`MatrixPipelines` in `yuv_converter.rs`).
- **Matrices supported: BT.601 and BT.709 only, limited (video) range only.** A source tagged BT.709 uses 709. Everything else, including untagged and BT.2020, falls back to 601 (`YuvMatrix::from_color_space` in `cpu_yuv.rs`). There is no full-range variant in the GPU shaders. That is consistent with asking AVAssetReader for `420v`, but a full-range `420f` source going through another path would be slightly off. There is no HDR/10-bit handling.
- **CPU reference:** `cpu_yuv.rs` has fixed-point limited-range conversion (with an AVX2 kernel) whose coefficients are shared with the SIMD path so they cannot drift. `yuv_converter.rs` tests check the GPU shaders against it, and check that the export RGBA→NV12 shader produces BT.709 limited range (the matrix the encoders tag). This is the "CPU reference renderer for golden tests" idea from our SPEC, applied to color.
- **Compositing color space:** all intermediate targets are `Rgba8Unorm` (not `*Srgb`), so blending and the SDF edge anti-aliasing happen on gamma-encoded values. That is a valid choice, but it has to be made the same way in the GPU compositor, the CPU reference, and Skia text, or golden tests will fail on edges.
- Separately, `crates/frame-converter` (VideoToolbox `VTPixelTransferSession`, D3D11 `VideoProcessorBlt`, swscale fallback) is a **recording-side** pixel-format converter used by `crates/recording` and `crates/enc-ffmpeg`, not by the editor. `crates/gpu-converters` (wgpu compute converters for NV12/UYVY/YUYV/BGRA) has **no dependents** in the workspace at this commit.

Lessons for us: tag our recordings explicitly (BT.709, video range), pass both matrix *and range* to the shader as uniforms, convert to RGBA once per new source frame (not once per output frame), and decide at the start whether compositing is linear or gamma-space.

## 6. Camera overlay: geometry and drawing

Geometry is computed on the CPU in `ProjectUniforms` (`rendering/src/lib.rs`, the camera block that builds `CompositeVideoFrameUniforms`; `compute_camera_position`). The model fields come from `crates/project/src/configuration.rs` (`Camera`, `CameraShape`, `CornerStyle`).

- **Size:** a percentage of the output's shorter side, plus a fixed padding term scaled from a 1080p reference. While a screen zoom is active it shrinks by `scale_during_zoom`.
- **Shapes:** only *Square* (centre-crop of the camera frame to 1:1) and *Source* (the camera's native aspect). There is no separate circle: a circle is Square with rounding at 100 %. There are no 4:3/16:9/9:16 presets, and no zoom/pan reframe inside the shape. A generalized "aspect-fill crop, then zoom, then re-centre on a focal point, clamped inside the source" helper (`fit_crop_to_target`) exists for other layers and is the shape of what our Reframe feature needs.
- **Position:** corner/edge presets (left/centre/right × top/bottom) with padding, or a normalized manual centre, clamped so the whole rectangle stays in frame. Final bounds are snapped to whole output pixels. The source crop is inset by 2 px to hide edge artifacts from the camera.
- **Rounding:** radius = rounding% × half the shorter side of the target rectangle. Two corner styles: an ordinary circular-arc rounded rectangle, or a **squircle** (a superellipse norm with power 4 in the shader). There are also per-corner multipliers.
- **Mirroring in the editor/export** is a shader flag that flips horizontal sampling. Cap never mirrors the recorded camera track itself. The live preview bubble mirrors separately (see §8).
- **Shadow:** computed analytically in the same fragment shader from the rounded-rectangle signed distance field: a smoothstep falloff whose size, blur, and opacity are fractions of the shorter side of the card. It is centred, with no x/y offset, and needs no separate blur pass. A border (an outer SDF band) is supported by the shader but is turned off for the camera.
- **Drawing:** the camera goes through the same `composite-video-frame.wgsl` pipeline as the screen. Each layer is drawn as a full-screen triangle, and pixels outside the card return early with just the shadow color. The shader also carries color grading, motion blur (directional and zoom), and an "enhance" path. The uniforms bundle crop rectangle, target rectangle, radii, style, mirror, shadow, border, opacity, and grading into one struct.
- `FrameLayout` (lib.rs) reports the final placed rectangles of the display and camera back to the UI after each frame, so on-canvas drag/resize handles stay attached to exactly what was drawn.

Compared with our SPEC, Cap is missing: rectangle aspect presets, reframe (zoom/pan inside the shape), a camera border, and timed keyframes of camera layout. Our "one pure geometry module" (SPEC Architecture) should own all of these and output exactly the kind of struct Cap passes in its uniforms: source crop rectangle, destination rectangle, per-corner radius, corner style, mirror, shadow, border, opacity.

## 7. Text: what renders it and how it is composited

**Not Skia.** Cap text is drawn with **glyphon 0.9** (cosmic-text shaping with rustybuzz, swash rasterization, a glyph atlas inside wgpu), straight into the frame's render pass (`rendering/src/layers/text.rs`, `rendering/src/text.rs`; captions use the same stack in `layers/captions.rs`). `crates/rendering-skia` is a Skia experiment whose own README says "Phase 1" (context and surfaces) is done and layer migration has not started. Nothing in the workspace depends on it. resvg/tiny-skia are used for SVG and still images, not for text.

How it works:

- **Fonts:** system fonts are scanned once per process, and the font database is cloned for each `FontSystem` (`layers/mod.rs::new_font_system`). Cap bundles no fonts. The generic families are **pinned to whatever the old editor webview resolved them to** (Helvetica/Times New Roman/Courier New on macOS, Arial/Times New Roman/Consolas on Windows, DejaVu on Linux). The comment explains why: otherwise cosmic-text quietly falls back to a different face than the one the webview measured.
- **Wrap slack:** text is shaped with a little extra width beyond the box measured in the editor, because the webview and cosmic-text disagree by a few pixels per line, and a line that fit in the editor would otherwise wrap in the render (comment in `layers/text.rs`). **This is a direct cost of measuring text in one engine and rendering it in another,** which is what our SPEC's "HTML input over the text" idea would also do.
- **Resolution independence:** sizes are authored against a 1080-px-tall reference and scaled to the output height, so glyphs are rasterized at output resolution (crisp at any export size).
- **Effects:** outline, glow, and drop shadow are made by **drawing the same shaped buffer several times**. Outline uses concentric rings of offset copies dense enough to merge into a solid stroke. Glow uses fading rings in the text color. Shadow is one offset copy. Each pass is re-tinted through the area's default color. A horizontal gradient is done with per-glyph color spans. This approach is cheap to build but only approximates a real stroke, and each ring costs another draw.
- **Backgrounds:** a separate SDF shader (`shaders/text_bg.wgsl`) draws a rounded box, a pill, or a per-line "highlight" stroke that hugs each line's ink extent. There is **no TikTok-style merged multi-line background** (one shape joining the per-line boxes).
- **Animation:** done per frame on the CPU as transforms of the buffer: scale about the box centre, translation, per-word or per-letter alpha stagger, typewriter (character truncation), wipe (clip width), and tracking (extra letter spacing). **There is no rotation.**
- **Caching:** shaped buffers are reused across frames when the shaping inputs are unchanged, since shaping with fallback dominates the per-frame cost.
- **Compositing:** text layers are drawn after video/camera into the same `Rgba8Unorm` target, ordered by overlay track.

What this means for our Skia plan: Cap's feature list (`TextSegment` in `crates/project/src/configuration.rs`) is close to ours, but it reaches it with a glyph-atlas renderer and redraw tricks. Our SPEC needs true strokes, merged line backgrounds, rotation, and per-glyph/word/line boxes for animation. Those favour Skia (`skia-safe`) rasterizing each text item to a cached texture (or a GPU Skia surface) that the wgpu compositor then transforms. Cap's experience also says: **use one text engine for both editing and rendering**, or be ready to fight measurement drift.

## 8. The preview path in the GPUI app

Source: `apps/desktop-gpui/README.md` ("The editor stack", "The editor window", "Playback", "Camera preview") and `apps/desktop-gpui/src/editor_window.rs`.

- **Renderer side:** `cap-editor`'s `EditorInstance` runs on a tokio runtime (`gpui_tokio`). The UI never calls render. It writes the latest wanted (frame, fps, resolution) into a `watch` channel, and the preview renderer produces frames through a callback. Both the frame channel and the playhead channel are **latest-wins** (a bounded channel plus an atomic), so a burst of updates collapses to the newest one instead of building a backlog. The transport (play/pause/seek) is stored as desired state that a driver task reconciles, not as a queue of commands.
- **macOS:** the renderer blits its RGBA output into a **BGRA IOSurface `CVPixelBuffer` ring**, and GPUI paints it with `paint_surface` / `paint_surface_fitted`. Those are GPUI's own surface primitive, patched in Cap's fork to accept BGRA (upstream asserted `420f`) and to clip with corner radii. Frames stay on the GPU the whole way and never touch GPUI's sprite atlas. `crates/rendering/examples/preview-surface-benchmark.rs` says this replaced an earlier NV12 IOSurface handoff, where GPUI sampled two planes and converted the color itself.
- **Windows/Linux:** CPU readback, then a CPU pass that removes the 256-byte row padding and swaps R and B (in `cap-gpui-kernels`, always built at opt-level 3), then a GPUI `RenderImage` through the sprite atlas, replacing the previous frame each time. Cap's README measured this at about 0.93 ms per 1080×702 frame, with 59.9 fps and zero dropped frames over a 16 s run, and concluded zero-copy was "not worth doing for playback yet" at that size. These are their numbers on their (unspecified) machine. The same per-pixel loop built at opt-level 0 took about 30 ms and cut playback to 33 fps.
- **Render size:** a fixed "resolution base" (default 1248×702, i.e. 1920×1080 at 65 %, "half" quality), not the window size. After 150 ms of no change (pause, end of scrub, resize), the same frame is rendered again at the canvas's device-pixel size so a Retina display looks sharp.
- **Live camera bubble** (recording, not editor): `420v` sample buffers are converted to BGRA by VideoToolbox into a 4-buffer IOSurface ring and painted with `paint_surface_fitted`: cover-fit by source-UV crop, circular clip by corner radius. Mirroring is a second hardware pass (`VTPixelRotationSession` horizontal flip). Background blur runs in its own wgpu device and comes back as a BGRA IOSurface. Their benchmark: 137 µs vs 196 µs per 720p frame for the conversion.

### Why Cap moved from Tauri to GPUI (as far as the repo says)

There is no design document. What the repo does say:

- The README's first line: no Tauri, no webview, "every pixel is Rust". The GPUI app is a 1:1 port of the Tauri/Solid app (last Tauri release 0.6.0; source removed after `cap-v0.6.2`) and still uses the Tauri *bundler and updater artifacts*, so existing installs update into it.
- The Tauri editor received frames over a **websocket** from Rust (`WSFrame` repacking in the deleted `apps/desktop/src-tauri/src/editor_window.rs`). It needed a "prewarm" that started decoders in parallel with building the webview. In GPUI the same frames become a native surface (macOS) or one atlas upload.
- Text overlays were measured in the webview and rendered by cosmic-text, which needed the font pinning and wrap slack described in §7.
- I found no explicit statement of the reason for the move. My reading of the evidence: the main motivations were preview latency and copying (websocket → zero-copy surface), one rendering model for UI and pixels, and native window control (panels, levels, glass). That last part is an inference, not a quote.

### What GPUI cost them (also from the README)

- A git-pinned fork of a fork of Zed's gpui (`wingleeio/zed` @5d1f83d), plus **11 patches totalling about 12.7k lines** (`apps/desktop-gpui/patches/`): streaming images, surface fixes, glass, Windows, Linux/Wayland, idle frames, device-lost, perf. It is a separate Cargo workspace because gpui's dependency tree conflicts with the root's. It also needs Rust 1.89 or newer.
- First build about 11 min and 6.4 GB of `target/`. The app crate is built at opt-level 0, so pixel loops have to live in a separate crate built at opt-level 3.
- Every widget had to be rebuilt by hand: text input with IME/undo, menus, sliders, selects, tooltips. Their rev has no transforms, masks, backdrop blur, inset shadows, or animation hooks, and some UI looks differ as a result.
- Platform traps: on macOS 26 gpui never started a window's display link because of a new occlusion-state bit, so every window froze after its first paint until they shimmed it. Touching AppKit inside a gpui update re-enters gpui and silently drops callbacks.

---

## 9. Lessons for our Tauri-vs-GPUI decision

1. **The shell choice does not touch the renderer.** Cap's decode/render/export crates are UI-agnostic, and their wgpu device is separate from the UI's under both shells. We should design the same way (SPEC already does), so a later switch only replaces the presentation layer.
2. **The real question is the preview transport, and Cap shows the target:** on macOS, render into an IOSurface ring and have the window composite it without a copy. Under Tauri we can do the equivalent ourselves: attach a native `CALayer`/`CAMetalLayer` (or an `NSView` whose layer `contents` is the IOSurface) next to the webview in the same `NSWindow`, with the webview transparent over the preview area. That is a spike to run (it fits "native child surface" in SPEC). Cap did not do it. Their Tauri app used a websocket.
3. **For Windows/Linux, a CPU handoff at preview size is acceptable.** By Cap's own figure, unpad plus swizzle at about 1 MP costs under 1 ms when optimized. For Tauri that means a binary IPC/channel or shared memory into a `<canvas>`/WebGL texture at the preview's on-screen size, which the SPEC already says. Our spike must measure the webview side (upload + paint), which Cap's GPUI number does not include. Build every pixel loop at opt-level 3 even in dev, or the measurements will be misleading.
4. **Text editing in a webview is a known source of drift.** If we keep Tauri, the inline editor should not let HTML lay out visible text. Use an invisible input only for IME/caret events, and draw the caret, selection, and glyphs from Skia's layout boxes. Load the same bundled font files in both places, and do not rely on CSS generic families.
5. **GPUI's cost is real for a small project:** forked framework, large patch set, long builds, hand-built widgets, and macOS-version breakage. Its benefit (zero-copy preview, native panels) can mostly be had in Tauri with targeted native code. So SPEC's rule stands: stay on Tauri unless the preview spike misses its targets. If it does, Cap's GPUI fork and patches are evidence it can work, but we would inherit their maintenance load.
6. **The things that actually decide smoothness are shell-independent** and we should copy the *ideas*: hold IOSurface-backed frames rather than bytes; byte-bounded caches; one newest-sample-at-or-before-t rule with VFR hole tracking shared by preview and export; AVAssetReader seeks that start exactly on a keyframe tick from an index parsed out of the container; a small pool of readers for scrubbing; latest-wins channels between engine and UI; render at a reduced preview resolution and re-render sharp when idle; keep source buffers alive until the GPU has finished with them. Avoid Cap's gaps: no Linux hardware decode (we planned VAAPI), no full-range or BT.2020 handling, and Windows zero-copy apparently not in use. Also prefer not to link FFmpeg on macOS just for probing; AVFoundation or a permissively licensed MP4 parser can provide the keyframe table. That last point is a suggestion I have not verified.

---

## License per crate/path read

| Path | License | Basis |
|---|---|---|
| `crates/video-decode` (cap-video-decode) | AGPL-3.0 | No `license` field and no per-crate LICENSE; root `LICENSE` makes everything outside `cap-camera*`/`scap-*` AGPL-3.0 |
| `crates/rendering` (cap-rendering), incl. `shaders/*.wgsl` | AGPL-3.0 | same; one shader comment mentions an MIT-licensed upstream idea, but the file itself is not marked MIT |
| `crates/rendering-skia` | AGPL-3.0 | same (unused crate) |
| `crates/gpu-converters` | AGPL-3.0 | same (no dependents) |
| `crates/frame-converter` | AGPL-3.0 | same (recording-side) |
| `crates/editor`, `crates/project` | AGPL-3.0 | same |
| `apps/desktop-gpui` | AGPL-3.0 | explicit `license = "AGPL-3.0"` in its Cargo.toml |
| gpui (Zed), as forked at `wingleeio/zed` | Apache-2.0 upstream | Not verified against the fork's files; check before depending on it |
| glyphon / cosmic-text / wgpu (third-party deps) | permissive (MIT/Apache-2.0, glyphon also Zlib) per my knowledge of crates.io metadata | Not verified locally; check with cargo-deny if adopted |

None of the crates studied here is MIT, so nothing from them may be adapted into our repo. Only the ideas described above carry over, implemented independently.
