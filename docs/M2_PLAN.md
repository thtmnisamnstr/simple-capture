# M2 plan: camera bubble on macOS

M2 ★ (SPEC milestones): camera bubble on macOS: shapes, reframing, size/position/snapping, mirror,
keyframes, stage preview, excluded from capture. The full requirements are SPEC "Camera overlay"
and the "own UI never in recordings" rules. The owner's M1 hands-on asks that land here: see the
camera while recording, and get the camera in the export.

Owner decisions are at the end (§9). Nothing here changes M1 behaviour without a camera.

---

## 1. What exists after M1

| Piece | State |
|---|---|
| `project.json` camera model | `CameraLayout { overlays: [CameraOverlay { track_id, keyframes: [CameraKey] }] }`. A key has `t_rec_ns`, `visible`, `shape` (circle, square, 4:3, 16:9, 3:4, 9:16, rounded), `corner_radius`, `center_norm`, `size_norm`, `zoom`, `pan_norm`, `mirror`, optional `border` and `shadow`, `easing`. Written empty in M1. |
| Camera track | Recorded as its own HEVC `.mov` (ADR-034), host-clock timed, offset in `project.json`. 53,975 frames, 0 dropped in the soak. |
| Camera device | `platform.camera().start(id, limit, sink)` opens an AVCaptureSession per recording (crates/record control.rs `begin`). Exact frame-rate pick and exception safety fixed in M1. |
| Compositor | `sc_render` `FrameJob { canvas, background, screen }`: background plus one screen layer, wgpu path and CPU reference path, diff tests. No camera layer. |
| Preview transport | Stills only (`sc://still/<id>.png`). The streaming path was measured in spike 2 (58.8 fps at 1080p over webview frames) but is not in the app. ADR-007: webview frames on every OS. |
| Overlays | Non-activating, excluded-from-capture panels (`OverlayRole`), exclusion checked by the driver for bar, switcher, panel and tray menu. |
| Export | Quick x264: screen segments decoded and composited per frame, audio mixed. No camera. |

## 2. Scope

In M2:
- Live camera bubble window, shown whenever a camera is selected and the recorder is open or
  recording; drag to move, handles to resize, snapping, arrow-key nudge, S/M/L and corner presets.
- Shapes (circle, square, 4:3, 16:9, 3:4, 9:16, rounded with radius up to a pill), reframe (zoom
  from fill, pan, Reset), mirror, optional border and soft shadow, hide/show.
- macOS "Video Effects" button (`AVCaptureDevice.showSystemUserInterface(.videoEffects)`).
- Moves, resizes and hides during a recording saved as keyframes and reproduced in the output.
- Stage preview: a live view of the composed output (screen in its frame + background + camera)
  for placing the camera when the screen itself can't show where it lands (§3.4).
- The compositor draws the camera; the bubble, the stage preview, the panel still and the quick
  export all use it, so what you see is what you get.
- Quick export includes the camera.
- Tests: bubble never in the capture, bubble/stage pixels match the export, keyframes reproduce,
  geometry and compositor parity.

Not in M2: editing keyframes after recording (M3 editor), text (M4), other export formats (M5),
Windows/Linux bubbles (M6), camera-only mode (nice-to-have), 2+ cameras (stretch; the model
already allows several overlays).

## 3. Design

### 3.1 Camera hub (one device session, several consumers)

Today a recording opens its own camera session, so nothing can show the camera before or during a
recording. M2 adds an app-level **camera hub**:
- One AVCaptureSession for the selected camera, opened when the bubble needs it (camera selected
  and recorder open, or recording) and closed when nothing does.
- Consumers attach and detach: the live preview (latest-wins, never blocks), and the recording's
  camera writer (attached at record start, detached at stop; same frames, same host clock).
- `Session::start` takes camera frames from the hub instead of opening the device. The record
  crate keeps a fallback that opens the device itself, so the CLI and tests are unchanged.
- Switching camera while not recording re-opens the hub; while recording it is disabled in M2
  (device change mid-recording is M7 hot-plug).

### 3.2 Layout geometry (sc-geometry, pure, mirrored in TS)

- `camera_at(keys, t) -> Option<CameraPlacement>`: step for `visible`, `shape`, `mirror`, border
  and shadow; linear or ease-in-out for centre, size, radius, zoom and pan (per the next key's
  `easing`).
