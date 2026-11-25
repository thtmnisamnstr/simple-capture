# Status

**M3–M7 built on branch `m3` (2026-11-21) under the owner's "Build the rest of it out"; no ★
stops were taken, so every milestone from M2 on waits for the owner's hands-on review.** M1 is
merged to `main` (35b7d3e); M2 is on `m2` and also contained in `m3`.


> The M0 spikes (`spikes/`, prototypes and their reports) were removed before 0.1.0; paths to them
> in these docs refer to commit d05da49 and earlier.

> **One Mac app (2026-11-25, ADR-058):** the DMG, Homebrew and Mac App Store builds are the same
> sandboxed app with the LGPL FFmpeg and no gifski, so on macOS H.264 and HEVC exports use
> VideoToolbox and GIFs use palette mode. Mac numbers below for x264, x265 and gifski were
> measured with the earlier GPL helpers; they now describe Windows and Linux.

## After M7: the owner's requests (2026-11-21, ADR-050)

| Feature | Where to try it | Verified here |
|---|---|---|
| Record modes: Screen, Screen + Camera, Camera Only; switch to or from the camera mid-recording | MANUAL_TEST R12–R14 | driver flow 21 live (14/14), synthetic recorder tests, panel in the browser |
| Rename from the Library, the editor title and the summary; the folder follows | Y9–Y11 | folder rename tests, Library rename in the browser |
| Library: sort, ⌘/⇧-click and ⌘A selection, Move N to Trash | Y12 | sort and selection tests, browser |
| Export destinations (Email under 18 MB, Slack & Teams, YouTube, Reels/Shorts, GIF) and Size Limit | §10e | size-limit export tests |
| Mic level in the recording bar | R9 | driver flow 20 (64 levels in 4.5 s) |
| Remove Silences | E18–E20 | silence and delete_ranges tests; driver flow 22 live on a real recording |
| Blur and black boxes | E21–E25 | GPU = CPU exactly; export test; driver flow 22 exports a black box over the real screen |
| Customizable shortcuts | R15–R17 | driver flow 23 (registration, clash, bare key refused, clear) |
| Windows: Media Foundation tracks (experimental) | TESTING_ON_WINDOWS_LINUX.md | round trips on GitHub's Windows runner |
| Linux: GStreamer screen track (experimental) | TESTING_ON_WINDOWS_LINUX.md | round trip in tools/linux-dev and on GitHub's Ubuntu runner |
| NVENC in the Linux and Windows helpers | — | `package` run 44958418772 built every installer with it; the Linux helper lists h264_nvenc/hevc_nvenc and links only system libraries; no NVIDIA GPU here to run it |

Independent reviews of the new code ran before handing over; their verified findings were fixed
(see the commits after "Guide for testing").

## M3–M7 build-out (2026-11-21)

| Milestone | What is in it | ADR | Hands-on script |
|---|---|---|---|
| M3 editor | timeline, every edit with undo/redo, speed with kept pitch, playback across cuts, camera keyframes, audio mix, frame/background, autosave, export of the edit | 045 | MANUAL_TEST §10c |
| M4 text | 13 bundled families + system fonts, colour emoji, styles, boxes, gradients, animations, on-preview editing | 046 | §10d |
| M5 export | MP4/MOV/WebM/GIF/WebP, Best/Fast encoders, quality tiers and custom, fps per format, sizes, presets, estimates, Copy/Open | 047 | §10e |
| M6 Windows/Linux | Windows and Linux backends, helper tracks, our own Windows/Linux helpers, package workflow, self-test and Diagnostics | 048 | §13, §10g |
| M7 polish | pause/resume, Restart, countdown, global shortcuts, Library, Trash, About/licenses, mic meter, stats overlay, export-after-recording, delete-raw-after-export, README | 049 | §10f, §10g |

**Verified here (macOS 26, M3 Pro):** the workspace tests and clippy; UI checks and 51 UI tests;
live app-driver flows 01, 17 (editor, text, MP4/GIF/WebM exports from the sheet: 23/23), 18
(self-test from the app: 7/7), 19 (Library posters) and 20 (pause: a 1.5 s pause cut into two
clips); `simple-capture --self-test` 7/7 including a real 2 s capture; x264 Best beats
VideoToolbox Fast on SSIM at a smaller size on a text-heavy clip (0.9365 at 134 KB vs 0.9341 at
233 KB). Every recording the agent made was deleted after its checks.

**Verified in CI and containers:** the Windows backend builds and runs its tests on GitHub's
Windows runner; the Linux backend on Ubuntu and in `tools/linux-dev` (PipeWire frames from a
test source). The Linux and Windows FFmpeg helpers build from the pinned sources with networking
off and link only system libraries; both pass the helper-track round trip on GitHub's Ubuntu and
Windows runners. The first `package` run (44939780618) built every installer: `Simple
Capture_0.1.0_aarch64.dmg` (37 MB), `…_x64-setup.exe` (32 MB), `…_x64_en-US.msi` (43 MB),
`…_amd64.deb` and `…_amd64.AppImage`, plus the helper source archive.

**Editor open time on a 10-minute recording:** first frame 900 ms (definition of done: under
2 s), driver flow 24 on a generated 3024×1964 HEVC recording (no capture involved).

**Not verified:** any real Windows or Linux desktop; the Linux portal dialog end to end (the
headless test compositor could not hand frames to the portal backend); the mic level meter and
the countdown by eye (driver mode turns both off); installing and running the built installers;
NVENC and VA-API on real GPUs.

**30-minute soak (2026-11-21, release build, driver flow 16):** the main display (3024×1964),
camera, mic and system audio for 1800.1 s: 0 dropped frames or samples on every track, no writer
stalls or gaps; the app held 78–91 MB while recording (85 MB in the second quarter, 81 MB in the
last; 208 MB peak during the export) at 33% of one core on average (41% peak); the Preview stayed
native. The take opened in the editor in 344 ms and exported whole at 1080p (54,003 frames at
122 fps, 1800.15 s long). The Preview then drew 23.5 of the camera's 30 fps; fixed (28.4 fps,
flow 25).

