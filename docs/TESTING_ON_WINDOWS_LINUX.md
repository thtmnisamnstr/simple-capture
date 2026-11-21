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

## 2. Build tools

- **Windows 11:** Visual Studio 2022+ Build Tools (C++), LLVM 22 or newer (`choco install llvm`),
  `ninja`, Python 3, Node 22 with pnpm, rustup (the toolchain in `rust-toolchain.toml` installs
  itself). WebView2 ships with Windows 11.
- **Ubuntu 24.04:** the packages in `.github/actions/setup/action.yml` (both the base list and the
  `full` list), which include the GStreamer development packages, plus Node 22 with pnpm.

```bash
pnpm --dir ui install --frozen-lockfile && pnpm --dir ui build
```

```bash
cargo build -p simple-capture-app
```

The first build compiles Skia and takes 20–40 minutes.

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

## 4. Which media path is in use

Start the app once (`cargo run -p simple-capture-app`), quit it from the tray, and read the log:
`%APPDATA%\com.simplecapture.desktop\logs\simple-capture-app.log` on Windows,
`~/.local/share/com.simplecapture.desktop/logs/simple-capture-app.log` on Linux. One line says
which path records tracks:

- Windows: `tracks: Media Foundation (beta)`, or `tracks: FFmpeg helper` with the reason above it.
- Linux: `tracks: GStreamer (beta)` with the encoder (`vah264enc`, `vaapih264enc`, `nvh264enc` or
  `openh264enc`), or `tracks: FFmpeg helper`. To list what GStreamer offers:
  `gst-inspect-1.0 | grep -E "h264enc"`.

`SC_MEDIA=helper` forces the helper path on either system; use it to tell a beta-path problem
from anything else.

## 5. Live checks (announce first: these record the real screen, camera and microphone)

Run the app, then go through docs/MANUAL_TEST.md §13 (Windows WN1–WN9 or Linux LX1–), then these
beta checks:

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
the test recordings afterwards (they are in Videos/Simple Capture or ~/Videos/Simple Capture).
