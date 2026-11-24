# Simple Capture

A desktop screen recorder for macOS, Windows and Linux: record the screen, your camera and
sound, edit, add text, and export. No account and no network: what you record stays on your
computer.

## What it does

- **Record** a display, a window, an app, a region, a camera, or an iPhone or iPad plugged in
  by cable (its screen and its sound), with an optional camera bubble, your microphone and
  system audio. Every source goes to its own crash-safe track, so a force-quit loses at most a
  second. Switch sources while recording (a resized window gets a new section at its new size),
  pause and resume, restart a take. Choose an output shape (original, 16:9, 4:3, 1:1, 4:5, 9:16)
  with a solid, gradient or image background, and Fit or Fill per section. Clicks can be
  highlighted.
- **Edit**: trim, split, cut, copy, paste, reorder, change speed (pitch kept), J/K/L shuttle,
  snapping, undo/redo, camera placement keyframes, separate screen and camera tracks, per-track
  volume, blur or black boxes, silence removal, and autosave.
- **Text overlays**: 13 bundled font families plus your system fonts, colour emoji, outlines,
  shadows, glow, boxes, gradients, layer order, and in/out/loop animations that look the same at
  any frame rate.
- **Export** MP4 (H.264, HEVC, AV1), MOV (ProRes 422, H.264, HEVC), WebM (VP9, AV1, VP8), GIF
  (gifski or a palette) and animated WebP, with quality presets, CRF/bitrate or a size limit,
  destinations (Email, Slack, YouTube, Reels/Shorts, GIF), per-format frame rates,
  aspect-locked resizing, size and time estimates, and named presets.
- **Library** of every recording, with Edit, Export, Show and Move to Trash.
- **Self-test** (`simple-capture --self-test`, or About → Diagnostics) writes a report of what
  your computer can do, with no recordings in it.

| | macOS 14+ | Windows 10 2004+ | Linux (PipeWire, xdg-desktop-portal) |
|---|---|---|---|
| Screen | ScreenCaptureKit | Windows.Graphics.Capture | Portal ScreenCast + PipeWire |
| Camera | AVFoundation (and iPhone/iPad over USB) | Media Foundation | V4L2 (NV12, YUYV, MJPEG) |
| Mic / system audio | AVFoundation / ScreenCaptureKit | WASAPI / loopback | PipeWire |
| Recording encode | VideoToolbox HEVC | GPU encoder or Media Foundation through the helper, else x264 | x264 through the helper |
| Our windows in the video | excluded | excluded | hidden while recording |

macOS is the primary platform. The Windows and Linux builds compile and pass their tests in CI,
but have not yet been tried on a real desktop; see `docs/MANUAL_TEST.md` §13.

## Install

Builds are made by the `package` workflow (GitHub Actions → package → a run → Artifacts):
`.dmg` for macOS, an NSIS setup `.exe` and an `.msi` for Windows, `.deb` and `.AppImage` for
Linux. They are not notarized or code-signed, so macOS needs right-click → Open the first time
and Windows SmartScreen asks to confirm. Because an unsigned app's identity changes with each
version, macOS asks again for Screen Recording, Camera and Microphone after an update.

## Build from source

Requirements: Rust 1.95 (`rust-toolchain.toml`), Node 26 with pnpm, and on Linux the packages
listed in `.github/actions/setup/action.yml`.

```bash
just setup      # tools check, UI dependencies
just helpers    # FFmpeg and gifski helpers from the pinned sources (macOS)
just run        # run the app
just test       # the test suite
just bundle     # an installable bundle for this OS
```

Linux and Windows helpers: `third_party/ffmpeg/build-linux.sh linux|windows` (Windows is
cross-built with mingw-w64), then `scripts/stage-helpers.sh`.

## Privacy

There is no networking code in the app, and the bundled FFmpeg is built with networking
disabled. Recordings, settings and self-test reports stay in your Movies/Videos folder and the
app's data folder.

## Licenses

Simple Capture is MIT licensed (`LICENSE`). Exports are encoded by two separate helper programs
that ship beside the app: FFmpeg (GPL-2.0-or-later, with x264, x265, SVT-AV1, libvpx, Opus and
Vorbis, libwebp) and gifski (AGPL-3.0-or-later). Their license texts are in the app (About → Licenses) and
their corresponding source is published with each release. Bundled fonts are under the SIL Open
Font License or Apache-2.0. Details: `docs/LICENSES.md`, `docs/THIRD_PARTY.md`.

## Documentation

- `docs/SPEC.md`: what the app must do
- `docs/ARCHITECTURE.md`, `docs/DECISIONS.md`: how it is built and why
- `docs/CODEC_MATRIX.md`: encoders, formats and measured speeds
- `docs/STATUS.md`: where things stand, open questions
- `docs/MANUAL_TEST.md`: the hands-on test script