### Owner decisions needed
1. **Windows records with Media Foundation and Linux's screen track goes through GStreamer, both
   experimental** (ADR-050; the owner ships the Windows and Linux builds as experimental,
   2026-11-24). Older recordings and any file these cannot open still go through the
   helper; `SC_MEDIA=helper` switches back. Untested on a real desktop until you try them
   (docs/TESTING_ON_WINDOWS_LINUX.md).
2. ⚑ **Linux records only what the system's share dialog picks** (displays and windows; no app or
   region sources), and hides our windows while recording (ADR-048).
3. **Pause keeps the paused time in the raw tracks and cuts it in the starting timeline**
   (ADR-049); recording time no longer subtracts pauses.
4. **Linux hardware encoding** goes through GStreamer (VA-API, NVENC) in the app and NVENC in the
   helper; the helper still has no VA-API (it would need libva to start).
5. **The package workflow runs on pull requests, version tags and manual runs** (owner, ok).

### Open items
- Carried: the Windows CI flake `stop_before_the_duration_ends_there`; Preview at ~24 fps while
  recording; camera A/V sync unmeasured (STATUS §3).
- The owner's own hands-on recording `~/Movies/Simple Capture/2026-11-20 15.35.33 Recording.sc`
  and its export `2026-11-20 15.35.33 Recording.mp4` were left in place.

## M2 ★ review (2026-11-20)

Plan: docs/M2_PLAN.md; owner answers: ADR-041; implementation decisions: ADR-042; owner change
after the first hands-on: ADR-043.

**Owner hands-on, M2 first pass (2026-11-20), and changes:**
- **Bubble plus preview window was confusing; the preview window is better.** The floating bubble
  is gone (ADR-043). One Preview window shows the composed output with the live camera for every
  source and frame, is where the camera is placed, opens with the panel and stays while recording,
  remembers its position, and never appears in recordings (live: 0 of 57 frames).
- **Control bar in the middle of the screen, not movable.** It now opens at the bottom centre of
  the recorded display, moves by its grip and remembers where it was put (live checks in flow 14).
- **Timer stuck at 0:00 on the bar and in the Saved window.** The bar only updated on state
  changes; it now ticks and re-syncs to the 1 Hz health events (the tray title too). The Saved
  window shows the project's duration.
- **"No way to export it as one video".** The owner's export
  (`2026-11-20 15.35.33 Recording.mp4`, 18.8 s, 3840×2160) does contain the screen, the camera
  composited where it was placed, and the mixed mic and system audio (the system audio track was
  silent in that recording; the mic peaks about −20 dBFS). The Saved window now lists what was
  recorded, says the export makes one video of it, and the button reads "Export video".

**Found and fixed before the second hands-on (2026-11-20)**, by the live driver flows and an
independent review of the branch (ADR-042 update, "review fixes"):
- The Preview page had no event permission, so panel edits never moved its dashed outline and
  handles (the picture did move). Flow 12 now checks the outline against the drawn camera.
- Moves made while recording slid across the whole idle time before them in the export; keys
  after a pause are now jumps, drags stay motion; the last drag position is always kept.
- After recording from the panel, the camera stayed on and the Preview stayed up.
- A recording that failed to start left the camera unusable until relaunch; an unplugged camera
  froze the Preview and was not reopened when plugged back in.
- Bar and Preview positions could be overwritten by the panel; panel edits could be reverted by
  out-of-order updates; the panel's little preview showed the previous camera edit; Rounded
  looked like Square; the export's first frame had no camera.
- The driver's summary ignored failures in flows 12 and 14.

**UI refresh (owner 2026-11-20, ADR-044):** every window now follows macOS conventions:
- system colours in light and dark, AppKit-sized controls and System Settings-style grouped forms;
- a red Record button with a summary of what will be recorded;
- a blurred HUD control bar and a popover-material source switcher (native `NSVisualEffectView`).

Checked on the build Mac in a live driver run (flows 01, 04 and 14: 26/26) and on window
screenshots.

**What M2 does.** A Preview window shows the composed output with the live camera (any shape,
size, position, zoom/pan, mirror, border, shadow) while the recorder panel is open or a recording
runs, and is where the camera is placed (ADR-043). Moves, resizes, shape changes and hiding during a recording become keyframes. The quick
export and the panel still draw the camera with the same compositor code as the Preview window. macOS
Video Effects button. One camera session serves preview and recording.

**Measured on the build Mac** (M3 Pro, macOS 26.7.2, LG 3840×1080 pt display at 2×, OBSBOT Tiny 2
at 1920×1080 30 fps):

