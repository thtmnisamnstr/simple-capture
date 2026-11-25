# Build "Simple Capture": a local-only screen recorder with a light editor for macOS, Windows, and Linux

Repo: simple-capture. Save this prompt as docs/SPEC.md and treat it as the source of truth. Update it whenever a decision changes.

## Goal
Native desktop apps for macOS, Windows, and Linux, built from one codebase, that:
- record the screen, with an optional camera overlay you can fully adjust;
- open a light editor when recording stops (trim, cut, copy/paste, and Instagram/TikTok-quality text);
- save locally as MP4, WebM, or GIF using the best available encoders.

It should be as simple as Loom and far simpler than OBS or a full video editor.

I'll test mostly on macOS (Apple Silicon), so macOS is first-class. Windows and Linux must build, pass automated tests in CI, and work. Document any limitation where an OS prevents something.

## Non-negotiables
- 100% local: no accounts, uploads, telemetry, analytics, update checks, or network requests.
- Simple Capture's own code is MIT-licensed. GPL/AGPL encoders are allowed only as separate bundled programs (see "Licensing").
- Simple flow: recorder panel → record → editor → export. One main track (the recording) plus one text track; no transitions or effects library.
- Reliability first: never lose a recording.
- Best tool for each job: use each OS's own media frameworks where they're strongest, the best-quality encoders for export, and FFmpeg only where it's genuinely the best option.
- Runs smoothly on Apple Silicon, including a base M1 with 8 GB of RAM, and reasonably well on mid-range Windows/Linux laptops.
- Minimum OS: macOS 14; Windows 10 2004+ (some features need Windows 11); Linux with PipeWire + xdg-desktop-portal (GNOME or KDE, Wayland or X11).
- Names: product "Simple Capture"; repo and crate prefix simple-capture; CLI simple-capture; app identifier com.simplecapture.desktop (decided 2026-11-19, ADR-001; reverse-DNS, not ending in .app). Dev builds that need permissions use com.simplecapture.dev (Platform notes).

## Primary references: Cap and Kap
- Cap (CapSoftware/Cap) is the closest existing design to what we're building. It uses:
  - native screen capture on each OS;
  - screen, camera, and audio recorded as separate tracks;
  - a wgpu renderer;
  - each OS's own encoders/decoders, with FFmpeg as a fallback;
  - gifski for GIFs;
  - an editor with timeline and text segments.
- Cap is AGPL-3.0 except for its MIT-licensed capture/camera crates. Study it closely, but never copy, translate, or closely paraphrase its non-MIT code.
- Kap (wulkano/Kap) is MIT. You may adapt its export UI and encoding pipeline (fps per format, aspect-locked size inputs, FFmpeg conversion arguments, GIF palettes), with attribution.
- File-level maps are under "References". Record attribution for anything adapted in docs/THIRD_PARTY.md.
- Hard requirement: the camera overlay is composited from its own recorded track. Never screen-capture a floating preview window.

## Tech stack
These are the defaults. Validate them with the M0 spikes (small throwaway prototypes) and record any changes in docs/DECISIONS.md.
- Rust workspace for capture, recording, decoding, rendering, editing, and export.
- Tauri v2 app shell with a small TypeScript UI (Svelte or React). The webview only draws controls; every video pixel comes from the Rust renderer.
  - Cap moved its desktop app to GPUI (Apache-2.0). If the M0 preview spike misses its targets, switch to GPUI before building the editor.
- wgpu compositor (Metal/D3D12/Vulkan), plus a CPU reference renderer for tests that run without a GPU.
- Skia (skia-safe) for text layout and rendering (emoji, strokes, shadows, line backgrounds); fontdb for system fonts.
- Audio: cpal (device input/output), rubato (resampling), signalsmith-stretch (speed changes without changing pitch).
- Global hotkeys via Tauri's global-shortcut plugin / global-hotkey.
- Packaging via the Tauri bundler on GitHub Actions: .dmg (macOS), .msi or NSIS (Windows), .AppImage + .deb (Linux).

