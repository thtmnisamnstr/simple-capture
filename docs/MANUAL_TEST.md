# Manual test checklist

The steps only a person can do: real clicks and key presses, region drags, system picker picks,
plugging and unplugging hardware, sleep and wake, and the release bundle, which agents never
launch. Everything else is covered by automated tests (`just test`, `just e2e`, `just app-driver`,
`just sync-test`, `just soak N`, `just killtest`; docs/M1_PLAN.md §6).

Every step has an ID, what to do (**Do**), the result that counts as a pass (**Expect**) and where
to check it (**Look**). Write the outcome in the results log (§11) and every Screen Recording,
Camera or Microphone prompt you see in the prompt log (§12).

Sections 1–10 cover macOS (M1). Windows and Linux are in §13.

---

## 0. Before you start

### 0.1 Setup (once)

```bash
cd ~/Documents/Development/simple-capture      # the main checkout, on the branch under test
just setup                                      # toolchain, ui/node_modules
just helpers                                    # FFmpeg/ffprobe helpers (once; needed for export)
cargo build --locked -p simple-capture-cli -p sc-devtools
```

- Everything with a permission (the app, CLI recordings) runs as a child of
  `~/Applications/Simple Capture Dev.app` through `tools/macos-dev-host/run.sh`; `just run` does
  this for you. The dev host already holds the Screen Recording, Camera and Microphone grants.
  **Expected: no permission prompt anywhere in §1–§9.** If one appears, write it in §12.
- `just run` takes the live lock. If an agent's live run holds it, the terminal waits (up to
  30 min) and prints `==== LIVE RUN START ...` when the lock is yours. While an agent's live run
  banner is up, stay off the screen.
- `just run` keeps the Mac awake (`caffeinate`) while the app runs; an explicit Sleep still works.
- Stop the app with tray → **Quit**. Ctrl-C in the `just run` terminal also quits gracefully.
- To try the real app (`com.simplecapture.desktop`, its own grants) build it with
  `scripts/local-bundle.sh` (release build in `target/skia/release/bundle/macos/`). It is signed
  with the self-signed "Simple Capture Local Signing" certificate in the login keychain when
  present, so Screen Recording, Camera and Microphone stay granted across rebuilds; without it
  the bundle is ad hoc signed and every rebuild needs the grants again.

### 0.2 Where to look

| What | Where |
|---|---|
| Projects | `~/Movies/Simple Capture/<YYYY-MM-DD HH.MM.SS> <title>.sc/` (a plain folder) |
| Inside a project | `project.json` (state, segments, tracks, health), `journal.jsonl` (one event per line), `session.log`, `tracks/screen-000.mov`, `screen-001.mov`, … one per segment, `tracks/camera.mov`, `tracks/mic.mov`, `tracks/system_audio.mov` |
| Exports | next to the project folder: `<project name without .sc>.mp4` (then ` 2.mp4`, …) |
| App log | the `just run` terminal, and `~/Library/Application Support/com.simplecapture.desktop/logs/simple-capture-app.log` |
| Settings, probe cache | `~/Library/Application Support/com.simplecapture.desktop/` (`settings.json`, `probe-v1.json`) |

`SC_PROJECTS_DIR=/some/folder just run` records somewhere else (used in §7.4).

### 0.3 Shell helpers

Paste once into the terminal you use for checks (not the one running `just run`):

```bash
cd ~/Documents/Development/simple-capture
FFPROBE="$PWD/third_party/out/macos-arm64/ffprobe"
SC="$PWD/target/debug/simple-capture"
sclast() { ls -dt "${SC_PROJECTS_DIR:-$HOME/Movies/Simple Capture}"/*.sc | head -1; }
scsize() { "$FFPROBE" -v error -select_streams v:0 -show_entries stream=width,height,r_frame_rate:format=duration -of compact=p=0 "$1"; }
scshow() {
  local d="${1:-$(sclast)}"
  python3 - "$d" <<'EOF'
import json, os, sys
d = sys.argv[1]
p = json.load(open(os.path.join(d, "project.json")))
r = p["recording"]
print("project:", os.path.basename(d.rstrip("/")))
print("state:  ", p["state"], "   canvas:", json.dumps(p["output"].get("canvas", {}).get("aspect")))
for s in r["screen"]["segments"]:
    end = s.get("end_rec_ns")
    print(f'  {s["id"]:<4} {s["start_rec_ns"]/1e9:7.2f} -> {"open" if end is None else f"{end/1e9:7.2f}"} s'
          f'  {s["frame_px"]["w"]}x{s["frame_px"]["h"]}  via={s["via"]}  end={s.get("end_reason")}'
          f'  source={json.dumps(s["source"])[:70]}')
for t in r["tracks"]:
    h = t["health"]
    dur = t.get("duration_ns")
    print(f'  {t["id"]:<13} {t["file"]:<24} {"?" if dur is None else f"{dur/1e9:.2f}"} s'
          f'  frames={h["frames"]} dropped={h["dropped"]} gaps={len(h["gaps"])}')
EOF
  echo "  journal (last 4):"; tail -n 4 "$d/journal.jsonl" | sed 's/^/    /'
}
```

`scshow` prints the newest project (or the one you pass): its state, every segment (times,
pixel size, `via`, end reason, source) and every track (duration, frames, dropped, gaps).

**A good recording, in general:** `state: finalized`; segments tile the timeline (each one starts
where the previous one ended, the last one ends at the stop); `dropped` is 0 or close to it; the
audio tracks run the whole recording; the journal ends with `{"ev":"stop",…,"reason":"user"}` (or
the reason the step names). QuickTime plays every `tracks/*.mov`.

### 0.4 Checking a recording for our own pixels

Several steps ask you to check that our UI is **not** in the recording. Open the segment file
(`open "$(sclast)/tracks/screen-000.mov"`, QuickTime) or the export and step through it with the
arrow keys at the moments our windows were on screen. Expect: where the floating bar, recorder
panel, switch popover, overlays or tray menu were, the recording shows whatever was behind them;
in the menu bar there is a gap where our status item sits. Any of our pixels in any frame is a
fail (note the frame time and which window).

---

## 1. Launch and onboarding

**L1. `just run`**
- Do: `just run` in a terminal.
- Expect: the build finishes, the live banner prints, our icon appears in the menu bar. No Dock
  icon, no focus change: the app you were using stays frontmost. No permission prompt.
- Look: menu bar, Dock, the terminal (`starting` log line with `permission_host:
  Some("com.simplecapture.dev")`).

**L2. Tray menu**
- Do: click the menu-bar icon.
- Expect: Open Recorder · Start Recording · Switch Source… · Quit.
- Look: the menu.

**L3. Recorder panel**
- Do: tray → Open Recorder.
- Expect: the "Simple Capture" panel with grouped sections Source (Displays / Windows / Apps),
  Frame, Camera, Audio and Video, and a footer with the chosen source and a red Record button. No
  "Permissions needed" banner (all three grants held). If a recording from an earlier crash exists,
  a yellow "A recording from a previous session was recovered." line with Show in Finder. The
  controls look and behave like macOS ones in light and dark mode (System Settings → Appearance).
