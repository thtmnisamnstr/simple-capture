# Testing on Windows and Linux (for Claude on the owner's machines)

The Windows and Linux backends were built and tested on CI runners and in a Linux container, never
on a real desktop. This guide is for a Claude Code session on the owner's Windows PC or Linux box:
what to build, what to run, what to look at, and what to send back. Work through it top to
bottom; stop and report at the first thing that fails.

Ground rules (same as on the Mac, see CLAUDE.md): never change OS privacy settings for the owner,
announce before recording the real screen, camera or microphone, delete recordings made for
testing, and never commit recordings or logs with personal content.

## 1. Get the code and the helpers

```bash
git clone https://github.com/thtmnisamnstr/simple-capture.git
```

```bash
cd simple-capture && git checkout m3
```

The FFmpeg and gifski helpers are built by the `package` workflow. Download the ones for this
machine from the newest successful run of that workflow on `m3`:

```bash
gh run list --workflow package.yml --branch m3 --limit 3
```

```bash
gh run download <run-id> -n helpers-windows-x86_64 -D third_party/out/windows-x86_64
```

(or `-n helpers-linux-x86_64 -D third_party/out/linux-x86_64`). Then point the app at them:
`SC_FFMPEG`, `SC_FFPROBE` and `SC_GIFSKI` set to the three binaries (PowerShell:
`$env:SC_FFMPEG = "$PWD\third_party\out\windows-x86_64\ffmpeg.exe"`). The installers from the
same run (`simple-capture-windows-x86_64`, `simple-capture-linux-x86_64`) carry the helpers
inside and need none of this.

The `package` workflow builds Linux helpers for x86_64 only. On an arm64 Linux machine (Asahi on
Apple silicon, for example), build them from the same pinned sources instead, which takes about
5 minutes:

```bash
third_party/fetch.sh && third_party/ffmpeg/build-linux.sh linux && third_party/gifski/build.sh linux
```

Debug builds find them in `third_party/out/linux-arm64` without `SC_FFMPEG`.

## 2. Build tools

- **Windows 11:** Visual Studio 2022+ Build Tools (C++), LLVM 22 or newer (`choco install llvm`),
  `ninja`, Python 3, Node 26 with pnpm, rustup (the toolchain in `rust-toolchain.toml` installs
  itself). WebView2 ships with Windows 11.
- **Ubuntu 24.04:** the packages in `.github/actions/setup/action.yml` (both the base list and the
  `full` list), which include the GStreamer development packages, plus Node 26 with pnpm.

```bash
pnpm --dir ui install --frozen-lockfile && pnpm --dir ui build
```

```bash
cargo build -p simple-capture-app
```

The first build compiles Skia and takes 20–40 minutes.

- **Kernels with 16 KB pages (Asahi):** the `gn` that skia-bindings downloads aborts with
  `<jemalloc>: Unsupported system page size`, and the build only reports an empty `""` panic at
  `skia-bindings/build_support/skia/config.rs:362`. Install Ubuntu's `generate-ninja` and point
  the build at it: `SKIA_GN_COMMAND=/usr/bin/gn`. `getconf PAGESIZE` says which kind you have.
- **Little memory:** dependencies build at `opt-level=3`, and Skia's ninja runs one job per core.
  On 8 GB or less, build with `cargo build -j2` and limit ninja too (`SKIA_NINJA_COMMAND` pointing
  at a wrapper that adds `-j2`); an unlimited build can push the desktop into the OOM killer.

## 3. Automated checks (no recording, no prompts)

```bash
cargo test -p simple-capture-capture-windows -- --nocapture
```

On Windows this runs the Media Foundation round trips (video with skipped frames, seeking,
audio with a gap, a track left unfinished as after a crash). On Linux use
`cargo test -p simple-capture-capture-linux -- --nocapture` (the GStreamer writer; it needs
`gstreamer1.0-plugins-ugly` for its test encoder).

```bash
cargo test --workspace -- --test-threads=2
```

Report every failure with its full output.

### End to end without a dialog (Linux)

`tools/linux-dev/e2e.sh FLOW` runs the debug app with a driver flow in a private headless sway
session whose portal picks its only screen without asking, so recording, the editor and exports
can be tested with no clicks, nothing on the real screen and no focus taken. It needs `sway`,
`xdg-desktop-portal-wlr` and `gstreamer1.0-plugins-bad`.

```bash
cargo build -p simple-capture-app && tools/linux-dev/e2e.sh tools/linux-dev/flows/editor.txt
```