| Check | Result | Source |
|---|---|---|
| Camera layer GPU vs CPU reference, every shape/option, RGBA and I420 | max difference 0 | `crates/render/tests/camera.rs` |
| Camera frames read in place from IOSurfaces vs uploaded, 420v and 420f | max difference 0 | `iosurface_camera_frames_match_uploaded_copies` |
| Geometry Rust vs TS, shared case table | all cases equal | `crates/geometry/tests/camera_cases.json` |
| Preview window: live frames, placement, camera edits, 9:16 (live, driver flow 12) | 25 fps (FaceTime HD), drawn by the native Metal layer; on screen in the bottom-right; edits arrive at once; a window screenshot shows the camera outline and handles on the drawn camera | `app/driver/flows/12-preview.txt` |
| Same, webview-frames fallback (`SC_PREVIEW=webview`) | 13/13 checks, 25 fps | flow 12 |
| Preview resize and outline (live, flow 12): resize to 720×480 pt, then compare the page with the renderer | drawable 1440×896 px = page viewport × 2; the page's camera outline is within 0.03 px of the drawn camera after a panel edit | flow 12 |
| Preview window absent from a display recording (live, flow 14) | 0 of 57 frames show it | `app/driver/flows/14-preview-capture.txt` |
| Keyframes while recording (live, flow 14; synthetic + recovery test) | start key at 0 plus the drag; journaled; rebuilt by recovery | flow 14, `camera_keys_are_recorded_journaled_and_recovered` |
| Export with the camera (unit + real CLI recording, 9:16) | camera drawn where the keys put it, hidden after a hide key; real export checked by eye | `export_camera.rs` |
| CPU, Preview window + panel shown, camera on (FaceTime HD, about 25 fps), **release**, native Metal layer (ADR-007) | app 17.5–17.9% + its WebKit processes 0.5–0.8% = **18.1–18.4% of one core** in three runs (plan budget: 15%) | CPU time over 15 s of a driver run, MacBook display |
| Same through webview frames (before the native layer) | app 20.5% + WebKit 9.0% = 29.5% (34.6% before the first 2026-11-20 cuts; 31.7% debug) | same |
| Same, camera off, release, native layer | **4.3%** (frames only when something changes) | same |
| Memory, Preview with the live camera for 2 min (release, 3,079 frames presented) | phys_footprint flat at 382–399 MB, RSS settles at about 135 MB: no per-frame growth | `footprint`/`ps` every 10 s |
| **30-minute soak** (M2 review item 8; release; display + FaceTime HD + mic + system audio; Preview shown; overnight 2026-11-21 01:20, commit 7e1dfc0) | 18/18: 1800.1 s; 0 dropped and 0 writer stalls on every track (camera 53,979 frames, screen 51,864); the Preview drawn natively throughout at 23.6–25.7 fps (camera 30); app CPU 35.5% average, 38.6% max of one core; RSS 145 → 89 MB; phys_footprint 387 → 390 MB (+0.08 MB/min) | `app/driver/run.sh 16-soak`, CPU and memory every 30 s |
| Same soak before the autorelease fix (two runs, 23:00 and 23:31) | 18/18 both, but phys_footprint grew 389 → 427 MB (+1.4 MB/min, linear). A/B runs of 6 min: Preview hidden 0 growth, Preview without camera +5 MB heap, Preview with camera +10 MB heap and +9 MB GPU. Cause: the Preview's render thread had no autorelease pool; renders and snapshots now drain one (7e1dfc0) | `footprint` by category at minutes 1 and 5 |
| Local CI (`ci-local-fast`, helpers and GPU required) | 394 Rust + 40 UI tests, 0 failures | |

**Not verified / open:**
- **The CPU budget is still missed with the camera on, by about 3 points** (about 18% release vs
  15%). WebKit is out of the per-frame path; what is left is macOS's camera pipeline (CoreMediaIO
  threads, about 5%), Metal's own command-queue threads, and wgpu's per-frame work on the stage
  thread (about 3.6%). A Preview capped below the camera's rate would close it.
- Native Preview layer: a live resize by hand and dragging to a display with another scale were
  not tried (a programmatic resize is checked in flow 12); photon latency was not measured.
  Windows and Linux still use webview frames (ADR-007).
- Real mouse interaction (camera drag and resize in the Preview, the bar's grip) was driven
  through commands, not real clicks; MANUAL_TEST §10b M1–M7 covers it.
- No A/V sync measurement of the camera (M1 gap remains).
- While recording, the Preview presents about 24 fps from a 30 fps camera (it keeps every frame
  when not recording); it never stalled in the soaks.
- Seen once on Windows CI (2026-11-21, `stop_before_the_duration_ends_there`): a stop that lands
  before the first segment has opened leaves that segment without an end time in the project. M1
  code; the re-run passed and it does not reproduce locally; to look at before M3.
- Keyframes are not editable until the M3 editor.
- The Preview refreshes the screen once a second (the camera is live).

M1 delivers (SPEC M1 line plus the 2026-11-19 additions): display, window, application, region and
system-picker sources; permission onboarding that never prompts by itself; separate crash-safe
tracks (screen as segments, camera, mic and system audio continuous); `project.json` v1, the journal
and the recovery scan; switching source while recording; the output canvas (Original, 16:9, 4:3,
1:1, 9:16) with solid, gradient and image backgrounds; a quick x264 MP4 export; capability probe v1;
the `simple-capture` CLI; the Tauri app with tray, recorder panel, overlays, floating bar and Saved
window; the sync, soak and kill tests; CI for macOS, Windows and Linux; docs/MANUAL_TEST.md.

**How the numbers here are sourced.** M1 was built by a team of agents in packages (docs/M1_PLAN.md
§8). Each package returned a report to the orchestrator; the reports are not stored in the repo, so
**§2 of this file is the in-repo record of every M1 number**, each naming its source as
[M1:<package>] (for example [M1:soak] = wp-soak-test) plus the local result file or commit where one
exists. Files under `target/` are gitignored and exist only on the build Mac. Every recording made
by an agent was deleted after its numbers were taken; nothing recorded is committed.

All numbers come from an Apple M3 Pro (Mac15,6, 18 GB, macOS 26.7.2, one 3024×1964 display at 2×
during the live M1 runs). **Nothing was measured on the base M1 8 GB target.**

**Owner hands-on, 2026-11-20 (first pass), and fixes:**
- **Crash on Record with an OBSBOT Tiny 2 camera.** The camera advertises 30 fps as exactly
  1000000/30000030 s; AVFoundation raised NSInvalidArgumentException for our rounded 1/30 s, and an
  Objective-C exception reaching Rust aborts the app. Fixed in 9ed2fe5: frame durations come from
  the device's own ranges, and `objc_try` turns Objective-C exceptions into errors across camera/mic
  setup and the AVAssetWriter create/start/finish calls. A review found one more raise of the same
  kind (a mic with 3–8 channels makes AVAssetWriterInput raise); such mics are now downmixed to
  stereo at capture. Live: all four cameras on the build Mac (OBSBOT, FaceTime HD, EOS Webcam
  Utility, iPhone) record at ~30 fps; a window recording with the OBSBOT camera and mic plus system
  audio writes all four tracks.
- **9:16 fit used the whole display for app sources.** An application source was captured as the
  whole display with only the app's windows drawn, so every aspect fitted the display width. Now
  cropped to the union of the app's windows at segment start (ADR-039, merge 324eabd). Live: a
  Finder app source recorded at 1840×872 instead of 7680×2160, and the 9:16 export fills the
  canvas width. Window sources were already window-sized. Preview sizes at 1x now match the
  recorded size for every source kind (e5b93af).

**Owner hands-on, 2026-11-20 (second pass), and fixes:**
- **Select region covered only the top part of the screen and could not select.** Overlays were
  moved, then resized; AppKit resizes around the bottom-left corner, so the window grew above the
  display. Now sized first, then placed (2360a18). The driver checks the overlay frames.
- **Pick window by clicking never picked a window; Windows/Apps lists held menu-bar items,
  Dock, Control Center and apps with nothing on screen.** The list included the WindowServer
  "Cursor" window, which is always under the pointer. Lists now hold only on-screen windows of
  regular apps (ADR-040): 37 → 12 windows and 10 → 7 apps on the build Mac.
- **9:16 showed side bars for a wide window.** Only the panel's preview box was wrong (fixed size,
  squashed to a square). A CLI 9:16 export of a 1920×949 pt window fills the full frame width, with
  stereo audio.