- Look: the panel.

**L4. Onboarding window**
- Do: if the panel shows "Permissions needed" (the dev host lacks one grant), click it. If all
  three grants are held the banner is hidden; then this step is covered by B1 and B5 in the release
  bundle. Do not change grants to test it.
- Expect (whenever it shows): three rows, Screen Recording, Camera, Microphone, each with a status
  badge (Granted / Denied / Not asked yet / Restricted / Unknown) read **without prompting**;
  Allow… only on "Not asked yet"; Open Settings… on anything not granted; the "Why macOS may ask
  again" disclosure explains the ad-hoc re-prompt and the macOS 15 reminder / private-picker bypass
  alert, and points to the System Picker.
- Look: the onboarding window; §12 for any prompt.

---

## 2. Sources (own chooser)

For each step: choose the source, press **Record**, record about 10 s while moving a window or
scrolling something inside the source, stop with the bar's **Stop**, then `scshow`.

**S1. Main display**
- Do: Displays tab → the "(main)" display → Record.
- Expect: the floating bar appears (timer counting, Switch, Stop, ✕); the tray title shows the
  elapsed time (`0:07`); Stop opens the "Recording saved" window (Duration, Size, Project, Export
  MP4, Show in Finder). One segment, `frame_px` = the display's pixel size shown in the list.
- Look: bar, menu bar title, Saved window, `scshow`, `screen-000.mov` in QuickTime.

**S2. Window**
- Do: Windows tab → a window of another app (e.g. a Finder window) → Record; move and resize the
  window, put another window on top of it for a second.
- Expect: the recording shows only that window, following it when moved; the covering window
  does not appear. `source.kind = window`.
- Look: QuickTime, `scshow`.

**S3. Application**
- Do: Apps tab → an app with two windows (e.g. Finder) → Record; open a third window of that app
  during the recording; also show a window of a different app on top.
- Expect: all windows of that app on the chosen display appear, including the one opened
  mid-recording; other apps' windows do not. Frame = that display.
- Look: QuickTime, `scshow` (`source.kind = application`).

**S4. Region (drag)**
- Do: Region… → the region overlay covers the display with "Drag to select a region ·
  Esc to cancel". Drag a rectangle; move it by dragging inside; resize from a corner and an edge
  handle; type W and H in the toolbar; try the shape presets (Free, 16:9, 9:16, 4:3, 1:1; Match
  frame is disabled while the frame is Original); Esc once to cancel, then again with Use region.
  Record.
- Expect: the toolbar shows `pt · W×H px`; Esc closes the overlay with nothing chosen; Use region
  sets "Source: region…" in the panel. The recording shows exactly the dragged rectangle;
  `frame_px` equals the px figure shown in the toolbar (rounded to even numbers).
- Look: overlay, panel Source line, `scshow`, `scsize "$(sclast)/tracks/screen-000.mov"`.

**S5. Odd-sized region**
- Do: a region with odd point sizes (e.g. W 333 × H 211) on a Retina display; Record 5 s.
- Expect: records; `frame_px` even (e.g. 666×422); QuickTime plays it without a green or black
  edge.
- Look: `scshow`, QuickTime.

**S6. Pick mode (click)**
- Do: Click a Window. A dimmed overlay with a card "Record… / Click a window / D whole
  display / Drag for a region / P system picker / Esc cancel". Hover over windows (each is
  highlighted with its title), click one.
- Expect: the panel Source line shows that window. Repeat with **D** (whole display), with a drag
  (region), and with **Esc** (nothing changes). Record once from a pick.
- Look: overlay, panel, `scshow`.

**S7. Second display at a different scale** (needs an external display whose scale differs from
the built-in one, e.g. a 1× monitor next to a 2× Retina screen; a Sidecar iPad also works)
- Do: (a) record the non-main display; (b) Region… with the cursor on the second display
  and drag a region there; (c) Click a Window, move the cursor from one display to the
  other, click a window on the second display; (d) Apps tab: "Record on" the second display.
- Expect: (a) `frame_px` = the second display's pixel size; (b) the overlay opens on the display
  under the cursor; px = pt × that display's scale; the recording shows the right area of the
  right display (region coordinates are display-local); (c) the pick overlay follows the cursor
  to the other display; (d) only that app's windows on that display.
- Look: `scshow` (`display_id`, `scale`, `region_pt`), QuickTime.