## Media engines: best tool for each job
| Job | macOS | Windows | Linux |
|---|---|---|---|
| Screen + system audio | ScreenCaptureKit | Windows.Graphics.Capture + WASAPI loopback | xdg-desktop-portal ScreenCast + PipeWire |
| Camera / mic | AVFoundation | Media Foundation (DirectShow fallback) / WASAPI | V4L2 or portal camera / PipeWire |
| Recording encode (raw tracks) | AVAssetWriter: VideoToolbox HEVC/H.264 + AudioToolbox AAC, zero-copy | Media Foundation hardware encoder (MFT), zero-copy D3D11 | FFmpeg VAAPI/NVENC (LGPL libraries in-process); x264 via the helper if there's no hardware encoder |
| Editor decode | AVAssetReader / VideoToolbox | Media Foundation (DXVA) | FFmpeg LGPL libraries + VAAPI |
| Frames into wgpu | IOSurface → Metal texture | shared D3D11 texture | dmabuf or CPU upload |
| Export encode + mux | bundled FFmpeg helper (GPL build) | same | same |
| GIF | bundled gifski helper | same | same |

- Export encoders all run through the FFmpeg helper, each with a "Best quality" and a "Fast" option:
  - H.264: x264 by default (best quality for the file size, CRF-based presets); hardware encoders (VideoToolbox/MF/NVENC/QSV/AMF/VAAPI) for Fast.
  - HEVC: x265 or hardware, tagged hvc1.
  - AV1: SVT-AV1, or hardware AV1 where available.
  - VP9 and VP8: libvpx.
  - Audio: AAC (aac_at on macOS; Media Foundation or FFmpeg's AAC elsewhere), Opus, Vorbis, FLAC, ALAC.
- GIF: gifski by default. It gives the best quality, reads Y4M video from stdin, and exposes quality, motion-quality, and lossy settings. FFmpeg's palettegen/paletteuse (Kap's approach) is the Fast option.
- How export works: the renderer streams frames to the helper over a pipe (Y4M, or raw NV12 video). Mixed audio goes through a temporary WAV file or a second pipe. Parse the helper's progress output; Cancel stops the helper cleanly.

