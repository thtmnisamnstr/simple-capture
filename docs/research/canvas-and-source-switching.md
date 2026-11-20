# Design note: output canvas (aspect + letterbox background) and source switching while recording

Status: design only, 2026-11-19. No capture code was written. The only thing run was a scratch Python
calculator for the fit math and preset sizes (numbers below). Sources are cited as `file:line` or URL.
"Read" means read from docs or code; "untested" means no spike evidence.

The spec already contains both requirements (SPEC.md:91-92, :144, M1 at :300, and the non-goal
wording at :322). This note proposes how to build them. It is meant to become a new ARCHITECTURE section
("§8a Canvas and screen segments") and three ADRs (033-035, below).

---

## 1. Summary

- **Canvas.** The output frame becomes a *canvas* = aspect + fit mode + background. Every screen
  segment is fit into it separately. The canvas lives in `project.json` and can be edited at any
  time; the raw files are never touched.
- **Segments.** The screen track becomes an ordered list of **segments**: one source, one native
  size, one file each. Mic, camera and system audio stay **one continuous file each**. Switching the
  source closes one screen segment and opens the next.
- **Everything else keeps working.** Segments live in RecTime, like the EDL and the camera keyframes.
  So trim, split, ripple, copy/paste and reorder need no change, and the compositor's existing frame
  rule ("newest frame at or before t") extends across segment files.
- **One pipeline change on macOS.** Spike 3 takes system audio from the *same* SCStream as the
  screen (`spikes/03-macos-pipeline/src/capture.rs:327-345`). A source switch replaces the screen
  stream, so system audio must move to its own long-lived SCStream (ADR-035).

---

## 2. Canvas model

### 2.1 Aspect options

| Option | Ratio | Notes |
|---|---|---|
| `original` (default) | the **first** screen segment's content aspect | With one source there is no letterbox, which matches today's behaviour. Later sources are letterboxed into the first source's shape. The recorder panel says so when a switch happens while Original is selected. |
| `16:9` | 16/9 | |
| `4:3` | 4/3 | |
| `1:1` | 1/1 | |
| `9:16` | 9/16 | TikTok/Reels/Shorts |
| `4:5` (later) | 4/5 | Editor nice-to-have (SPEC:144). The schema reserves it now. |

Ratios are stored as exact integers (`{num, den}`), never as floats. Resize and preset math then
never drifts.

"Original" is resolved **at record start** from the first segment and stored as
`canvas.original_ratio`. It does not change when later segments arrive. (The editor may later offer
"match source N", in M3 or later.)

### 2.2 Fit modes

- **`contain` (required).** The whole source is visible and scaled to touch the canvas edges. The
  rest is background.
- **`cover` (later, the nice-to-have "fill/crop-and-pan").** The source fills the canvas and the
  overflow is cropped. A per-segment focus point (pan) is clamped so the crop stays inside the
  source.
- **No padding, inset, rounded corners or shadow.** SPEC:322 already scopes "background
  beautification (padding, rounded corners, shadows around the screen)" out, while keeping "the
  letterbox background for the output aspect ratio" in scope. Proposed ADR wording (below) keeps this
  narrow: the background only ever fills area the content does not cover, and the content is never
  shrunk to expose more of it.

### 2.3 Fit math (geometry crate, pure)

Inputs:
- canvas output size `Wc × Hc` (integers, the export/preview size);
- the segment's content size `w × h`. This is `content_px` (a crop rect inside the frame for
  region-by-crop, otherwise the full frame).

**contain:**
```
s  = min(Wc / w, Hc / h)
dw = w·s,  dh = h·s
x  = (Wc − dw)/2,  y = (Hc − dh)/2
dst (pixel-snapped) = x0 = round(x), y0 = round(y), x1 = round(x+dw), y1 = round(y+dh)
src = content_px (full content)
```
Snapping both edges (not the origin plus size) means adjacent rounding never opens a 1-px seam.
Content edges need not be even. Only the canvas must be (4:2:0). A one-chroma-sample blend at the
letterbox edge is acceptable. Even-aligning the content edges was considered and rejected, because it
distorts the scale by up to 2 px.

**cover (later):**
```
s   = max(Wc / w, Hc / h)
vis = (Wc / s, Hc / s)                      // visible source size
c   = clamp(focus_norm · (w,h), vis/2, (w,h) − vis/2)
src = rect(c − vis/2, vis)   dst = (0,0,Wc,Hc)
```

**Worked examples** (computed with `fit.py`; Python `round`):

