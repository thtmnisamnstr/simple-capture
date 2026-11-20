# System pickers and switching source mid-recording (macOS, Windows, Linux)

Status: desk research, 2026-11-19. No capture code was written or run for this note.

**What was RAN** (fetched and read):
- `curl` of the Apple documentation JSON, Microsoft Learn pages and the flatpak portal docs.
- The ScreenCaptureKit headers in the local SDKs (CLT `MacOSX26.5.sdk`, Xcode `MacOSX27.2.sdk`).
- Sources downloaded to the scratchpad and read: OBS Studio master (GPL, study only), xdg-desktop-portal-kde (LGPL), xdg-desktop-portal-gnome, xdg-desktop-portal, robmikh/Win32CaptureSample (MIT).
- `grep` over the locally cached crates `screencapturekit` 11.1.2 and `windows-capture` 2.0.1.

**What was only READ:** WWDC pages, forum threads, third-party PRs. These are marked *secondary*.

**Not measured:** every latency, gap and cost figure. None are given here.

Builds on [ecosystem] §2a, which covers the re-prompt facts, the DTS thread and the entitlement. Those are not repeated here.

## TL;DR

1. **Every OS has a system screen picker**, so the owner's condition for ADR-028 is met. The pickers are not identical:
   - macOS can pick windows, apps or a display, several at once.
   - Windows can pick only one window or one display.
   - Linux (the portal) can pick monitors and windows, several at once. KDE also offers a region and the whole workspace.
   - None offers our region drag, numeric W×H or aspect presets. On every OS, "region" becomes *pick a display, then crop with our overlay*. KDE can also return a region itself.
2. **Switching source while recording is feasible on every OS**:
   - macOS can retarget a live stream in place (`updateContentFilter`).
   - Windows needs a new capture session.
   - Linux needs a new portal session (a dialog) unless the source was granted earlier: picked up front with `multiple`, restorable with a token, or a crop of a monitor already being captured.
   - On all three, the new capture can be started before the old one stops ("make-before-break").
3. **Recommended design:**
   - The recorded screen track becomes a list of **source spans**, one screen file per span.
   - The compositor fits each span into the fixed output aspect frame (the new 16:9 / 4:3 / 1:1 / 9:16 requirement).
   - Audio and camera never restart.
   - On macOS, system audio in "All" mode must come from a capture whose filter does **not** change. SCK audio is per app, so a window filter would narrow it.

## 1. System pickers

### 1a. macOS: `SCContentSharingPicker` (macOS 14.0+)