- **Simple Capture UI in the recording.** Most likely the unfiltered list (picking our own overlay
  or the Cursor window). The exclusion driver flow now also shows the recorder panel during a
  display recording. Our menu-bar icon is still captured in display recordings, like other apps'.
- **Camera not shown while recording, and not in the export.** By design for M1: the camera is
  recorded as its own track; the camera bubble and compositing are M2 (SPEC milestones).

---

## 1. M1 exit checklist (M1_PLAN §1), state on 2026-11-20

| # | Item | State | Evidence |
|---|---|---|---|
| 1 | `just run` under the dev host, no new prompt | **Met for agents** | `just run` printed SC_READY and SIGTERM exited 0 [M1:app-shell]; permissions read granted before and after every e2e and driver run, with status reads only [M1:e2e, M1:app-integration]. Caveat: early fixture launches from `~/Documents` probably raised a Documents-folder consent for an ad-hoc `sc-devtools` (§4, action 3). |
| 2 | Every source records; every switch path; segments tile; audio continuous; closed window = source lost | **Met, except paths that need a second display or the picker UI** | `just e2e`: 13 of 14 steps pass twice, step 11 (non-main display) skipped, one display [M1:e2e]. App driver: 82 of 82 checks, 11 flows, every switch path (bar, tray, ⌥⇧S + pick, region overlay, command) [M1:app-integration]. The system picker was configured and read back, never presented (it takes focus): MANUAL_TEST P1–P7. |
| 3 | Every aspect × background exports to a valid MP4; colour within 3/255 | **Met for validity and colour; the QuickTime look is the owner's** | 20 of 20 exports pass `validate` [M1:e2e step 13]; colour patches within 3/255 [M1:export tests]; look in QuickTime: MANUAL_TEST E2. |
| 4 | Kill tests recover; killed app recovers on launch; Quit/SIGTERM finalize | **Met** (loss bound is an owner question) | Live 12 of 12 kills recover and export; CI `kill_synth` 6 of 6 [M1:kill-recovery-test, merge 76426a6]; driver: `kill -9` → `recovered`, Quit and SIGTERM → `finalized` [M1:app-integration]. Loss per track up to 1.076 s with 1 s fragments (ADR-008, §4). |
| 5 | Sync at start, after each switch, at end; system audio, mic, camera | **Partly met** | System audio: median −10.2 ms, 27 of 30 events within ±33 ms; no fixed compensation (ADR-011). **Mic and camera not measured**: the output was muted and the room dark [M1:sync-test]. Owner rerun (§4). Sync across pauses is M7. |
| 6 | 30-minute soak decides ADR-010 | **Met** | Agent run on the real screen, camera, mic and system audio: every threshold passed; **no frame copy** [M1:soak, `target/soak/20261120-000343/summary.md`]. Mic end sync n/a (mic did not hear the beep). |
| 7 | CI green on all three OSes | **Met** | GitHub run 44719974664 (m1, 2026-11-20): all 8 jobs pass: ui, licenses, fast and full on macos-15, ubuntu-24.04 and windows-2025. Windows needed the stale-lock fix at close-out; the Windows Skia FreeType build passed in this run but failed in earlier ones (STL1000), so watch it (§3 item 2). |
| 8 | cargo-deny, pnpm licences, no networking, approved deps only | **Met** | `licenses` job green in runs 44695891492, 44705677113 and 44710191597; `netcheck: ok`; Cargo.lock unchanged since the scaffold (LICENSES §10). |
| 9 | Release bundle built and inspected, not launched | **Met** | App and both helpers signed `adhoc,runtime`; both entitlements; three usage strings [M1:app-shell]. |
| 10 | MANUAL_TEST; STATUS, DECISIONS, ARCHITECTURE updated | **Met** | docs/MANUAL_TEST.md (53 owner steps) [M1:manual-test]; this update (ADR-037, ADR-038 and M1 updates to ADR-008/009/010/011/018/022/024/028/031/034/035). |

---

## 2. M1 measured results

### 2.1 Soak, 30 minutes (ADR-010 decision run)

Source: [M1:soak], `target/soak/20261120-000343/summary.md` (2026-11-20 00:03, debug build, deps at
opt-level 3). Display 3024×1964 at 2×, 30 fps, default camera (FaceTime HD), default mic, system
audio All; switches display ↔ window every 5 min (segments s0–s5, 0 switch failures).

