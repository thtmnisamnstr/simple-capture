# Changelog

## 0.1.0 (unreleased)

The first release of Simple Capture: a screen recorder and editor for macOS, Windows and Linux
with no account and no network.

### Record
- A display, a window, an app, a region, a camera, or an iPhone or iPad plugged in by cable
  (its screen and its sound), with an optional camera bubble, the microphone and system audio.
- Every source on its own crash-safe track; a force-quit loses at most about a second, and the
  next launch recovers the take.
- Switch sources while recording; a window resized mid-take gets a new section at its new size.
- Pause, resume and restart a take; click highlights; output shapes original, 16:9, 4:3, 1:1,
  4:5 and 9:16 with a solid, gradient or image background, and Fit or Fill per section.

### Edit
- Trim, split, cut, copy, paste, reorder, speed changes with pitch kept, J/K/L shuttle,
  snapping, undo and redo, and autosave.
- Camera placement keyframes, and screen and camera tracks that can be unlinked and edited
  apart.
- Per-track volume and mute, waveforms, silence removal, and blur or black boxes.
- Text overlays: 13 bundled font families plus system fonts, colour emoji, outlines, shadows,
  glow, boxes, gradients, layer order, and in, out and loop animations.

### Export
- MP4 (H.264, HEVC, AV1), MOV (ProRes 422, H.264, HEVC), WebM (VP9, AV1, VP8), GIF (gifski or a
  palette) and animated WebP.
- Quality presets, CRF or bitrate, or a file-size limit; destinations for email, Slack and
  Teams, YouTube, and vertical video; per-format frame rates, sizes, estimates and named presets.

### Distribution
- The macOS app and DMG are signed with a Developer ID and notarized, so permissions survive
  updates. The DMG opens on a drag-to-Applications window.
- Simple Capture checks permissions without asking; macOS asks only when you press Allow.

### Library and diagnostics
- A library of every recording with posters, Edit, Export, Show, Move to Trash, and what was
  recovered from damaged takes.
- A self-test report (`simple-capture --self-test`, or About → Diagnostics) with no recordings in
  it.

### Known limits
- The Windows installers are not code-signed, so SmartScreen asks to confirm them.
- The Windows and Linux builds are experimental: they pass CI but have not been tried on a real
  desktop yet.
- Linux records what the system's share dialog picks (displays and windows), and hides the
  app's own windows while recording.
- Copy-protected apps (Apple Music and most streaming apps) send silence to every recorder,
  including over USB from an iPhone.