| Topic | Fact | Source |
|---|---|---|
| Object | The singleton `SCContentSharingPicker.shared` (ObjC `sharedPicker`). It must be marked `active`. The macOS 27.2 SDK header says `active` enables user interaction with "displayed picker, video menu bar, directly from a window". | SDK 27.2 `SCContentSharingPicker.h:93` (RAN: read local header) |
| Modes (`SCContentSharingPickerMode`) | `singleWindow` (1<<0), `multipleWindows` (1<<1), `singleApplication` (1<<2), `multipleApplications` (1<<3), `singleDisplay` (1<<4). **No region mode.** `allowedPickerModes` defaults to 0 ("no excluded picking modes"). Once changed, it cannot be reset to "all" with an enum value. | SDK 26.5 header; [allowedPickerModes](https://developer.apple.com/documentation/screencapturekit/sccontentsharingpickerconfiguration-swift.struct/allowedpickermodes) |
| Configuration | `excludedWindowIDs` (CGWindowIDs), `excludedBundleIDs`, `allowsChangingSelectedContent` (default YES). Set it per stream with `setConfiguration(_:for:)` or for all streams with `defaultConfiguration`. | header; [docs](https://developer.apple.com/documentation/screencapturekit/sccontentsharingpicker/setconfiguration(_:for:)) |
| Presenting | `present()`, `present(using: style)`, **`present(for: stream)`** ("Displays the picker with an already running capture stream") and `present(for:using:)`. | [present(for:)](https://developer.apple.com/documentation/screencapturekit/sccontentsharingpicker/present(for:)) |
| Callbacks (`SCContentSharingPickerObserver`) | `didUpdateWith filter, for stream?`, `didCancelFor stream?`, `contentSharingPickerStartDidFailWithError`. WWDC23 sample: if `stream != nil`, call `stream.updateContentFilter(filter)`; otherwise create a new stream. | header; [WWDC23 10136](https://developer.apple.com/videos/play/wwdc2023/10136/) |
| System entry points | The user can also change a running stream's content from the **Video menu bar item** (WWDC23), and can share "directly from a window" (27.2 header). Both arrive through the same observer. `maximumStreamCount` limits when Control Center offers a picker with no stream; **0 turns that off**. `allowsChangingSelectedContent = false` locks a stream's content. | header; WWDC23 |
| What a pick returns | An `SCContentFilter`. `contentRect` (points) and `pointPixelScale` are macOS 14.0+. `includedDisplays` / `includedWindows` / `includedApplications` are **macOS 15.2+**. | [contentRect](https://developer.apple.com/documentation/screencapturekit/sccontentfilter/contentrect), [includedDisplays](https://developer.apple.com/documentation/screencapturekit/sccontentfilter/includeddisplays) |
| Region | Possible as "pick a display, then crop". Our overlay draws on the live desktop (no capture needed) and we set `SCStreamConfiguration.sourceRect`. That works for display capture; it is ignored for single-window capture. We find the screen from `contentRect` (14.0) or `includedDisplays` (15.2). Alternatively capture the whole display and crop in the compositor. | [sourceRect](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/sourcerect) |
| Using `SCShareableContent` too | It still works, but it needs the Screen Recording grant. Calling it brings back the prompt, the "bypass" alert and the periodic re-prompt. **Exception:** `SCShareableContent.currentProcess` (macOS 14.4) returns redacted info about content "available to capture by current process **without user consent via TCC**", i.e. our own windows. | SDK 27.2 `SCShareableContent.h:155-159` (RAN) |
| Consent / TCC | DTS (Oct 2026): the picker and the Screen Recording permission are "the two documented ways for the person to give consent". *Secondary:* openscreen PR #737 (merged 2026-09-24) reports full-resolution video on **macOS 26.5 with no Screen Recording grant** for display and window picks, but **system audio arriving as silence without the grant**. It moved picked takes to a Core Audio process tap. Not verified by us. | [forums 849969](https://developer.apple.com/forums/thread/849969), [openscreen #737](https://github.com/getopenscreen/openscreen/pull/737) |
| Monthly "bypass" re-prompt | DTS (macOS 15) named the picker as the way to avoid it ([ecosystem] §2a). Not verified on 26. | [forums 765103](https://developer.apple.com/forums/thread/765103) |
| Keeping our own windows out | Picker config `excludedWindowIDs` (our `NSWindow.windowNumber` values, no TCC needed). *Secondary (openscreen):* without it, the app's own window "does appear" in a display pick, which suggests it also affects the captured display. Fallback, not verified: build `SCContentFilter(display: picked.includedDisplays[0], excludingWindows: currentProcess.windows)`. Whether such a rebuilt filter keeps picker consent is **unknown**. | as above |
| Persistence | A pick cannot be saved across launches (*secondary*, openscreen notes it re-asks after each launch). | openscreen #737 |
| Rust | `screencapturekit` 11.1.2 has `present`, `present_for_stream(_using_style)`, `add_observer` (repeating events, `Updated{stream: Option<_>}`), `set_allows_changing_selected_content`, `set_maximum_stream_count`, `update_content_filter` and `update_configuration`. It needs the main thread. | `src/content_sharing_picker.rs:1050-1180`, `src/stream/sc_stream.rs:917-955` (RAN: read) |
| macOS 27 SDK additions | `isAvailable` (macOS 27.0). `showsCameraControl`, `showsMicrophoneControl` and `presentPickerForCurrentApplication` are iOS/visionOS only (`API_UNAVAILABLE(macos)`). | SDK 27.2 header diff (RAN) |

### 1b. Windows: `Windows.Graphics.Capture.GraphicsCapturePicker`

| Topic | Fact | Source |
|---|---|---|
| API | Constructor plus `PickSingleItemAsync()` only. **No options**: no mode, no exclusion list, no multi-select. Returns a `GraphicsCaptureItem`, or null if cancelled. Must be called on the UI thread. Windows 10 1803 (17134). | [class](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturepicker), [PickSingleItemAsync](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturepicker.picksingleitemasync) |
| From Win32 / Tauri | Set the owner HWND with `IInitializeWithWindow::Initialize` before showing it. `GraphicsCapturePicker` is in Microsoft's list of classes that implement it. robmikh's Win32 sample (MIT) does exactly this. | [Display WinRT UI objects](https://learn.microsoft.com/en-us/windows/apps/develop/ui-input/display-ui-objects), Win32CaptureSample `App.cpp:99-113` |
| What it picks | "the display or application window to be captured". **No application-level and no region.** Region = pick a display, then crop (as the spec already says). | [Screen capture](https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture) (2026-08-23) |
| Without a picker | `IGraphicsCaptureItemInterop::CreateForWindow` / `CreateForMonitor` (1903, 18362). OBS and robmikh use these in unpackaged apps with no consent step. The WinRT `TryCreateFromWindowId` / `TryCreateFromDisplayId` (20348) require `RequestAccessAsync(Programmatic)` and the `graphicsCaptureProgrammatic` **package** capability. | [CreateForWindow](https://learn.microsoft.com/en-us/windows/win32/api/windows.graphics.capture.interop/nf-windows-graphics-capture-interop-igraphicscaptureiteminterop-createforwindow), [TryCreateFromWindowId](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscaptureitem.trycreatefromwindowid) |
| Yellow border | Drawn around every captured item, one per session. `IsBorderRequired` (20348, contract v12): set it to false after `RequestAccessAsync(Borderless)`, which prompts the user. The docs require the `graphicsCaptureWithoutBorder` package capability. If access is denied, the setter is silently ignored. If another app sets `true` on the same item, the border shows. OBS (`winrt-capture.cpp:316-320`) and robmikh (`App.cpp:321-330`, "Even if the user or system policy denies access, it's still safe to set") request access and then set false regardless. **Unpackaged behaviour is unverified** (also an open item in [ecosystem]). | [IsBorderRequired](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturesession.isborderrequired) |
| Windows 10 2004 vs 11 | 2004 (19041) adds `IsCursorCaptureEnabled`. 20348 (Windows 11 for consumers) adds `IsBorderRequired`, `GraphicsCaptureAccess` and `TryCreateFrom*Id`. 24H2 (26100) adds `IncludeSecondaryWindows` (popups and tool windows, can change during capture). So on Windows 10 the border is always drawn. | MS Learn version tables (RAN) |
| Privacy toggle (*secondary, unverified*) | Windows 11 has Settings → Privacy & security → "Screenshots and apps" (`ms-settings:privacy-graphicscaptureprogrammatic`) and a separate "Screenshot borders" page. Unknown whether turning off programmatic capture affects `CreateForWindow` in unpackaged apps, or whether the picker still works when it is off. | search results only |

### 1c. Linux: xdg-desktop-portal ScreenCast (always a system picker)

| Topic | Fact | Source |
|---|---|---|
| Lifecycle | `CreateSession` → `SelectSources` (**only once per session**) → `Start` (usually a dialog; **only once per session**) → `OpenPipeWireRemote`. Interface v6. | [ScreenCast](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html) (RAN) |
| Source types | 1 MONITOR, 2 WINDOW, 4 VIRTUAL. `multiple` (default false) lets the user pick several; `Start` then returns one PipeWire stream per source. v6: target streams by `pipewire-serial`, not node id. | same |
| Cursor | `cursor_mode` 1 hidden, 2 embedded, 4 metadata (v2+). It must be one the portal advertises, or the session closes. OBS prefers metadata (`screencast-portal.c:361-370`). | same, OBS |
| Persistence | `persist_mode` 0 / 1 (while the app runs) / 2 (until revoked). The restore token is **single-use**; save the new one after every Start. An invalid token falls back silently to the dialog. Permanent grants are stored **under the app id** (`xdp-session-persistence.c:98-170`), and unsandboxed apps may have an empty id (see the risks). | docs; xdg-desktop-portal source (RAN) |
| GNOME (xdg-desktop-portal-gnome) | Monitor, window and virtual sources; multi-select through `GTK_SELECTION_MULTIPLE`. **No region.** "_Remember this selection" checkbox, visible only for persist_mode 2, default on. Restore matches monitors by an id string and windows by app_id plus fuzzy title (`screencast.c:448-470`). Valid restore data skips the dialog. | GNOME source (RAN) |
| KDE (xdg-desktop-portal-kde) | Outputs, **full workspace**, **rectangular region** (KDE's own selection editor; commit 2023-02-13), virtual output, windows. Restore covers outputs, region and windows (appId plus Levenshtein title; region restore commit 2024-11-08). Valid restore data skips the dialog (`screencast.cpp:392-394`). The user must tick "allow restore". | KDE source and commit log (RAN) |
| Changing sources after Start | **Not possible.** A new source needs a new session, and a new session shows the dialog unless a valid restore token is passed. OBS's "Reload" clears the token and starts a new session (`screencast-portal.c:473-493`). | docs, OBS |
| Host-app identity | Unsandboxed apps get their app id from an `app-*.scope` systemd unit name, or from `org.freedesktop.host.portal.Registry.Register` (portal 1.19.4+), which must be the first portal call. Otherwise the id is empty. *Secondary:* registration requires an installed desktop file, and Ubuntu 24.04 ships portal 1.18. | [GNOME blog 2025-06-04](https://blogs.gnome.org/ignapk/2025/06/04/using-portals-with-unsandboxed-apps/), search results |

### 1d. Per-OS comparison

| | macOS `SCContentSharingPicker` | Windows `GraphicsCapturePicker` | Linux portal ScreenCast |
|---|---|---|---|
| Min OS | 14.0 (`includedDisplays` 15.2) | 10 1803; interop 1903 | portal v4+ for tokens; KDE region in Plasma releases after Feb 2023 (exact version not checked) |
| Window / display / app | yes / yes / yes | yes / yes / **no** | yes / yes / no (KDE: also whole workspace) |
| Several sources at once | yes (multiple windows or apps) | no | yes (`multiple`) |
| Region | no (pick display + `sourceRect` or compositor crop) | no (pick display + crop) | KDE yes; GNOME no (pick monitor + crop) |
| Exclude our windows from the list | `excludedWindowIDs`, `excludedBundleIDs` | no | no |
| Re-pick for a running capture | `present(for: stream)` plus menu-bar Video item → `updateContentFilter` | new `PickSingleItemAsync` → new session | new session plus dialog |
| Saved across launches | no (*secondary*) | no (an item is a live object) | restore token (persist_mode 2) |
| Without a picker | `SCShareableContent` (TCC grant, re-prompt) | `CreateForWindow` / `CreateForMonitor` (no consent, unpackaged) | not on Wayland |
| Main gain from the picker | may skip the TCC grant and the monthly alert (test) | none for unpackaged apps today (maybe the privacy toggle, unverified) | none; it is mandatory |

### Recommendation for (1)

- **The owner's condition is met:** each OS has a system picker.
- **Add one cross-platform source kind, "System picker…",** behind a `SourcePicker` trait:
  - **Linux:** the portal is the only path. Use `persist_mode = 2` and store the token. Request `types = MONITOR|WINDOW` and `cursor_mode = metadata` when the portal advertises it. Regions: pick a monitor, then crop (spec), and accept KDE's own region when the user chooses it there.
  - **macOS:** make the picker the **primary** window, display and app path **if** the M1 test below passes in the shipped app. Why it matters more now:
    - The owner has no Apple Development identity, so ad-hoc release builds lose the Screen Recording grant on every update.
    - Picker-consented video may not need that grant at all.

    Other macOS settings:
    - Keep our own chooser (`SCShareableContent`) as a fallback, for users who already granted the permission, and for hotkey quick-starts that must skip any UI after a fresh launch.
    - Regions: pick a display, then use our overlay. Set `maximumStreamCount = 0` so Control Center cannot start streams we did not ask for.
    - Set `excludedWindowIDs` to our own windows.
  - **Windows:** offer the picker next to our own chooser (`CreateForWindow` / `CreateForMonitor`), which supports thumbnails, app capture, regions and hotkeys. Make the picker the primary path only if Windows 11 privacy settings ever restrict programmatic capture for unpackaged apps.
- **System audio caveat (macOS):** if picker-only capture gives silent SCK system audio, "All system audio" needs the Screen Recording grant anyway, or a Core Audio process tap, which has its own TCC prompt. Then the picker helps only when system audio is Off. The ADR-028 test must check audio.

## 2. Switching source while recording

### 2a. Per-OS mechanics

| | macOS (SCK) | Windows (WGC) | Linux (portal + PipeWire) |
|---|---|---|---|
| Retarget a live capture | `SCStream.updateContentFilter` (12.3+). Apple's sample applies new filter and config objects to the running stream when the user switches display ↔ window ("don't require restarting it"). WWDC23 picker code does the same. | **Not possible.** A session is created from one item (`framePool.CreateCaptureSession(item)`). `Recreate` changes only device, format, buffer count or size. robmikh: switch = new `SimpleCapture` (pool plus session). | **Not possible.** `SelectSources` and `Start` run once per session. |
| New capture alongside the old | Yes: concurrent streams are supported (WWDC22 runs one stream per window for thumbnails). | Yes: one border per item, so two show briefly. | Yes per spec (sessions are independent). Not verified on GNOME or KDE. |
| Region change on the same display | `updateConfiguration(sourceRect)` (output size changes), or capture the full display and crop in the compositor (no OS call). | crop change only | crop change only |
| Frame size after a switch | The output buffer stays at the configured `width`/`height`; the content is scaled to fit (`preservesAspectRatio`, default true, 14.0). `SCStreamFrameInfo.contentRect`, `contentScale` and `scaleFactor` say where the content is (WWDC22 13:34). Changing dimensions often "can lead to additional memory allocation" (WWDC22). OBS calls `updateConfiguration` when a window's size changes (`mac-sck-common.m:289-320`). | New pool at the new item's `Size`. Content larger than the buffer is clipped; smaller leaves undefined pixels, so copy `ContentSize`. | PipeWire renegotiates the format per stream. |
| Audio | **SCK audio follows the filter at app level** (WWDC22). A window filter gives that whole app's audio, so retargeting the stream that also carries "All" audio would narrow it. | Separate WASAPI loopback / process loopback, so not affected. | Separate PipeWire sink-monitor stream, so not affected. |
| Gap | not measured | not measured | user dialog time plus first frame; old stream keeps recording meanwhile; not measured |
| What OBS does | Source change = `destroy_screen_stream` + `init_screen_stream` (`mac-sck-video-capture.m:405-461`), not `updateContentFilter`. OBS's compositor hides the gap. | new item, new pool | "Reload" = new session plus dialog |

### 2b. What "switch source" with the system picker looks like

- **macOS:** the floating bar's "Switch source" button calls `present(for: currentVideoStream)`, optionally `using:` a style.
  - The pick arrives in `didUpdateWith(filter, for: stream)`.
  - We either `updateContentFilter`, or start a second stream from that filter and stop the old one after its first frame.
  - The user can also switch from the menu-bar Video item. That path is the same callback, so we must handle it at any time; or set `allowsChangingSelectedContent = false` if we want switching only from our UI.
  - Regions: pick a display, then our overlay.
- **Windows:** the same button calls `PickSingleItemAsync()` again (owner HWND = floating bar).
  - It returns a new item, which gets a new pool and session.
  - On cancel (null), keep the current source.
- **Linux:** the same button opens a new portal session, and the system dialog appears.
  - To avoid dialogs: (a) offer "Pick several windows" at the start (`multiple: true`) and switch between those granted streams with no dialog. Inactive streams can be paused with `pw_stream_set_active(false)`; their cost is not measured.
  - (b) Keep a restore token per previously picked source (each restore returns a fresh token). Switching back to a known window or monitor then skips the dialog where the backend restores it.
  - (c) Region changes on a captured monitor are crops.

### Recommendation for (2)

1. **Source spans.** Each switch closes the current span and opens a new one:
   - New screen file `screen-NNN.mov`, its own `AVAssetWriter` / MF / FFmpeg encoder at the new source's native size.
   - `project.json` stores each span's host-time start and end, a source descriptor and the content size.
   - Mic, system audio and camera writers never restart.
   - The compositor fits every span into the fixed output frame (Original uses the first span's size, or the largest; to decide), with the chosen background in the letterbox area. That is exactly the new aspect-ratio requirement.
   - This keeps "recorded files are never altered" and avoids scaling mixed-size sources into one buffer.
   - It changes ADR-008's "one screen file" assumption and needs project-schema, editor and crash-recovery work.
   - Alternative: one fixed-size screen file with content scaled in by SCK, or by a GPU blit on Windows/Linux. Simpler files, but it loses resolution and needs `contentRect` metadata per frame. Not recommended.
2. **Make-before-break.**
   - Start the new capture and its writer, wait for the first frame, mark the span boundary at that frame's timestamp, then stop the old capture.
   - Until then the compositor holds the last old frame, so the preview never goes black.
   - On macOS, also prototype plain `updateContentFilter` and keep it if the measured gap is small, because it needs no second stream.
3. **Regions** on the display already being captured are crop changes:
   - Same span if we record the full display and crop in the compositor.
   - New span if we use `sourceRect` (macOS) or a cropped encode. The full-display-plus-crop option is the most uniform across OSes, at the cost of larger raw files.
4. **macOS system audio:**
   - "All": a separate audio stream with a constant display filter (excluding our app), so a video filter change cannot narrow it.
   - "Only the captured app": audio deliberately follows the source.
5. **Keeping our UI out of the recording on every switch:** rebuild the macOS filter with our window exceptions. Windows `WDA_EXCLUDEFROMCAPTURE` is per window and survives switches. Linux auto-hides overlays.

## 3. Hardware tests needed

- **macOS, shipped `com.simplecapture.desktop`** (ad-hoc signed, never granted; needs the owner's approval to run). The dev host already holds the grant and cannot answer these.
  - Picker video works with no TCC prompt and no bypass alert.
  - System audio on the picker path: silent or not.
  - Behaviour after a rebuild.
  - Whether a filter rebuilt from `includedDisplays` plus `currentProcess` windows keeps consent.
  - Log every Screen Recording prompt for a month (ADR-028).
- **macOS, dev host:**
  - `present(for:)` on a running stream.
  - Time from the last old frame to the first new frame: `updateContentFilter` versus a second stream.
  - Output and `contentRect` behaviour across window → display → window.
  - `excludedWindowIDs` effect on a picked display.
  - A separate audio stream stays complete while the video filter changes.
- **Windows 10 2004 and Windows 11 24H2** (no machine available):
  - Picker from a Tauri HWND.
  - Switch gap with a new pool and session.
  - `IsBorderRequired(false)` in an unpackaged build.
  - Behaviour with the privacy toggle off.
  - `WDA_EXCLUDEFROMCAPTURE` on a picker-chosen display.
- **Linux, GNOME 48+ and Plasma 6.x on Wayland** (real sessions; Docker cannot do this):
  - `multiple` switching.
  - persist_mode 2 restore for an AppImage/deb build, with and without `Registry.Register`.
  - KDE region and workspace streams.
  - A concurrent second session while recording.
  - Time from the end of the dialog to the first frame.

## Sources

- Apple:
  - [SCContentSharingPicker](https://developer.apple.com/documentation/screencapturekit/sccontentsharingpicker) and its member pages
  - local SDK headers `MacOSX26.5.sdk` / `MacOSX27.2.sdk` ScreenCaptureKit
  - [Capturing screen content in macOS](https://developer.apple.com/documentation/screencapturekit/capturing-screen-content-in-macos)
  - [WWDC22 10155](https://developer.apple.com/videos/play/wwdc2022/10155/), [WWDC23 10136](https://developer.apple.com/videos/play/wwdc2023/10136/)
  - forums [849969](https://developer.apple.com/forums/thread/849969), [765103](https://developer.apple.com/forums/thread/765103)
- Microsoft:
  - [Screen capture](https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture)
  - [GraphicsCapturePicker](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturepicker)
  - [IsBorderRequired](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturesession.isborderrequired)
  - [GraphicsCaptureAccessKind](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscaptureaccesskind)
  - [CreateForWindow](https://learn.microsoft.com/en-us/windows/win32/api/windows.graphics.capture.interop/nf-windows-graphics-capture-interop-igraphicscaptureiteminterop-createforwindow)
  - [Display WinRT UI objects](https://learn.microsoft.com/en-us/windows/apps/develop/ui-input/display-ui-objects)
  - [robmikh/Win32CaptureSample](https://github.com/robmikh/Win32CaptureSample) (MIT)
- Portal:
  - [ScreenCast v6](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html)
  - xdg-desktop-portal `desktop-portal/xdp-session-persistence.c`
  - [xdg-desktop-portal-kde](https://invent.kde.org/plasma/xdg-desktop-portal-kde) `src/screencast.cpp`, `src/screenchooserdialog.cpp`
  - [xdg-desktop-portal-gnome](https://gitlab.gnome.org/GNOME/xdg-desktop-portal-gnome) `src/screencast.c`, `src/screencastwidget.{c,ui}`
  - [GNOME blog on unsandboxed apps](https://blogs.gnome.org/ignapk/2025/06/04/using-portals-with-unsandboxed-apps/)
- OBS Studio master (GPL, study only): `plugins/mac-capture/mac-sck-*.m`, `libobs-winrt/winrt-capture.cpp`, `plugins/linux-pipewire/screencast-portal.c`.
- Secondary: [openscreen PR #737](https://github.com/getopenscreen/openscreen/pull/737), [Nonstrict 2023](https://nonstrict.eu/blog/2023/a-look-at-screencapturekit-on-macos-sonoma/), [addpipe 2025](https://blog.addpipe.com/screen-sharing-got-smarter-and-more-private-on-macos-understanding-the-system-private-window-picker/).

---

## Independent verification (2026-11-19)

An adversarial verifier re-read the sources and re-ran the builds and measurements. Verdict: **partially confirmed**. Where this section and the text above disagree, this section wins.

### Discrepancies found

- Windows version label is wrong. MS Learn lists IsBorderRequired, GraphicsCaptureAccessKind and TryCreateFromWindowId as 'Windows 10, version 2104 (introduced in 10.0.20348.0)', UniversalApiContract v12. The note calls 20348 'Windows 11 for consumers', but 20348 is the Server 2022 base build, and consumer Windows 11 starts at 22000. The practical conclusion still holds: consumer Windows 10 (19041-19045) lacks these APIs, so the border cannot be turned off there.
- sourceRect for windows: the sources disagree. The Apple doc JSON (scstreamconfiguration/sourcerect) says the system 'doesn't reference this value when you capture a single window'. The local header (MacOSX26.5.sdk SCStream.h, around line 266) says 'For independent window streams, if not set, then the entire window is streamed', which implies it does apply. The note should flag the conflict and leave it to a hardware test instead of stating it as fact.
- SCShareableContent.currentProcess is called 'new in the header' (27.2), but the same declaration (API_AVAILABLE macos 14.4) is already in the CLT MacOSX26.5.sdk SCShareableContent.h:149-153. The header text says 'redacted information about windows, displays and applications that are available to capture by current process without user consent via TCC'. Reading that as 'our own windows' is an inference.
- Apple forums thread 849969 (the DTS quote 'two documented ways for the person to give consent', dated Oct 2026) could not be verified. developer.apple.com/forums returned a bot 'Security Verification' page, which I did not bypass. Thread 765103 (DTS naming the picker as the way to avoid the alert) is the one already cited in docs/research/ecosystem.md:178-179.
- 'Portal 1.19.4+' is attributed to the GNOME blog, but the blog gives no version. The version is correct per xdg-desktop-portal NEWS.md: 'Changes in 1.19.4', released 2025-02-15, 'Introduce the host app registry'. 'Ubuntu 24.04 ships 1.18' and 'registration requires an installed desktop file' remain secondary and unverified.
- KDE 'the user must tick allow restore' was not verified. screencast.cpp only shows an allowRestore flag being passed through. Also unmentioned: a 2026-06-25 KDE commit, 'Do not check persist mode before trying to restore'.
- Omission: windows-capture 2.0.1 (cached locally) already wraps GraphicsCapturePicker through IInitializeWithWindow on a hidden HWND (src/graphics_capture_picker.rs:135-137). It also sets SetIsBorderRequired(false) without calling RequestAccessAsync (src/graphics_capture_api.rs:323-331), which differs from what OBS and robmikh do.
- Minor wording: the Apple sample text says changes made 'after you update the capture configuration in the user interface' don't require a restart. That the change is a display/window captureType switch comes from the sample's code, not the article text.

### Corrected summary

Desk research only. No capture code was written or run, and no latency, gap or cost figure was measured on any OS.

(1) System pickers exist on all three OSes, so the owner's condition for ADR-028 is met.

- **macOS: SCContentSharingPicker (14.0+).**
  - Modes: single window, multiple windows, single app, multiple apps, single display. There is no region mode.
  - Config: excludedWindowIDs and excludedBundleIDs. allowsChangingSelectedContent defaults to YES. maximumStreamCount = 0 stops Control Center offering a picker with no stream. allowedPickerModes cannot be reset to "all" once changed.
  - presentPickerForStream(:usingContentStyle:) re-picks for a running stream. The result arrives in didUpdateWithFilter:forStream:, and Apple's WWDC23 sample then calls updateContentFilter.
  - Users can also re-pick from the Video menu bar item. The 27.2 SDK says `active` gates the picker, the video menu bar and sharing "directly from a window".
  - A picked filter exposes contentRect and pointPixelScale (14.0) and includedDisplays/Windows/Applications (15.2).
  - Region: pick a display, then crop with our own overlay, using sourceRect or a compositor crop. Whether sourceRect applies to single-window capture is contradictory: the docs say it is ignored, the 26.5 header implies it applies. Test it.
  - SCShareableContent still works alongside the picker but needs the TCC grant. SCShareableContent.currentProcess (14.4, present in both SDKs) returns redacted content "available to capture by current process without user consent via TCC".
  - Secondary source (openscreen PR #737, merged 2026-09-24, signed test apps on macOS 26.5 with no grant): picker video worked at full resolution with no grant and no bypass alert. System audio was all zeros without the grant. excludedWindowIDs was needed to keep the app's own window out of a display pick. Picks do not persist across launches.
  - DTS named the picker as the way to avoid the bypass alert on macOS 15 (forums 765103, via ecosystem.md). The newer DTS quote (forums 849969) could not be verified because of a bot check.
- **Windows: GraphicsCapturePicker (1803+).**
  - API: a constructor plus PickSingleItemAsync only. Call it on the UI thread. Desktop apps set the owner HWND through IInitializeWithWindow (robmikh sample, MIT). windows-capture 2.0.1 already wraps this.
  - It picks one display or one application window: no app, no region, no multi-select, no exclusion list.
  - CreateForWindow and CreateForMonitor (1903) need no picker. TryCreateFrom*Id (20348) needs Programmatic access and a package capability.
  - Yellow border: drawn per captured item. IsBorderRequired (10.0.20348, labelled "Windows 10 version 2104"; absent on consumer Windows 10 19041-19045, present on Windows 11) needs RequestAccessAsync(Borderless) and the graphicsCaptureWithoutBorder package capability, and a denied request is silently ignored. OBS and robmikh request access and then set false. windows-capture sets false without requesting. Whether this works for unpackaged apps is unverified.
  - Other version gates: IsCursorCaptureEnabled 2004 (19041). IncludeSecondaryWindows 24H2 (26100), changeable during capture.
- **Linux: the xdg-desktop-portal ScreenCast picker (v6).** It is the only path on Wayland.
  - SelectSources and Start may each be called once per session.
  - Options: types 1/2/4 (monitor, window, virtual); multiple (default no); cursor_mode 1/2/4, which must be advertised or the session closes; persist_mode 0/1/2. Restore tokens are single-use, and an invalid one falls back to the dialog.
  - Persistent grants are keyed by app id. Unsandboxed apps get the id from an app-* systemd scope or from Registry.Register, which must be the first portal call (added in xdg-desktop-portal 1.19.4). Otherwise the id is empty.
  - KDE: region and workspace streams; restore of outputs, region and windows (appId plus Levenshtein title); valid restore data skips the dialog.
  - GNOME: monitor, window and virtual sources; multi-select; no region; "Remember this selection" is shown only for persist_mode 2 and checked by default; window restore by app_id plus fuzzy title; valid restore data skips the dialog.

Recommendation (1) is unchanged. Add a cross-platform "System picker" source:
- Linux: mandatory. Use persist_mode 2 and store the token.
- macOS: the primary path only if a test in the shipped, never-granted app confirms video works with no grant, no alert and no reset after rebuilds. System audio likely still needs the grant or a Core Audio process tap.
- Windows: optional, next to CreateForWindow/CreateForMonitor.
- Region everywhere: pick a display or monitor, then crop. KDE can also supply its own region.

(2) Switching source mid-recording is feasible everywhere.
- **macOS:** updateContentFilter and updateConfiguration on a live stream (Apple sample and WWDC23). Output dimensions are "mostly fixed", and changing them often costs allocations (WWDC22). SCK audio follows the filter at app level, so "All" system audio needs a separate capture whose filter never changes. OBS instead destroys and recreates the stream.
- **Windows:** one item per session, so a switch means a new frame pool and session (robmikh). Recreate only changes device, format, buffer count or size. Each session draws its own border.
- **Linux:** a new source needs a new portal session and dialog, unless it was picked up front with multiple, is restorable with a token, or is a crop of a monitor already being captured.
- Concurrent captures are supported on macOS (WWDC22) and Windows (border per session). They are unverified for portal sessions on GNOME and KDE.
- Proposed design (not tested): per-span screen files fitted into the fixed output aspect frame. Make-before-break switching. Audio and camera never restart. This changes the single-screen-file assumption in ADR-008.

Hardware tests are still needed for every timing and consent claim: macOS in the shipped app (no grant) and under the dev host; Windows 10 and 11 (no machine available); real GNOME and KDE Wayland sessions.