| Measure | Threshold (M1_PLAN §1 item 6) | Result |
|---|---|---|
| Screen drops | < 0.5 % | 0 of 52,581 frames (0.000 %); writer not ready 0 |
| Longest screen stall / lowest 10 s rate | – | 0.0 s / 28.1 fps |
| RSS growth from minute 2 | ≤ 50 MB | 73.3 → 47.9 MB (−25.5 MB, slope −0.83 MB/min), peak 78.4 MB |
| StreamStopped / source lost | 0 | 0 / 0 |
| End sync, system audio vs screen | change within 33.3 ms | −21.1 ms (start +26.8 ms, end +5.6 ms; 5-flash bursts) |
| End sync, mic vs screen | within 33.3 ms | not measured (mic did not hear the beep) |
| Audio drift, count-based vs host time | reported | mic 0.0 ms, system audio 0.0 ms; 0 gaps, 0 overlaps |
| Camera | – | 53,975 frames, 0 dropped |
| CPU, average / max of one core | – | simple-capture 26.8 / 36.1 %, replayd 3.8 / 4.8 %, VTEncoderXPCService 6.5 / 7.4 % |

Minor open item: the mic's `health.frames` was 13,824 samples (+288 ms) above duration × rate, while
its file was exactly 1800.0 s; not growing with run length [M1:soak].

### 2.2 Switching

| Measure | Result | Source |
|---|---|---|
| Request → cut, live (two runs) | display→window 146.3 / 142.4 ms; window→region 112.7 / 114.5 ms; region→display 114.0 / 130.2 ms (target 500 ms) | [M1:e2e] run 5, `target/e2e/report-20261119-232627.json`, `…232905.json` |
| Recorded gap, last old → first new frame | 5.6–41.6 ms | same |
| Trailing frames dropped per switch | 1 | same |
| Peak RSS, two native-size streams overlapping | 48.3 / 47.3 MB | same (`ps` every 200 ms) |
| Switch to a static window (SC-STATIC) | 149.3 / 151.9 ms | [M1:e2e] step 8 |
| Wrong-content frames | 0 in 5 runs (55–111 frames each) | [M1:mac-sck] |
| Gap at the new stream's first frame vs best point in the overlap | > 34 ms in 6 of 15 switches (worst 41.7 ms); best point 0–20.8 ms | [M1:mac-sck] |
| `switch_begin` → `segment_prepare`; `segment_prepare` → cut (real writer, synthetic source, median / max) | 4.0 / 11.4 ms; 60.2 / 88.5 ms | [M1:record-session], `session_macos` |
| Synthetic stop-then-start gap | 64 ms | same |
| Source lost after a window closes | about 80 ms (watcher; SCK gives no error) | [M1:mac-sck]; e2e: lost 4.13 s, last s0 frame 4.09 s [M1:e2e] |
| App mode filter change (`follow_app`) | 144–178 ms; 76–95 ms audio timestamp hole, filled | [M1:mac-sck] |

### 2.3 Sources and capture

| Measure | Result | Source |
|---|---|---|
| First-frame latency | display 158 ms, window 63 ms, application 347 ms, regions 161–360 ms | [M1:mac-sck] final live run |
| Regions by `sourceRect` | 617×283.5 pt → 1234×566 px; 1121×641 px → 1120×640 px; 1×1 pt → 2×2 px; content correct | [M1:mac-sck], [M1:e2e] step 4 |
| Application capture, second window opened mid-recording | picked up after 66–77 ms | [M1:mac-sck]; e2e step 3 |
| Hidden-then-shown own window | excluded, 9 of 9 frames; a window created after start visible until `update_exclusions` (82–105 ms) | [M1:mac-sck] |
| Own test window in recordings | absent from 963 checked frames | [M1:e2e] step 12 |
| Tray menu in display recordings | 0, 0 and 1 frame in three runs (was 9 of 9 before the background refresh) | [M1:app-integration], `target/app-driver/20261119-230228/report.json` |
| `--fps 60` delivered | 52.3 / 51.6 fps native 3024×1964; 52.5 / 50.8 fps at 1× | [M1:e2e] step 9 |
| Camera (live) | 1920×1080 420v 30.0 fps, start 1176 ms, worst delivery latency 112 ms | [M1:mac-av] |
| Mic (live) | 48 kHz mono f32, worst latency 140 ms | [M1:mac-av] |
| Devices listed | 4 cameras (incl. Continuity and Desk View), 4 mics | [M1:mac-av] |

### 2.4 Sync

| Measure | Result | Source |
|---|---|---|
| Video → system audio, final 3 runs | median −10.2 ms (min −41.1, max +8.5); 27 of 30 within ±33 ms; per run −26.4, −4.8, −3.1 ms | [M1:sync-test] 2nd attempt, `target/sync/20261120-005228/` |
| By phase (median, within ±33 ms) | start −13.9 (5/6); after switch 1 (window) −14.6 (5/6); after switch 2 (display) −10.2 (6/6); end −5.5 (6/6) | same |
| Earlier sets | median −15.6 ms (28/30); −6.8 ms (30/30) | [M1:sync-test] 1st attempt |
| issue → system audio | stable within a run; about +0.5 ms in one run and about +19.5 ms in every other | both attempts |
| issue → video (stimulus display latency) | median +23.5 ms, range 10.8–48.6 ms | 2nd attempt |
| Mic | heard 0 of 30 beeps (output muted: `osascript` read `muted:true`) | 2nd attempt |
| Camera | 0 of 6 full-screen events above the noise floor (largest step 0.02 levels, dark room); one earlier low-confidence onset about 108 ms after issue | both attempts |
| Recommendation | no fixed system-audio compensation (ADR-011) | – |