| Canvas | Source content | s | dst (x, y, w, h) |
|---|---|---|---|
| 1920×1080 (16:9) | 7680×2160 (owner's 32:9 display) | 0.25 | 0, 270, 1920, 540 |
| 1080×1920 (9:16) | 7680×2160 | 0.140625 | 0, 808, 1080, 304 |
| 1080×1920 (9:16) | 1234×567 region | 0.8752 | 0, 712, 1080, 496 |
| 1080×1920 (9:16) | 3024×1964 window | 0.3571 | 0, 609, 1080, 702 |
| 1080×1080 (1:1) | 3024×1964 window | 0.3571 | 0, 189, 1080, 702 |
| 1440×1080 (4:3) | 3840×2160 | 0.375 | 0, 135, 1440, 810 |

UX consequence: a 32:9 display in 9:16 fills 304 of 1920 px of height. The recorder panel should warn
when content covers less than about 30% of the canvas area ("This source is very wide for 9:16; try a
window or region"). When the source is a region and an aspect is set, the region tool's aspect lock
(SPEC:89) defaults to **"Match frame"**, so the region fills the canvas with no letterbox.

### 2.4 Canvas pixel size ("reference size")

The canvas is resolution-independent:
- text and camera positions are `_norm` (0..1 of the canvas);
- camera size is a fraction of the canvas short side (ARCHITECTURE §6);
- text sizes are in "1080 units". **Proposed clarification:** 1080 units = the canvas *short side*.
  A 9:16 caption then has the same width share as its 16:9 counterpart. Today's draft is ambiguous on
  this.

So pixel size is only needed for export, thumbnails and the preview. It is **derived, not stored**:

```
natural(A, seg) = smallest canvas of aspect A that shows seg's content at 1:1:
    if w/h ≥ A: (w, w/A) else (h·A, h)
ref(A, segments):
    S  = max over segments of short_side(natural(A, seg))
    fixed aspects:  S = min(S, 2160)
    original:       if one segment → exactly that segment's size (even-rounded down);
                    else S such that long side ≤ 8192
    W×H = from S and A, each side rounded to the nearest even number
```

Why cap fixed aspects at a 2160 short side:
- every fixed-aspect reference canvas is then ≤ 3840 px on its long side (2160×3840, 3840×2160,
  2880×2160, 2160×2160);
- that is always inside VideoToolbox H.264 hardware's 4096 px limit ([S1], ARCHITECTURE §11);
- it is inside the 4K text-raster budget [S4].

Why 8192: Cap clamps output to `MAX_OUTPUT_DIMENSION = 8192` (`reference/Cap/crates/rendering/src/lib.rs:3764`,
`:4171-4192`). wgpu's default 2D texture limit is believed to be 8192 (not re-verified here).

Examples:
- 7680×2160 display: in 16:9 the natural size has a 4320 short side, capped to **3840×2160**; in 9:16
  it is **2160×3840**; in Original it is **7680×2160** (VT H.264 hidden by the probe; x264/HEVC fine).
- 3024×1964 window in 16:9: natural **3492×1964** (no resampling at "Original" size).

### 2.5 Background

The background is drawn only where content does not cover (in practice the whole canvas is drawn
first, then the content).

| kind | fields | render |
|---|---|---|
| `solid` | `color: Rgba` (sRGB, straight alpha; alpha forced to 1 at export) | Skia clear |
| `linear` | `stops: [{at, color}]` (2–4), `angle_deg` | Skia linear gradient shader, **dither on** (8-bit + 4:2:0 bands without it) |
| `radial` | `stops`, `center_norm`, `radius_norm` (of the canvas diagonal/2) | Skia radial gradient, dither on |
| `image` | `asset` (project-relative), `sha256`, `fit: "cover"` (v1 only), `focus_norm` | Skia `Image::from_encoded`, drawn cover-fit with high-quality sampling |
| `content_blur` (later, optional) | `radius_1080`, `dim` | A scaled-up, blurred copy of the current screen frame: the common TikTok "blur fill". Per-frame GPU blur, so cost not measured. Only if the owner wants it. |

- **One raster, cached.** The background is rasterized by Skia into an RGBA8 premultiplied image at
  the output size. It is cached by `(hash(background), Wc, Hc)` and uploaded once. The wgpu compositor
  and the CPU reference renderer sample the *same* pixels, so golden tests stay within ADR-022
  tolerances at no extra cost. Blending stays in gamma space (ADR-021).
- **Images.**
  - On selection the file is **copied** into `<project>/assets/background-<sha256[0..12]>.<ext>`, so
    projects stay self-contained when the original moves.
  - PNG decodes with the current Skia build. JPEG and WebP need the skia-safe features `jpeg-decode` and
    `webp-decode`. These are flags on the existing dependency (`skia-bindings-0.157.0/Cargo.toml:61-101`,
    off by default; spike 4 used only `textlayout`).
  - HEIC is not decoded by Skia. On macOS, a later option is to convert it to PNG with ImageIO at
    import.
- **Defaults.** Background: a neutral dark solid. Presets: about 8 solids and about 8 gradients. The
  last choice is remembered.

### 2.6 Where it lives in project.json

```jsonc
"output": {
  "canvas": {
    "aspect": "9:16",                       // "original" | "16:9" | "4:3" | "1:1" | "9:16" | "4:5"
    "ratio": { "num": 9, "den": 16 },       // resolved; for "original" = first segment's content aspect
    "fit": "contain",                        // "contain" | "cover" (later)
    "background": {
      "kind": "linear",
      "angle_deg": 135,
      "stops": [ { "at": 0.0, "color": [0.10, 0.12, 0.30, 1] },
                 { "at": 1.0, "color": [0.55, 0.20, 0.60, 1] } ]
    }
  },
  "segment_framing": {                       // later (cover + pan); keyed by segment id, RecTime-agnostic
    "s1": { "fit": "cover", "focus_norm": { "x": 0.30, "y": 0.50 } }
  }
}
```
Other background examples:
- `{ "kind": "solid", "color": [0.07,0.07,0.08,1] }`
- `{ "kind": "radial", "center_norm": {"x":0.5,"y":0.4}, "radius_norm": 0.9, "stops": [...] }`
- `{ "kind": "image", "asset": "assets/background-3f9a1c07be21.jpg", "sha256": "3f9a…", "fit": "cover", "focus_norm": {"x":0.5,"y":0.5} }`

This replaces the draft's `output: { aspect, screen_crop_norm }` (ARCHITECTURE §6). A single global
crop no longer makes sense with several sources, so crop and pan become per segment.

---

## 3. Screen track as segments

### 3.1 Schema (replaces `recording.source`)

```jsonc
"recording": {
  "clock": "mach_host",
  "start_host_ns": 912345678901234,
  "stop_host_ns":  912405778901234,
  "pauses": [],
  "screen": {
    "fps_requested": 30, "resolution": "native", "cursor": true, "click_highlight": false,
    "segments": [
      {
        "id": "s0", "track_id": "screen-0",
        "start_rec_ns": 0, "end_rec_ns": 21480000000,
        "start_reason": "record_start", "end_reason": "user_switch",
        "via": "own_chooser",                       // "own_chooser" | "system_picker" | "portal"
        "source": { "kind": "display", "display_id": "1", "display_name": "LG HDR 5K",
                    "scale": 2.0, "frame_pt": { "x": 0, "y": 0, "w": 3840, "h": 1080 } },
        "frame_px":   { "w": 7680, "h": 2160 },     // decoded buffer size
        "content_px": { "x": 0, "y": 0, "w": 7680, "h": 2160 }   // what is fit into the canvas
      },
      {
        "id": "s1", "track_id": "screen-1",
        "start_rec_ns": 21480000000, "end_rec_ns": 47020000000,
        "start_reason": "user_switch", "end_reason": "user_switch", "via": "own_chooser",
        "source": { "kind": "window", "window_id": 4512, "app_bundle_id": "com.apple.Safari",
                    "app_name": "Safari", "window_title": "Docs", "scale": 2.0 },
        "frame_px":   { "w": 3024, "h": 1964 },
        "content_px": { "x": 0, "y": 0, "w": 3024, "h": 1964 }
      },
      {
        "id": "s2", "track_id": "screen-2",
        "start_rec_ns": 47020000000, "end_rec_ns": 60100000000,
        "start_reason": "user_switch", "end_reason": "record_stop", "via": "own_chooser",
        "source": { "kind": "region", "display_id": "1", "scale": 2.0,
                    "region_pt": { "x": 400, "y": 200, "w": 617, "h": 283.5 },
                    "method": "crop" },               // "source_rect" (SCK sourceRect) | "crop"
        "frame_px":   { "w": 7680, "h": 2160 },
        "content_px": { "x": 800, "y": 400, "w": 1234, "h": 567 }
      }
    ]
  },
  "tracks": [
    { "id": "screen-0", "kind": "screen", "segment_id": "s0", "file": "tracks/screen-000.mov", "...": "as today" },
    { "id": "screen-1", "kind": "screen", "segment_id": "s1", "file": "tracks/screen-001.mov" },
    { "id": "screen-2", "kind": "screen", "segment_id": "s2", "file": "tracks/screen-002.mov" },
    { "id": "mic", "kind": "mic", "file": "tracks/mic.mov" },
    { "id": "camera", "kind": "camera", "file": "tracks/camera.mov" },
    { "id": "system_audio", "kind": "system_audio", "file": "tracks/system_audio.mov" }
  ]
}
```

Rules:
- `start_reason` / `end_reason` values: `record_start | user_switch | source_lost | source_resized |
  record_stop | recovered`.
- Segments are sorted, and their `[start, end)` ranges never overlap. **Gaps are allowed**: a switch on
  Linux, or a lost source before the user picks a new one. A gap shows the last frame held.
- **File time 0 = `start_rec_ns`.** The writer starts its session at the segment's first (re-stamped)
  frame, so the local time is `rec − start_rec_ns`. Pauses are already removed (ADR-012).
- `frame_px` vs `content_px` lets "region" be either SCK `sourceRect` capture (content = whole frame)
  or crop-after-capture (Windows/Linux, and macOS if `sourceRect` misbehaves). This is the open M1
  item in ARCHITECTURE §14.
- Health counters stay per Track (per file), so each segment reports its own drops.
- project.json v1 has not shipped, so this is a change to the v1 draft and needs no migration.

### 3.2 Journal events (journal.jsonl, fsync'd)

```jsonl
{"ev":"switch_begin","host_ns":912367000000000,"to":{"kind":"window","window_id":4512}}
{"ev":"segment_open","seg":"s1","start_rec_ns":21480000000,"file":"tracks/screen-001.mov","source":{...},"frame_px":{"w":3024,"h":1964}}
{"ev":"segment_close","seg":"s0","end_rec_ns":21480000000,"reason":"user_switch"}
{"ev":"switch_failed","host_ns":912367400000000,"error":"window closed"}
```

### 3.3 Cursor (cursor.jsonl)

Cursor events are stored per segment, normalised to that segment's content rect with a top-left
origin. Cap does the same, normalising to the capture crop bounds ([cap-recording] §2.1,
`docs/research/cap-recording.md:184-192`).

```jsonl
{"t_rec_ns":21500000000,"seg":"s1","u":0.412,"v":0.733,"inside":true,"ev":"move"}
{"t_rec_ns":21712000000,"seg":"s1","u":0.415,"v":0.731,"inside":true,"ev":"down","btn":0}
```

- Canvas position = `dst.x + u·dst.w`, `dst.y + v·dst.h`. That is the same `dst` the compositor uses,
  so click highlights stay correct under any aspect and any later canvas change.
- Window sources need the window's live screen frame to normalise against: poll its bounds at the
  cursor rate. Cost not measured; M1 item.

### 3.4 Compositor per output frame

```
t_rec = tl_to_src(edl, t_tl)                        // unchanged
seg   = last segment with start_rec_ns ≤ t_rec      // binary search
frame = newest frame of seg's file at or before (t_rec − seg.start)
        ?: last frame of the previous segment       // hold across a gap / before the first frame
dst   = fit(canvas_px, seg.content_px.size, framing(seg))
draw: background texture → screen(src = content_px, dst) → camera (unchanged) → text (unchanged)
```

- This is the existing frame rule (ARCHITECTURE §7) applied to one logical screen track made of
  several files.
- Decoding: one reader pool per segment file. Prefetch already runs in timeline space, so it reads
  ahead across a segment boundary exactly as across a cut.
- **Segment boundary = hard video cut.** No crossfade (no transitions, by spec). Audio is continuous
  across it, so **no cut fade** is applied: the fade rule only fires on RecTime discontinuities
  (ARCHITECTURE §7).
- **Camera layout is unaffected.** It lives in RecTime and canvas-normalised coordinates. A bubble
  placed over content in one segment may sit over background in another, and the stage preview shows
  this.

### 3.5 Edits

- The EDL is unchanged: clips hold RecTime ranges, and segments are just what those ranges contain.
  Split, trim, ripple delete, cut/copy/paste, duplicate, reorder and speed all work without special
  cases. Pasted pieces keep their source and framing for the same reason camera keyframes do.
- In the editor:
  - Segment boundaries are drawn as markers on the main track (a source icon and name on hover).
  - They are snap targets, and ↑/↓ "jump between cuts" includes them.
  - "Select source span" (double-click the marker band) selects the timeline ranges that map to
    `[start, end)` of that segment. One Delete then removes everything recorded from that source.
- Per-segment framing (cover + pan, later) is keyed by segment id. It survives every edit and is
  undoable as a command.
- Changing aspect or background in the editor is one undoable command on `output.canvas`. Text and
  camera keep their normalised positions.

---

## 4. Recording pipeline for a switch

### 4.1 macOS (M1)

Make-before-break, with our own chooser or the system picker:

1. The user commits source S′. Journal `switch_begin`.
2. Create a new SCStream:
   - filter = S′ minus our windows;
   - config = S′ native px, same fps, 420v, queueDepth 6;
   - add a screen output only.
   Then start it. The new `TrackWriter` (AVAssetWriter, fragmented .mov, HEVC; ADR-008) is created on
   the first frame, when the exact size is known.
3. On the new stream's first complete frame at host time `t1`:
   - `cut = to_rec(t1)`;
   - the old stream drops frames with PTS ≥ t1;
   - the old writer re-appends its last frame at `cut` (the same rule as stop, ADR-011);
   - then `finishWriting` runs off the critical path and the old stream stops;
   - journal `segment_open` / `segment_close`; project.json is rewritten (as on pause).
4. If the new stream fails to start, the old segment simply continues: journal `switch_failed` and
   show a toast.

Notes:
- **Alternative to measure in the M1 spike:** keep one stream and call `updateContentFilter` +
  `updateConfiguration`. Apple's page says it updates a running stream (macOS 12.3+), but says nothing
  about which frames are old or new during the change
  ([updateContentFilter](https://developer.apple.com/documentation/screencapturekit/scstream/updatecontentfilter(_:completionhandler:))).
  We would need to tell them apart by buffer size or frame info. Fewer moving parts, but less
  predictable.
- **System audio (ADR-035).** Spike 3 adds the audio output to the screen stream
  (`capture.rs:327-345`). With segments, system audio comes from a separate SCStream that lives for
  the whole recording:
  - display filter, `capturesAudio`, `excludesCurrentProcessAudio`, a minimal video configuration;
  - "Only the captured app" mode follows the active segment's app through `updateContentFilter` on that
    audio stream, falling back to "All" for display and region segments.
  - Untested: the gap-free filter change, and the cost of a near-idle video side.
- **Pause.** Switching while paused is allowed. The new segment starts at resume, which gives a clean
  cut with no "choosing" footage.
- **Encoder gate.** The 60 fps / native-size probe (ADR-009) runs at the largest connected display's
  native size. Every window and region is no larger, so a switch never exceeds a probed size.
- **Source lost** (window closed, display unplugged; Pitfall 12):
  - the segment ends with `source_lost` and the last frame is held;
  - audio and camera continue;
  - the floating bar shows "Source closed: Switch source · Stop".
- **Source resized** (window resize, resolution change): an automatic rollover to a new segment
  (`source_resized`, debounced) reuses the same machinery. In M1 we only log the event and observe SCK's
  behaviour (`scalesToFit` / `preservesAspectRatio` exist on SCStreamConfiguration; their interaction
  is undocumented on Apple's page). Rollover goes in M7 if needed.
- Latency from click to cut, and the cost of two concurrent streams, are **not measured**. Proposed
  M1 target: cut within 500 ms, 0 extra dropped frames, audio sample count continuous.

### 4.2 Windows (M6, read only)

- Our chooser uses `CreateForWindow` / `CreateForMonitor`, so no dialog appears.
- The system path uses `GraphicsCapturePicker.PickSingleItemAsync` (Windows 10 1803+,
  [Microsoft Learn](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturepicker)).
- A new frame pool and session per segment, make-before-break.
- Region = cropped monitor capture (`content_px`).
- Our overlay windows get `WDA_EXCLUDEFROMCAPTURE` when they are created.
- Process loopback for "app" audio is re-created for the new process, written into the same
  continuous file. Untested.

### 4.3 Linux (M6, read only)

- The ScreenCast portal allows `SelectSources` only once per session, before `Start`. With
  `multiple: true`, `Start` returns at least one stream
  ([portal docs](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html)).
- Plan:
  - At record start, offer "Select all sources you'll show" with `multiple: true`, MONITOR|WINDOW.
    Switching between those pre-selected streams then needs **no dialog**.
  - Region = crop of a pre-selected monitor.
  - "Add another source…" opens a new portal session, and therefore a dialog. That dialog cannot be
    excluded from capture, so the old segment ends at the moment the user asks. The screen holds the
    last frame until the new stream's first frame (a gap, §3.1).
  - The restore token (persist_mode 2) is saved per session, as today.
- Unverified: GNOME and KDE multi-select UI for windows, and whether a restore token restores a
  multi-source selection.

### 4.4 "System picker" on all three OSes (owner condition on ADR-028)

| OS | API | Notes |
|---|---|---|
| macOS | `SCContentSharingPicker` | macOS 14.0+. Has `present(for: SCStream)`, documented as "for an already running capture stream" ([Apple](https://developer.apple.com/documentation/screencapturekit/sccontentsharingpicker)). |
| Windows | `GraphicsCapturePicker` | Windows 10 1803+. |
| Linux | the portal dialog itself | |

So a "System picker…" entry in the switch menu is possible on every OS. All three are read from docs
only and untested. The macOS one must be tested in the signed app (ADR-028).

---

## 5. Recording UX

- **Recorder panel** (before recording):
  - A "Frame" segmented control: `Original | 16:9 | 4:3 | 1:1 | 9:16`, with shape icons.
  - A "Background" swatch opening a popover with three tabs:
    - Solid: palette + custom picker;
    - Gradient: presets, 2 stops, angle, a linear/radial toggle;
    - Image: "Choose…" (PNG/JPEG/WebP), with cover fit.
  - Under it, a **mini stage preview**: a still of the selected source (SCK screenshot, taken on
    source change) composited with the background by the real compositor.
  - The live stage preview with camera is M2 (SPEC:110).
  - The panel remembers the last choices.
  - With Original selected, the background control reads "used if later sources don't fill the frame".
- **Floating bar** (recording): a new **Switch source** button. It opens a popover anchored to the bar
  (our window, so excluded from capture):
  - **Recent** (the last 5 sources, previous source first);
  - Displays (thumbnails);
  - Windows (searchable list with app icons);
  - "Select region…";
  - "Pick window by clicking";
  - "System picker…".
  One click switches. The same entries appear in the tray menu, which is the main path on Linux, where
  overlays auto-hide.
- **Hotkey:** "Switch source" (default proposed: ⌥⇧S, configurable). It enters **pick mode** on the
  display under the cursor:
  - hovering highlights the window under the cursor, and a click picks it;
  - D picks the whole display;
  - dragging picks a region (live, with aspect lock defaulting to "Match frame");
  - P opens the system picker;
  - Esc cancels.
  A small hint card follows the cursor. Hotkeys are generally M7, but this one ships in M1 because the
  spec ties it to the feature.
- **What is recorded while choosing:**
  - macOS and Windows: the old source keeps recording live, because the chooser is excluded. Trimming
    is easy thanks to segment markers. Pausing first gives a clean cut.
  - Linux: the screen holds the last frame while the portal dialog is open.
- **Own-UI exclusion:**
  - The chooser popover, pick-mode overlay and region overlay are created **hidden at launch**, and
    their windows are put in the SCContentFilter exclusion list of every stream (ADR-010).
  - Untested: whether hidden-then-shown windows can be excluded that way. Fallback: update the filter
    and wait for the completion handler before showing an overlay.
  - The new stream's filter is built with the same exclusion list before it starts.
  - Windows: WDA on creation. Linux: no exclusion; a hold instead.

---

## 6. Export interaction

- **Size presets per aspect, named by short side.** This extends SPEC:175's height presets, which
  were landscape-only:

| class | 16:9 | 4:3 | 1:1 | 9:16 | 4:5 |
|---|---|---|---|---|---|
| 2160 | 3840×2160 | 2880×2160 | 2160×2160 | 2160×3840 | 2160×2700 |
| 1440 | 2560×1440 | 1920×1440 | 1440×1440 | 1440×2560 | 1440×1800 |
| 1080 | 1920×1080 | 1440×1080 | 1080×1080 | 1080×1920 | 1080×1350 |
| 720 | 1280×720 | 960×720 | 720×720 | 720×1280 | 720×900 |
| 480 | 854×480 | 640×480 | 480×480 | 480×854 | 480×600 |
| 360 (GIF) | 640×360 | 480×360 | 360×360 | 360×640 | 360×450 |
| 240 (GIF) | 427×240 | 320×240 | 240×240 | 240×427 | 240×300 |

  Even rounding is `2·round(x/2)` for video. GIF is not rounded to even (427, not 426).
- **Aspect-locked resize (M5).** The lock ratio is the canvas's exact `{num, den}`, not the rounded
  pixel size. When the user types the short side S:
  - S is rounded to even;
  - long side = `2·round(S·num/(2·den))` (integer-exact);
  - GIF uses `round(S·num/den)`.
  This is Kap's aspect-locked inputs (`reference/Kap/renderer/components/editor/options/left.tsx:77-106`)
  with the evenness moved into the UI ([kap] §4).
  - "Original" = the reference size of §2.4.
  - "Don't upscale" compares against the reference size.
- **Encoder limits.** Every fixed-aspect preset is ≤ 3840 px, so VideoToolbox H.264 hardware (cannot
  open above 4096, [S1]) is always offered for those. Only Original canvases above 4096 hit the
  existing probe gate (ARCHITECTURE §11).
- **Readback.** wgpu's 256-byte row alignment is handled with padded rows, not by rounding the width.
  Cap rounds the output width to a multiple of 4 (`lib.rs:4183`). We keep even-only.
- **GIF default maximum.** SPEC:178 says 960 px *width*. For 9:16 that gives 960×1707. Proposed: cap
  the **long side** at 960, giving 540×960. Needs owner OK.

---

## 7. Milestone split

| Milestone | Canvas | Segments / switching |
|---|---|---|
| **M1** (the spec already lists both) | Canvas model plus geometry fit (contain) with unit tests. All background kinds (solid, linear, radial, image PNG/JPEG). Background raster cache in GPU and CPU paths. Recorder-panel Frame + Background controls with a still mini preview. Quick x264 export at the reference size. | Segment schema, journal and recovery. macOS make-before-break switch. Separate system-audio SCStream (ADR-035). Floating-bar button, tray menu, switch hotkey, pick mode, region overlay. Switch while paused. Source-lost handling. Cursor `seg` field. Tests below. |
| M2 | Live stage preview (canvas + camera) | System-picker path test in the signed app (ADR-028) |
| M3 | Editor Frame panel (undoable). 4:5 preset; cover + per-segment pan (nice-to-haves) | Segment markers, snapping, "Select source span", per-segment thumbnails |
| M4 | Text units = canvas short side | |
| M5 | Per-aspect size presets, canvas-ratio resize, GIF long-side cap | |
| M6 | | Windows (WGC sessions, GraphicsCapturePicker, WDA overlays); Linux (portal multiple=true, per-add dialog with hold) |
| M7 | `content_blur` background (only if wanted) | Auto-rollover on window resize; previous-source hotkey polish |

**M1 tests:**
- **geometry:** property tests (dst within the canvas, aspect error ≤ 1 px, centred within 0.5 px),
  the preset table, reference-size rules.
- **record:** a synthetic ScreenSource switching between two patterned sources of different sizes.
  Assert:
  - segments are sorted and non-overlapping;
  - every frame's PTS lies inside its segment;
  - mic and system-audio sample counts are continuous (duration × rate within one video frame);
  - the camera track is unaffected.
- **Kill tests:** SIGKILL between `switch_begin` and the first new frame, and between `segment_open`
  and `segment_close`, then recovery.
- **Golden images:** every background kind × every aspect with a test pattern, GPU vs CPU.
- **ffprobe:** export dimensions per preset.
- **Sync:** the flash/beep test repeated after a switch.

**Recovery (ARCHITECTURE §12)** rebuilds segments from the journal:
- a segment without `segment_close` ends at start + its measured file duration;
- an unreadable segment file is dropped (a gap shows a hold, or background only if it is s0);
- the originals are never modified.

---

## 8. Prior art

- **Cap (AGPL; read only, nothing copied):**
  - Aspect presets: `AspectRatio {Wide, Vertical, Square, Classic, Tall}`, with `None` = auto
    (`crates/project/src/configuration.rs:27-34`).
  - Backgrounds: `BackgroundSource {Wallpaper, Image, Color, Gradient, AnimatedGradient}` (`:40-71`).
    The background config also carries padding, rounding, inset, crop, shadow, border and frame
    (`:378-405`), which is the beautification we exclude.
  - Output base size: derived from crop + padding (`crates/rendering/src/lib.rs:4103-4169`), scaled
    to a resolution base, width rounded to a multiple of 4, height to even, clamped to 8192
    (`:3764`, `:4171-4192`).
  - **No source switching:**
    - the capture target is a fixed base input of the recording (`crates/recording/src/studio_recording.rs:375`,
      `:2438`);
    - every new segment (created on pause/resume) reuses `segment_base_inputs()` (`:3470-3530`);
    - per-segment metadata bundles display, camera, mic, system audio and cursor together
      (`crates/project/src/meta.rs:831-850`).
    Our design segments only the screen and keeps audio and camera continuous.
- **Kap (MIT):** region aspect lock and nearest-normal ratio (`renderer/containers/cropper.js:2-36`,
  `:180-192`), aspect-locked export size inputs (`left.tsx:77-106`), `makeEven` ([kap]:41). No
  output-canvas or letterbox concept and no source switching were found in the files read.

---

## 9. Proposed ADR text

**ADR-033: Output canvas: aspect ratio, contain fit and letterbox background**
- **Status:** Proposed. The owner requested the feature on 2026-11-19; the details need OK.
- **Decision:**
  - The output is a canvas with aspect `original | 16:9 | 4:3 | 1:1 | 9:16` (4:5 reserved), stored
    as an exact ratio. "Original" is resolved from the first screen segment at record start.
  - Every screen segment is contain-fit (cover + pan later).
  - The uncovered area is filled with a solid, linear, radial or image background, rasterized by Skia
    into a cached texture used by both the GPU and CPU renderers.
  - Canvas pixel size is derived: reference size = 1:1 natural size, with the short side capped at
    2160 for fixed aspects and the long side capped at 8192 for Original.
  - Export presets are named by short side.
  - Text `_1080` units are relative to the canvas short side.
  - Background images are copied into `assets/` with a sha256.
  - Enable the skia-safe `jpeg-decode` and `webp-decode` features.
- **Scope wording:** "The letterbox background fills only area the content does not cover. Padding,
  insets, rounded corners, shadows or frames around the screen remain out of scope (background
  beautification)."
- **Consequences:**
  - project.json `output.canvas` replaces `output.aspect` / `screen_crop_norm`.
  - The GIF default maximum becomes a long-side cap (owner OK).

**ADR-034: Screen track as segments; switching source while recording**
- **Status:** Proposed.
- **Decision:**
  - The screen track is an ordered list of segments: one source and one file each, in RecTime, with
    file time 0 at the segment's start. Gaps are allowed and hold the last frame.
  - Mic, camera and system audio stay continuous.
  - macOS switches make-before-break (new SCStream + writer; cut at the new first frame; old last
    frame re-appended at the cut). Windows uses a new WGC session. Linux uses pre-selected portal
    streams, or a new portal session with a hold.
  - Segment boundaries are hard video cuts with no audio fade. The EDL and edit commands are
    unchanged.
  - Cursor events carry the segment id and are normalised to its content rect.
  - The journal records `switch_begin`, `segment_open`, `segment_close` and `switch_failed`, and
    recovery rebuilds segments from them.
- **Alternatives:**
  1. One file at a fixed canvas size, letterboxed at record time. Rejected: it alters the raw
     recording, loses native resolution, and locks the aspect.
  2. `updateContentFilter` on one stream. Kept as an M1 spike comparison.
  3. Stop and restart the whole recording per source, as Cap does per pause. Rejected: gaps in audio
     and camera.

**ADR-035: macOS system audio comes from its own SCStream**
- **Status:** Proposed. This changes the spike 3 structure.
- **Decision:**
  - System audio is captured by a dedicated SCStream that lives for the whole recording, with a
    minimal video configuration, `capturesAudio` and `excludesCurrentProcessAudio`.
  - Screen streams carry video only.
  - "Only the captured app" mode follows the active segment through `updateContentFilter` on the audio
    stream, falling back to "All" for display and region segments.
- **Evidence:** spike 3 attaches audio to the screen stream (`capture.rs:327-345`), so a stream swap
  would interrupt it. The continuity and cost of the separate stream are untested (M1 spike).

---

## Independent verification (2026-11-19)

An adversarial verifier re-read the sources and re-ran the builds and measurements. Verdict: **partially confirmed**. Where this section and the text above disagree, this section wins.

### Discrepancies found

- REFUTED: 'PNG decodes with the current Skia build. JPEG and WebP need the skia-safe features jpeg-decode / webp-decode (off by default; spike 4 used only textlayout).' JPEG decode is already on. skia-safe 0.157.0's default features are [binary-cache, embed-icudtl, pdf, jpeg] (skia-safe-0.157.0/Cargo.toml [features] default), and spike 4 does not turn default features off (spikes/04-skia-text/Cargo.toml:11). `cargo tree --offline -e features -i skia-bindings` in spikes/04-skia-text resolves skia-bindings features jpeg-decode and jpeg-encode through skia-safe 'jpeg' and 'pdf'. The ADR-018 FreeType build's args.gn (target/ft/release/build/skia-bindings-34f27f714f966df0/out/skia/args.gn) has skia_use_libjpeg_turbo_decode = true and skia_use_libwebp_decode = false. Its libskia.a exports 33 SkJpeg codec/decoder symbols and 0 SkWebp symbols. Only WebP needs a new feature flag (webp-decode). Even skia-bindings' own default (pdf -> jpeg) enables jpeg-decode, so 'off by default' is wrong at that level too.
- REFUTED (consequence of the above): the claim that LICENSES.md/cargo-about need a new entry for libjpeg-turbo. docs/LICENSES.md:85 already lists 'expat, libjpeg-turbo, libpng, zlib | Skia build'. Only libwebp would be new.
- UNSUPPORTED: 'The 60 fps / native-size probe (ADR-009) runs at the largest connected display's native size. Every window and region is no larger, so a switch never exceeds a probed size.' ARCHITECTURE.md §11 (line ~491) says the native recording encode probe runs 'at the actual capture size and fps', and ADR-009 says 'at the actual capture size'. The note presents a new rule as existing behaviour. Whether a post-switch source needs a re-probe is open.
- UNMEASURED: 'in Original it is 7680x2160 (VT H.264 hidden by the probe; x264/HEVC fine)'. Only VT HEVC at 7680x2160 is measured (ADR-009: 46.3 fps, raw recording). x264 export at 7680x2160 is not measured.
- UNMEASURED: 'dither on (8-bit + 4:2:0 bands without it)' is a rationale, not a measurement.
- Minor citation errors. The GIF 'default maximum width 960 px' is at SPEC.md:180, not :178. The height presets are at SPEC.md:176, not :175. The evidence range '175-179' misses the GIF line. Cap rendering/src/lib.rs:4183 is the limit_scale line; the multiple-of-4 width rounding is a few lines later (~4187), though still inside the cited 4171-4192 range.
- Minor misquote: Apple's SCContentSharingPicker present(for:) abstract reads 'Displays the picker with an already running capture stream', not 'for an already running capture stream'.
- Context the note omits: Cap's AspectRatio::Tall is 3:4 (rendering/src/lib.rs aspect_ratio_value) and project aspect_ratio is Option<AspectRatio> (configuration.rs:3151), so 'None = auto' is accurate. Cap's BackgroundConfiguration also has blur and display_position, besides the listed padding, rounding, inset, crop, shadow, border and frame.

### Corrected summary

Design only. No capture code was written. The investigator ran one scratch Python calculator (fit.py). I re-ran it and every fit and preset number in the note reproduces exactly.

The overall verdict holds: both requirements fit the existing architecture if the screen track becomes a list of segments and the output frame becomes a canvas. The key finding is confirmed from source: spike 3 adds the system-audio output to the same SCStream as the screen (spikes/03-macos-pipeline/src/capture.rs ~327-345). Hot-swapping screen streams would therefore interrupt system audio, so it needs its own long-lived SCStream (proposed ADR-035). The continuity and cost of that stream are untested.

Aspect ratio (A), design proposals, internally consistent:
- The canvas has an aspect (Original, 16:9, 4:3, 1:1, 9:16; 4:5 reserved) stored as exact {num, den}, a fit mode (contain required; cover + pan later) and a background (solid, linear, radial, image).
- Contain fit: s = min(Wc/w, Hc/h), centred, both edges pixel-snapped. Worked examples verified, e.g. 7680x2160 into 1080x1920 gives s = 0.140625 and dst (0,808,1080,304).
- Canvas pixel size is derived. For fixed aspects the reference size has its short side capped at 2160, so the long side is at most 3840, which stays under VT H.264 hardware's 4096 px limit [S1].
- Export presets are named by short side; the table was verified.
- Proposal: "_1080" text units = the canvas short side. This matches the existing camera size_norm definition (ARCHITECTURE.md:292).
- The background is rasterized by Skia into one cached texture shared by the GPU and CPU paths.
- Background images are copied into assets/ with a sha256.
- Correction on image decoding: PNG and JPEG already decode with the ADR-018 Skia build. skia-safe's default features include jpeg, args.gn has skia_use_libjpeg_turbo_decode = true, and docs/LICENSES.md:85 already credits libjpeg-turbo. Only WebP needs a new feature flag (webp-decode), plus a LICENSES entry for libwebp.
- HEIC is not in the build (no SkHeif symbols).
- Padding, rounded corners and shadows stay out of scope, as SPEC.md:322 already says.

Source switching (B), design proposals:
- Only the screen track is segmented. Mic, camera and system audio stay one continuous file each.
- Segment fields: source descriptor, frame_px / content_px, start/end in RecTime, reasons, one file each (file time 0 = segment start).
- The EDL and edit commands are unchanged. A segment boundary is a hard video cut with no audio fade.
- Compositor rule: newest frame at or before t across segments; gaps hold the last frame.
- macOS switch is make-before-break (new SCStream and writer, cut at the new stream's first frame). updateContentFilter is an alternative for the M1 spike: Apple's page (macOS 12.3+) says nothing about frame timing during the update.
- Correction: the note says the 60 fps encoder probe runs at the largest display's size, so a switch never exceeds a probed size. ARCHITECTURE §11 and ADR-009 say the probe runs at the actual capture size. Whether a new source needs a re-probe is an open item.
- System pickers, confirmed from docs (all untested):
  - macOS: SCContentSharingPicker, macOS 14.0+, with present(for:) ("with an already running capture stream").
  - Windows: GraphicsCapturePicker, Windows 10 1803 / 10.0.17134.
  - Linux: the ScreenCast portal dialog. SelectSources can be called only once per session; with multiple=true, Start returns at least one stream; restore tokens are single-use.
- Linux plan: pre-select several sources with multiple=true. Adding a new source opens a portal dialog with a hold.

Milestone split: as proposed, with M1 covering the segment model, macOS switching, the separate audio stream, contain fit, backgrounds and a quick export. The spec already puts both features in M1 (SPEC.md:300).

Prior art, verified at the cited lines:
- Cap has aspect presets, including Tall = 3:4, and rich backgrounds with padding, rounding, shadow and frame. Its output is clamped to 8192, width rounded up to a multiple of 4 and height to even. Its capture target is a fixed base input reused for every pause/resume segment; no code was found that switches source mid-recording.
- Kap has region aspect lock, aspect-locked export inputs and makeEven, but no canvas and no switching.

Unmeasured items:
- x264 export at 7680x2160;
- the effect of gradient dithering;
- two concurrent SCStreams;
- click-to-cut latency;
- audio-stream continuity.

Minor citation fixes: the GIF 960 px width is at SPEC.md:180 and the height presets at :176.