- `camera_rects(placement, canvas, camera_size) -> CameraRects`: destination rect in canvas px,
  source crop in camera px (start from fill-the-shape, then zoom ≥ 1 and pan inside the frame),
  corner radius px (circle = half the short side), mirror flag, border px (from `width_px_1080`,
  ADR-033 1080 units).
- Sizes: `size_norm` is the shape's short side over the canvas short side. S/M/L = 0.18 / 0.26 /
  0.36. Snapping to corners, edges and centre with a margin of 0.03 of the short side, within
  12 px on screen. Corner presets use the same margin. All clamped inside the canvas.
- The same functions in `ui/src/lib/geometry.ts`, with one shared table of cases run by both test
  suites (as M1 did for regions and app bounds).

### 3.3 Compositor camera layer

- `FrameJob` gains `camera: Option<CameraLayer>`: frame, src crop, dst rect, radius, mirror,
  border (px, colour), shadow (strength).
- wgpu: a rounded-rect signed-distance mask with 1 px anti-aliasing, mirror by flipping the
  source x, border as a ring on the same distance field, soft shadow as a blurred distance field
  drawn first. BT.709 limited input like the screen layer.
- CPU reference path draws the same thing; GPU vs CPU diff tests per shape, mirror, border and
  shadow, as M1 does for the screen layer.
- Output with alpha for the bubble window (transparent outside the shape, shadow included).

### 3.4 Where you place the camera: bubble on screen or stage preview

The canonical position is in the **output frame** (canvas-normalized `CameraKey`). The screen can
only show it truthfully when the output frame is the display itself.
- **Whole-display source, Original frame:** the bubble floats on the screen at exactly the spot it
  will have in the video (canvas ↔ display is a scale). Dragging it on screen is the placement.
- **Display source with a letterboxed frame, or a window, app or region source:** where the bubble
  sits on screen doesn't correspond to the output, so placement happens in the **stage preview**:
  a small floating window (excluded from capture) showing the live composed output, where the
  camera is dragged, resized and reframed. Owner question Q1 decides whether a floating bubble is
  also shown on screen in this case.
- Stage preview frames come from a low-rate preview of the selected source before recording (SCK,
  small size, about 15 fps) and from the recording stream during recording, downscaled on the GPU;
  plus the camera hub. Latest-wins, never blocks recording.

### 3.5 Bubble and stage windows

- Two new overlay roles: `Bubble` (transparent, borderless, floating level, all Spaces, non-
  activating, excluded) and `Stage` (same, opaque, resizable). Registered like the M1 overlays, so
  they never appear in recordings; covered by the driver's exclusion flow.
- Frames: the compositor renders at the window's device-pixel size into RGBA; frames reach the
  webview over a binary Tauri channel, latest-wins, drawn into a `<canvas>`. About 0.5 MB per
  frame for a 360 px bubble at 2×, 30 fps. CPU budget for bubble plus stage: under 15% of one core
  on the build Mac (measured in the package).
- Interaction in the web layer: drag to move, corner handles to resize (aspect locked to the
  shape), scroll/pinch to zoom, ⌥-drag to pan, Reset; hover toolbar with shape, S/M/L, mirror,
  hide; arrow keys nudge (1 px, ⇧ 10 px) once the bubble is clicked (it becomes key without
  activating the app). Snapping guides while dragging.
- The panel's Camera section: device, shape, radius slider, size presets, corner presets, mirror,
  border, shadow, Video Effects, Reset reframe, show/hide bubble.

### 3.6 Keyframes while recording

- Every change during a recording is sent to the session as a `CameraKey` stamped with the
  current recording time; drags are sampled at up to 30 Hz with linear easing, so the output
  replays the motion; hide/show and shape changes are step keys.
- Each key is appended to the journal (`camera_key` event, fsync batched like other events) so a
  crash keeps them; finalize and recovery write them to `camera_layout`.
- The key in effect at record start is the first key (t = 0), from the recorder settings.

### 3.7 Export and stills

- Quick export decodes the camera track alongside the screen segments, at RecTime through the
  track offset, holding the last frame across gaps, and composites it with `camera_at` per output
  frame. No camera track or no keys: unchanged M1 output.
- The panel's still preview includes the camera (latest hub frame) and the current placement.

## 4. Packages and order