`flows/record.txt` records 10 s; `flows/editor.txt` also edits, plays and exports MP4, GIF and
WebM. Output goes to `target/linux-e2e/<stamp>`. The audio graph there has no hardware clock, so
leave the mic and system audio off (the default settings): a fake source records silence, and a
null sink can deliver no first sample.

## 4. Which media path is in use

Start the app once (`cargo run -p simple-capture-app`), quit it from the tray, and read the log:
`%APPDATA%\com.simplecapture.desktop\logs\simple-capture-app.log` on Windows,
`~/.local/share/com.simplecapture.desktop/logs/simple-capture-app.log` on Linux. One line says
which path records tracks:

- Windows: `tracks: Media Foundation (experimental)`, or `tracks: FFmpeg helper` with the reason above it.
- Linux: `tracks: GStreamer (experimental)` with the encoder (`vah264enc`, `vaapih264enc`, `nvh264enc` or
  `openh264enc`), or `tracks: FFmpeg helper`. To list what GStreamer offers:
  `gst-inspect-1.0 | grep -E "h264enc"`.

`SC_MEDIA=helper` forces the helper path on either system; use it to tell a experimental-path problem
from anything else.

## 5. Live checks (announce first: these record the real screen, camera and microphone)

Run the app, then go through docs/MANUAL_TEST.md §13 (Windows WN1–WN9 or Linux LX1–), then these
experimental checks:

| # | Step | Expected |
|---|---|---|
| BT1 | Record the screen 60 s with mic, play a video with sound in the middle | Tracks in the project folder are `.mp4` (Windows) or `screen*.mkv` written by GStreamer (Linux log) |
| BT2 | Open it in the editor, scrub to the middle and the end, play | Frames appear within a second of a seek; sound stays in step with the video |
| BT3 | Export MP4 at 1080p | It plays in the system player; length matches the edit |
| BT4 | Record 30 s, then end the app from Task Manager / `kill -9` mid-recording; start it again | "A recording from a previous session was recovered"; it opens and plays up to near the kill |
| BT5 | Repeat BT1 with `SC_MEDIA=helper` | Same result through the helper (the baseline) |
| BT6 | Task Manager / `top` while recording 1080p30 | Note the app's CPU with each path |

Then the features added after M7 (§10c E18–E25, §10f R12–R17, §10g Y9–Y12 of MANUAL_TEST).

## 6. What to send back

A short report with: the OS and GPU, which path section 4 found, each failing step with what
happened, the last 200 lines of the log after any failure, and the CPU numbers from BT6. Delete
the test recordings afterwards (they are in Videos/simple-capture or ~/Videos/simple-capture).

## 7. Findings on Ubuntu 26.04, GNOME 50 on Wayland, Asahi (aarch64), 2026-11-27

Tested on branch `linux-testing`: the whole workspace suite (465 tests), the PipeWire and portal
tests, and the app driven end to end in the headless session (portal pick, recording, editor,
MP4/GIF/WebM export). Fixed:

- Every portal recording crashed the app. xdg-desktop-portal-wlr shares memfd frames not flagged
  readable, PipeWire mapped them `PROT_NONE`, and the first read segfaulted; the app now maps
  such frames read-only itself.
- The GStreamer screen track never worked without VA-API or NVENC: `openh264enc` takes only I420
  and was fed NV12. A `videoconvert` now sits before the encoder.
- The tray could not stop a recording: AppIndicator trays send no clicks, so the menu now stays.
- The mic list showed recording streams as microphones (our own while recording, which broke the
  panel's list with a duplicate key; on Asahi a second "MacBook Air J313 Microphone").
- After every relaunch the panel offered the last portal pick, which no longer exists, and Record
  failed with "picked source … is no longer available".

Still open:

- Global shortcuts do not fire on Wayland: `tauri-plugin-global-shortcut` grabs keys through
  X11 only. GNOME 48+ has the GlobalShortcuts portal (version 1 here).
- The portal is asked to forget the pick (`PersistMode::DoNot`); SPEC asks for a saved restore
  token.
- A Stop pressed while a recording is still starting is refused ("not recording") and lost; the
  start can take 25 s when xdg-desktop-portal is slow to open the PipeWire remote.
- The portal dialog has no parent window ("Failed to associate portal window with parent window").
- Not yet tried on the real GNOME desktop: the portal dialog with Mutter's buffers, a real
  microphone's sound in a recording, system audio from a real sink, and the camera.