**S8. Frame and small-content warning**
- Do: choose a small window or region, then Frame 9:16 (or a shape far from the source's).
- Expect: the warning "The source will look small in this frame…" appears; the preview shows the
  source inside the frame with the background around it; "Video size W×H" updates per shape.
- Look: panel Frame section.

**S9. Camera, mic, system audio**
- Do: choose a camera, a microphone, System audio All; Record 15 s while talking and playing a
  video or music; stop.
- Expect: "Recorded now, shown in M2." under Camera; `tracks/camera.mov`, `mic.mov` and
  `system_audio.mov` exist, each about as long as the recording, no gaps; QuickTime plays camera
  video, your voice in mic.mov, the music in system_audio.mov. The camera is not in the export
  (M2).
- Look: `scshow`, QuickTime on each track file.

**S10. System audio App**
- Do: Apps tab → an app playing sound (e.g. Music); System audio App; meanwhile play a different
  sound from another app (e.g. a YouTube tab in Safari while recording Music). Record 10 s.
- Expect: App is only enabled when the source is an app ("Choose an app as the source to record
  only its audio" otherwise); `system_audio.mov` has only the chosen app's sound.
- Look: panel, QuickTime on `system_audio.mov`.

**S11. Mic Mode**
- Do: choose a microphone, click the ⋯ button next to it (Mic Mode; also once during a recording).
- Expect: macOS's own Mic Mode panel (Control Center) opens; the button is hidden with no mic.
  If macOS shows nothing while the mic is idle, note whether it works during a recording.
- Look: the Control Center panel; note the macOS version in §11.

**S12. Cursor and click highlight**
- Do: record 5 s with Show the cursor off, then 5 s on with Highlight clicks on, clicking a few
  times.
- Expect: off: no cursor in the video; on: cursor visible. Highlight clicks shows on macOS 15+
  only (hidden on macOS 14) and draws the system's click circles.
- Look: QuickTime.

**S13. 60 fps gating**
- Do: Frame rate: try 30, 60 and 60 at 1× on each display; then plug in a display
  that was not connected at launch and select it.
- Expect: 60 fps options are enabled only when the launch probe passed for that display; a
  disabled option says why ("This Mac can't keep up…" or, for a display connected after launch,
  "Not measured for this source's display yet" until the next launch). With 60 fps a 10 s
  recording has about 600 frames (`frames` in `scshow`) and no "Frames are dropping" notice;
  `r_frame_rate` in `scsize` is about 60.
- Look: panel Video section, `probe-v1.json`, `scshow`, `scsize`.

---

## 3. Switching while recording

Start each recording on the main display, switch at least twice, stop, then `scshow`. Every switch
adds a segment whose start equals the previous segment's end; audio and camera tracks run the
whole recording without gaps.

**W1. From the bar**
- Do: bar → Switch → the switch popover (Recent, Displays, Windows, search box, Region…,
  Click a Window, System… (System Picker)). Type part of a window title, choose it. Then Switch → a
  display.
- Expect: the bar says "Switching…" briefly, then "Recording"; the popover closes; Esc closes the
  popover without switching. Segments: display → window → display, `end=user_switch`.
- Look: bar, `scshow`, QuickTime per segment.

**W2. From the tray**
- Do: tray → Switch Source… (the popover opens below the icon) → a window; tray → Stop
  Recording.
- Expect: as W1; the tray item reads "Stop Recording" while recording and "Start Recording" after.
- Look: tray, `scshow`.

**W3. ⌥⇧S key press**
- Do: during a recording, put the cursor on a display and press ⌥⇧S (from any app); click a
  window. Again ⌥⇧S → D. Again ⌥⇧S → drag a region. Again ⌥⇧S → Esc. Also press ⌥⇧S when not
  recording.
- Expect: the pick overlay opens on the display under the cursor with "Switch to…"; each pick
  switches; Esc keeps the current source. Not recording: the overlay says "Record…" and the pick
  sets the panel's source. No Accessibility or Input Monitoring prompt. If another app owns ⌥⇧S,
  a notice "⌥⇧S is unavailable: …" appears on the bar instead.
- Look: overlay, bar, `scshow`, §12.

**W4. Switch to a region and between displays of different scales**
- Do: (needs S7's second display) switch main display → region on the second display → second
  display → window on the main display.
- Expect: one segment per source with its own `frame_px`; the export (§5) shows each segment
  contain-fitted in the frame.
- Look: `scshow`, export in QuickTime.

**W5. Closed window → "Switch source · Stop"**
- Do: record a window (TextEdit document works well), close that window (⌘W) mid-recording.
  Then choose **Switch source** on the bar and pick a display. Repeat, this time choosing
  **Stop**.
- Expect: the bar shows "Source closed — Switch source · Stop"; the video holds the last frame
  until you choose. Switch source continues in a new segment; Stop finalizes. The closed segment
  has `end=source_lost`; the journal has a `source_lost` event.
- Look: bar, `scshow`, `grep source_lost "$(sclast)/journal.jsonl"`.

**W6. Discard**
- Do: during a recording press ✕ on the bar → "Discard this recording?" → Keep; then ✕ → Discard.
- Expect: Keep continues recording; Discard stops and removes the project folder; no Saved window.
- Look: bar, `ls ~/Movies/Simple\ Capture`.

---

## 4. System picker (under the dev host)

Panel → System… (System Picker) (or **P** in pick mode). macOS shows its own content-sharing picker.

**P1. Pick a display**
- Do: System… (System Picker) → choose a display → in the panel "Record the whole display, or a region of
  it?" → Whole Display → Record 10 s.
- Expect: Source line ends "(system picker)"; segment `via=system_picker`, `frame_px` = display
  pixels. Any prompt or "bypass the private window picker" alert goes in §12.
- Look: panel, `scshow`, §12.

**P2. Pick a window**
- Do: System… (System Picker) → a window → Record 10 s.
- Expect: as S2, `via=system_picker`.
- Look: `scshow`, QuickTime.

**P3. Pick an app**
- Do: System… (System Picker) → an app → Record 10 s.
- Expect: as S3 (that app's windows only), `via=system_picker`.
- Look: `scshow`, QuickTime.

**P4. Cancel**
- Do: System… (System Picker) → Cancel in the picker. Again from pick mode with **P** → Cancel.
- Expect: nothing changes, no error; the source stays as before.
- Look: panel.

**P5. Display then region**
- Do: System… (System Picker) → a display → Region… → drag a region on that display → Use region →
  Record 10 s.
- Expect: the region overlay opens on the picked display; the recording shows the region;
  `source.kind = region`, `via=system_picker`.
- Look: `scshow`, QuickTime.

**P6. Switch with the picker**
- Do: during a recording, bar → Switch → System… (System Picker) → a window; then Switch → System
  picker… → a display.
- Expect: two new segments with `via=system_picker`; no gap in audio.
- Look: `scshow`.

**P7. Picked display with the floating bar visible — our pixels**
- Do: System… (System Picker) → the display where the floating bar will show → Whole Display → Record
  15 s. During the recording open the switch popover and close it, and open the tray menu.
- Expect: the bar, the popover and the tray menu are **absent** from the recording
  (`excludedWindowIDs` on a picked display), per §0.4.
- Look: QuickTime frame by frame (§0.4).

---

## 5. Export

**E1. Quick export from the Saved window**
- Do: after a recording, Size "Full size (long side up to 3840)" → Export MP4; then 1080p; then
  start one more and press Cancel.
- Expect: a progress bar with % and about-N-s-left, then "Exported <name>.mp4" with Show in Finder; the
  MP4 sits next to the project folder. Cancel shows "Export cancelled." and leaves no `.part`
  file and no `ffmpeg` process.
- Look: Saved window; `ls "$(dirname "$(sclast)")"/*.part`; `pgrep -l ffmpeg`.

**E2. Every aspect × background, in QuickTime**

The 20 combinations are recorded and exported by a script (4 s each, main display, no audio),
then you check them by eye:

```bash
sips -s format png "/Library/User Pictures/Animals/Owl.heic" --out "$HOME/Movies/sc-bg.png" >/dev/null
OUT="$HOME/Movies/sc-manual-exports"; mkdir -p "$OUT"
scripts/lib/live.sh bash -c '
OUT="$HOME/Movies/sc-manual-exports"
for a in original 16:9 4:3 1:1 9:16; do
  for bg in "solid:#1E6FD9" "linear:135:#FF5F6D@0,#FFC371@1" "radial:0.5,0.5,0.8:#2B5876@0,#F3E7E9@1" "image:$HOME/Movies/sc-bg.png"; do
    t="$(echo "$a ${bg%%:*}" | tr : x)"
    p=$(tools/macos-dev-host/run.sh "$PWD/target/debug/simple-capture" record --source display:main --duration 4 \
          --aspect "$a" --background "$bg" --camera none --mic none --system-audio off \
          --projects-dir "$OUT" --title "$t" | tail -n 1)
    target/debug/simple-capture export "$p"
  done
done'
open "$OUT"
```

- Do: open each MP4 in QuickTime; Window → Show Movie Inspector (⌘I) for the size.
- Expect, every file: plays; original: no background visible, the size is the display's pixel
  size (long side capped at 3840); 16:9 / 4:3 / 1:1 / 9:16: the screen is contain-fitted
  (centred, not stretched, not cropped) with the background on the two free sides; solid = one
  flat colour; linear = a diagonal blend; radial = a centre-out blend; image = the owl photo
  covering the frame (cropped, not stretched). No black bars except where the background is
  black.
- Look: QuickTime and its inspector; `scsize FILE` for exact numbers.

**E3. Export of a multi-segment recording**
- Do: export W1's or W4's recording.
- Expect: every segment appears in turn, each contain-fitted on the same canvas (a window segment
  looks smaller with the background around it); audio continuous across the cuts.
- Look: QuickTime.

**E4. Our UI and the tray menu absent from recordings**
- Do: record the main display 20 s with Original; during it: open the recorder panel (tray →
  Open Recorder), open the switch popover, open the tray menu and hold it open 3 s, press ⌥⇧S and
  Esc, open Region… from the popover and Esc. Export.
- Expect: none of our windows, overlays or the tray menu are in any frame; the menu bar shows a
  gap where our status item is (§0.4). If the open tray menu does appear, write it down with the
  frame time: the docs then ask whether to accept that exception.
- Look: QuickTime frame by frame.

---

## 6. Hardware and system events

**H1. Device lists update on hot-plug**
- Do: with the panel open and not recording, plug in (or turn on) a USB/Bluetooth microphone or
  camera (AirPods out of the case; Continuity Camera iPhone nearby); then remove it.
- Expect: within a few seconds the Camera / Microphone menus gain and then lose the device
  without reopening the panel.
- Look: panel menus.

**H2. Unplug camera mid-recording**
- Do: record with an external camera (USB camera, or Continuity Camera: lock or move the iPhone
  away) plus a mic; after 10 s remove the camera; record 10 s more; stop.
- Expect: the bar shows "Camera "<name>" disconnected"; recording continues; `camera.mov` ends at
  the unplug time and plays; the screen and audio tracks run the whole recording; the journal has
  a `device` event with `"change":"lost"` (or `disconnected`).
- Look: bar, `scshow`, `grep '"ev":"device"' "$(sclast)/journal.jsonl"`.

**H3. Unplug mic mid-recording**
- Do: as H2 with an external mic (AirPods: put them in the case).
- Expect: "Microphone "<name>" disconnected"; `mic.mov` ends then and plays; everything else
  continues.
- Look: as H2.

**H4. Unplug the display being recorded**
- Do: record the external display; after 10 s unplug it (or disconnect Sidecar).
- Expect: the bar (moved to a remaining display) shows "Source closed — Switch source · Stop";
  choose Switch source → the main display; stop. The external segment ends `end=source_lost`;
  the next segment starts at the switch; the recording finalizes and exports.
- Look: bar, `scshow`, export.

**H5. Sleep mid-recording**
- Do: record 10 s, then Apple menu → Sleep (or `pmset sleepnow`, or close the lid); wake after
  30 s and unlock.
- Expect: after wake the Saved window says "Recording stopped because the Mac went to sleep.
  Everything up to that point is saved."; `state: finalized`; journal `stop` with
  `"reason":"sleep"`; the files play up to the sleep.
- Look: Saved window, `scshow`.

**H6. Quit during a recording**
- Do: (a) record, tray → Quit. (b) `SC_DEV_PID_FILE="$TMPDIR/sc-app.pid" just run`, record, then
  in another terminal `kill -TERM "$(cat "$TMPDIR/sc-app.pid")"`. (c) Optional: log out during a
  recording.
- Expect: the app exits within a few seconds; `state: finalized` (not `recovered`); journal `stop`
  reason `quit` (a, c) or `signal` (b). If an export was running instead, its `.part` is gone and
  no `ffmpeg` is left.
- Look: `scshow`, `pgrep -l ffmpeg`.

**H7. Low disk (optional, disk image)**
- Do:
  ```bash
  hdiutil create -size 3g -fs APFS -volname SCLow "$TMPDIR/sclow.dmg" && hdiutil attach "$TMPDIR/sclow.dmg"
  scfill() { dd if=/dev/zero of="/Volumes/SCLow/$1" bs=1m count=$(( $(df -m /Volumes/SCLow | awk 'NR==2{print $4}') - $2 )); }
  scfill fill1 1500                              # leaves about 1.5 GB free
  SC_PROJECTS_DIR=/Volumes/SCLow just run
  ```
  Record the main display. After 10 s, in another terminal (with `scfill` defined there too):
  `scfill fill2 300` (leaves about 300 MB, under the 500 MB stop threshold). Afterwards, with
  under 1 GB free, try to start a new recording. Clean up: quit, `hdiutil detach /Volumes/SCLow`, `rm "$TMPDIR/sclow.dmg"`.
- Expect: at start (under 2 GB free) the bar shows "Disk almost full: … left"; after `fill2` the
  recording stops by itself and the Saved window says "Recording stopped because the disk is
  full."; `state: finalized`, journal `stop` reason `disk`; the files play. Starting with under
  1 GB free fails with "not enough free disk space …".
- Look: bar, Saved window, panel error, `scshow` (with `SC_PROJECTS_DIR=/Volumes/SCLow`).

**H8. Frames-dropping hint**
- Do: only if it shows up naturally (e.g. 60 fps at native on a big display while compiling).
- Expect: the bar shows "Frames are dropping: try 30 fps or 1× next time".
- Look: bar, `dropped` in `scshow`.

---

## 7. Crash safety

**K1. Kill and relaunch**
- Do: `SC_DEV_PID_FILE="$TMPDIR/sc-app.pid" just run`; record the main display with mic and
  system audio; switch source once; after 15 s `kill -KILL "$(cat "$TMPDIR/sc-app.pid")"`. Run
  `just run` again.
- Expect: on relaunch the panel shows "A recording from a previous session was recovered." with
  Show in Finder; `state: recovered`; the last segment ends with `end=recovered`; at most about 1 s is
  lost per track; every track file plays; `"$SC" export "$(sclast)"` writes an MP4 that plays.
- Look: panel, `scshow`, QuickTime, the export.

**K2. CLI recovery**
- Do: repeat K1's kill, but do not relaunch the app; run `"$SC" recover`.
- Expect: the CLI lists the project as recovered; a second `recover` changes nothing.
- Look: CLI output, `scshow`.

The scripted kill points (`after_switch_begin`, `after_segment_prepare`, `after_segment_open`)
are covered by `just killtest`.

---

## 8. A/V sync: clap

The flash-and-beep test (`just sync-test`) measures system audio and mic automatically. The camera
is only automated where its full-screen flash is detectable; this step is the camera check.

**C1. Clap**
- Do: record the main display with the camera and mic on for 10 s; in view of the camera, clap
  your hands sharply three times, about 2 s apart. Stop. Then:
  ```bash
  target/debug/sc-devtools sync clap "$(sclast)"
  ```
- Expect: for each clap the tool prints the mic transient time and the camera frame with the most
  motion near it, and the offset in ms; the frame it shows has your hands touching (confirm by
  eye). Offsets within one frame (33 ms at 30 fps). Write the three offsets in §11.
- Look: the tool's output and the frames it names.
- If `sync clap` is not available on this build: put camera video and mic audio in one file and
  watch it in QuickTime:
  ```bash
  d="$(sclast)"; third_party/out/macos-arm64/ffmpeg -v error -i "$d/tracks/camera.mov" -i "$d/tracks/mic.mov" \
    -map 0:v -map 1:a -c copy "$TMPDIR/clap-check.mov" && open "$TMPDIR/clap-check.mov"
  ```
  Both tracks share the recording's start (check that `first_sample_offset_ns` is 0 for both in
  `project.json`; otherwise the difference is a shift to allow for). Step frame by frame around
  each clap: the sound should land on the frame where the hands meet. Note "by eye" in §11.

---

## 9. Soak (30 minutes)

The owner allowed an agent to run the 30-minute soak (Q7 = yes), so this step is only for a
comparison run or if the agent run did not happen.

**O1. `just soak 30`**
- Do: `just soak 30` with a camera and mic connected and something playing sound. You may keep
  using the Mac, but leave the screen unlocked and awake; two small white flash windows with
  beeps appear at 5 s and at 29 min.
- Expect: the summary below, every threshold met.
- Look: the summary printed at the end (the soak script prints it in this format).

**Soak summary format** (the agent soak report uses the same fields):

```
soak summary v1
run          date=YYYY-MM-DD by=owner|agent minutes=30 macos=… mac=… commit=…
setup        source=display:<id> native, switch display<->window every 5 min, camera=<name> 1080p,
             mic=<name>, system_audio=all, fps=30
screen       frames=… dropped=… dropped_pct=…%            threshold < 0.5 %
segments     count=… gaps=… overlaps=…                     threshold 0 gaps, 0 overlaps
rss_mb       at_2min=… end=… growth=… slope_mb_per_min=…   threshold growth ≤ 50 MB
cpu_pct      ours avg/max=…/…  replayd avg/max=…/…  VTEncoderXPCService avg/max=…/…
stream_stopped count=…                                     threshold 0
audio        mic: drift_ns=… gaps=…   system_audio: drift_ns=… gaps=…   (count-based vs host time)
camera       frames=… dropped=…
sync_ms      start: system_audio=… mic=…   end: system_audio=… mic=…   threshold |end| ≤ 33
verdict      PASS|FAIL (list failed thresholds); ADR-010 frame copy: copy|no copy (why)
```

- Rule for the verdict: frame copy (ADR-010) is recommended only if SCK stalls,
  `StreamStopped` appears or delivery collapses; otherwise no copy. A run shorter than 30 min is
  "regression only".
- Recordings are deleted after the summary unless you keep them on purpose; never commit them.

---

## 10. Release bundle (owner only)

The release bundle is `com.simplecapture.desktop`, ad-hoc signed, hardened runtime. It has never
been granted anything, so it **will** prompt; that is what this section tests (ADR-028, Q8).
Agents never launch it. Quit the dev app (`just run`) first so only one tray icon is in the menu
bar.

**B0. Build and inspect**
- Do: `just bundle`. The bundle is `target/release/bundle/macos/Simple Capture.app` (the path is
  printed at the end). Then:
  ```bash
  APP="target/release/bundle/macos/Simple Capture.app"
  codesign -dv --entitlements - "$APP" 2>&1 | grep -E 'flags|Identifier|camera|audio-input'
  plutil -p "$APP/Contents/Info.plist" | grep -E 'UsageDescription|CFBundleIdentifier'
  codesign -dv "$APP/Contents/MacOS/ffmpeg" 2>&1 | grep -E 'Signature|flags'
  ```
- Expect: `flags=0x10002(adhoc,runtime)` (hardened runtime), `Identifier=com.simplecapture.desktop`,
  the camera and audio-input entitlements, the three usage strings (camera, microphone, system
  audio), and signed ffmpeg/ffprobe sidecars.
- Look: the command output.

**B1. Never-granted: onboarding and lists without a prompt**
- Do: `open "$APP"`. Tray → Open Recorder. Click the Windows and Apps tabs. Click
  "Permissions needed".
- Expect: no prompt appears from opening the app, the panel, the tabs or the onboarding window
  (status is read without asking). Windows/Apps say "Allow Screen Recording to list windows and
  apps." with Allow… and Use the System Picker…. Onboarding shows "Not asked yet" (or the real state) for
  all three.
- Look: screen, §12.

**B2. Never-granted: record through the system picker**
- Do: System… (System Picker) → a display → Whole Display; System audio All; Record 15 s with music
  playing; stop; export. Repeat once with a window.
- Expect: the macOS picker works without a Screen Recording grant (picking is consent for that
  recording). Note in §12 every prompt or alert (Screen Recording prompt, "bypass the private
  window picker" alert, a reminder) with its exact wording. Note whether `system_audio.mov` has
  sound on the picker path.
- Look: screen, `scshow`, QuickTime, §12.

**B3. After a rebuild**
- Do: `just bundle` again (any trivial rebuild), `open "$APP"`, repeat B2 once.
- Expect: record whether macOS asks again and what it asks (an ad-hoc app is a new app to TCC
  after every build).
- Look: §12.

**B4. Grant Screen Recording; whole-app exclusion re-test**
- Do: onboarding → Screen Recording → Allow… (write down the prompt), grant it in System
  Settings as macOS asks, and relaunch as macOS suggests. Quit the app, then:
  ```bash
  open -n --env SC_EXCLUSION=app "$APP"
  ```
  Record the **main display** from the Displays tab (own chooser, not the picker) with the floating
  bar visible for 15 s; during it open the recorder panel, the switch popover and the tray menu.
- Expect: none of our windows or menus in any frame (§0.4), now through the whole-app filter
  (`excludingApplications`). Write PASS/FAIL in §11; this decides whether whole-app exclusion
  works in a signed bundle (ADR-010).
- Look: QuickTime frame by frame.

**B5. Camera and microphone prompts**
- Do: onboarding → Camera → Allow…; Microphone → Allow…. Then record with a camera and mic.
- Expect: each prompt shows our usage string: "Simple Capture records your camera when you turn
  the camera on for a recording." / "Simple Capture records your microphone when you choose a
  microphone for a recording."; after granting, the recording has `camera.mov` and `mic.mov`.
- Look: the prompts, `scshow`, §12.

**B6. Clean up**
- Do: quit the bundle. Optionally remove its grants in System Settings → Privacy & Security
  (your call; never needed for the dev host).
- Expect: no Simple Capture icon left in the menu bar; the dev host's grants are unchanged (the
  next `just run` shows no prompt).
- Look: menu bar, `pgrep -lf "Simple Capture.app"` prints nothing, §12.

---

## 10b. Camera and Preview window (M2)

Choose a camera in the recorder panel first. The OBSBOT, FaceTime HD, EOS Webcam Utility and
iPhone cameras all work on the build Mac.

**M1. Preview window appears with the panel**
- Do: open the recorder panel (tray → Open Recorder), pick a camera, source = the whole display.
- Expect: a "Preview" window shows the video as it will be recorded: the screen in its frame and
  the camera in a circle, not mirrored; the camera light is on. Closing the panel turns the camera
  off and hides the Preview. Its close button unticks "Show the Preview window".
- Look: Preview window, camera light.

**M2. Place, snap, resize, zoom**
- Do: in the Preview, drag the camera around, near the edges, corners and centre lines; drag a
  corner handle out and in; scroll on it. Move the Preview window itself and reopen the panel.
- Expect: the camera follows, snaps to the margins and centre lines, grows and shrinks, zooms, and
  never leaves the frame; the Preview reopens where you left it.
- Also: resize the Preview by its edges and corners, and drag it to the other display if there is
  one, and try full screen (green button). The picture stays centred and the camera sharp (the
  screen part is a once-a-second snapshot at the window's point size, so it is softer), and the
  dashed outline and handles stay on the camera (a fast resize may lag a frame, then catch up).
- Look: Preview window.

**M3. Shape, framing, mirror, border, shadow**
- Do: in the panel's Camera section try every shape, the Corners slider, S/M/L, the four corner
  buttons, Mirror, Border, Shadow, "In the video", and Reset framing.
- Expect: the Preview and the panel's little preview change at once and match; the Preview's
  dashed outline and handles move with the camera.

**M4. Video Effects**
- Do: press Effects… in the Camera section.
- Expect: macOS's Video Effects panel opens for the camera and the Preview shows the effect.

**M5. Other sources and frame shapes**
- Do: switch the source to a window, an app, a region; change the frame to 9:16 and 1:1.
- Expect: the Preview always shows the real output, and the camera stays inside the frame.

**M6. Record, move the camera, export**
- Do: record for about 15 s; at 5 s drag the camera in the Preview to another corner, at 10 s
  untick "In the video", stop. Check the control bar's timer counts up, drag the bar by its grip,
  and check the Saved window's duration and "Recorded" list; press Export Video.
- Expect: the export is one MP4 with the screen, the camera staying put until about 5 s, then
  following your drag, and gone from about 10 s, and the mic and system audio mixed; the Preview
  never appears in it. After Stop (the panel is hidden) the camera light goes off and the Preview
  closes.
- Look: the export in QuickTime, the camera light.

**M7. The camera survives a crash**
- Do: start a recording with the camera, move it in the Preview, then `kill -9` the app (or Force
  Quit) and relaunch.
- Expect: the recovered recording exports with the camera, including the move (up to 0.2 s of the
  last motion may be lost).

## 10c. Editor (M3)

Record about 30 seconds with screen, camera and mic (talk, and show something that moves), then
stop. The editor opens by itself. Check, in order:

| # | Step | Expected |
|---|---|---|
| E1 | Look at the editor | Preview of the composed video on top; inspector on the right (Clip, Camera, Audio, Frame); timeline with thumbnails and a waveform filling in within a few seconds |
| E2 | Space; Space again after a few seconds | Plays with sound from the playhead, picture and sound in step; pauses |
| E3 | Click and drag on the ruler | The playhead follows and the preview shows that moment |
| E4 | Put the playhead somewhere and press S | The clip splits there |
| E5 | Click a clip, press ⌫ | It is removed and the gap closes (ripple); playing across the new cut has no click |
| E6 | Shift-drag a range on the ruler, ⌘X, move the playhead, ⌘V | The range moves to the playhead; the camera in the pasted piece is where it was when recorded |
| E7 | Select a clip, set 2× in the Clip tab, play it | Twice as fast, voice at the same pitch; the clip shows 2× |
| E8 | Drag a clip's yellow edge (select it first) | Trims; the timeline gets shorter or longer as you drag, applied on release |
| E9 | Drag a clip body sideways | A blue marker shows where it will land; release reorders |
| E10 | ⌘Z repeatedly, then ⇧⌘Z | Every edit undoes and redoes in order |
| E11 | Camera tab: change shape, size, corner; drag the camera in the preview; scroll on it | The preview updates; orange keyframe marks show on the timeline; "Use Everywhere" makes one placement for the whole video |
| E12 | Audio tab: lower the microphone, mute system audio | Playback and the export follow |
| E13 | Frame tab: 9:16, then a gradient background | The preview reshapes and letterboxes with the background |
| E14 | Export → 1080p | Progress in the toolbar; "Exported. Show in Finder"; the file plays with every edit |
| E15 | Close the editor, tray → Edit Last Recording | The editor opens with the edits kept (autosaved) |
| E16 | Recorder panel → When Recording Stops → Show Summary; record again | The Saved window appears instead; its Edit… button opens the editor |
| E17 | Light and dark appearance | Both look right |
| E18 | Record 30 s talking with a few long pauses; editor → Remove Silences… | The pauses are marked red on the timeline; the count and total update as you move the level or pick 0.5 s / 1 s / 2 s |
| E19 | Remove | The marked pauses are gone in one step; speech keeps a short breath at each cut; ⌘Z brings them all back |
| E20 | Mute the system audio track in Audio, open Remove Silences again | Only the microphone counts: music in the muted track no longer hides the pauses |
| E21 | Pause on an email address; Blur (or B) | A box appears over the middle of the screen for 3 s and the preview shows a mosaic there; a Blur item appears in its own lane |
| E22 | Drag the box over the address and resize it from a corner; switch it to Black | The preview follows; the camera bubble and text stay on top |
| E23 | Change the frame to 9:16 or add padding | The box stays on the same screen content |
| E24 | Drag the Blur item's edges on the timeline; cut a range before it; ⌘Z | It moves and ripples like text; undo restores it |
| E25 | Export | The exported video hides the area for exactly that stretch |
| E26 | Record 30 s with screen, camera and mic; open the editor | The timeline shows Screen (system audio waveform), Camera (microphone waveform), Text and, once there is one, Blur; the chain button between Screen and Camera is on (linked) |
| E27 | Linked: split, delete a clip, drag a clip | Both tracks change together, as before |
| E28 | Click the chain to unlink; select a camera clip, ⌫ | A striped gap is left; the screen does not move; playing over it shows the screen with no camera and no mic |
| E29 | Drag the camera clip after the gap left | It slides in time and stops at the start; the camera's mouth and voice stay in step |
| E30 | Select a screen clip, ⌫ | A gap on the screen track; the camera fills the frame there; the timeline keeps its length |
| E31 | Select the screen gap, ⌫ | That time leaves every track; text after it moves back |
| E32 | Click the chain again | One step: the camera follows the screen again and the screen's gaps close; ⌘Z brings the separate tracks back |
| E33 | Export while unlinked | The export matches the preview: camera gaps, the full-frame camera and the slid camera clip |
| E34 | Add text; select it | The dashed box sits exactly on the drawn text; the camera and blur boxes sit exactly on the camera and the blurred area |
| E35 | Text tab | Text colour and Background (box, colour, opacity, corners) are separate sections |

## 10d. Text (M4)

In the editor with any recording:

| # | Step | Expected |
|---|---|---|
| T1 | Press T | "Your text" appears near the bottom for 3 s from the playhead, selected (yellow box); the Text tab opens |
| T2 | Double-click the text in the preview, type "Hello 👋 world" | The video updates as you type; the emoji is in colour |
| T3 | Try each style preset | Each looks like its button; Box and Inverted put a rounded box behind the lines; Merged joins the boxes of several lines |
| T4 | Change font, weight, italic, size, alignment, line height, spacing | The preview follows; heavy weights are real weights, not smeared |
| T5 | Gradient on; eyedropper; pick a colour from anywhere on screen | The fill uses it |
| T6 | Drag the text; watch the pink guides at the centre and thirds | Snaps to them |
| T7 | Drag a corner handle; drag the round handle above the box | Scales; rotates and snaps at 0°, 15° and 90° |
| T8 | Set In: Typewriter, Out: Slide down, Loop: Pulse; play | Letters appear one by one, the text pulses, then slides away |
| T9 | Drag the text item and its edges on the timeline | It moves and retimes |
| T10 | Delete a range that covers the text; undo | The text goes with it; undo brings both back |
| T11 | ⌘C, move the playhead, ⌘V | A copy appears at the playhead |
| T12 | Export 1080p and 720p | The text looks identical in both and in the preview |

## 10e. Export sheet (M5)

In the editor with any recording, press ⌘E:

| # | Step | Expected |
|---|---|---|
| X1 | Look at the sheet | Format tabs MP4, WebM, GIF, MOV; codecs, encoder, quality, fps, size; an estimate line with size range and time |
| X2 | Change the size to 720p, then type a width | The other side follows (aspect locked); the estimate updates |
| X3 | Pick fps 60 on a 30 fps recording | The sheet says it exports at 30 (capped at the recording's rate) |
| X4 | MP4 + HEVC, export; open in QuickTime | Plays; ⌘I shows HEVC |
| X5 | MP4 + AV1 + Opus | Marked "less compatible"; plays in Chrome and VLC |
| X6 | WebM VP9 + Opus, then VP8 + Vorbis | Both play in Chrome and Firefox with the right colours |
| X7 | MOV ProRes 422 + PCM | Plays in QuickTime; large file |
| X8 | GIF (gifski) at 15 fps, 480p, loop 3 times | Plays in Safari, Chrome and Firefox at normal speed, three times then stops |
| X9 | GIF palette mode, 64 colours, dither off | Visible banding; plays at the chosen speed |
| X10 | Fast encoder on a 5K recording with H.264 at Original | A warning that hardware H.264 stops at 4096 px |
| X11 | Save the settings as a preset, switch format, apply the preset | Every setting comes back; delete removes it |
| X12 | Close and reopen the sheet | Each format kept its last settings |
| X13 | Start an export and press Cancel | "Export cancelled"; no `.part` file left |
| X14 | After an export: Show in Finder, Copy (paste into Finder or Messages), Open | Each works |
| X15 | Output folder: choose another folder; name template `{title} {size}` | The file lands there with that name; a second export gets " 2" |

## 10f. Recording controls (M7)

| # | Step | Expected |
|---|---|---|
| R1 | Start a recording from the panel | The bar shows 3, 2, 1, then recording starts; nothing of the countdown is in the video |
| R2 | Start again and press Escape (or Cancel) during the countdown | Nothing records |
| R3 | While recording, press ⌥⇧P (or the pause button); wait 5 s; press it again | The bar says Paused and its timer stops; the tray title shows ❚❚; resuming continues the timer |
| R4 | Stop; look at the editor | Two clips with the paused stretch cut out; playback jumps over it with no click |
| R5 | ⌥⇧R from any app | Starts recording (the last source); ⌥⇧R again stops |
| R6 | ⌥⇧C while recording with a camera | The camera disappears from the recording from then on; again brings it back |
| R7 | Restart in the bar, confirm | The take is gone (not in the Library) and a new recording starts |
| R8 | Settings → Show frame rate and drops; record | The bar shows fps, dropped frames and drift |
| R9 | Panel open, a microphone chosen; speak | The level bar under the microphone moves; it turns orange near the top |
| R10 | When Recording Stops → Export; record | The summary shows the export running with the format you exported last |
| R11 | Turn on "Move the recording to the Trash after a quick export"; export from the summary | The project folder is in the Trash after the export finishes; the exported file is kept |
| R12 | Panel → Camera Only; record 10 s talking | The Preview window and the panel picture show the camera filling the frame; the recording is the camera with your voice, no screen |
| R13 | Screen + Camera, then Screen | The camera section appears and goes away; Screen returns to the display or window you had |
| R14 | While recording the screen, switch source → Camera | The camera fills the frame from then on and the corner camera hides; switching back to a display brings the corner camera back |
| R15 | Panel → Starting and Shortcuts → click "Pause or resume", press ⌃⌥P | It shows ⌃⌥P; ⌃⌥P now pauses a recording and ⌥⇧P does nothing; the bar's Pause tooltip says ⌃⌥P |
| R16 | Give "Switch source" the same keys as "Start or stop recording" | An orange note says it is also used for the other action; Default puts it back |
| R17 | Click a shortcut, press Delete; click another, press Escape | The first shows None and no longer works; the second keeps its keys |
| R18 | While recording, hover each button in the bar | The bar's message area says what the button does (and its shortcut); the normal tooltip shows too |
| R19 | Plug in an iPhone, unlock it, tap Trust; panel → Source → iPhone | Within a few seconds the phone is listed with a USB tag; it is not in the camera menu |
| R20 | Pick it, Screen + Camera, record 20 s while using the phone | The Preview shows the phone's screen live with your camera in the corner; the recording is phone-shaped (portrait or landscape) and smooth |
| R21 | Rotate the phone while recording | The picture follows the new shape |
| R22 | Unplug the phone while recording | The bar says the source closed; Switch… or Stop work |
| R23 | iPhone tab → Open iPhone Mirroring; pick the iPhone Mirroring item (Wireless) and record | The mirrored phone records like a window; with system audio on, the phone's sound is in the recording |
| R24 | Camera Only, frame 9:16, Camera → Fill; record | A vertical video filled by the middle of the camera, no bars; Fit gives the whole camera with the background above and below |
| R25 | Screen, frame 9:16, a wide window, Picture → Fill | The window's middle fills the frame; blur boxes stay on their content in the editor and the export |
| R26 | Region… and Click a Window | The screen stays visible (lightly dimmed) while choosing |
| R27 | Screen only (no camera); open the Preview | No camera box or handles in it |
| R28 | Grant Screen Recording in System Settings, return to Permissions | It reads Granted |
| R29 | Panel open; plug in a USB camera or microphone, then unplug it | The menus gain and lose it within a few seconds, without reopening the panel |
| R30 | Screen + Camera; click the Preview, press the arrow keys (and with Shift) | The camera moves a little (Shift: further) and stays where it lands |
| R31 | Camera → Quality → 1080p, 60 fps (and 720p) | The Preview's camera changes format at once (Diagnostics/log shows the new format); recordings use it |
| R32 | Record a window; resize it mid-recording; stop | The editor shows a new section from shortly after the resize, at the new size, with no black area or squashing |
| R33 | macOS 14 or Windows: Highlight clicks on; record a window, click around in it; export | Each click shows a yellow ring that grows and fades where you clicked, in the editor and the export; with Fill it follows the crop |
| R34 | iPhone over USB as the source; start recording, then press play on a YouTube video (starting pauses it); record 15 s; open the editor | Audio tab lists Device sound with a waveform; the export plays the phone's sound in step with its picture; muting Device sound silences it |
| R35 | Connect AirPods (or another Bluetooth headset) as the microphone; record 10 s while talking | The editor shows the mic waveform and the export has your voice (headset mics run at 16–24 kHz) |
| Y13 | A recording that crashed (end the app from Activity Monitor while recording, start it again) → Library → What Was Recovered… | The sheet says what was kept and left out; Edit opens what was saved |
| E36 | Record a 16:9 frame from one window, switch to a much wider window; in the editor put the playhead in the second part → Frame → This section → Fill, drag Keep to Left | Only that part changes: it fills the frame and keeps its left side; the first part is unchanged; the export matches |
| E37 | J, J, L, L, L, K in the editor | J plays backwards at 1× then 2× (silent, with ◀◀ 2× by the time); L plays forward with sound, again 2×, again 4× (silent); K stops |
| E38 | Turn Snap off (or press N), drag a text edge near a cut | It no longer jumps to the cut; Snap on again restores it and is remembered next time |
| E39 | Two overlapping text items; Text tab → Bring to Front / Send to Back | The chosen one draws above or below the other in the preview and the export |
| E40 | Frame tab → 4:5 | A portrait 4:5 frame in the preview and the export |
| E41 | Export → WebP, Loop: Several times (3) | It plays in a browser with the right colours and stops after three plays; it is much smaller than the GIF |
| E42 | Add three texts at the same playhead, and two blurs that overlap | Overlapping items stack on separate rows; items that don't overlap share the first row; the timeline grows to fit |
| E43 | Scrub the timeline, switch to another app and back, scrub again | The screen track's pictures stay; Play works after changing a text's animation time |

## 10g. Library, About and Diagnostics (M6, M7)

| # | Step | Expected |
|---|---|---|
| Y1 | Tray → Library… | Every recording, newest first, with a picture, date, length and size |
| Y2 | Type in Search | The list narrows by title |
| Y3 | Double-click a recording | The editor opens it |
| Y4 | Select one → Export… | The editor opens with the export sheet showing |
| Y5 | Select one → Move to Trash → confirm | It leaves the list and is in the Trash (restorable) |
| Y6 | Tray → About Simple Capture | Version and licenses; each license opens as text; "Open notices" opens the Rust notices (release builds) |
| Y7 | About → Diagnostics… → Run Self-Test | Lines stream in; "7 passed, 0 failed" on a Mac with the screen grant; Show in Finder finds the report |
| Y8 | Terminal: `simple-capture --self-test` | The same report, printed and saved |
| Y9 | Library: select a recording, click its name, type a new one, Return | The name changes; in Finder the folder is now "<date> <new name>.sc" |
| Y10 | Rename a recording that is open in the editor (from the editor's title) | The title changes at once; the folder follows when the editor closes |
| Y11 | After recording, type a name in the summary's Name field | The Library and the editor show that name; exports use it |
| Y12 | Library: sort by Longest, Largest, Name; ⌘-click and ⇧-click several; ⌘A | Order changes and is remembered; the footer shows the count and size; Move N to Trash asks once |

## 11. Results log

Copy this table into the M1 review notes (or STATUS) with one line per step run.

| ID | Date | macOS / Mac | Result (PASS / FAIL / SKIP) | Notes (frame times, numbers, error text) |
|---|---|---|---|---|
| L1 | | | | |
| … | | | | |

Steps: L1–L4, S1–S13, W1–W6, P1–P7, E1–E4, H1–H8, K1–K2, C1 (three clap offsets in ms), O1 (paste
the soak summary), B0–B6, M1–M7.

## 12. Prompt log

Every Screen Recording, Camera, Microphone or system-audio prompt, reminder or alert seen, with the
path that led to it.

| Date | App (dev host / release bundle) | Path (what you clicked, step ID) | Prompt or alert (exact wording) | Your answer | Notes |
|---|---|---|---|---|---|
| | | | | | |

Under the dev host (§1–§9) this table should stay empty.

---

## 13. Windows and Linux

Installers come from the `package` workflow's artifacts (Actions → package → the run →
`simple-capture-windows-x86_64` or `simple-capture-linux-x86_64`). Nothing here has been tried
on a real Windows or Linux desktop yet (ADR-048).

### Windows (10 2004 or later; 11 removes the yellow capture border)
| # | Step | Expected |
|---|---|---|
| WN1 | Install the NSIS setup (per user) and start Simple Capture | A tray icon; the recorder panel opens from it |
| WN2 | Run the self-test from About → Diagnostics | Encoders list `h264_mf` or a GPU encoder; the 2 s real capture passes |
| WN3 | Record the main display 20 s with mic and system audio playing | The editor opens; picture and sound play in step |
| WN4 | Record a window, resize it while recording | The recording letterboxes the window instead of stretching |
| WN5 | Record a region, then an app | Only the region / the app's windows' area is in the video |
| WN6 | Our panel and bar during a recording | Not in the video (excluded from capture) |
| WN7 | Camera on | The camera records; unplugging it shows "Camera disconnected" and recording goes on |
| WN8 | Export MP4, WebM and GIF | Each plays (Films & TV, a browser) |
| WN9 | Settings → Privacy → Camera off for desktop apps, then reopen | The app shows the camera as denied and links to the setting |

### Linux (Ubuntu 24.04 or similar with PipeWire and xdg-desktop-portal)
| # | Step | Expected |
|---|---|---|
| LX1 | Install the .deb (or run the AppImage) | The app starts; a tray icon (needs an AppIndicator host on GNOME) |
| LX2 | Panel → Choose Screen or Window… | The system's share dialog; after choosing, the panel says it will record your choice |
| LX3 | Record 20 s with mic and system audio | Our windows hide while recording; the editor opens afterwards; sound in step |
| LX4 | Camera (a webcam that offers YUYV) | Records; an MJPEG-only camera says so |
| LX5 | Self-test | Encoders list `libx264`; the real-capture step says it is skipped on Linux |
| LX6 | Export MP4 and GIF; Move to Trash from the Library | Files play; the project is in the Trash |