| # | Package | Depends on | Main files |
|---|---|---|---|
| P1 | Camera geometry (`camera_at`, `camera_rects`, snapping, presets; TS mirror; shared case table) | – | crates/geometry, ui/src/lib/geometry.ts |
| P2 | Compositor camera layer (GPU + CPU + parity tests) | P1 | crates/render |
| P3 | Camera hub (attach/detach consumers; record session uses it; CLI fallback) | – | crates/platform, crates/capture-macos/src/av, crates/record, app/src/core.rs |
| P4 | Live frame transport (binary channel, latest-wins, renderer thread) | P2 | app/src, ui/src/lib |
| P5 | Bubble window + interactions + panel Camera section | P1, P3, P4 | app/src, ui/src/routes/Bubble.svelte, Panel.svelte, crates/capture-macos/src/sck/overlays.rs |
| P6 | Source preview stream + stage preview window | P2, P4 | crates/capture-macos/src/sck, app/src, ui/src/routes/Stage.svelte |
| P7 | Keyframes during recording (session API, journal, finalize, recovery) | P1, P3 | crates/record, crates/core |
| P8 | Quick export and stills with the camera | P2, P7 | crates/export, app/src/still.rs |
| P9 | Tests: driver flows (bubble exclusion, keyframes replay, WYSIWYG pixel match), MANUAL_TEST §, docs (SPEC, ARCHITECTURE, DECISIONS, STATUS) | all | app/driver, docs |

P1 and P3 start in parallel; P2 after P1; P4–P8 as their inputs land; P9 last. All builds and
tests run at low CPU priority while the owner works (`nice -n 19`, `-j 4`).

## 5. Tests and acceptance (★ review checklist)

1. Bubble shows the selected camera within 1 s of choosing it, in every shape, with mirror,
   border and shadow; reframe zoom/pan/Reset work; S/M/L, corner presets, snapping and arrow keys
   work.
2. The bubble and stage preview never appear in a recording (driver exclusion flow, all source
   kinds, 0 frames showing).
3. WYSIWYG: for a whole-display recording, the bubble's on-screen pixels and the export's pixels
   at the same time match within the compositor tolerance (same code path; only camera-frame
   timing differs); for a window source the stage preview frame matches the export frame.
4. Moves, resizes, shape changes and hides during a recording appear at the same times (± 1
   frame) in the export, and survive a `kill -9` (recovery keeps the keys).
5. Quick export contains screen, camera and audio; a recording without a camera is unchanged.
6. Camera hub: bubble before recording, recording, stop: one device session throughout, camera
   track frame count matches the soak's rate, 0 dropped; CLI recordings unchanged.
7. Geometry and compositor: Rust and TS case tables agree; GPU vs CPU diffs within tolerance.
8. 30-minute soak with the bubble visible: thresholds as in M1, plus bubble CPU budget.
9. CI green on all three OSes twice in a row; no app UI in captures; no focus stealing in tests.

## 6. Risks

- **Bubble frame cost.** Webview frames at 30 fps for a bubble plus a stage preview, on top of a
  recording. Mitigation: render at window size only, latest-wins, drop to 15 fps when CPU is high;
  the native CALayer slot (ADR-007) stays available if measurements demand it.
- **Camera device sharing.** One session must feed preview and writer without reordering or
  latency spikes in the writer. Mitigation: the writer path keeps its own queue; the preview path
  is try-send only.
- **Placement truth for non-display sources.** Solved by the stage preview (§3.4); Q1 decides the
  on-screen bubble's behaviour there.
- **Keyframe volume.** A long drag at 30 Hz is small (about 100 bytes per key); journal batching
  keeps fsync cost flat.

## 7. Not changing

ADR-007 (webview frames), ADR-033 (canvas sizes), ADR-034 (segments), ADR-039/040 (app crop,
lists). New ADRs: camera hub, camera geometry units and presets, bubble/stage placement rule,
keyframe sampling.

## 8. Owner actions during M2

- Keep a camera connected (any of the four on the build Mac) for live checks; the agents will
  announce live runs as in M1.

## 9. Owner answers (2026-11-20)

- **Q1 placement:** stage preview plus the floating bubble. For window, app and region sources and
  for letterboxed frames, a floating bubble stays on screen so you see yourself, and the stage
  preview is where the camera is placed exactly. Whole-display recordings with the Original frame
  use the bubble alone.
- **Q2 mirror:** not mirrored by default; mirroring is an option (toggle in the panel and the
  bubble toolbar). The preview still always matches the output.
- **Q3 build:** one agent, packages in order (no multi-agent workflow).
- **Q4 camera on:** whenever a camera is selected and the recorder panel or a recording is active.