## Licensing
- The app binary contains only permissive code (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode, CC0; MPL-2.0 only if unavoidable), plus dynamically linked LGPL code (the Linux FFmpeg libraries). Enforce this with cargo-deny and an npm license check in CI.
- GPL/AGPL encoders run only as separate bundled executables (Tauri sidecars), never linked into the app. Kap (MIT) ships a GPL FFmpeg binary the same way.
  - ffmpeg, GPL build: x264, x265, SVT-AV1, libvpx, Opus, Vorbis, plus hardware encoders. Never --enable-nonfree.
    - macOS: build static arm64 + x86_64 binaries from source with a pinned script. markus-perl/ffmpeg-build-script (MIT) is a good reference; skip its non-free options.
    - Windows/Linux (owner decision 2026-11-19): build our own from pinned sources with --disable-network, like macOS (FFmpeg-Builds' MIT scripts may be adapted). Prebuilt BtbN releases are not used, because they include network protocols.
  - gifski (AGPL-3.0): built from source with cargo in CI for each target.
  - Mac App Store edition (owner 2026-11-24, ADR-057): the store's terms conflict with the GPL and AGPL, so it ships an LGPL-2.1-or-later ffmpeg and ffprobe (no --enable-gpl, x264 or x265) and no gifski. Best H.264/HEVC falls back to VideoToolbox and GIF to palette mode.
  - Linux in-process decoding and hardware encoding: LGPL shared FFmpeg libraries, also built by us with --disable-network.
- Never link x264, x265, gifski, or libimagequant into the app. No fdk-aac or anything else nonfree.
- Every release includes each helper's license, version, build flags, and exact source (or a link to it). Add an About → Licenses screen and third-party notices (cargo-about plus an npm license tool).
- H.264, HEVC, and AAC are covered by patents. That's fine for personal use; document it in case of wider distribution.
- Fonts: bundle only OFL/Apache-licensed fonts, with their licenses. Icons: MIT/ISC-licensed sets.

## v1 features

### Capture
- Sources:
  - Full display (multi-monitor).
  - Single window. Browser windows count; native apps can't capture an individual tab.
  - Application (all of one app's windows), where the OS supports it.
  - Region: drag on screen with resize handles, numeric W×H, and aspect presets (Free, 16:9, 9:16, 4:3, 1:1). Remembers the last region; Esc cancels.
- Options: 30 fps (default) or 60; native/Retina resolution (default) or 1×; show cursor (on by default); optional click highlight.
- Output aspect ratio (added 2026-11-19): Original (default), 16:9, 4:3, 1:1, or 9:16 (TikTok). Chosen in the recorder panel before recording and changeable later in the editor. The captured source (window, display, region, application, or anything else) is fit whole inside the chosen frame, and the letterboxed area is filled with a selectable background: a solid color, a gradient, or an image. The recorded files are never altered; the framing is applied by the compositor in the preview and in every export. An application source is captured as the area its windows cover, not the whole display: the union of the app's on-screen windows on that display when the segment starts (ADR-039). Windows moved or opened outside that area later are clipped; switching to the source again, or picking it again, re-crops; an app with no windows there records the whole display.
- Switch source while recording (added 2026-11-19): during a recording, the user can pick a new window, the full screen, a region, or another source (floating bar button and a hotkey), and the recording continues from the new source without stopping. One recording can show several windows in turn. Audio and camera keep recording without a gap; each source span is fit into the same output frame.
- Simple Capture's own UI (camera bubble, control bar, countdown, region border) must never appear in recordings:
  - macOS: exclude it with SCContentFilter.
  - Windows: SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE).
  - Linux: portals can't exclude windows, so auto-hide any overlay that overlaps the captured area while recording, and keep the controls in the tray and hotkeys.
  - A bubble that ends up in the screen capture would appear twice in the output. Test for this.
- Nice-to-have: camera-only mode.

### Camera overlay
- Camera picker: built-in, USB, Continuity Camera (macOS), and virtual cameras. The list updates as devices connect. Auto-pick the best format up to 1080p30; Settings can override.
- macOS: a "Video Effects" button that calls AVCaptureDevice.showSystemUserInterface(.videoEffects). Don't build our own background blur.
- Shape: circle, square, rectangle (4:3, 16:9, 3:4, 9:16), or rounded rectangle with a corner-radius slider (the maximum gives a circle or pill).
- Reframe: zoom in (starting from fill-the-frame) and pan the image inside the shape, with a Reset button.
- Size: resize handles plus S/M/L presets.
- Position: drag it anywhere. It snaps to corners, edges, and center with a margin; arrow keys nudge it; corner presets.
- Mirror toggle (the preview always matches the output). Optional border and soft shadow.
- Moving, resizing, or hiding the camera during recording is saved as timestamped keyframes, reproduced in the output, and editable later.
- What you see is what you get: the bubble on screen matches the output exactly.
- Single-window capture: position the camera in a live "stage preview" of the composed output.
- Placement (ADR-043, owner 2026-11-20, replaces the floating bubble of ADR-041): one Preview window shows the composed output (the source in its frame, the letterbox background and the live camera) for every source and frame shape; the camera is placed, sized and reframed there. It is open while the recorder panel is open or a recording runs (it can be turned off), and it never appears in recordings. The camera runs while the recorder panel is open or a recording is on. Not mirrored by default; mirroring is a toggle.
- The data model allows multiple overlays; 2+ cameras at once is a stretch goal.

### Audio
- Mic picker (plus "None") with a live level meter; the list updates as devices connect.
- macOS: a "Mic Mode" button that calls showSystemUserInterface(.microphoneModes).
- System audio: Off / All / Only the captured app, where the OS allows it:
  - macOS: ScreenCaptureKit filters.
  - Windows 11: process loopback.
  - Linux: "All" only in v1.
- Never capture our own app's audio.
- Mic and system audio are separate tracks, mixed in the editor and export with their own volume and mute.

### Recording flow
- Tray/menu-bar app. Global hotkeys: open recorder, start/stop, pause/resume, toggle camera. On Linux, use the GlobalShortcuts portal where available; otherwise fall back to the tray menu.
- The recorder panel opens by itself on the first launch and, on macOS, when the running app is opened again from Finder or Launchpad (2026-11-25, ADR-057); other launches show only the tray icon.
- The recorder panel remembers the last choices. Before recording, check permissions, free disk space, and devices. Optional 3-2-1 countdown.
- While recording, show a small floating bar (stop, pause/resume, restart, discard, timer, mic level) that's excluded from capture where the OS allows, plus a timer in the tray.
- When recording stops, the editor opens. Settings can change this to "quick export with last settings" or "just save to Library".

### Editor
Opens automatically after recording. It must stay light and fast.
- Layout: the preview (the compositor's actual output) on top. Below it, a simple timeline: a main track (thumbnail strip + audio waveform), a text track, a ruler, and a playhead.
- Opens in under 2 seconds for a 10-minute recording; thumbnails and waveforms fill in progressively.
- Edits never modify the recorded files, and all streams in the recording move together:
  - Trim either end by dragging clip edges.
  - Split at the playhead (S).
  - Select clips or a time range (drag on the ruler, or I/O keys). Delete/Backspace removes the selection and closes the gap (ripple delete).
  - Cut, copy, paste (⌘ on macOS, Ctrl elsewhere). Paste inserts at the playhead, and pasted pieces keep their camera layout. Duplicate; drag clips to reorder.
  - Unlimited undo/redo within the session.
  - Timeline zoom, snapping (to the playhead, cuts, and text edges), Space play/pause, J/K/L shuttle, ←/→ frame step, ↑/↓ jump between cuts.
  - Per-clip speed (0.5×, 1×, 1.5×, 2×, 4×, pitch preserved) and mute.
  - Camera overlay adjustments (same controls as the bubble; edit or clear recorded keyframes) and per-track volume and mute.
  - Short automatic audio fades at every cut so there are no clicks (Cap does the same in crates/editor join_fade.rs).
- Autosaves. Any recording can be reopened from the Library and edited further.
- Output aspect ratio and letterbox background (see Capture) can be changed for the whole project. Nice-to-have: a 4:5 preset, and fill/crop-and-pan of the screen layer instead of letterboxing.

### Text overlays (on par with Instagram/TikTok)
- Add text with a button or T; type directly on the preview; multi-line and emoji supported.
- Fonts: 10–15 curated bundled OFL or Apache-2.0 fonts, each checked to render correctly (no missing glyphs for common Latin text) before it ships (classic, modern, bold condensed, serif, typewriter, handwriting, neon, rounded) plus any installed system font.
- Color: palette, custom picker, gradient fill, and an eyedropper that samples the video.
- Styles: plain, outline, drop shadow, glow/neon, uppercase toggle, opacity, and a per-line background box (solid or translucent, rounded, with TikTok-style merged line backgrounds). Also an inverted style preset (colored box, white text). Alignment, line height, and letter spacing.
- Transform: drag with snap guides (center, thirds, safe margins), a resize handle, a rotate handle (snaps at 0°, ±15°, 90°), and layer order.
- Timing: each text item has a start and end on the text track (drag the edges; default 3 seconds starting at the playhead).
  - Text is positioned on the edited timeline, not the original recording.
  - Ripple edits shift any text that comes after the edit point; text inside a deleted range is removed. Undo restores both.
- Animations, each with adjustable duration and easing:
  - In: fade, pop, slide (4 directions), typewriter, word-by-word, bounce.
  - Out: fade, pop, slide.
  - Emphasis loops: pulse, wiggle, float.
  - Animations look identical at every export fps.
- Style presets, recent styles, and duplicate and copy/paste for text items.
- Text stays vector until export, so it's crisp at any resolution.
- Feature checklist (study only): Cap's TextSegment in crates/project/src/configuration.rs.

### Export
- Formats. The video and audio codecs are chosen separately within each container. Only show options the capability probe has confirmed work:
  - MP4. Video: H.264 (default), HEVC, AV1. Audio: AAC (default), Opus, FLAC, ALAC. Label the less-compatible choices.
  - WebM. Video: VP9 (default), AV1, VP8. Audio: Opus (default), Vorbis.
  - GIF: gifski (default) or Fast palette mode; no audio.
  - MOV (ProRes 422 + PCM) for editing.
  - Animated WebP (libwebp, Small/Balanced/High); no audio, GIF frame rates and sizes, a loop setting. The sheet offers MP4, MOV, WebM, GIF, WebP in that order.
- Encoder: "Best quality" (x264/x265/SVT-AV1/libvpx/gifski) or "Fast" (hardware/palette), with estimated time and file size.
- FPS per format. Each format has its own fps picker and remembers its own last value, capped at the recording's fps (adapt Kap's fpsHistory):
  - Video formats: 15/24/25/30/50/60.
  - GIF: 5/8/10/12/15 (default)/20/25/30/50. GIF frame timing is in hundredths of a second, and browsers slow down anything faster than 50 fps, so cap at 50 and note that 30 is approximate.
  - Text animations and camera moves render at the export fps.
- Resize (optional; the aspect ratio is always locked; adapt Kap's width/height inputs):
  - Choices: Original, height presets (2160/1440/1080/720/480, plus 360/240 for GIF), or type a width or height and the other follows.
  - Round to even numbers (except GIF). Don't upscale by default.
  - Check encoder limits (hardware H.264 tops out around 4K).
- Quality: Small / Balanced / High / Custom (CRF or bitrate).
- GIF settings: quality, motion quality, and lossy (gifski), or colors and dithering (palette mode); loop (forever/once/N); default maximum width 960 px; size estimate.
- Output folder and filename template; progress with Cancel; then Reveal / Copy file / Open. Remember the last settings per format and support named presets.

### Library and safety
- Each recording is a project folder (raw tracks + project.json + thumbnails) under Movies/Simple Capture (Videos on Windows/Linux). A Library window lists them (thumbnail, date, duration, size) with Edit, Export, Reveal, and Delete.
- Crash recovery on launch, a low-disk-space guard, and an optional "delete raw files after export" (off by default).
- Per-OS onboarding for screen, camera, and mic permissions.
- A Diagnostics page and a "simple-capture --self-test" CLI command. The self-test:
  - lists capture APIs, devices, encoders, and decoders;
  - runs a short synthetic record → edit → export (and a real capture where possible);
  - writes a report someone can share.

  That way a Windows/Linux user can run one command and send me the result.

## Architecture
- Recording writes each source to its own crash-safe file (fragmented MP4/MOV or Matroska):
  - Screen: hardware HEVC/H.264 at native resolution. Variable frame rate is fine. Put a keyframe about every second so the editor can seek quickly.
  - Camera: native resolution, same keyframe rule.
  - Mic and system audio: separate tracks.
  - project.json (schema-versioned): start time on the shared clock, per-track offsets, pauses, source details, devices, the camera layout timeline (in recording time), the edit decision list (EDL), text overlays (in edited-timeline time), and export presets.
- Edit model: an immutable project document plus command-based undo/redo. The EDL is a list of clips (source in/out, speed, mute) that point into the synchronized raw tracks.
- Rendering: one pure geometry module (layout → pixel rectangles, crops, corner radii, mirroring) and one wgpu compositor that turns (screen frame, camera frame, text layers, time) into a frame. The live bubble, editor preview, thumbnails, and export all use it, so the preview always matches the output.
  - Skia lays out and renders each text item (cached until edited) and provides glyph/word/line boxes, so animations become transforms in the compositor.
  - Inline editing may place an HTML input over the text, using the same font files for the cursor, but the final pixels always come from Skia.
  - A CPU reference renderer produces matching pixels for golden-image tests.
- Playback engine: maps edited-timeline time to (clip, source time). It decodes with the native decoders (see the table), reads ahead across cuts, caches frames, and supports scrubbing. Audio plays through cpal and drives the playback clock.
- Preview transport: frames go to the UI at the preview's on-screen size, not full resolution, through Tauri's binary IPC channel, shared memory, or a native child surface. Measure this in the spike.
- Export renders the same pipeline offline at the chosen fps and size and pipes it to the FFmpeg or gifski helper. Audio is mixed in Rust (volume, speed, fades) and handed to the helper.
- Capability probe, run on each machine and cached. It drives the export UI. It checks:
  - which helper encoders actually work (hardware encoders can be listed yet fail);
  - which container/codec pairs work, via tiny test encodes;
  - which native decode/encode paths exist.
- Clocks: convert every sample to one monotonic host clock (mach host time / QPC / CLOCK_MONOTONIC). Verify this for each backend.
- Platform layer: traits for screen, camera, and audio sources, track encoders, decoders, overlay-window behavior, and permissions. Capability flags let the UI hide what an OS can't do.
- Suggested layout:
  - crates/: core (models, EDL, undo), capture-{macos,windows,linux}, record, decode, render (wgpu + CPU reference), text (Skia), player, export, helpers (helper-process management), platform
  - app/ (Tauri), ui/, cli/
  - third_party/ (helper build scripts, pinned versions, checksums, licenses)

## References
Clone these into ./reference/ (gitignored).

Cap (study only, AGPL; never copy non-MIT code):
- crates/recording: per-OS pipelines (output_pipeline/macos.rs) and crash recovery (recovery.rs).
- crates/enc-avfoundation (AVAssetWriter), crates/enc-mediafoundation (MFT/D3D11), crates/enc-ffmpeg (how it picks encoders: libx264 with hardware fallbacks), crates/enc-gif (gifski).
- crates/video-decode: avassetreader.rs, media_foundation.rs, ffmpeg.rs.
- crates/rendering: layers/camera.rs, layers/text.rs, iosurface_texture.rs, d3d_texture.rs, yuv_converter.rs, composite_frame.rs.
- crates/editor: playback.rs, segments.rs, join_fade.rs, audio_output, thumbnail.rs.
- crates/project/src/configuration.rs: the Camera overlay model, TimelineConfiguration (segments, text_segments), and TextSegment.
- crates/export (mp4, gif, estimates) and crates/timestamp.

Cap MIT crates (may adapt with attribution): scap-screencapturekit, scap-targets, scap-direct3d, scap-cpal, cap-camera, cap-camera-avfoundation, cap-camera-windows, cap-camera-mediafoundation, cap-camera-directshow. Some depend on non-MIT Cap crates (cap-d3d-adapter, cap-mediafoundation-utils, workspace-hack). Don't pull those in, and check each file's license.

Kap (MIT; adapt with attribution):
- main/converters/h264.ts: FFmpeg arguments for GIF (palettegen/paletteuse), WebM, AV1, HEVC, APNG.
- main/converters/process.ts and main/utils/ffmpeg-path.ts: running a bundled FFmpeg and parsing its progress.
- renderer/components/editor/options/left.tsx: aspect-locked width/height inputs.
- renderer/components/editor/options-container.tsx and main/remote-states/editor-options.ts: per-format fps memory, capped at the source fps.
- renderer/containers/cropper.js: region selection with aspect lock.

Other references:
- Crates: screencapturekit, objc2-*, cidre, windows-capture, ashpd, pipewire, cpal, nokhwa, wgpu, skia-safe, fontdb, rubato, signalsmith-stretch, tauri, global-hotkey.
- Build sources to pin: markus-perl/ffmpeg-build-script (MIT), BtbN/FFmpeg-Builds, ImageOptim/gifski.
- Docs: Apple's "Capturing screen content in macOS" sample, the Windows.Graphics.Capture docs, Microsoft's ApplicationLoopback sample, and the xdg-desktop-portal ScreenCast docs.

## Platform notes
- macOS (primary):
  - The bubble and control bar don't steal focus and stay visible on all Spaces and over full-screen apps (set via objc2 on the window handle). Transparent windows no longer need Tauri's macOSPrivateApi setting (Tauri 2.12.1+, ADR-026).
  - The UI follows macOS conventions in light and dark mode (system colours and accent, AppKit-sized controls, grouped forms), with system materials behind the floating control bar and source switcher (owner 2026-11-20, ADR-044).
  - The Preview window draws its video into a Metal layer under its transparent webview (owner 2026-11-20, ADR-007), not through the webview. Windows and Linux send frames to the webview until their native paths are proven on real hardware.
  - Hardened runtime with camera/mic entitlements; sign the helpers too.
  - Sign every build with one stable Apple Development identity (a free personal team works). Ad-hoc signing makes macOS forget the Screen Recording permission on every build. Document "tccutil reset ScreenCapture <bundle-id>". (2026-11-19: the owner has no identity and may not create one. Until one exists, run development builds of the app through the Simple Capture Dev host, and expect ad-hoc-signed release builds to ask for permissions again after each update. See DECISIONS ADR-003.)
  - macOS 15+ may periodically re-ask permission for apps that don't use the system screen picker (SCContentSharingPicker). Owner decision 2026-11-19: use the system picker only if equivalent system pickers can be used on Windows and Linux too (ADR-028).
  - Distribution (owner 2026-11-24, ADR-003, ADR-057): a Developer ID-signed, notarized DMG on GitHub Releases and through the Homebrew tap, and a sandboxed Mac App Store edition built from the same tag with the cargo feature `mas`, LGPL-only helpers and no gifski. The store edition's sandbox entitlements include `com.apple.security.network.client`, because WKWebView's WebContent and Networking processes need it in any sandboxed app, even for local content; the app itself still makes no network connections.
  - Development (decided 2026-11-19): every dev/spike/test binary that needs a privacy permission runs as a child of one fixed host app, "Simple Capture Dev" (com.simplecapture.dev, tools/macos-dev-host), which is signed once and never rebuilt, so the owner approves each permission once. No per-spike bundles or bundle IDs.
- Windows:
  - Disable the yellow capture border where Windows 11 allows it. Region capture = a cropped display capture.
  - Use process loopback for "only this app" audio (Windows 11).
  - Make the app per-monitor DPI aware. Use named pipes for helper I/O if anonymous pipes are too slow.
- Linux:
  - Save the portal's restore token so it doesn't ask again every time.
  - Region selection: draw the region on a frozen frame of the chosen monitor, then crop.
  - Wayland offers no way to exclude windows from capture, keep a window always on top, or position windows globally, so auto-hide overlapping overlays during capture.
  - Capture system audio from the default output device's monitor source. An X11-only fallback is optional.

## Pitfalls to handle explicitly
1. Screen capture APIs only deliver frames when something changes on screen. Never drive timing from screen frames, and extend the screen track to the moment recording stopped.
2. Audio/video sync across four sources, including pauses. Prove it with a test using an on-screen flash, a beep, and a clap.
3. Coordinates and DPI: macOS (Cocoa) puts the origin at the bottom-left; everything else uses top-left. Expect monitors with different scaling, and keep points and pixels distinct.
4. Native Retina capture can be 5K or more. Use HEVC for raw tracks, measure how much headroom the encoder has on an M1, and suggest 30 fps or 1× if frames drop.
5. Color: sRGB in, Rec.709 SDR out, tagged explicitly, including the pixel format piped to helpers. Check every format against a test pattern. Exception: VP8 cannot signal Rec.709, so VP8 exports use the BT.601 matrix and are tagged that way, so players show the right colors (ADR-029).
6. Hardware encoders can be listed yet fail to open, or fail at certain sizes. Probe by actually encoding, and fall back gracefully.
7. Encoding details: even dimensions for 4:2:0 video, the hvc1 tag for HEVC, and hardware H.264 size limits.
8. Cuts must land on the exact frame (decode from the previous keyframe), with tiny audio fades at each cut.
9. Text must lay out identically in preview and export: same Skia engine, bundled fonts, and never the webview for export. Color emoji must render in exports on every OS.
10. GIF frame timing (hundredths of a second, max 50 fps) and file size (downscale by default).
11. Helper processes: limit pipe buffering so a slow helper applies backpressure, parse stderr and progress, cancel cleanly with timeouts, leave no orphaned processes after quitting or a crash, and show clear errors when a helper is missing or fails.
12. Devices plugged in or removed, the captured window closing, a display disconnecting, sleep/wake: keep what's recorded, finalize the files, and notify the user.
13. Frame processing: no work on the UI thread, reuse buffers from a pool, use bounded queues that count dropped frames, and never hold on to capture buffers (that starves the capture system of buffers).
14. Webviews: never stream full-resolution frames to the UI. WebKitGTK (Linux) is the slowest webview, so test the preview path there.

## Engineering and testing
- Stable Rust and a cargo workspace; pnpm for the UI. A justfile/Makefile with setup, helpers (fetch/build/verify), dev, test, and bundle targets.
- GitHub Actions on macOS (arm64), Windows (x64), and Ubuntu (x64): build, clippy, tests, license checks, helper builds (cached), and bundle artifacts. Keep all three green from M1 on; placeholder backends are fine until M6.
- GPUs in CI: use wgpu's software adapters (WARP, lavapipe). If a CI machine has no usable GPU, run the GPU golden-image tests locally on my Mac and the CPU reference renderer in CI.
- Tests:
  - Geometry: layouts, crops, snapping, aspect-locked resize.
  - Edit operations (split, trim, ripple delete, cut/copy/paste, reorder, speed), including how text shifts, plus randomized undo/redo tests.
  - Golden images: compositor output, every text style, and animations at sampled times (GPU vs. CPU within a tolerance).
  - Export integration with synthetic sources (moving pattern, burned-in timecode, 1 kHz beeps, emoji text). Validate every available format/codec/encoder/fps combination with ffprobe (codecs, dimensions, fps, duration within one frame, audio), and check GIF frame timings.
  - The recording pipeline with synthetic sources, including killing the app mid-recording and recovering.
- Logging: tracing plus a session.log per project; a debug overlay showing fps, dropped frames, and audio/video offsets.
- docs/MANUAL_TEST.md: a full checklist for macOS and short ones for Windows and Linux.

## Milestones
At each ★, stop, summarize, tell me what to test, and wait for my OK.
- M0 ★ Research, plan, and spikes. Read the Cap and Kap files above and check my Mac (sw_vers, chip). Spikes:
  1. Helpers: a pinned static GPL FFmpeg for macOS arm64, plus gifski built with cargo. Pipe synthetic frames through x264 MP4, x265, SVT-AV1, VP9/Opus WebM, and gifski GIF, and measure speed on an M1.
  2. Tauri preview transport: 1080p composited frames to the editor at ≥30 fps with low latency on an M1.
  3. macOS pipeline: ScreenCaptureKit + camera + mic + system audio → separate AVAssetWriter tracks → AVAssetReader decode → zero-copy wgpu texture.
  4. Skia text with emoji, rounded per-line backgrounds, stroke, and shadow, composited in wgpu.

  Then write docs/ARCHITECTURE.md, DECISIONS.md, CODEC_MATRIX.md (per OS), and LICENSES.md.
- M1 ★ macOS recording: sources, permissions, separate crash-safe tracks, project file, switching source while recording, output aspect ratio with letterbox background, a quick x264 MP4 export, and the sync test. A CI skeleton for all three OSes.
- M2 ★ Camera bubble on macOS: shapes, reframing, size/position/snapping, mirror, keyframes, stage preview, excluded from capture.
- M3 ★ Editor v1: playback across cuts, timeline, trim/split/delete/cut/copy/paste/reorder/speed, undo/redo, autosave, overlay and audio adjustments.
- M4 ★ Text overlays (the full feature set above).
- M5 ★ Export: all formats, encoders, and codecs; fps per format; gifski GIF; aspect-locked resize; quality and speed settings; presets; progress and cancel.
- M6 ★ Windows and Linux backends, packaging for all three OSes, self-test and diagnostics.
- M7 ★ Hardening and polish: pause/resume, countdown, hotkeys, Library, recovery UI, disk-space guard, device hot-plug, a performance pass, licenses screen, README.

## Definition of done
- macOS: a 30-minute recording (native-resolution display + 1080p camera + mic + system audio) with no crash, under 0.5% dropped frames, stable memory, and audio/video sync within one frame at both the start and the end.
- The bubble matches the output. No app UI appears in captures on macOS/Windows; auto-hide works on Linux.
- Editor: opens in under 2 seconds for a 10-minute recording, plays smoothly across cuts, and every edit operation plus undo/redo is covered by tests.
- Every text style and animation looks identical in the preview and in every export format.
- Every export combination offered passes ffprobe validation and plays in QuickTime, Chrome, Firefox, or VLC as appropriate. GIFs play at the chosen fps in browsers. x264 "Best quality" scores higher on SSIM (an image-quality metric) than hardware "Fast" at the same file size on a text-heavy test clip.
- Force-quitting mid-recording is recoverable, and unplugging the camera or mic doesn't stop the recording.
- CI is green on all three OSes, with downloadable builds. The app binary contains no GPL/AGPL code, and the helpers' license and source files are included.
- No networking code anywhere.

## Working agreements and non-goals
- Ask before adding dependencies, changing the architecture or stack, or dropping a requirement. Log decisions in docs/DECISIONS.md and keep docs/STATUS.md current.
- Have working software at every milestone, macOS first. Windows/Linux work must never block macOS milestones, but those platforms must keep compiling and passing tests.
- Check assumptions with tiny spikes and update the spec rather than guessing.
- Out of scope for v1: cloud or share links, transcription/AI, drawing/annotations, auto-zoom, background beautification (padding, rounded corners, shadows around the screen; the letterbox background for the output aspect ratio is in scope), transitions or an effects library, layering anything beyond screen + camera + text, live streaming. (App Store and notarized distribution were here until the owner decided on 2026-11-24 to ship both; see Platform notes and DECISIONS ADR-057.)
