# simple-capture

A desktop screen recorder for macOS, Windows and Linux: record the screen, your camera and
sound, edit, add text, and export. No account, and it makes no network connections: what you
record stays on your computer.

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
  and animated WebP, with quality presets, CRF/bitrate or a size limit, destinations (Email,
  Slack, YouTube, Reels/Shorts, GIF), per-format frame rates, aspect-locked resizing, size and
  time estimates, and named presets.
- **Library** of every recording, with Edit, Export, Show and Move to Trash.
- **Self-test** (`simple-capture --self-test`, or About → Diagnostics) writes a report of what
  your computer can do, with no recordings in it.

| | macOS 14+ | Windows 10 2004+ | Linux (PipeWire, xdg-desktop-portal) |
|---|---|---|---|
| Screen | ScreenCaptureKit | Windows.Graphics.Capture | Portal ScreenCast + PipeWire |
| Camera | AVFoundation (and iPhone/iPad over USB) | Media Foundation | V4L2 (NV12, YUYV, MJPEG) |
| Mic / system audio | AVFoundation / ScreenCaptureKit | WASAPI / loopback | PipeWire |
| Recording encode | VideoToolbox HEVC | GPU encoder or Media Foundation through the helper, else x264 | x264 through the helper |
| Export H.264, HEVC, GIF | VideoToolbox, VideoToolbox, FFmpeg palette | x264, x265, gifski (or the Fast encoders) | x264, x265, gifski (or the Fast encoders) |
| Our windows in the video | excluded | excluded | hidden while recording |

macOS is the primary platform. The Windows and Linux builds are experimental: they compile and
pass their tests in CI, but have not yet been tried on a real desktop; see `docs/MANUAL_TEST.md` §13.

## Install

macOS 14 or later on Apple silicon, three ways to the same app:

- **Download** the `.dmg` from [Releases](https://github.com/thtmnisamnstr/simple-capture/releases)
  and drag simple-capture to Applications. It is signed with a Developer ID and notarized by
  Apple, so it opens normally and keeps its Screen Recording, Camera and Microphone permissions
  across updates. Check a download against `SHA256SUMS.txt`, or with
  `gh attestation verify <file> --repo thtmnisamnstr/simple-capture`.
- **Homebrew:** `brew install --cask thtmnisamnstr/simple-capture/simple-capture` installs that DMG, and
  `brew upgrade` updates it.
- **Mac App Store:** coming soon, with updates through the App Store.

All three are the same sandboxed app; only the signature differs. On macOS, H.264 and HEVC
exports use Apple's VideoToolbox encoders (there is no x264 or x265 "Best Quality" setting), GIFs
use FFmpeg's palette, and media keys can't be shortcuts. The store and DMG copies are both
`simple-capture.app` with the same bundle ID, so one replaces the other, and switching between
them asks for the permissions again.

Releases also have an NSIS setup `.exe` for Windows (a per-user install, not code-signed yet, so
SmartScreen asks to confirm). Builds of any commit are under GitHub Actions → package → a run →
Artifacts.

### Linux

x86_64 and arm64, on Wayland or X11, GNOME and KDE tested. The app uses the desktop's own screen
picker (the xdg-desktop-portal), remembers the last screen or window you shared, and takes its
global shortcuts through the portal on Wayland.

- **Debian 13, Ubuntu 24.04 and newer:** from the first Linux release, the signed apt repository at
  https://thtmnisamnstr.github.io/simple-capture (instructions there), or the `.deb` from Releases.
- **Fedora:** the signed dnf repository at the same address, or the `.rpm` from Releases.
- **Arch Linux:** `simple-capture-bin` in the AUR (from the first Linux release).
- **Any distro:** the Flatpak (Flathub, coming) or the `.AppImage` from Releases; the Snap is
  coming to the Snap Store.

Linux builds the screen track with GStreamer (VA-API, NVENC or OpenH264), so install your
distro's GStreamer "bad" plugins for hardware encoding; the packages recommend them.

## Build from source

Requirements: Rust 1.95 (`rust-toolchain.toml`), Node 26 with pnpm, and on Linux the packages
listed in `.github/actions/setup/action.yml`.

```bash
just setup      # tools check, UI dependencies
just helpers    # the LGPL FFmpeg helpers from the pinned sources (macOS)
just run        # run the app
just test       # the test suite
just bundle     # an installable bundle for this OS
```

Linux and Windows helpers: `third_party/ffmpeg/build-linux.sh linux|windows` (Windows is
cross-built with mingw-w64), then `scripts/stage-helpers.sh`.

## Privacy

simple-capture makes no network connections: there is no networking code in the app, and the
bundled FFmpeg is built with networking disabled. The Mac app holds the sandbox's
outgoing-network entitlement only because macOS's web view, which draws the app's windows, needs
it. Recordings, settings and self-test reports stay in your Movies/Videos folder and the app's
data folder (on macOS, its sandbox container). The full privacy policy is in
[docs/PRIVACY.md](docs/PRIVACY.md).

## Licenses

simple-capture is MIT licensed (`LICENSE`). Exports are encoded by separate helper programs
that ship beside the app. On macOS (DMG and Mac App Store alike) that is FFmpeg alone,
LGPL-2.1-or-later with SVT-AV1, libvpx, Opus, Vorbis and libwebp. On Windows and Linux it is
FFmpeg under GPL-2.0-or-later (adding x264 and x265) and gifski (AGPL-3.0-or-later). Their
license texts are in the app (About → Licenses), and their corresponding source is attached to
each GitHub release, which the Source code button in About → Licenses opens. Bundled fonts are under the SIL Open Font
License or Apache-2.0. Details: `docs/LICENSES.md`, `docs/THIRD_PARTY.md`.

## Documentation

- `docs/SPEC.md`: what the app must do
- `docs/ARCHITECTURE.md`, `docs/DECISIONS.md`: how it is built and why
- `docs/CODEC_MATRIX.md`: encoders, formats and measured speeds
- `docs/STATUS.md`: where things stand, open questions
- `docs/MANUAL_TEST.md`: the hands-on test script
- `docs/RELEASING.md`: signing, releases, Homebrew and the Mac App Store
- `docs/PRIVACY.md`: the privacy policy