### 2.5 Crash safety

| Measure | Result | Source |
|---|---|---|
| Live kills (plain 6 s, after `switch_begin`, after `segment_prepare`, after `segment_open`; 3 runs each) | 12 of 12 recover, files decode, exports validate at 740×480 | [M1:kill-recovery-test] 2nd attempt, with the empty-file rule (merge 76426a6) |
| Loss per track, live, `fragment_interval_s = 1.0` | up to 1.056 s (2nd attempt), 1.076 s (1st); 14 of 21 measurements over 1 s | same |
| CI kill tests (`kill_synth`, synthetic) | 6 of 6, ≤ 1 s loss, about 2–3 s wall | same |
| SIGKILLed writer unit test | screen 0.53 s, mic 0.57 s lost | [M1:mac-av] |
| Recovery unit tests | 13 synthetic tests | [M1:record-recovery] |

### 2.6 Export, probe and render

| Measure | Result | Source |
|---|---|---|
| Quick export 1920×1080, 30 s at 30 fps | 5.20 s, 173.1 fps, 5.77× realtime | [M1:export], `export_bench`, synthetic frames |
| Quick export 3840×2160, 30 s | 13.85 s, 65.0 fps, 2.17× | same |
| x264 7680×2160, 10 s | 11.18 s, 26.8 fps, 0.89× (why quick export caps at 3840, Q6) | same |
| Reference export of a real 10 s 3024×1964 recording | 4.07–4.3 s | [M1:e2e] step 1 |
| Probe v1, HEVC 60 fps | native 3024×1964 117.9 fps (60 allowed, threshold 78); 1× 1512×982 386.8 fps | [M1:export], `simple-capture probe --refresh` |
| `h264_videotoolbox -allow_sw 0` | opens at 3840 and 4096 ×2160, fails at 4098 | same |
| HEVC `encode_benchmark` 1920×1080, 60 frames | 129.3 fps (short run, includes setup) | [M1:mac-av] |
| Compositor GPU vs CPU | max 0, mean 0 (bit-identical) for every aspect and scale tested | [M1:render] |
| Compositor `render()` on Metal, Rgba8 / I420 | 1080p 6.3–9.7 ms; 2160p 17.4–18.7 ms; CPU reference 27–161 ms | [M1:render], noisy machine |

### 2.7 Build and CI

| Measure | Result | Source |
|---|---|---|
| Clean workspace dev build incl. Skia FreeType | 4 min 17 s [M1:scaffold]; 233 s under load average 30–50 [M1:ci-green] | – |
| Same with deps at opt-level 0 | 132 s (ADR-031: keep opt-level 3) | [M1:ci-green] |
| Incremental, real edits | app main.rs 1.6 s; geometry lib.rs (all 15 crates) 3.5 s | [M1:ci-green] |
| Workspace tests (`ci-local-fast`) | 303 passed, 0 failed, 23 ignored/live at the integration merge; UI 32 of 32 | integration merge report |
| CI durations, run 44695891492 | ui 22 s, licenses 1 m 41 s, fast ubuntu 4 m 13 s, fast macOS 6 m 26 s, full ubuntu 27 m 41 s, full macOS 23 m 49 s | [M1:ci-green] |
| First green macOS run | 44705677113: fast 7 m 26 s, full 23 m 12 s | [M1:ci-green] 2nd attempt |
| Latest run | ui 16 s, licenses 1 m 44 s, fast ubuntu 4 m 18 s, fast macOS 8 m 10 s, full ubuntu 19 m 06 s, full macOS 23 m 04 s (all pass); fast windows 5 m 24 s and full windows 10 m 34 s (fail) | GitHub run 44710191597 (m1 @ 76426a6) |

---

## 3. Open risks

1. **No M1 8 GB data.** Every number is from the M3 Pro, including the soak (one debug-build run),
   export speeds (synthetic content) and the probe.
2. **Windows CI is green but young** (run 44719974664). The Windows Skia FreeType build failed in
   earlier runs with STL1000 (clang-cl vs the VS 18 STL) and passed here; it may be cache-dependent.
3. **Kill loss is up to about 1.08 s per track** with 1 s fragments, over the documented ≤ 1 s
   (ADR-008, owner question).
4. **The open tray menu can leak 0–1 frame** into a display recording (ADR-010, owner question).
5. **Mic and camera A/V sync are unmeasured.** System audio is within target. The mic never heard
   a beep (muted output) and the camera saw a dark room.
6. **System audio's time base moved by about 19 ms between sessions** (stable within a session).
   Within the one-frame target, but unexplained; worth a look at the SCK audio PTS mapping.
7. **"Only the captured app" audio** cannot isolate unbundled processes (SCK groups by bundle ID),
   and the wp-mac-sck and e2e results on two unbundled tone apps disagree. Bundled apps isolate.
8. **One display only.** Non-main displays, displays of a different scale, regions on them and
   overlay repositioning across displays were never run live.
9. **Picker path and whole-app exclusion** are untested by design (they need the owner and the
   never-granted release bundle, Q8): MANUAL_TEST P1–P7, B0–B6.
10. **Not run live by agents:** real device unplug (camera, mic, display), real sleep, disk full,
    logout, Mic Mode panel (`SC_LIVE_MIC_MODES=1`). All are owner steps in MANUAL_TEST.
11. **Cut gap up to about 42 ms** at a switch (6 of 15 switches over 34 ms in wp-mac-sck's test);
    cutting at the best point inside the overlap would give 0–21 ms.
12. **Dev environment:** binaries under `~/Documents` hung in dyld `open()` as hosted or disclaimed
    children while a Documents-folder consent was pending; live scripts now stage in `$TMPDIR`, but
    `just test-live` and `just app-driver` still start from the repo. Builds into the shared
    `target/skia` from different worktrees can link another worktree's crates.
13. **60 fps** delivered 51–52 fps at 3024×1964 on a changing test screen (probe allows it).
15. **Timing-sensitive synthetic tests.** `session_synthetic` passed 9 of 10 runs locally; the
    failing run (2 tests, different ones each time) happened while the machine was busy. Watch CI;
    widen the waits if they flake there.

**Fixed at close-out (orchestrator, 2026-11-20, commit after 1b17b1b):**
- The acceptance check found that at 60 fps the last segment could end up to one frame after the
  recording's stop (e2e step 9 failed 2 of 4 runs): a frame stamped just after the stop time could
  reach the router first. The stop now moves to the last segment's end. Step 9: 3 of 3 live runs
  pass.
- Windows: a project `.lock` held by a dead process is now detected (`OpenProcess` /
  `GetExitCodeProcess`), which was why `kill_synth` returned `Locked` on Windows CI.
14. **Carried from M0, unchanged:** gifski notice texts and the gifsicle-feature provenance; BtbN
    for Windows/Linux and the Windows 10 minimum (M6); FFmpeg GPG signature unverified; the helper's
    missing PNG and AV1 decoders; gifski x86_64 not built; Small and High quality tiers untested;
    Linux WebKitGTK and Windows WebView2 preview cost (M6).

Resolved in M1 (were M0 risks): SCK frame copy (not needed), own-UI exclusion across switches,
region/application/app-audio/hot-plug/onboarding/Mic Mode untested (built and tested; see the
caveats above), objc2-app-kit linking CloudKit and CoreData (trimmed), Skia FreeType on Linux
(passes), pause (moved to M7 by plan).

---

## 4. Owner questions and actions

**Questions (answer at the ★ review):**

| # | Question | Options | Where |
|---|---|---|---|
| Q-a | Kill loss bound | **Answered 2026-11-24:** about 1.08 s with 1 s fragments is accepted, nothing longer; the live kill test fails above 1.1 s and a test keeps fragments at 1 s | ADR-008 |
| Q-b | Tray menu leak (0–1 frame) | **Fixed 2026-11-24:** while recording, the tray opens our own menu window, excluded before the recording starts (0 of 37 frames in flow 05) | ADR-010 |
| Q-c | Mic level meter and debug overlay (SPEC v1, on no milestone line) | proposed: M7 (polish and performance pass) | M1_PLAN §1 |
| Q-d | App audio for unbundled apps | proposed: keep App mode, note in the UI that it isolates apps with a bundle ID | ADR-010 |
| Q-e (optional) | Drop Skia's default `pdf` feature (and with it expat) | `default-features = false, features = ["textlayout", "embed-freetype", "jpeg"]`; rebuilds Skia once | LICENSES §2.4 |
| Q8 | Release-bundle test (never-granted bundle, picker, whole-app exclusion, camera/mic prompts) | owner only | MANUAL_TEST §10 |

**Actions:**

| # | Action | Why / when |
|---|---|---|
| 1 | Work through docs/MANUAL_TEST.md (about 53 steps; the ★ list below orders them). | ★ review |
| 2 | Unmute the speakers, light the room, put something in front of the camera, and run `SC_SYNC_RUNS=3 just sync-test`; then the clap step C1. | Gives the missing mic and camera sync numbers (§1 item 5) |
| 3 | Probably nothing to do: agents saw hosted processes hang on `~/Documents` paths from about 22:23 on 2026-11-19, but at close-out (2026-11-20) the dev host read the repo without hanging. If a "would like to access files in your Documents folder" dialog for Simple Capture Dev or `sc-devtools` is still open, answer it. | Only if one is open |
| 4 | Optional: `SC_LIVE_MIC_MODES=1 just test-live simple-capture-capture-macos av_ av_show_mic_modes` (opens Control Center's Mic Mode panel). | Agents skip it to keep your focus |
| 5 | Connect a second display at a different scale for MANUAL_TEST S7 and W4. | Only live gap in sources |
| 6 | **Do not** rebuild or re-sign `~/Applications/Simple Capture Dev.app` (`install.sh --force`). It holds the Screen Recording, Camera and Microphone grants. | Always |
| 7 | Run the benches and a soak on a **base M1 8 GB** if you can get one. | Before M2 (encoder headroom) and M3 (preview, memory) |
| 8 | Arrange a **physical Linux machine** (Intel/AMD iGPU, Wayland and X11) for the WebKitGTK preview bench. | Before M3 preview work, so a fallback decision does not land in M6 |
| 9 | Choose the Windows minimum (10 22H2 vs 10 2004) once our own Windows FFmpeg build exists (you already chose our own builds with networking off, ADR-013). | Before M6 |
| 10 | Review the Small and High quality tiers on a real screen recording (ADR-016). | Before M5 |

---

## 5. Carried to M7

**From pause (M1_PLAN §1 "Not in M1"; ADR-012).** `Session::pause/resume` exist and return
`Unsupported`; the schema's `pauses`, `RecClock` pause maths and the journal `pause`/`resume` events
exist and recovery already folds them, so M7 needs no migration. M7 test items:
- re-stamp the newest frame dropped during a pause at resume (Cap had a bug here);
- rewrite `project.json` at pause and at resume;
- A/V sync across pauses (SPEC Pitfall 2): the sync test with pauses, at start, after each pause and
  at the end;
- switching while paused (the new segment starts at resume);
- resume across sleep (M1 stops and finalizes on sleep);
- the 3-2-1 countdown, start/stop and pause hotkeys, and the floating bar's Restart button.

**Also M7 (SPEC M7 line or proposed):** recovery UI beyond the launch notice, Library, segment
rollover on window resize (`source_resized` is only logged), disk-guard and hot-plug polish,
`cursor.jsonl` and our own click drawing (Q5), the mic level meter and the debug overlay (Q-c).

---

## 6. M0 record (2026-11-19)

M0 was approved by the owner on 2026-11-19. Pre-work before M1, each re-checked by an independent
verifier: native preview on Windows/Linux was not proven, so **webview frames on every OS**
(ADR-007); all three OSes have a system picker, so "System picker…" is a source everywhere
(ADR-028); make-before-break switching on macOS gave 0–28 ms gaps with no wrong frames, and system
audio moved to its own stream with silence filling (ADR-034, ADR-035); 3 of 13 fonts were replaced
(ADR-020); the output became a canvas with a letterbox background (ADR-033). Owner decisions are
recorded in DECISIONS.md (ADR-036 for the M1 plan answers).

| Spike | Verdict | Key verified result | Main corrections / caveats |
|---|---|---|---|
| 1 Helpers | **Pass** | Static FFmpeg 9.0.2 (GPL-2.0+, no network) and gifski 1.34.0. All 29 outputs pass ffprobe, 3 runs out of 3. 1080p30: x264 medium 197 fps, x265 120, SVT-AV1 p8 189, VP9 64, VideoToolbox H.264 191 / HEVC 177. gifski 41 fps at 960×540. Cancel is clean with no leftover processes. | The 4-thread proxy is noisy. FFmpeg palette GIF runs at about realtime. A SIGTERMed gifski partial is truncated. VideoToolbox H.264 (`-allow_sw 0`) fails when either dimension exceeds 4096. |
| 2 Preview | **Pass, keep Tauri** | Every transport: 58.5–60.1 of 60 fps, including full 1080 through the webview. | The constrained pass did not constrain anything. Photon latency, WebKitGTK and WebView2 are untested. |
| 3 macOS pipeline | **Pass** | SCK, camera, mic and system audio each write a fragmented HEVC/AAC .mov; a SIGKILL lost ≤ 0.86 s (M1: up to 1.08 s); all clocks on host time; zero-copy decode into wgpu. | 60 fps at full native size reached only 44.5 fps. Whole-app UI exclusion fails under the dev host. |
| 4 Skia text | **Pass** | Every text style plus colour emoji works; GPU vs CPU max 1/255. | Needs a FreeType source build (stock macOS skia-safe uses CoreText). |

Spike reports: `spikes/0N-*/REPORT.md`; raw evidence as listed in DECISIONS.md "Source keys". No spike
app has its own name or bundle ID any more; everything that needs a permission runs under
`Simple Capture Dev` (ADR-002, CLAUDE.md).

| Item | Where |
|---|---|
| Spec (source of truth) | [docs/SPEC.md](SPEC.md) |
| Architecture | [docs/ARCHITECTURE.md](ARCHITECTURE.md) |
| Decisions (38 ADRs) | [docs/DECISIONS.md](DECISIONS.md) |
| M1 plan and package briefs | [docs/M1_PLAN.md](M1_PLAN.md) |
| Owner test checklist | [docs/MANUAL_TEST.md](MANUAL_TEST.md) |
| Codec matrix, licences, third-party | [CODEC_MATRIX](CODEC_MATRIX.md), [LICENSES](LICENSES.md), [THIRD_PARTY](THIRD_PARTY.md) |
| Research notes | `docs/research/` |
| Helper build tooling (pinned, hash-verified) | [third_party/README.md](../third_party/README.md) |
| Permission dev host (one app, never renamed) | `tools/macos-dev-host/`, rules in [CLAUDE.md](../CLAUDE.md) |

---

## 7. ★ review list (M1)

Allow about 2–3 hours for everything; items 1–5 take about 30 minutes and need no recording.

1. **Read** §1 and §2 above, then the M1 updates in [DECISIONS.md](DECISIONS.md): ADR-037 (the
   plan's own choices and their results) and ADR-038 (choices made inside packages), plus the
   "M1 result/update" blocks in ADR-008, 009, 010, 011, 018, 022, 024, 028, 031, 034 and 035.
2. **Answer** questions Q-a to Q-e in §4 (fragment interval, tray menu, mic meter/debug overlay
   milestone, App audio wording, optional Skia features).
3. **CI:** GitHub run 44719974664 on `m1`: all eight jobs green (macOS, Ubuntu, Windows).
4. **Code tour:** `git diff main...m1 --stat`; ARCHITECTURE §4 (switching as built), §6 (journal
   table), §11 (probe-size rule), §12 (recovery and kill tests).
5. **Licences:** LICENSES §2 (MPL exceptions, Skia vendored set) and §10.
6. **Run the app:** `just run`, then MANUAL_TEST §1 (L1–L4: tray, panel, onboarding) and §2 sources
   S1–S6, S8–S13. Note any prompt in the MANUAL_TEST §12 prompt log.
7. **Switching:** MANUAL_TEST W1–W6 (bar, tray, ⌥⇧S, region, closed window, discard).
8. **System picker:** MANUAL_TEST P1–P7, including P7 (picked display with the bar visible: check
   for our pixels).
9. **Export:** MANUAL_TEST E1–E4; open the 20 aspect × background exports (E2) in QuickTime and
   check the letterbox and colours; E4: is the tray menu or our UI visible anywhere?
10. **Sync:** §4 action 2 (`SC_SYNC_RUNS=3 just sync-test` with sound on and a lit room), then C1
    (clap).
11. **Crash safety and system events:** K1–K2 (kill and relaunch), H1–H8 (hot-plug, unplug,
    display unplug, sleep, quit, low disk, frames-dropping hint).
12. **Second display** (if available): S7 and W4.
13. **Release bundle (Q8, owner only):** MANUAL_TEST B0–B6: never-granted bundle through the
    picker, behaviour after a rebuild, whole-app exclusion (`SC_EXCLUSION=app`), camera and mic
    prompts with our usage strings.
14. **Optional:** `just soak 30` (O1) for an owner soak in the same summary format;
    §4 action 4 (Mic Mode).
15. **Decide:** approve M1 and merge `m1` into `main`, or list fixes.
