# Decisions

Architecture decision records for Simple Capture. docs/SPEC.md stays the source of truth. When a
decision here changes the spec, the spec is updated only after the owner says OK.

**Status values**
- **Accepted**: the owner or the spec already decided this, and M0 evidence supports it.
- **Proposed — needs owner OK**: new, or changed from the spec's defaults. Not final until the owner
  approves it.
- **Conditional**: the owner approved it only if a stated condition holds; the entry says whether it does.
- **Superseded**: replaced by a later entry.

**Source keys** used below
- Research notes: [cap-recording], [cap-render-decode], [cap-editor-export], [cap-mit-crates],
  [kap], [ecosystem] = `docs/research/<key>.md`.
- Spikes: [S1] `spikes/01-helpers/`, [S2] `spikes/02-preview-transport/`,
  [S3] `spikes/03-macos-pipeline/`, [S4] `spikes/04-skia-text/`. Each has a `REPORT.md`
  (saved by the orchestrator from the consistency pass's drafts). The verifiers' own corrected
  numbers were never saved, so every spike number here was re-read from these saved files in the
  2026-11-19 consistency pass:
  - [S1] the three verification runs `results/run-20261119T214043Z`, `...T214312Z` and `...T214545Z`
    (the last is mirrored in `results/latest.md`); figures are their medians. All saved runs use a
    10 s clip; 30 s figures and the VideoToolbox size probe were re-measured in the consistency pass
    (outputs not in the repo; numbers quoted in CODEC_MATRIX §2.2).
  - [S2] `results/verify-r1..r3` and `results/verify-con1..2`.
  - [S3] `out/bench-quiet`, `out/bench-quiet2` and `out/bench-quiet3` (`summary.json`, per-recording
    `analysis.json`). The recordings in `out/` are gitignored; the summaries, analyses, `project.json`
    files and run logs are not (STATUS §1).
  - [S4] `out/bench-quiet.md` (one run) and `out/*.png`, plus three `--no-png` re-runs in the
    consistency pass (numbers quoted in ADR-021).
- M1 (2026-11-19/20): **[M1:<package>]** is the report that package returned to the orchestrator
  (for example [M1:soak] = wp-soak-test, [M1:e2e] = wp-e2e-cli). The reports themselves are not in
  the repo; **docs/STATUS.md §2 is the in-repo record of every M1 number**, each with its source.
  Local result files named there (`target/soak/…`, `target/e2e/…`, `target/app-driver/…`) are
  gitignored and exist only on the build Mac; every recording was deleted after its numbers were
  taken. GitHub CI runs are cited by run id.

**Machine caveat for every number**: measured on an Apple M3 Pro (5P + 6E cores, 18 GB), macOS 26.7.2.
The target is a base M1 with 8 GB, and **nothing was measured on an M1**. The "constrained" proxies
(4 threads, E-cores only, CPU hogs) turned out to be noisy or not constraining (see each spike), so
they are not a substitute for an M1 run.

---

## ADR-001: App identifier `com.simplecapture.desktop`
- **Status:** Accepted (owner decided, 2026-11-19).
- **Context:** The spec asked for a reverse-DNS identifier "like com.simplecapture.desktop" and said
  not to end it in `.app`. Tauri warns about identifiers ending in `.app`, because that suffix
  clashes with the macOS bundle extension. macOS keys privacy grants (TCC) to the identifier and the
  signing identity.
- **Decision:** The shipped app uses `com.simplecapture.desktop` on every OS. It is used for the
  Tauri `identifier`, the macOS `CFBundleIdentifier`, the Linux single-instance D-Bus name and app
  data paths.
- **Alternatives:** `com.simplecapture.app` (rejected: ends in `.app`). A personal-domain prefix
  (rejected: the owner chose this one).
- **Consequences:** Changing it later resets every macOS permission grant for existing users and
  moves their settings folder, so treat it as permanent. To clear a stuck Screen Recording grant,
  run `tccutil reset ScreenCapture com.simplecapture.desktop` (documentation only; agents never run
  it).

## ADR-002: One permanent dev host app for macOS permissions; no per-spike app names
- **Status:** Accepted (owner decided 2026-11-19, spec Platform notes; re-confirmed by the owner:
  "stop giving the capture spike apps new names"). This **supersedes** the earlier
  `com.simplecapture.spike.<name>` convention: no new spike, test or bench identifiers.
- **Context:** Every new bundle ID, app name or re-signed bundle makes macOS ask for Screen
  Recording, Camera and Microphone again, and the owner has to approve each prompt by hand.
  Spike 3 originally built its own identity (`com.simplecapture.spike.macos-pipeline`, plus a
  separate camera `.app`), which caused repeated approvals [S3].
- **Decision:** Every dev, spike, test or bench binary that needs a privacy permission runs as a
  child of `~/Applications/Simple Capture Dev.app` (`com.simplecapture.dev`) through
  `tools/macos-dev-host/run.sh <binary> [args]`. That host is signed once and never rebuilt.
  No other `.app`, Info.plist or bundle ID is ever created to obtain a permission. The rules are in
  `CLAUDE.md`.
- **Evidence:**
  - [S3] after the change: three rebuilds of the spike binary, 0 new prompts. All 39 `project.json`
    files made after the change (`out/bench-*`) record `permission_host = com.simplecapture.dev`, and the binary has no embedded
    Info.plist.
  - [S2] keeps the config strings `com.simplecapture.spike.preview` / "Preview Transport Spike"
    from before the rule, unchanged. Its `bench.sh` release build (default feature
    `custom-protocol`) builds no bundle (`bundle.active = false`), embeds no Info.plist and requests
    no permission, so it never prompts. It was deliberately **not** renamed again. Caveats:
    `pnpm tauri dev` disables `custom-protocol`, and tauri-codegen then embeds an Info.plist with
    CFBundleName "Preview Transport Spike", a new app name, so spike 2 must never be run that way;
    so its `"tauri"` package script now refuses to run. Its window snapshot is off by default,
    and `SC_SNAPSHOT=1` runs it through the dev host (STATUS §1).
  - The M0 workflow's task text still mentioned the old convention; it is retired by this ADR.
- **Consequences:**
  - Running `install.sh --force` resets every grant; only the owner may ask for that.
  - Under the host, whole-app capture exclusion does not work (ADR-010).
  - The host may need a one-time "Documents folder" grant when a binary runs from inside
    `~/Documents` (see STATUS risks).
  - Grants held by the dev host do not carry over to the shipped app (ADR-003).

## ADR-003: Sign the shipped app with one stable Apple Development identity
- **Status:** Accepted (spec Platform notes), **but blocked**: on 2026-11-19 the owner said they won't create an Apple Development identity for now ("I may in the future, but no promises"). See "Update 2026-11-19" at the end of this ADR.
- **Context:** An ad-hoc signature ties TCC grants to the binary's hash, so every rebuild counts as a
  new app. A real signing identity ties grants to the identifier plus team, so they survive
  rebuilds [S3]. No signing identity exists on this machine yet.
- **Decision:** From M1 on, sign every `com.simplecapture.desktop` build, and each helper inside it,
  with one Apple Development identity from a free personal team. Use the hardened runtime with the
  camera and audio-input entitlements.
- **Consequences:**
  - The owner creates the identity in Xcode (see STATUS).
  - tauri-bundler signs sidecars with the app's identity and entitlements; there is no per-sidecar
    entitlements option [ecosystem]. Narrower helper entitlements would need a custom signing step.
    We accept this for v1; a custom per-helper signing step is optional later work, not a v1
    requirement (LICENSES §5 is aligned with this).
  - On macOS 15+, apps that capture without `SCContentSharingPicker` get a "bypass the private
    window picker" alert and a re-prompt about once a month. We researched this but have not
    observed it [ecosystem]. Onboarding must explain it. Whether to use the picker is decided in ADR-028
    (conditional).

- **Update 2026-11-19 (no signing identity):**
  - Development: run the real app's debug and release binaries as children of the Simple Capture
    Dev host (`tools/macos-dev-host/run.sh`), exactly like the spikes, so its grants apply and
    rebuilds never re-prompt. A `just run` target does this.
  - Release bundles stay ad-hoc signed. TCC keys an ad-hoc app on its code hash, so after every
    update the user approves Screen Recording, Camera and Microphone again. Onboarding explains
    this and links to System Settings.
  - Option for the owner (not done; it creates a signing identity, so it needs their go-ahead): a
    **self-signed code-signing certificate** made in Keychain Access gives a stable designated
    requirement without an Apple account, so the owner's own builds would keep their grants
    across updates. It does not satisfy Gatekeeper for other people's Macs.
  - The ADR-028 picker test and the whole-app exclusion re-test (ADR-010) assumed a signed app;
    they run under the dev host instead, which cannot show signed-app behaviour.

## ADR-004: Keep Tauri v2; do not switch to GPUI
- **Status:** Accepted (the spec default, confirmed by [S2]).
- **Context:** The spec says to switch to GPUI if the preview spike misses ≥30 fps at 1080p.
- **Evidence [S2]:**
  - Every transport delivered 58.5–60.1 fps out of the 60 produced (target 30), including full
    1920×1080 frames through the webview.
  - The native-surface path ran at 60.1 fps using about 20% of one core in total.
  - A/B latency from producer to draw: p50 10–12 ms, p95 16–18 ms.
- **Decision:** Pin Tauri 2.12.x and its plugins on `^2` / `~2.N`, never the 3.0 alphas
  [ecosystem]. Keep the renderer UI-agnostic on its own wgpu device so presentation is the only part
  tied to Tauri [cap-render-decode].
- **Alternatives:** GPUI, as Cap did. Rejected for now: it has no stable crates.io release since
  0.2.2 (October 2025), Cap maintains a fork with about 12.7k lines of patches, and the first build
  takes 11 min / 6.4 GB [cap-render-decode, ecosystem].
- **Consequences:** Revisit only if the base-M1 run or the Linux WebKitGTK run fails (both untested).

## ADR-005: UI framework is Svelte 5 + TypeScript + Vite
- **Status:** Accepted (owner 2026-11-19: "Svelte 5. Sure.").
- **Evidence:** [S2] built its UI on Svelte 5.58, Vite 8 and TypeScript 6 with
  `@tauri-apps/api` 2.12 and had no framework-related problems. No React comparison was made.
- **Decision:** Use Svelte 5 (runes) for `ui/`, managed with pnpm.
- **Alternatives:** React. Equally viable. Svelte has a smaller runtime and less boilerplate, and the
  spike already proved it with Tauri.
- **Consequences:** Check npm licenses with `pnpm licenses list --prod --json` plus an allowlist
  script [ecosystem].

## ADR-006: macOS bindings use the objc2 family
- **Status:** Accepted (owner approved M0 on 2026-11-19 without objecting to this proposal).
- **Context:** The candidates were objc2-*, cidre (which Cap uses through a git fork), the
  `screencapturekit` crate, and vendoring Cap's MIT macOS crates.
- **Evidence:**
  - [S3] built screen, camera, mic and system-audio capture, AVAssetWriter, AVAssetReader and Metal
    interop on objc2 0.6 / objc2-* 0.3.2.
  - Tauri 2.12.7 uses the same versions [ecosystem].
  - wgpu-hal 30 takes objc2-metal texture types directly, so decoded frames pass into wgpu with no
    glue types [S3].
  - cidre's build script runs `xcodebuild` and fails on this machine without a first-launched Xcode
    [cap-mit-crates]. `screencapturekit` compiles a Swift bridge through `xcrun` [ecosystem].
- **Decision:** Use objc2 0.6 and objc2-* 0.3.x, at the versions Tauri uses, for ScreenCaptureKit,
  AVFoundation, CoreMedia, CoreVideo, IOSurface, Metal and AppKit. Write our own wrappers. Cap's
  `scap-screencapturekit` serves as a design reference only (ADR-022).
- **Consequences:**
  - Do not upgrade objc2 ahead of Tauri, or the crates get duplicated. The objc2 main branch has
    unreleased breaking changes.
  - objc2-app-kit's default features link CloudKit and CoreData into the binary [S3]. M1 should
    trim those features. No network code is called, but the shipped binary should not link them.

## ADR-007: Preview transport per OS
- **Status:** **Accepted for macOS** (owner 2026-11-20, choosing "Try native layer" to close the M2 Preview's CPU gap). Windows and Linux stay on webview frames until their native paths are proven on real hardware (owner 2026-11-19: "Yeah, but only if you can figure out ways to do this in Windows and Linux"). See the 2026-11-19 and 2026-11-20 updates below.
- **Evidence [S2]**, quiet medians:

  | path | fps (of 60) | total CPU (% of one core) | WebContent memory |
  |---|---|---|---|
  | C, native CAMetalLayer under a transparent webview, 1080 | 60.1 | 20% | 39 MB |
  | B, URI-protocol pull + WebGL2, 960×540 device px | 59.5 | 38% | — |
  | B, 1920×1080 | 58.8 | 47% | 342 MB |
  | A, push Channel + WebGL2, 1920×1080 | 59.1 | 57% | 346 MB |

  - HTML, even with `backdrop-filter`, composites over the Metal layer (checked by window snapshot).
  - 2D `putImageData` costs 1.5–1.7× more WebKit GPU-process CPU than WebGL2.
- **Decision:**
  - **macOS:** render the preview into a CAMetalLayer-backed NSView placed *under* a
    transparent-background WKWebView. The UI reports the preview rectangle and Rust keeps the view's
    frame in sync with it.
  - **Fallback on every OS:** "webview frames", meaning frames at the preview's on-screen
    *device-pixel* size, pulled through a custom URI protocol (latest frame wins) into WebGL2
    `texSubImage2D`.
  - Both sit behind one `PreviewSink` trait.
  - **Windows and Linux** start with webview frames. A native surface under HTML would need WebView2
    composition hosting (not exposed by wry) or a GtkOverlay.
- **Alternatives:** The push Channel (A). It works, but costs more at 1080, and its backpressure
  accounting needs care: a bug in it stalled the producer during the spike.
- **Consequences:**
  - Relies on wry setting the private `drawsBackground = NO` key when a background colour is set
    (wry 0.57.3, `src/wkwebview/mod.rs:413-416`).
  - Keeping the view in sync on resize, scroll and zoom is untested.
  - Photon (glass) latency was not measured for any path.
  - Reword Pitfall 14 from "never send full-resolution frames" to "send at the preview's
    device-pixel size": on a Retina display, a preview about 960 pt wide *is* 1920×1080 device
    pixels.
  - WebKitGTK must be benchmarked on real Linux hardware by the end of M1 (STATUS).
- **Update 2026-11-19 (M1 pre-work, spikes 5 and 6):**
  - **Windows** ([spike 5](../spikes/05-native-preview-windows/REPORT.md)): plausible, unproven.
    Three layouts (a non-topmost DirectComposition visual, a child HWND below, or the parent
    swapchain as in wry's `examples/wgpu.rs`) compile for `x86_64-pc-windows-msvc`, and a built-in
    pixel probe decides between them, but nothing has been composited on Windows. Microsoft's docs
    are ambiguous about whether the webview's child window clips the visual.
  - **Linux** ([spike 6](../spikes/06-native-preview-linux/REPORT.md), Docker, software rendering):
    Wayland `wl_subsurface` under a transparent window **passes**; X11 native child window
    **fails** (Xvfb, no compositor); a GtkOverlay + GtkGLArea under the webview passes on both, but
    only with WebKit's DMA-BUF renderer, with a CPU copy per frame, and the HTML vanishes when the
    common NVIDIA workaround variables are set. Webview frames (pull mode) rendered everywhere.
  - **Decision:** the owner's condition is not met yet, so native preview is **off on every OS**,
    and the editor (M3) ships on webview frames, which spike 2 measured at 58.8–60 fps on macOS.
    `PreviewSink` keeps a native implementation slot per OS. Native paths are switched on only
    after hardware runs (Windows 10/11 for spike 5's probe; GNOME/KDE Wayland and Xorg for spike 6)
    show Windows and at least Wayland working; X11 would then stay on webview frames.
  - Cost of the decision on macOS: about 47% vs 20% of one core at 1080 in spike 2.
- **Update 2026-11-20 (macOS native layer on, M2 Preview window):**
  - The owner lifted the condition for macOS. The Preview window draws into a `CAMetalLayer` in an
    NSView just below its WKWebView (`sc_capture_macos::preview`). The view covers the window's
    content layout rectangle (the page's viewport, which stops under the title bar), so the
    page's camera handles line up with the drawn camera; it resizes with the window.
  - The stage renderer draws at the display's pixel size with `Compositor::present` (the same
    compositor passes, then a blit into the drawable; no readback, BT.709 values passed through as
    sRGB, layer colour space sRGB to match WebKit). The page learns only the frame size, through a
    pixel-less `sc://live/stage` frame (format 2), and turns its background transparent.
  - The window gets `backgroundColor` in `tauri.conf.json`, which makes wry clear
    `drawsBackground`. `SC_PREVIEW=webview`, or any failure to attach the layer, falls back to
    webview frames.
  - Measured (release, FaceTime HD at about 25 fps, same driver method as ADR-042): **about 18% of
    one core** for the app and its WebKit processes (18.1–18.4% in three runs), down from 29.5%;
    WebKit's share fell from 9.0% to 0.5%. Camera off: 4.3%. The rest is macOS's camera pipeline,
    Metal's own threads and wgpu's per-frame work; the plan's 15% is still missed by about 3 points.
  - Resizing: the window's `Resized`/`ScaleFactorChanged` events re-fit the view to the content
    layout rectangle (so full screen, with no title bar, is covered too) and update the layer's
    scale; the renderer redraws at the new size on its next wake (at most 30 ms later, or the next
    camera frame). Flow 12 resizes the window and checks that the drawable equals the page's
    viewport in pixels and that the page's camera outline is where the camera was drawn.
  - Later the same day: the renderer waits for the next camera frame instead of polling every
    8 ms, and snapshots reuse their capture filter for 3 s instead of enumerating every window and
    app each second; measured 16.5–17.8% (within the run-to-run noise of the figure above).
  - Not measured: photon latency.
  - Windows and Linux: unchanged, webview frames.

## ADR-008: Raw tracks are fragmented QuickTime files from AVAssetWriter
- **Status:** Accepted (within the spec: "fragmented MP4/MOV", HEVC screen, ~1 s keyframes; verified by [S3]). Amended 2026-11-19 by ADR-034: the screen track is a list of segment files, one per source.
- **Context:** Cap's macOS non-fragmented path loses the whole file on a crash, and Cap turns
  fragmentation off whenever the camera is active [cap-recording]. The spec requires that a
  recording is never lost.
- **Decision:**
  - One AVAssetWriter per source, writing `.mov` (`AVFileTypeQuickTimeMovie`) with
    `movieFragmentInterval = 1 s`, in every mode, including screen + camera.
  - Screen: VideoToolbox HEVC (`hvc1`), 420v input, tagged BT.709 limited range, keyframe interval
    1 s set with both the frame-count and the duration keys.
  - Camera: HEVC at the camera's native format, up to 1080p30.
  - Mic and system audio: AAC LC, 48 kHz, each in its own file.
  - No in-process FFmpeg on macOS.
- **Evidence [S3]:**
  - SIGKILL at 6 s: every file stays readable, with at most 0.86 s of media lost (screen 0.08–0.14 s,
    camera about 0.8 s, mic about 0.77 s). ffmpeg reports no decode errors.
  - AVAssetReader reads every packet of the killed `screen.mov` (97, 169 and 114 frames, matching
    ffprobe's packet counts).
  - Writer "not ready" drops (`encoder_not_ready_drops` in `summary.json`): screen, mic and system
    audio 0 in all three quiet runs; camera 0, 0 and **6** (`bench-quiet3`). Capture-side camera
    drops were 0 in all three. So the writer did fall behind once, briefly, on the camera track; the
    bounded queue and drop counter in ARCHITECTURE §4 exist for exactly this, and the M1 soak test
    must report these counters.
- **Alternatives:** Segment files with our own manifest (Cap's DASH approach) or Matroska. Not
  needed: fragmented MOV already recovers.
- **Consequences:**
  - An idle screen delivers no frames, so the gap between keyframes can grow to 2.8 s. That does not
    hurt seeking (there are no frames in between) or crash safety.
  - Fragment interval vs. overhead is still to tune. The M0 wording "at 1 s, loss stays under 1 s"
    is corrected by the M1 result below.
- **M1 result (2026-11-20):**
  - Live kill tests under the dev host [M1:kill-recovery-test]: 12 SIGKILLs (a plain 6 s
    recording with camera, mic and system audio, and kills after `switch_begin`,
    `segment_prepare` and `segment_open`, 3 runs each). All 12 recover and export after the
    empty-file rule (ADR-038). Loss per track with `fragment_interval_s = 1.0`: 14 of the 21
    per-track measurements in the passing run were 1.003–1.056 s; the first attempt's worst was
    1.076 s. A kill just before a fragment boundary loses the whole unwritten fragment plus encoder
    latency, so [S3]'s 0.86 s was a favourable phase, not a bound. The live tool fails above
    fragment interval + 250 ms and warns above 1 s; the synthetic CI test (`kill_synth`) stays
    strict at ≤ 1 s because `.sctrk` writes are unbuffered.
  - The SIGKILLed-writer unit test lost 0.53 s (screen) and 0.57 s (mic) [M1:mac-av].
  - A writer killed between `segment_open` and its first append leaves a 0-byte file; recovery now
    drops it as empty instead of marking the project damaged (ADR-038, recovery).
  - Writer not-ready counts in the 30-minute soak: 0 on every track [M1:soak].
  - **Owner question (STATUS §5):** shorten `fragment_interval_s` (about 0.5 s should meet ≤ 1 s;
    the file-overhead cost is not measured) or restate the target as "≤ fragment interval +
    encoder latency".

## ADR-009: Frame-rate and resolution policy for recording
- **Status:** Accepted (owner approved M0 on 2026-11-19 without objecting to this proposal).
- **Evidence [S3]:** HEVC encoder throughput on the M3 Pro:
  - 7680×2160: 46.3 fps (1.54× realtime at 30 fps).
  - 5120×2880: 52.3 fps.
  - 3840×2160: 90.3 fps.
  - 1920×1080: 315 fps.

  A full-display capture at 60 fps requested delivered only 44.5 fps (0 encoder drops).

  H.264 has two different limits, from two different encode paths:
  - [S3] AVAssetWriter H.264 (hardware not required) **does** encode above 4096 px, but slowly and
    unstably: 7680×2160 at 25.8 / 19.6 / 31.8 fps and 5120×2880 at 18.5 / 24.3 / 36.6 fps across
    the three quiet runs, against 46 and 52 fps for HEVC.
  - [S1] FFmpeg's `h264_videotoolbox -allow_sw 0` (hardware required) cannot open a session when
    either dimension exceeds 4096 (probed at 4098×2160 up to 8192×4320 in the consistency pass).
    With `-allow_sw 1` it opens at 5120×2880 and 7680×2160.
  - Together these suggest that above 4096 px VideoToolbox serves H.264 from a software encoder,
    which would explain [S3]'s low, noisy speed. That is an inference: [S3] did not record which
    encoder it got.
- **Decision:**
  - Default: 30 fps at native resolution, HEVC.
  - Offer 60 fps only when the capability probe's real encode at the actual capture size shows at
    least about 1.3× headroom. Otherwise offer "60 fps at 1×" or suggest 30 fps.
  - Never use H.264 for raw tracks with either dimension above 4096 px. The reason is throughput
    and stability (above), plus the likely software fallback, not that the encoder refuses to open:
    through AVAssetWriter it does open.
- **Consequences:** The M1's media engine ceiling is unknown, so the probe decides on each machine.
  Running an M1 is a pre-M2 action.
- **M1 update (2026-11-20, M1 plan §10 item 11, the probe-size rule):**
  - Probe v1 runs at launch and measures HEVC at 60 fps for every connected display's native and
    1× pixel size (sizes from CoreGraphics, no permission). A source may record at 60 fps (native or
    1×) only if its display's probe reached at least 1.3 × 60 = 78 fps. Windows and regions are
    never larger than their display, so a switch never re-probes. A display connected later is
    probed on the next launch and is not offered 60 fps until then. Sustained drops raise
    `FramesDropping`.
  - Measured rates are rounded to 0.01 fps, so the cached `probe-v1.json` compares exactly with a
    fresh probe (serde_json had read a 17-digit value back 1 ULP off; merge fix 96fbc5d).
  - On the M3 Pro [M1:export]: native 3024×1964 encodes at 117.9 fps (60 fps allowed), 1×
    1512×982 at 386.8 fps. `h264_videotoolbox -allow_sw 0` opens at 3840×2160 and 4096×2160 and
    fails at 4098×2160, as in [S1].
  - Delivered rate with `--fps 60` [M1:e2e]: 52.3 / 51.6 fps at native 3024×1964 and 52.5 / 50.8
    fps at 1× (two runs, `target/e2e/report-20261119-232627.json`, `…232905.json`). SCK delivers
    only on change, so this is a delivery figure on a changing test screen, not an encoder limit.

## ADR-010: Capture details on macOS
- **Status:** Accepted (within the spec); the formerly Proposed parts accepted with the M0 approval on 2026-11-19.
- **Decision:**
  - **ScreenCaptureKit:** pixel format 420v, BT.709 matrix, `queueDepth` 6, cursor shown, and
    `excludesCurrentProcessAudio = true`. 420v was chosen over BGRA because the HEVC encoder takes
    it directly and it uses 12 bpp instead of 32. BGRA was not measured [S3].
  - **Own-UI exclusion:** use an `SCContentFilter` that lists our windows one by one, and refresh it
    whenever our windows change.
    - Excluding the whole app fails under the dev host, because the child process is listed under
      the host's bundle ID [S3].
    - Re-test whole-app exclusion in the signed app in M1.
    - Tauri's `contentProtected` does not hide windows on macOS [ecosystem].
  - **Microphone:** AVCaptureSession, not AVAudioEngine. Its buffers carry host-time timestamps [S3].
    The SCStream mic output needs macOS 15+, so it is optional [ecosystem].
  - **Camera:** AVCaptureSession, auto-picking up to 1080p30 420v. Continuity Camera is detected by
    name and its Desk View companion, not by device type [S3].
  - **Frame copy:** *Proposed — decide in M1.* Cap copies every SCK frame into its own pixel-buffer
    pool, because AVAssetWriter can hold buffers long enough to starve SCK [cap-recording]. [S3] did
    not copy and saw no starvation in 10 s runs. A 30-minute soak test decides this.
- **M1 result (2026-11-20):**
  - **Frame copy: decided, no copy.** The 30-minute soak [M1:soak]
    (`target/soak/20261120-000343/summary.md`): display 3024×1964 at 2×, 30 fps, camera + mic +
    system audio All, switching display ↔ window every 5 min (6 segments). Screen 52,581 frames,
    0 dropped (0.000 %), writer not ready 0, longest stall 0.0 s, lowest 10 s rate 28.1 fps;
    0 StreamStopped, 0 source lost; RSS 73.3 MB at 2 min and 47.9 MB at 30 min (peak 78.4 MB,
    growth −25.5 MB). Every M1 plan §1 item 6 threshold passed. One debug-build run on the M3 Pro;
    an M1 8 GB run is still missing.
  - **Own-UI exclusion as built (M1 plan §10 item 9):** every stream excludes our registered
    window IDs plus every window our process owns at start, fetched with
    `onScreenWindowsOnly = false`. `SCShareableContent.currentProcess` (14.4+) also lists redacted
    windows of other processes (Menubar, Wallpaper, StatusIndicator), so it is filtered by our pid
    [M1:mac-sck]. A window created hidden and given a window number by `configure` stays excluded
    when shown (9 of 9 frames); a window created after the start is visible until
    `update_exclusions`, which takes 82–105 ms [M1:mac-sck]. When a menu of ours begins tracking
    (`OwnWindowsChanged`), every stream's exclusions are refreshed on a background thread at 0, 120
    and 400 ms; each refresh takes 35–153 ms [M1:app-integration]. The tray's Switch Source item
    opens the excluded popover rather than a submenu; our status item is excluded, leaving a gap in
    the menu bar.
  - **Tray menu leak: owner question (STATUS §5).** With the refresh on the main thread, 9 of 9
    sampled frames showed the open tray menu. With the background refresh: 0 frames in two runs,
    1 frame (about 66 ms after the menu opened) in a third [M1:app-integration,
    `target/app-driver/20261119-230228/report.json`]. A menu window becomes excludable only after
    `NSMenuDidBeginTracking` plus a filter update, so 0–1 frame can leak. Options: accept it, or
    exclude the whole app (`SC_EXCLUSION=app`), which fails under the dev host [S3] and is
    re-tested by the owner in the release bundle (MANUAL_TEST B4).
  - The floating bar and switch popover were absent from the compared frames
    [M1:app-integration]; the CLI's own test window (magenta) was absent from all 963 checked
    frames [M1:e2e].
  - "Only the captured app" audio: SCK groups app audio by bundle ID, and every unbundled process
    lands in one group. With two unbundled tone apps the 440-vs-660 Hz isolation did not hold in
    the wp-mac-sck live test (both tones about 0.053), while a bundled app (Finder) isolated to
    silence [M1:mac-sck]. The later e2e step 6 (SC-TONE-440 → SC-TONE-660W, App mode) measured
    the other tone at zero power in both spans [M1:e2e]. The two results disagree and the cause is
    not known; until it is, App mode is documented as reliable for bundled apps only.
  - Screen Recording preflight cannot tell "denied" from "never asked", so without a grant the
    status reads `not_determined` [M1:mac-sck].

## ADR-011: Clock and track alignment
- **Status:** Accepted (the spec's clock rule, verified on macOS); the formerly Proposed details (start gate) accepted with the M0 approval on 2026-11-19.
- **Evidence [S3]:**
  - All four macOS sources already timestamp in mach host time: SCStream within 6 µs, camera under
    1 µs, mic within 0.15–1.18 ms.
  - Screen PTS equals the display time.
  - The `startCapture` completion arrives about 240 ms *after* the first frame (spike agent's
    figure; not re-derived from saved fields).
  - Camera and mic first samples came 2.67–2.83 s before the recording start in the three quiet
    `main` runs, and 1.67–1.72 s before it in the kill runs (`out/bench-quiet*/*/project.json`,
    `tracks[].offset_from_start_s`). The screen's first frame came 0.10–0.15 s before it.
  - The only A/V offset measured was screen flash to system-audio onset: 39–58 ms.
- **Decision:**
  - Keep everything on one host clock per OS (mach, QPC, CLOCK_MONOTONIC), converted once at the
    source boundary.
  - *Proposed:* use a start gate. Warm up every source during the countdown, pick one shared start
    time, discard earlier samples, and release the last pre-start screen frame.
  - Store each track's measured first-sample offset in `project.json`. Never snap start times
    together (Cap snaps within about 21 ms, which hides real offsets [cap-recording]).
  - Derive audio timestamps from cumulative sample counts anchored at the first buffer.
  - At stop, re-append the last screen frame at the stop time. [S3] verified this: 10.03–10.15 s
    files for 10 s recordings.
- **Consequences:** Mic sync is unverified, because the spike's mic did not hear the test sounds.
  The flash/beep/clap sync test is an M1 deliverable. Any fixed system-audio latency compensation
  must come from that test.
- **M1 result (2026-11-20):**
  - The start gate is built as proposed (ADR-038, record): `start_timeout_ms` 5000; the router
    buffers up to 16 frames that arrive before the start is fixed and places the newest one with
    PTS < start at rec 0; s0 always starts at rec 0.
  - **Decision: no fixed system-audio compensation.** `just sync-test` (flash in a fixture window
    plus a 1 kHz beep, at the start, after each of two switches and at the end):
    - final 3 runs [M1:sync-test, second attempt, `target/sync/20261120-005228/`]: video →
      system audio median −10.2 ms over 30 events (min −41.1, max +8.5), 27 of 30 within ±33 ms;
      per run −26.4, −4.8 and −3.1 ms;
    - earlier sets [M1:sync-test, first attempt]: median −15.6 ms (28 of 30) and −6.8 ms
      (30 of 30).
    - Negative means system audio is earlier than the flash. The three events outside ±33 ms each
      had a slow stimulus (issue → video 38–49 ms), and `issue → system_audio` was stable within a
      run but moved between sessions (about +0.5 ms in one run, about +19.5 ms in every other),
      roughly one IO buffer. A constant offset fixes neither.
  - Soak [M1:soak]: the system audio − screen offset changed by −21.1 ms between the start and
    end flash bursts over 30 minutes (start +26.8 ms, end +5.6 ms), within one frame; count-based
    vs host-time drift 0.0 ms on mic and system audio.
  - **Mic and camera sync are still unmeasured.** The system output was muted (`osascript` read
    `output muted:true`; agents never change the volume), so the mic heard none of the beeps; the
    camera saw a dark, static scene (largest luma step 0.02 levels). One earlier low-confidence
    camera onset was about 108 ms after the issue. Owner step: unmute, light the room and rerun
    `SC_SYNC_RUNS=3 just sync-test`, plus the clap check (MANUAL_TEST C1). Sync across pauses is
    M7.

## ADR-012: Pause keeps capture running
- **Status:** Accepted (owner approved M0 on 2026-11-19 without objecting to this proposal).
- **Decision:** While paused, keep sessions and writers open and drop samples. Apply a running pause
  offset in our writer wrapper, and log pause intervals in host time in `project.json`. Handle the
  held-back last frame carefully, since Cap had a bug there [cap-recording].
- **Alternatives:** Stop and restart the pipeline into a new segment, as Cap's studio mode does.
  This is simpler and safer, but resume is slower and each pause creates more files. It stays the
  fallback if the M7 tests show problems.

## ADR-013: Export helpers are built from pinned sources
- **Status:** Accepted (spec Licensing). Owner decision 2026-11-19 on sub-point 3: "we need to build our own with networking disabled". Windows and Linux helpers (and the Linux LGPL libraries) are built by us from pinned sources with `--disable-network`; BtbN prebuilt binaries are not used. Sub-point 1 (Windows floor) is re-evaluated against our own build in M6. Sub-point 2 (zlib/PNG and an AV1 decoder) accepted with the M0 approval.
- **Decision:**
  - **macOS:** static FFmpeg 9.0.2 built by our own script `third_party/ffmpeg/build-macos.sh`,
    using markus-perl/ffmpeg-build-script as a reference (attributed in THIRD_PARTY.md).
    - Configured with `--enable-gpl`, no `--enable-nonfree`, no `--enable-version3`, giving
      GPL-2.0-or-later.
    - Also `--disable-autodetect --disable-network`, with the file and pipe protocols only, minimum
      macOS 14.0, and builds for arm64 and x86_64.
    - Includes x264 r3222, x265 4.3, SVT-AV1 4.3.0, libvpx 1.17.0, Opus 1.6.1, Ogg 1.3.6 and
      Vorbis 1.3.7, plus VideoToolbox and AudioToolbox.
    - Every tarball is SHA-256-pinned in `third_party/versions.toml` [S1].
  - **Windows and Linux:** pinned BtbN "gpl" builds, verified against both `checksums.sha256` and
    the per-asset digest. Pin month-end `autobuild-YYYY-MM-DD-HH-MM` tags, because daily tags are
    deleted after 14 days [ecosystem].
- **Evidence [S1]:**
  - `otool -L` shows only system libraries and frameworks.
  - All 29 outputs pass ffprobe validation in 3/3 runs.
  - arm64 sizes: ffmpeg 20.3 MB, ffprobe 20.1 MB.
- **Sub-points (decided 2026-11-19, see Status):**
  1. BtbN's stated Windows minimum is 10 22H2, but the spec says 10 2004+. Either raise the spec's
     Windows minimum or record it as a helper limitation [ecosystem].
  2. Enable system zlib (which restores the PNG decoder) or drop PNG from the decoder list. Also add
     an AV1 decoder so the helper can decode its own AV1 output for the probe.
  3. BtbN builds are general-purpose and probably keep FFmpeg's network protocols. That would put
     networking code into the shipped Windows and Linux helpers (and the Linux LGPL libraries),
     against the spec's "no networking code anywhere", even though the app never passes them a
     URL. Either build our own helpers with FFmpeg-Builds' scripts plus `--disable-network`, or
     record an explicit exception here (see docs/LICENSES.md, "Networking"). Not checked yet.
- **Consequences:** The FFmpeg GPG signature is unverified, because gpg is not installed. Whether to
  ship ffprobe (20 MB per architecture) is decided in M5.

## ADR-014: gifski is a separate sidecar
- **Status:** Accepted (spec).
- **Decision:** gifski 1.34.0 CLI, built from the pinned crates.io source with `cargo --locked`.
  It reads Y4M on stdin, always with explicit colour tags. It is never linked into the app, and
  cargo-deny bans the `gifski` and `imagequant` crates in the app graph [S1, ecosystem].
- **Consequences:**
  - The binary is AGPL-3.0-or-later and includes GPL-3.0-or-later libimagequant.
  - The default `gifsicle` feature (needed for the spec's "lossy" setting) compiles a Rust port of
    gifsicle's lossy LZW code. Its upstream licensing needs review before M5 [S1 verification].
  - gifski prints no progress, so progress comes from frames written to its stdin.
  - gifski x86_64 has not been built.

## ADR-015: Export pipe protocol, colour tagging and cancel
- **Status:** Accepted (within the spec's export design; verified by [S1]), the VP8 colour handling
  is split out as ADR-029 (accepted).
- **Decision:**
  - **Video:** Y4M on stdin (raw NV12 is also supported) through a bounded 3-frame queue, so a slow
    helper applies backpressure.
  - **Audio:** mixed in Rust and written to a temporary WAV file.
  - **Recipes:** never use `-ss`, `-to`, `-r` or scaling, because the renderer applies the EDL, size
    and fps [kap].
  - **Colour:** tag BT.709 limited range *on the frames* (`setparams`) as well as with the output
    options. FFmpeg 9 leaves primaries and transfer as "unknown" without it.
    - VP8: see ADR-029 (Proposed).
    - The palette-GIF path needs a BT.709 input tag.
  - **Encoder flags:** always pass `-tag:v hvc1` for HEVC and `-movflags +faststart` for MP4. Pass
    `-allow_sw 0` for VideoToolbox, so Fast never silently falls back to software.
  - **Cancel:**
    - Write to `<name>.part` and rename on success.
    - On cancel or failure, send SIGTERM, wait 1.5 s, then SIGKILL, and delete the `.part` file.
    - Sweep stale `.part` files on launch.
    - Keep "canceled" and "failed" as separate states.
  - **Progress:** frames written divided by total frames, plus `-progress` key=value lines for the
    final flush and the end signal [kap].
- **Evidence [S1]:**
  - SIGTERM finalizes in 0.22–0.24 s; a hung helper is killed by escalation in 1.53–1.55 s.
  - If the parent is killed, the helper sees EOF and exits by itself in about 0.23 s.
  - No helper processes were left over.
  - A SIGTERMed gifski exits in about 5 ms. ffprobe counts 18–20 frames in its partial in all five
    saved runs (the table's "playable = true"), but the file is truncated: re-checked in the
    consistency pass, it has no GIF trailer byte and its last frame fails LZW decode. Partials are
    always deleted anyway.

## ADR-016: Default export quality tiers
- **Status:** Accepted (owner approved M0 on 2026-11-19 without objecting to this proposal). The Small and High tiers still need a visual check before M5.
- **Decision (Small / Balanced / High):**

  | Encoder | Small | Balanced | High |
  |---|---|---|---|
  | x264 medium (CRF) | 26 | 22 | 18 |
  | x265 medium (CRF) | 28 | 25 | 22 |
  | SVT-AV1 preset 8 (CRF) | 38 | 32 | 26 |
  | VP9 good, cpu-used 4, row-mt, b:v 0 (CRF) | 38 | 33 | 28 |
  | VideoToolbox (q) | 50 | 60 | 70 |
  | AAC (aac_at) | 128k | 160k | 256k |
  | Opus | 96k | 128k | 192k |
  | gifski quality | 60 | 80 | 90 |

  Hide AV1 "Fast", since there is no AV1 hardware encoder here.
- **Consequences:** The owner should review these on real screen recordings before M5.

## ADR-017: GIF encoder naming
- **Status:** Accepted (owner approved M0 on 2026-11-19 without objecting to this proposal).
- **Evidence [S1]:** At 960×540, 15 fps:
  - FFmpeg palette mode (BT.709-tagged): 14.2 fps median (12.8–16.3), about 1× realtime,
    single-threaded. (15.6 fps, quoted earlier, was the untagged demo job in one run.)
  - gifski: 40.9 fps at quality 90 and 57.2 fps at quality 70.

  Palette mode is *slower* than gifski.
- **Decision:** gifski stays the default. Present FFmpeg palette mode as "Palette (colours and
  dithering control)", not "Fast".

## ADR-018: Skia is built from source with FreeType on every OS
- **Status:** Accepted (owner approved M0 on 2026-11-19 without objecting to this proposal).
- **Evidence [S4]:**
  - The stock macOS skia-safe binaries use CoreText. CoreText refuses to load the CBDT emoji font
    (tofu) and loads COLRv1 but draws nothing (0 coloured pixels).
  - With FreeType and `FontMgr::custom_empty()`, both formats render.
- **Decision:**
  - Build skia-safe 0.157.0 (`textlayout`) with `FORCE_SKIA_BUILD=1` and
    `SKIA_GN_ARGS="skia_use_freetype=true skia_use_system_freetype2=false skia_enable_fontmgr_custom_empty=true"`.
  - Load every font through `TypefaceFontProvider`, with no default font manager and font fallback
    disabled.
  - Cache the build in CI, or host our own archive through `SKIA_BINARIES_URL`.
- **Consequences:**
  - Build time on the M3 Pro: the saved first build (`target/ft-build.log`, machine under load)
    took 7 min 55 s. A clean build in a fresh target folder in the consistency pass took 2 min 42 s
    wall (load average 3.4 at the start), of which the skia-bindings build script took 144 s;
    whether the Skia source download came from a cache was not checked. The earlier "2 min 54 s"
    had no saved log. Incremental builds take seconds.
  - `FORCE_SKIA_BUILD` is what forces the source build. `SKIA_GN_ARGS` alone is silently ignored
    when a prebuilt archive matches [S4 verification].
  - Windows (clang-cl, Python, Ninja) and Linux are untested. They are the first M1 CI risk.
  - FreeType's FTL licence needs a credit in About → Licenses.
- **M1 update (2026-11-20):**
  - skia-safe also enables `embed-freetype`: on Linux, libfontconfig1-dev pulls in
    libfreetype-dev and skia-bindings otherwise adds `-I/usr/include/freetype2`, which clashes with
    the vendored FreeType (merge fix ec7f762, from [M1:ci]). No crate added. On macOS the build
    script reports the feature as redundant [M1:text].
  - Linux: the Skia FreeType build passes in the local ubuntu:24.04 container (aarch64) and on the
    GitHub ubuntu-24.04 runner (full job green in run 44695891492) [M1:ci, M1:ci-green].
  - Windows: windows-2025 now ships VS 18, whose STL rejects the image's clang-cl 20 (STL1000), so
    a cold Skia build upgrades LLVM to 22.1.8 (only on a Skia cache miss) [M1:ci-green]. In run
    44710191597 (m1 @ 76426a6) the Windows Skia cache was **restored**, so the upgrade was skipped,
    yet the skia-bindings build script still ran a source build during clippy (the cached out dir
    evidently did not match this build's hash) and clang-cl 20 failed with STL1000. So the Windows
    Skia FreeType build has **not yet passed on a runner** with the current workspace; gating the
    LLVM upgrade on a cache miss is not enough (CI fix for the orchestrator). The Q10 prebuilt
    fallback (repo variable `SC_WINDOWS_SKIA_PREBUILT=1`) exists and is unused.
  - Build time: a clean whole-workspace dev build including the Skia FreeType build took 4 min 17 s
    wall [M1:scaffold] and 233 s on a heavily loaded machine (Skia build script 118 s)
    [M1:ci-green].
  - Each distinct feature set builds Skia again: `cargo clippy` and `cargo test` got different
    skia-bindings hashes, and the shared `target/skia` held 7 full Skia builds during stage 1
    [M1:text]. The CI cache key is skia-bindings version + the root manifest's skia-safe line +
    `.cargo/config.toml` (ADR-038, CI).

## ADR-019: Emoji font is Noto Color Emoji COLRv1
- **Status:** Accepted (owner approved M0 on 2026-11-19 without objecting to this proposal).
- **Decision:** Bundle Noto Color Emoji COLRv1 v2.051 (OFL, 5.0 MB), pinned by commit and SHA-256.
  Keep CBDT (10.7 MB, bitmaps 109–136 px that go soft at 4K sizes) documented as a fallback [S4].
- **Consequences:** Emoji look the same in preview and export on every OS, independent of system
  fonts. Emoji currently get no outline or glow pass.

## ADR-020: Bundled fonts
- **Status:** Accepted with the owner's condition applied (2026-11-19: "don't use any that don't display correctly"). Three fonts replaced; see the update below.
- **Decision:** 13 fonts plus emoji: Inter, Montserrat, Oswald, Bebas Neue, Playfair Display,
  DM Serif Display, Courier Prime, Special Elite (Apache-2.0), Caveat, Permanent Marker (Apache-2.0),
  Pacifico, Tilt Neon, Fredoka. All were checked in google/fonts at `cc5661d` [S4].
  - System fonts come only through fontdb (path plus face index, loaded via FreeType).
  - Projects store font hashes, so export can warn when a font is missing.
- **Alternatives:** Swap the two Apache fonts for OFL ones if the owner wants OFL only.
- **Update 2026-11-19 ([font check](../spikes/04-skia-text/REPORT-fonts.md)):** all 13 rendered
  through the app's FreeType path with Latin-1, Latin Extended-A, punctuation, emoji, every weight
  and every text style.
  - **Dropped:** Fredoka (15 missing letters such as Đ đ, and 103 accented letters with the accent
    beside the letter), Permanent Marker (118 of 128 Latin Extended-A letters missing, and they
    vanish silently), DM Serif Display (11 missing letters and misplaced accents on 30 more).
  - **Replacements** (OFL, verified the same way): **Baloo 2** (rounded), **Sedgwick Ave Display**
    (marker), **Abril Fatface** (display serif).
  - **Final list (13):** Inter, Montserrat, Oswald, Bebas Neue, Playfair Display, Abril Fatface,
    Courier Prime, Special Elite (Apache-2.0), Caveat, Sedgwick Ave Display, Pacifico, Tilt Neon,
    Baloo 2, plus Noto Color Emoji COLRv1. Small remaining gaps are rare letters (ſ, ŉ, Ĳ, ′ ″, ặ)
    listed per font in the report.
  - **Rules for the text engine (M4):** pick weights with font arguments (`wght`), never
    `FontStyle::bold()` on a variable face (it gives a different, synthetic-looking bold); set
    each family's default instance explicitly (Montserrat's axis default is Thin); NFC-normalize
    text before layout; count unresolved glyphs and warn in the UI, because several fonts draw
    nothing for a missing letter.

## ADR-021: Text rendering and compositor colour handling
- **Status:** Accepted (owner approved M0 on 2026-11-19 without objecting to this proposal).
- **Decision:**
  - **Layout:** Skia paragraph layout, done once in 1080p reference units.
  - **Rasterizing:** rasterize each text item at output scale into premultiplied RGBA8 and cache it
    under the key (text, style, font IDs, scale). Hinting off, subpixel positioning on.
  - **Animations:** rotation, opacity and animations are compositor transforms on the cached
    texture.
  - **Reveal order:** typewriter and word-by-word reveals take their order from
    `getClosestGlyphClusterAt` / `getRectsForRange` (UTF-16), never from `extendedVisit`
    `utf8Starts`. That field is wrong on continuation lines in Skia m157 (a bug found and fixed in
    [S4]).
  - **Layers:** cache the box and the text as separate layers, so tiles do not cut boxes and glow is
    not clipped.
  - **Colour:** use `Rgba8Unorm` for the text textures and the render target, which means blending in
    gamma space, as Cap does [cap-render-decode]. Use the same choice in the CPU reference.
    Exported PNG and golden files carry an sRGB chunk.
- **Evidence [S4], quiet** (sum over the 11 test items, warm / cold, median of 30 reps;
  `out/bench-quiet.md`, then the three re-runs from the consistency pass in brackets):
  - Layout: 0.45 / 0.58 ms [0.44 / 0.55–0.56].
  - Raster at 1080p: 4.27 / 9.67 ms [3.91 / 9.10–9.13].
  - Raster at 4K: 15.91 / 26.73 ms [15.18–15.33 / 26.09–26.33].
  - GPU composite of 11 quads: 0.441 ms at 1080p [0.413–0.638], 0.765 ms at 4K [0.625–0.789].
  - Readback: 1.247 ms at 1080p [1.230–1.749], 6.923 ms at 4K [5.464–6.703].
  - The earlier figures (layout 0.21 / 0.32, raster 3.82 / 8.68 and 14.5 / 25.1, composite
    0.56 ms, readback 1.2 / 5.4 ms) came from the verifier's runs, which were not saved; they are
    withdrawn.
- **Consequences:** Re-rasterizing at 4K goes on a worker thread. RTL and complex scripts are
  untested.

## ADR-022: CPU reference renderer for tests
- **Status:** Accepted (spec).
- **Decision:** A CPU path that reproduces the compositor's sampling and blending, used for golden
  images in CI without a GPU. Tolerance against the GPU: max 2, mean 0.01 on Metal. CI starts at
  max 3 until WARP and lavapipe are measured.
- **Evidence [S4]:** GPU vs CPU max difference 1, mean 0.00059 at 1080p (0.00030 at 4K), and
  animation frames max 1. The same in the saved quiet run (`out/bench-quiet.md`), the saved E-core
  proxy run (`out/bench-ecore-proxy.md`) and the three re-runs from the consistency pass. (The
  earlier "identical in all 4 runs" referred to unsaved verifier runs.)
- **Consequences:** In the spike, the CPU and GPU paths share the affine and quad code. The real
  reference needs independent transform tests (geometry unit tests), or a shared bug would go
  unnoticed.
- **M1 update (2026-11-20):** the M1 compositor is stricter than this ADR's tolerance. Every stage
  (BT.709 YUV → RGB with nearest 4:2:0 chroma, a k×k integer box prefilter with
  k = floor(src/dst) per axis capped at 64, an 8-bit fixed-point bilinear step, premultiplied
  blending over the background, and RGB → I420 with centre-sited 2×2 chroma) is fixed-point i32
  maths shared line for line by the WGSL shaders and `cpu::render_cpu`. GPU vs CPU on Metal: max
  0, mean 0 for every aspect at scales 1, 0.5, 0.25, 0.140625 and 2, in RGBA8 and I420; YUV → RGB
  → YUV round trip max 0 [M1:render]. Geometry has its own property tests (100k contain-fit cases,
  [M1:geometry]). Software adapters and DX12/Vulkan/GL were not exercised; CI's Ubuntu job runs
  the render tests on lavapipe with `SC_REQUIRE_GPU=1`.

## ADR-023: How Cap's MIT crates are used
- **Status:** Accepted (owner approved M0 on 2026-11-19 without objecting to this proposal).
- **Context [cap-mit-crates]:**
  - Only the `cap-camera*` and `scap-*` families are MIT.
  - Every MIT crate depends on the AGPL `workspace-hack`, whose manifest pulls in a reqwest fork and
    other networking crates.
  - Some Windows crates call AGPL helpers (`cap-d3d-adapter`, `cap-mediafoundation-utils`).
  - `scap-targets` has no license field, so we treat it as not confirmed MIT.
- **Decision:**
  - Never depend on Cap through git or path.
  - On macOS, write our own objc2 code and read Cap's MIT crates only for design. Nothing has been
    adapted, and THIRD_PARTY.md says so.
  - For Windows camera support (M6), vendor and adapt `cap-camera-windows`,
    `cap-camera-mediafoundation` and `cap-camera-directshow` at `880ec89`:
    - keep their MIT license text and add a THIRD_PARTY.md entry;
    - strip `workspace-hack`;
    - replace the `cap-mediafoundation-utils` buffer lock with clean-room code;
    - add tests.
  - Never adapt `scap-targets`.
  - Ban `workspace-hack`, `cap-d3d-adapter`, `cap-mediafoundation-utils`, `cap-cursor-info` and
    `cap-cpal-ffmpeg` in cargo-deny.
- **Consequences:** No Cap code is in the repo today. Every AGPL Cap crate was read for design only
  and is described in our own words in the research notes.

## ADR-024: License enforcement in CI
- **Status:** Accepted (owner approved M0 on 2026-11-19 without objecting to this proposal).
- **Decision:**
  - Use cargo-deny 0.22.0 (fall back to 0.21.0) and cargo-about 0.9.3.
  - Allow MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0
    and CC0-1.0.
  - Allow MPL-2.0 only as a named exception, and WTFPL only for ffmpeg-next / ffmpeg-sys-next on
    Linux.
  - Ban gifski, imagequant and the Cap crates in ADR-023.
  - For npm, run `pnpm licenses list --prod --json` with an allowlist [ecosystem].
- **M1 update (2026-11-20, [M1:ci]):**
  - Five named MPL-2.0 exceptions, all unavoidable with Tauri 2 and used unmodified (file-level
    copyleft only): `cssparser`, `cssparser-macros`, `selectors`, `dtoa-short` (tauri-utils →
    dom_query, HTML/CSP processing of the bundled UI) and `option-ext` (tauri → dirs → dirs-sys).
  - HTTP clients are banned (`reqwest`, `hyper`, `ureq`, `isahc`, `curl`, `curl-sys`,
    `attohttpc`, `surf`). reqwest and hyper are in Cargo.lock only as Tauri's Android/iOS
    dependencies; `scripts/ci/deny.sh` proves with `cargo tree --target <each desktop target> -i`
    that nothing builds them. On Linux, `soup3` arrives through webkit2gtk (the system WebView) and
    is not banned [M1:scaffold].
  - `[advisories]` uses `unmaintained = "workspace"` and `yanked = "deny"`.
  - The npm gate checks production packages strictly and also dev packages, because svelte is a
    devDependency compiled into the bundle; the only dev exception is lightningcss (MPL-2.0, a
    Vite build tool, not shipped).
  - cargo-about 0.9.3 installs its binary only with `cargo install cargo-about --features cli`.
  - Results: the `licenses` CI job (cargo-deny, pnpm licenses, netcheck, cargo-about notices)
    passed in runs 44695891492 and 44705677113 [M1:ci-green]; details in LICENSES §2.

## ADR-025: Playback clock
- **Status:** Accepted (owner approved M0 on 2026-11-19 without objecting to this proposal).
- **Decision:**
  - When an output device exists, the cpal output position (minus device latency) is the master
    clock. Otherwise, use a monotonic wall clock.
  - The clock starts only after the audio callback confirms playback.
  - Re-seek audio when it drifts more than 40 ms from the video.
  - Keep one output stream open for the whole editor session [cap-editor-export].
- **Consequences:** Cap drives playback from video, so there is no reference implementation of an
  audio-master clock. The M3 editor work must test this.

## ADR-026: Tauri `macOSPrivateApi` is no longer needed
- **Status:** Accepted (owner approved M0 on 2026-11-19 without objecting to this proposal). The spec's Platform note is updated.
- **Evidence:** Since Tauri 2.12.1, transparency and fullscreen on macOS no longer need
  `macOSPrivateApi`, and the flag does nothing [ecosystem]. [S2] ran transparent with it set to
  `false`.
- **Decision:** Leave `macOSPrivateApi` out. Note in the spec that the native preview still relies
  on wry's private `drawsBackground` key (ADR-007).

## ADR-027: Homebrew build tools installed for M0
- **Status:** Accepted (owner approved 2026-11-19).
- **Decision:** These Homebrew packages are installed and may be used for builds: cmake, nasm,
  pkgconf, ninja, meson, autoconf, automake, libtool (glibtool) and just. Homebrew ffmpeg 9.0.2
  (dynamic GPL build) is a **validation and reference tool only** and is never shipped. Agents
  install nothing else; a missing tool is reported as a blocker.
- **Consequences:**
  - gpg is not installed, so FFmpeg's signature check is skipped (ADR-013).
  - Full Xcode 27.2 is present but has not been first-launched; xcode-select points at the Command
    Line Tools. That is fine with objc2 (ADR-006). It would block cidre.
  - CI runners must install the same tools. The scripts' `--disable-autodetect` and private
    pkg-config prefix keep Homebrew libraries out of the helper builds [S1].

## ADR-028: `SCContentSharingPicker` and the macOS 15+ re-prompt
- **Status:** **Accepted** (owner 2026-11-19, on condition that Windows and Linux have equivalents; they do). See the 2026-11-19 update below.
- **Context:**
  - On macOS 15+, an app that enumerates content itself (`SCShareableContent`) gets a "bypass the
    system private window picker" alert and a periodic re-approval, reported as about monthly and
    still present on macOS 26 (one secondary source). Apple DTS named two ways to avoid it: the
    managed `com.apple.developer.persistent-content-capture` entitlement (meant for VNC-style
    products, not available to us in practice) and presenting `SCContentSharingPicker`
    (macOS 14.0+) [ecosystem].
  - The picker is a system UI: the user picks a display, window or app each time, and the app
    receives a content filter. It does not offer region dragging, our numeric W×H and aspect
    presets, or a hotkey-only start.
  - **Not tested.** Spike 3 used `SCShareableContent` only, under the dev host. Nobody recorded
    whether the bypass alert appeared, and the monthly re-prompt cannot be observed in a one-day
    milestone. Whether picker sessions really avoid the re-prompt on macOS 26 is unknown ([ecosystem]
    open item; the note there that said "test this in spike 3" was not acted on).
  - Also unknown: whether a picker-provided filter can be combined with our own-window exclusions
    (ADR-010), and how the picker behaves under the dev host vs. the signed app.
- **Options:**
  1. **Adopt** the picker as the only way to choose a source. Avoids the alert (per DTS), but drops
     the spec's region drag, presets and hotkey-first flow, and our exclusion list may not apply.
  2. **Optional source path** (recommended default): keep our own source chooser
     (`SCShareableContent`) as the primary flow, and add "Choose with system picker…" as an extra
     source that hands its filter to the same pipeline. Region capture through it would be "pick a
     display in the picker, then crop" (crop after capture, as on Windows and Linux).
  3. **Reject**: own chooser only, and explain the re-prompt in onboarding (ADR-003).
- **Proposed decision:** option 2, but only if an M1 test shows the picker path helps. M1 test, in
  the signed `com.simplecapture.desktop` app (the dev host cannot answer it):
  - record once through our chooser and once through the picker on macOS 26 and note any bypass
    alert;
  - check that the picker's filter still excludes our own windows;
  - from M1 on, log every Screen Recording prompt the owner sees during daily use, with the path
    used, so the monthly behaviour is observed rather than assumed.
  If the picker path does not avoid the alert on macOS 26, fall back to option 3.
- **Owner questions:** Is a system picker acceptable as an optional path? Is "pick a display, then
  crop" acceptable for regions, or must region selection always be our own overlay?
- **Update 2026-11-19 (condition met; [research](research/pickers-and-hotswap.md), spike 3b):**
  - Windows has `GraphicsCapturePicker` (1803+, one window or one display) and Linux's ScreenCast
    portal *is* a system picker (mandatory on Wayland). So the condition holds on paper; none of
    the three was used interactively yet.
  - **Decision:** every OS gets a **"System picker…"** source next to our own chooser, and it can
    also be used to switch source while recording (ADR-034). Region capture through it = pick a
    display, then crop with our overlay.
    - macOS: `SCContentSharingPicker` (configured and checked without presenting in spike 3b), with
      `excludedBundleIDs`/`excludedWindowIDs` for our own UI and `presentPickerForStream` for
      switching.
    - Windows (M6): `GraphicsCapturePicker` via `IInitializeWithWindow`.
    - Linux (M6): the portal dialog, `persist_mode = 2`, restore token saved.
  - **Why this matters more now (ADR-003 update):** without a signing identity, release builds
    are ad-hoc signed and lose the Screen Recording grant on every update. A secondary source
    reports picker capture working with **no** Screen Recording grant and no bypass alert on
    macOS 26.5 (video only; system audio was silent without the grant). If that holds, the picker
    is the better default for release builds. M1 manual test: run the release bundle (never
    granted) and record through the picker; note any prompt or alert, and whether system audio
    works.
- **M1 update (2026-11-20, M1 plan §10 item 10), as built [M1:mac-sck, M1:app-integration]:**
  - The picker's modes are set once per launch: single window, single app, single display
    (read back as 21), `maximumStreamCount = 0`, `excludedWindowIDs` = our registered overlays plus
    the request's windows. Agents configured and read it back but never presented it (it would take
    focus).
  - Switching through the picker = `present()` plus a new make-before-break stream from the picked
    filter, not `present(for:)` with an in-place filter update ([S3b] saw wrong-size frames from
    that). Picks made for purpose "switch" switch inside the app core, so the driver, tray and
    picker share one path; before the integration fix a picker pick during a recording did nothing.
  - "Pick a display, then a region" = `PickedSource.region_pt` + `sourceRect`.
  - `update_exclusions` on a picked stream returns Unsupported until the release-bundle test shows
    that a rebuilt filter keeps picker consent.
  - Still open, owner only (MANUAL_TEST P1–P7 and B0–B6): never-granted picker recording, system
    audio on the picker path, behaviour after a rebuild, and `excludedWindowIDs` on a picked display
    with the floating bar visible.

## ADR-029: VP8 exports are BT.601 (exception to Pitfall 5)
- **Status:** Accepted on the orchestrator's recommendation (owner 2026-11-19: "I have no clue. Is this ok?"). VP8 cannot carry a Rec.709 tag that players honour, so BT.601 is the only way VP8 files show correct colours. Spec Pitfall 5 now carries the exception. The owner may still overrule it.
- **Context [S1]:** The VP8 bitstream cannot signal BT.709, and FFmpeg's VP8 decoder ignores the
  WebM container's colour elements, so a BT.709-encoded VP8 file decodes with the BT.601 matrix in
  FFmpeg-based players and many browsers, shifting colours. The spike converts VP8 to the BT.601
  matrix inside the filter graph and tags it `smpte170m`: decoded colour-patch error 4/255 (all
  BT.709 outputs: 1–3/255).
- **Decision (proposed):** VP8 is the one export that is not Rec.709: BT.601 matrix,
  limited range, primaries and transfer still BT.709, matrix tagged `smpte170m`. Its recipe and
  test say so explicitly.
- **Alternatives:** Keep BT.709 and accept wrong colours in some players (rejected: violates the
  intent of Pitfall 5 more). Drop VP8 from WebM (the spec lists it, so that is the owner's call).
- **Consequences:** The spec's Pitfall 5 gets a one-line VP8 exception once the owner agrees.

## ADR-030: Two workspace crates beyond the spec's suggested layout
- **Status:** Accepted (owner 2026-11-19: "Two extra code crates. Sure.").
- **Decision (proposed):**
  - `crates/geometry`: the spec's "one pure geometry module" as its own crate, with no GPU
    dependency, so UI hit tests, the CPU reference renderer and unit tests use it without pulling
    in wgpu.
  - `crates/preview`: the `PreviewSink` implementations (native CAMetalLayer surface on macOS,
    webview frames elsewhere, ADR-007), separate because the transport differs per OS.
- **Alternatives:** Keep geometry as a module inside `render`, and preview inside `app` or
  `render`. Simpler workspace, but `render` then drags wgpu into the UI-side and test users.
- **Evidence:** None beyond design reasoning; no spike built either crate.

## ADR-031: Optimize dependencies in dev builds
- **Status:** Accepted (owner approved M0 on 2026-11-19 without objecting to this proposal).
- **Decision (proposed):** `[profile.dev.package."*"] opt-level = 3` in the workspace, so decode,
  render, Skia and image code is usable in debug builds while our own crates stay debuggable.
- **Consequences:** Slower clean debug builds, faster debug runs. **Not measured in M0** (every
  spike benchmarked release builds only). Measure clean and incremental dev build times in M1 and
  drop this if the cost is high.
- **M1 result (2026-11-20): keep it.** [M1:ci-green], M3 Pro under heavy load (load average 30–50,
  so upper bounds), `cargo build --locked --workspace --timings`: clean build with deps at
  opt-level 3: 233 s wall, 1395 s CPU; with deps at opt-level 0: 132 s wall, 530 s CPU. So opt-level 3
  costs about 100 s wall and 2.6× the CPU on a clean build. Incremental builds after real edits are
  unaffected: `app/src/main.rs` 1.6 s (opt 3) vs 2.5 s (opt 0); `crates/geometry/src/lib.rs`, which
  rebuilds all 15 workspace crates, 3.5 s vs 4.1 s. The scaffold measured a clean build of 4 min 17 s
  and incremental builds of 3.26 s (`crates/core`) and 11.10 s (the app) [M1:scaffold].

## ADR-032: Project folder naming
- **Status:** Accepted (owner approved M0 on 2026-11-19 without objecting to this proposal).
- **Decision (proposed):** `Movies/Simple Capture/<YYYY-MM-DD HH.MM.SS> <title>.sc/` (ARCHITECTURE
  §6): sortable by date, no `:` (invalid on Windows), and a `.sc` suffix so the Library and the
  recovery scan can recognise project folders.
- **Open points for the owner:** Whether to keep the `.sc` suffix at all (Finder shows it as part
  of the name unless we declare a package type, and `.sc` is used by other software); whether the
  folder should be a macOS package (one icon in Finder) or a plain folder (raw tracks visible).
- **Evidence:** None; no spike created project folders in this layout.

## ADR-033: Output canvas: aspect ratio, contain fit and letterbox background
- **Status:** Accepted (owner requirement 2026-11-19; details chosen by the orchestrator, the owner
  may adjust them). Design: [canvas-and-source-switching](research/canvas-and-source-switching.md).
- **Decision:**
  - The output is a canvas with aspect `original | 16:9 | 4:3 | 1:1 | 9:16` (4:5 reserved), stored
    as an exact ratio. "Original" is the first screen segment's shape, resolved at record start;
    the recorder panel says so, because later sources are letterboxed into that shape.
  - Every screen segment is contain-fit separately: `s = min(Wc/w, Hc/h)`, centred, edges snapped
    to whole pixels. Cover + pan is a later nice-to-have.
  - The uncovered area is filled with a solid colour, a linear or radial gradient, or an image
    (cover-fit), rasterized by Skia into one cached texture used by both the GPU and CPU
    renderers. Images are copied into the project's `assets/` with a sha256. PNG and JPEG decode
    with the current Skia build; WebP needs the skia-safe `webp-decode` feature and is left out
    until the owner OKs it.
  - Canvas pixel size is derived, never stored: the reference size shows the source at 1:1, with
    the short side capped at 2160 for fixed aspects (long side ≤ 3840, under VideoToolbox H.264's
    4096 limit) and the long side capped at 8192 for Original. Export presets are named by short
    side (the "1080" class is 1920×1080, 1440×1080, 1080×1080, 1080×1920).
  - Text `_1080` units are relative to the canvas short side, like the camera's `size_norm`.
  - Scope wording in the spec: the letterbox background is in scope; padding, rounded corners and
    shadows around the screen stay out.
- **Consequences:** project.json `output.canvas` replaces `output.aspect`. A wide source in 9:16 is
  small (7680×2160 fills 304 of 1920 px of height); the recorder panel warns. The GIF default size
  rule (960 px width) needs a long-side version for 9:16; owner question for M5.

## ADR-034: Screen track as segments; switching source while recording
- **Status:** Accepted (owner requirement 2026-11-19; strategy measured in spike 3b on macOS).
- **Evidence ([spike 3b](../spikes/03-macos-pipeline/REPORT-swap.md), 15 runs under the dev host +
  verifier re-runs):** display ↔ 720×480 window ↔ sourceRect region every 3 s.
  - **Make-before-break** (start the new SCStream, cut on its first frame, stop the old one): gap
    0–28 ms between the last old and the first new frame (frames arrive every ~34 ms), no
    wrong-content frames in any segment.
  - Stop-then-start: 74–99 ms. Update in place (`updateContentFilter` + `updateConfiguration`):
    39–96 ms for filter changes, 1–3 transitional frames of the wrong size, and a `-3812` failure
    that leaves the stream stuck when a sourceRect config meets a window filter.
  - 58 + 18 segment files: all hevc/hvc1, BT.709 tags, correct sizes, 0 decode errors.
- **Decision:**
  - The screen track is an ordered list of segments, each with one source descriptor and one
    fragmented `.mov` (`tracks/screen-NNN.mov`), placed on the recording clock, file time 0 = the
    segment start. A gap holds the last frame. Mic, camera and system audio stay continuous.
  - macOS switches make-before-break and routes frames by stream identity; it falls back to
    stop-then-start if the new stream fails. `updateConfiguration` alone (no filter change) is used
    only for region changes on the same display (7–28 ms, no audio impact).
  - Windows (M6): a new WGC session per switch, made before the old one ends. Linux (M6):
    switching between sources picked up front (`multiple = true`) needs no dialog; any other new
    source opens a portal dialog while the screen holds its last frame.
  - Segment boundaries are hard video cuts with no audio fade. The EDL and the edit commands are
    unchanged (segments live in recording time).
  - Each segment is written to the journal when it opens and patched when it closes, so recovery
    rebuilds the list after a crash.
  - Switching UI: a "Switch source" button on the floating bar and in the tray menu, plus a hotkey,
    opening displays, windows, "Select region…" and "System picker…". A closed captured window ends
    its segment, holds the last frame and prompts for a new source.
- **M1 update (2026-11-20, M1 plan §10 items 2, 3 and 12), as built:**
  - Region changes during a recording are switches (new segment, make-before-break);
    `updateConfiguration` in place is not used in M1. `ScreenSource::start` returns after the stream
    has started, and cuts and the start gate use first-frame PTS only.
  - Order of a switch: journal `switch_begin`; journal `segment_prepare` and pre-create the new
    segment's writer; start the new stream; its first frame is the cut (`segment_open`); the old
    stream keeps running until its first frame at or past the cut, for at most two frame intervals
    + 10 ms (about 77 ms at 30 fps), so in-flight frames land in the old segment and trailing frames
    are counted; then `segment_close` (its `end_rec_ns` is still the cut) and `project.json` is
    rewritten on the control thread.
  - A new stream with no first frame within 1.5 s is abandoned. On a start failure the ladder is
    retry stop-then-start → restore the old source → source lost (`switch_failed.next` =
    `retry_stop_then_start` | `restore_old` | `source_lost` | `keep_old`). A minimized window
    target ends with `switch_failed` / `keep_old` [M1:e2e].
  - Measured live [M1:e2e, run 5, two runs]: request → cut 146.3 / 142.4 ms (display → window),
    112.7 / 114.5 ms (window → region) and 114.0 / 130.2 ms (region → display), all under the 500 ms
    target; 1 trailing frame dropped per switch; recorded gap 5.6–41.6 ms between the last old and
    first new frame; peak RSS with two native-size streams overlapping 48.3 / 47.3 MB. Switch to a
    static window (SC-STATIC): 149.3 / 151.9 ms.
  - [M1:mac-sck] measured 0 wrong-content frames in 5 runs; the gap at the new stream's first frame
    exceeded 34 ms in 6 of 15 switches (worst 41.7 ms, one extra 120 Hz vsync, mostly when the
    7680×2160 display stream starts), while the best switch point inside the overlap was 0–20.8 ms
    every time. The router cuts at the first frame, so gaps up to about 42 ms remain possible.
  - Synthetic timings with real AVAssetWriter files [M1:record-session, `session_macos`, 11
    switches, median / max]: `switch_begin` → `segment_prepare` 4.0 / 11.4 ms, `segment_prepare` →
    cut 60.2 / 88.5 ms; stop-then-start gap 64 ms.
  - Source lost: a closed window gives no SCK error, only one Suspended frame; a watcher reports
    `SourceLost` about 80 ms after the close [M1:mac-sck]. e2e: lost at 4.13 s, last s0 frame
    4.09 s, s1 opened by the scripted switch at 7.15 s [M1:e2e].
  - Over 30 minutes with a switch every 5 minutes: 6 segments, 0 switch failures, 0 drops
    [M1:soak].

## ADR-035: macOS system audio comes from its own SCStream, and audio gaps are filled with silence
- **Status:** Accepted (required by ADR-034; measured in spike 3b).
- **Evidence:** spike 3 took system audio from the screen's SCStream, and every filter change or
  restart of that stream lost 60–171 ms of system audio. Worse, AVAssetWriter's AAC input closes
  timestamp gaps instead of keeping them, so everything after a gap played 0.08–0.48 s early.
  A separate audio-only SCStream had 0 gaps over 1 ms in 13 of 13 runs, and its file length matched
  the host-time span. Mic: 0 gaps in 20 of 20 runs.
- **Decision:**
  - System audio is captured by a dedicated SCStream for the whole recording (minimal video
    config, `capturesAudio`, `excludesCurrentProcessAudio`). Screen streams carry video only.
  - "Only the captured app" follows the active segment by updating the audio stream's filter;
    display and region segments use "All".
  - Every audio writer (mic, system audio, on every OS) inserts silence for any timestamp gap, so
    audio never drifts early.
- **M1 update (2026-11-20, M1 plan §10 item 15), as built:**
  - Overlap trimming is symmetric with gap filling: the filler re-anchors audio to host time and
    corrects device clock skew in small steps, each at most the threshold plus one buffer.
    `gap_threshold_ns` is 2 ms. At ±100 ppm over 30 simulated minutes: 89 fills or trims, each
    exactly 2.0 ms [M1:record-session]. Resampling is not used.
  - Soak [M1:soak]: mic and system audio 0 gaps, 0 overlaps, drift 0.0 ms over 30 minutes. The
    mic's `health.frames` was 13,824 samples (288 ms) above duration × rate while its file was
    exactly 1800.0 s; the surplus does not grow with run length and is probably counted before the
    start trim (minor, open).
  - Track ends: mic, camera and system-audio files end at the stop time, and a missing audio tail
    (in-flight buffers that never arrive, 40–60 ms) is filled with silence [M1:e2e fix, 6bb31a8,
    9413bbd].
  - App mode follows the active segment with `updateContentFilter`: 144–178 ms per change, with a
    76–95 ms timestamp hole around it that the filler closes [M1:mac-sck]. App-audio isolation
    caveat: ADR-010 M1 result.

## ADR-036: M1 dependency set, git policy and plan-level answers
- **Status:** Accepted (owner 2026-11-19).
- **Decision:**
  - New dependencies approved by the owner: tracing-subscriber, sha2, serde_json, anyhow, libc,
    pollster, bytemuck (Rust); svelte-check, @tsconfig/svelte (npm, dev only); GitHub's own CI
    actions pinned by commit; local `cargo install` of cargo-deny 0.22.0 and cargo-about 0.9.3; the
    ubuntu:24.04 Docker image for a local Linux build check. Full list with licences:
    docs/M1_PLAN.md §9.
  - Git: agents may branch, commit, use worktrees, merge, rebase and push to the public origin
    (never recordings or secrets). M1 work happens on branch `m1`.
  - Project folders are plain folders with the `.sc` suffix (ADR-032). Click highlight: SCK
    `showMouseClicks` on macOS 15+, own drawing in M7. Quick export caps the long side at 3840.
    Backgrounds: PNG and JPEG. Windows CI may temporarily use prebuilt Skia if the FreeType build
    fails there.
  - Test binaries never take focus from the owner (CLAUDE.md).


## ADR-037: M1 plan-level choices (M1 plan §10) and their results
- **Status:** Accepted (made by the M1 plan, 2026-11-19, under the owner's "you figure this out";
  logged at M1 close, 2026-11-20). The owner may revisit any of them at the ★ review.
- **Decision and result**, by §10 item. Items 2, 3, 9, 10, 11, 12 and 15 changed earlier ADRs and are
  recorded there.
  1. **CPU upload in M1 export.** Decoded NV12 planes are uploaded to wgpu from mapped buffers
     (`R8Uint` + `Rg8Uint` textures); the zero-copy IOSurface import of [S3] moves to M3. Same path
     on every OS and on the synthetic platform. Quick export speeds with it: CODEC_MATRIX §2.7.
  2. Region changes are switches: ADR-034 M1 update.
  3. `ScreenSource::start` returns after start; cuts use first-frame PTS: ADR-034 M1 update.
     First-frame latency [M1:mac-sck]: display 158 ms, window 63 ms, application 347 ms, regions
     161–360 ms.
  4. **No new utility crates where std suffices.** Held: `Cargo.lock` did not change after the
     scaffold (every merge report), and no package filed a lockfile change request.
  5. **Live tests** run through `scripts/test-live.sh` under the dev host and fail when 0 tests
     ran; a debug-build TCC tripwire makes plain `cargo test` fail instead of touching TCC. Result:
     14 `sck_live` tests pass under the host [M1:mac-sck]; running `simple-capture sources`
     outside the host exits 1 with the tripwire message naming `run.sh` [M1:cli]. Release builds
     have no tripwire, so the test that checks it runs in debug builds only.
  6. **Camera track recorded in M1** without a bubble: 53,975 camera frames, 0 dropped, in the
     30-minute soak [M1:soak].
  7. **`cursor.jsonl` deferred.** Q5: click highlight uses SCK `showMouseClicks` on macOS 15+ only
     (hidden on 14); our own click drawing and `cursor.jsonl` clicks wait for M7.
  8. **M7 items stay in M7:** pause/resume, countdown, start/stop and pause hotkeys, Restart.
     `Session::pause/resume` return `Unsupported`; the schema's `pauses`, `RecClock` pause maths and
     the journal `pause`/`resume` events exist and recovery folds them, so M7 needs no migration.
     The mic level meter and debug overlay are on no milestone line; STATUS §4 proposes M7.
  9. Own-UI exclusion rule: ADR-010 M1 result (owner question on the tray menu).
  10. Picker: ADR-028 M1 update.
  11. Probe-size rule: ADR-009 M1 update.
  12. Switch timing and failures: ADR-034 M1 update.
  13. **Sleep stops and finalizes** the recording (`stop.reason = sleep`) with a notice after wake;
      with no pause in M1 there is no resume across sleep. The driver's simulated-sleep flow passes;
      the `StoppedForSleep` notice is now emitted before the finalized state (it used to race the
      exit) [M1:app-integration]. A real sleep is an owner step (MANUAL_TEST H5).
  14. **Quit, SIGTERM and logout stop gracefully** and cancel an export; a project is `recovered`
      only after a real crash or kill. Driver flows [M1:app-integration]: Quit then relaunch stays
      `finalized`; SIGTERM to `run.sh` gives stop reason `signal`; `kill -9` then relaunch gives
      `recovered` with the launch notice; Quit during an export leaves no `.part` and no ffmpeg
      process. e2e step 14: SIGTERM and a `stop` line on stdin both finalize [M1:e2e]. Logout was
      not tested by agents.
  15. Audio overlap trimming: ADR-035 M1 update.
  16. **App identity is `AppRef { pid, bundle_id? }`.** The project schema gains `app_pid` on
      application sources and makes `app_bundle_id` optional; `region_pt` is display-local
      (ARCHITECTURE §6). The CLI records an application by `app-pid:` and adds the bundle ID when
      it finds one [M1:cli].
  17. **`session.log`** is written by `sc_core::sessionlog` from record and recovery
      (`<rfc3339> <host_ns> LEVEL msg`, one write per line, no fsync), not through a tracing
      subscriber; app and cli own their subscribers. The session writes one `config {…}` line at
      start with the requested settings [M1:e2e].
  18. **`run.sh`** forwards INT/TERM/HUP to the child, exports `SC_DEV_HOST=1`, always writes a pid
      file and supports a stdin FIFO (`SC_DEV_STDIN`). Script change only; the signed host bundle
      is untouched [M1:scaffold].
  19. **Whole-app exclusion re-test** (ADR-010) moves to the owner's release-bundle test via
      `open -n --env SC_EXCLUSION=app -a "Simple Capture"` (MANUAL_TEST B4); per-window exclusion
      stays the default. Not run yet.
  20. **Overlays are repositioned** onto the target display before they show; the pick overlay
      follows the cursor across displays [M1:app-shell]. Only one display was connected, so the
      non-main-display paths were skipped in every live test (e2e step 11, the `sck_live`
      non-main test) and are an owner step (MANUAL_TEST S7).
  21. **Release bundle config** lives only in `app/tauri.bundle.conf.json`, so plain builds work
      everywhere. The built bundle was inspected, never launched: app and both helpers signed
      `adhoc,runtime`, both entitlements present, three usage strings in Info.plist
      [M1:app-shell].

## ADR-038: M1 implementation choices made inside packages
- **Status:** Accepted (package decisions within frozen contracts, 2026-11-19/20; listed so the
  ★ review can see them). Source: each package's report.
- **core** [M1:core]:
  - Project folder titles are capped at 100 UTF-8 bytes (cut at a char boundary, never leaving a
    dangling ZWJ or variation selector); `/ \ : * ? " < > |` become `-`; an empty title becomes
    "Recording"; collisions get " 2", " 3".
  - `is_project_dir` = a `.sc` folder holding `project.json` or `journal.jsonl`.
  - A missing journal reads as empty; a torn last line is skipped and flagged; a torn line that
    later appends pushed into the middle counts as unknown.
  - `segment_at` returns None in gaps; holding the previous frame is the caller's job.
  - `ProjectLock`: `.lock` with `<pid> <uuid>`; on unix a lock whose pid is dead is stale and taken
    over; on Windows an existing lock always counts as held (a crashed Windows recording stays
    locked until the file is deleted). This makes `kill_synth` mid-s0 / mid-s1 fail on
    windows-2025 (`Locked` instead of `Recovered`, run 44710191597); needs a fix or a Windows skip
    before CI can be green. `fsync_dir` is a no-op on Windows.
  - Journal appends are one write + fsync (F_FULLFSYNC on macOS); not measured, fine at journal
    event rates.
- **geometry** [M1:geometry]: edge rounding uses exact rationals, round half up; the reference long
  side is computed from the even-rounded short side, so a reference whose short side equals a
  preset class gives exactly that preset; extreme Original ratios (beyond 4096:1) also clamp the
  long side at 8192; with no segments the reference short side is 1080; `cover_src_rect`'s `focus`
  is the normalised crop centre, clamped inside the image; `aspect_locked_resize` `Move` takes the
  new top-left corner, "Match frame" = `Some(canvas ratio)`, Free = None.
- **synthetic platform** [M1:platform-synth]: one PTS-ordered scheduler thread per platform, so runs
  are reproducible; after a newer stream's first frame the old stream sends exactly
  `trailing_after_new_stream` more frames (default 0) and goes quiet; `.sctrk` tracks are
  RLE-compressed with unbuffered length-prefixed records; `AudioReader::read` returns f32 samples
  in whole frames (the meaning every reader now follows).
- **decode** [M1:decode]: `held` is true only when the frame is not the active segment's own frame
  at t; a held frame reports the segment, `frame_px` and `content_px` it came from;
  `TrackAudio::read` always fills the buffer (silence past the end); the audio start comes from
  `first_sample_offset_ns`, not `first_sample_file_ns()`.
- **render**: fixed-point maths shared by GPU and CPU (ADR-022 M1 update). The background is
  re-uploaded every frame (about 33 MB at 4K); an upload cache keyed on the `BackgroundCache` Arc is
  a later optimisation [M1:render].
- **text** [M1:text]: linear gradients use the CSS angle convention (0° up, clockwise), so export
  matches the UI preview; every background is forced opaque; gradients interpolate in
  sRGB-encoded values; the cache key is FNV-1a (`DefaultHasher` is not stable across Rust
  versions); images sample mipmapped trilinear when shrinking and Mitchell cubic when enlarging;
  image fit Contain letterboxes on black. `build_info`: FreeType true, milestone 157, JPEG decode
  true, WebP false.
- **helpers** [M1:helpers]: `sweep_stale` removes only `<name>.<ext>.part` files with an export
  extension (mp4, mov, m4v, gif, webm, mkv, wav, m4a) idle for at least 60 s; commit hard-links
  (check-then-rename fallback) and never overwrites; a failed commit deletes the `.part`; `finish`
  reports a non-zero exit in `ExitReport.code`, not as an error; cancel is SIGTERM to the process
  group, 1.5 s, then SIGKILL (measured within 1500 ± 300 ms).
- **export** [M1:export]: frames are tagged with `setparams` as well as output tags (FFmpeg 9 lets
  untagged frame properties override `-color_*`); the mix WAV is `<out>.wav.part` next to the mp4
  `.part`; a size class at least as large as the reference falls back to the reference; for
  Original with several segments the size-class ratio comes from `canvas.ratio`.
- **record, session** [M1:record-session, M1:e2e]:
  - `Tuning` defaults: screen queue 3 (SCK `queueDepth` 6; video drops the newest frame when
    full), camera queue 8 ([S3] saw camera writer bursts), audio queue 256 (about 2.5 s of 10 ms
    buffers, so audio never drops), gap threshold 2 ms, start timeout 5 s, switch first-frame
    timeout 1.5 s, `FramesDropping` at 2 % over 10 s, disk warning at 2 GB and stop at 500 MB
    (start needs 2 × stop = 1 GB), fragment interval 1.0 s (ADR-008 owner question), default HEVC
    0.05 bits per pixel [M1:mac-av].
  - The old-stream drain and the 16-frame early buffer (ADR-034, ADR-011 M1 updates);
    `segment_close` is journaled after the drain.
  - s0 starts at rec 0; screen frames with PTS ≥ stop are dropped; continuous tracks end at the
    stop; audio tails are padded with silence (fixes from `just e2e`).
- **record, recovery** [M1:record-recovery, M1:kill-recovery-test]:
  - Frameless screen segments and **0-byte track files** (a writer killed before its first append)
    are dropped with a warning, file kept, without marking the project damaged (empty-file rule:
    merge fix 76426a6, test `kill_after_segment_open_with_zero_byte_segment_file`).
  - Unreadable non-screen tracks are dropped from tracks, `audio_mix` and overlays, and the project
    is marked `damaged`.
  - Recovery details go to `recording.extra.recovery = {dropped: [...], notes: [...]}`.
  - `AlreadyFinal` is decided before taking the lock, so a second run writes nothing; per-project
    errors during `scan` are reported as `Damaged` and the scan continues.
- **cli** [M1:cli]: `display:N` and `@DISPLAY` try an exact display ID first, then a 1-based index;
  `--own-test-window T=SECONDS`; the own test window is a borderless non-activating NSPanel shown
  with `orderFrontRegardless` only after it is excluded; the probe cache lives in the app data
  folder (`~/Library/Application Support/com.simplecapture.desktop`, override `SC_DATA_DIR`);
  default title "Recording"; if the session finalized itself (disk, sleep) the CLI still prints the
  folder and exits 0; a second signal kills the process. Headless binaries that build SCK window or
  app filters must first create an accessory NSApplication, or SCK aborts with `CGS_REQUIRE_INIT`
  [M1:e2e].
- **capture-macos** [M1:mac-sck, M1:mac-av, M1:e2e]:
  - Regions use `sourceRect`, no crop: 617×283.5 pt → 1234×566 px, 1×1 pt → 2×2 px, 1121×641 px →
    1120×640 px (pixel sizes floored to even), content verified. `method` is `source_rect`.
  - `content_px` uses SCK's scale factor (it was in points on Retina, which would have exported only
    the top-left quarter; e2e fix 4c0883b).
  - Application capture picks up a second window opened mid-recording 66–77 ms after it appears.
  - `MacOverlays::configure` swaps the window's class to a non-activating NSPanel subclass and puts
    the original class back before `close`; without that, AppKit's KVO throws on macOS 26 when the
    window closes.
  - Camera and mic `start()` require `authorized` and never call a request API, so they cannot
    prompt; `DeviceLost` comes from the device watcher; a writer that never got a sample is
    cancelled and reports 0 samples; probe durations come from the edit-aware track time range
    (raw AAC packets add about 61 ms of priming).
  - The live `show_mic_modes` test runs only with `SC_LIVE_MIC_MODES=1`, because it opens Control
    Center's panel on the owner's screen.
- **app and UI** [M1:ui, M1:app-shell, M1:app-integration]: the app sets the accessory activation
  policy and never activates; seven windows are created hidden and configured at launch;
  `app/build.rs` writes a placeholder `ui/dist/index.html` so a fresh checkout builds; overlays get
  their purpose from a UI event `overlay-context` (fallback: hash query, then the display under the
  window, then the main display); overlay UIs emit `region-selected`, `source-picked` (own chooser)
  and the cancel events, while Rust emits `source-picked` for the system picker; new events
  `launch-tasks-done` and the UI-only `ui-log` (CSP violations and uncaught errors go to the app
  log); capabilities add `core:window:allow-show` / `allow-hide`.
- **CI** [M1:ci, M1:ci-green]: fast job (no Skia or Tauri) and full job per OS; Skia build cached
  apart from `target/`, keyed on skia-bindings version + the root manifest's skia-safe line +
  `.cargo/config.toml`; `SC_REQUIRE_GPU=1` on Ubuntu only (lavapipe); `SC_ALLOW_NO_HW_ENCODER=1`
  on macOS runners with a visible warning; Windows target dir `C:\t`; tests run with
  `--test-threads=2`; LLVM 22.1.8 on a cold Windows Skia build; `kill_synth` runs with
  `--features test-hooks` on all three OSes; two real-time record tests get 100 ms extra when `CI`
  is set; the local merge check (`ci-local-fast`) also runs Windows and Linux cross-target clippy
  of the fast crate set, because cfg-gated code broke CI twice.
- **Test tooling** [M1:soak, M1:kill-recovery-test, M1:e2e, M1:app-integration, merge reports]:
  - Soak sync points are bursts of 5 flashes 2 s apart, averaged (one flash sits on the 33 ms frame
    grid, the size of the threshold); end sync is judged as the change in the audio − screen offset
    from start to end.
  - The kill tool arms `SC_TEST_HOLD_AT` only at switch time, lets s0's hold through, and builds
    its hooks CLI in a private target dir.
  - The live lock is machine-wide (under the main checkout's `target/`, found with
    `git rev-parse --git-common-dir`).
  - Live scripts stage binaries, working folders and recordings outside `~/Documents` (in
    `$TMPDIR`): hosted or disclaimed children of binaries under `~/Documents` hung in dyld `open()`
    on a pending Documents-folder consent. Signed binaries are copied to a new file and renamed,
    never overwritten in place (an in-place copy gets SIGKILLed at exec).
  - Shared `target/skia` hazard: a build from one worktree can link crates compiled from another
    worktree's sources, so live results from shared-target binaries may not match the branch under
    test. The kill tool builds in a private target dir for this reason.

## ADR-039: Application sources are cropped to their windows at segment start
- **Status:** Accepted (bug fix for the owner's hands-on report, 2026-11-20: at 9:16 an application
  source was fit as the whole display, so the app's windows were tiny). The owner may revisit it at
  the ★ review.
- **Evidence:** an application source used `SCContentFilter(display:including:exceptingWindows:)`
  at the display's full size with no `sourceRect`, so the frame, `content_px`, the Original ratio,
  the preview and the export all followed the display, not the app.
- **Decision:**
  - At segment start the capture crop is the union of the app's on-screen windows below the menu
    bar level (window layer 0–23, so floating panels count; status items, pop-up menus and
    overlays do not; at least 2 pt on a side, minus excluded windows), each clipped to the segment's display
    (`sc_geometry::app_bounds_pt`). It is snapped like a region (`snap_region_even_px`, Native =
    display scale, 1x = 1) and captured with `sourceRect`, so the frame is crop-sized and every
    later step (router `content_px`, Original ratio, export) follows it unchanged.
  - The crop is fixed for the segment. Windows moved or opened outside it later are clipped;
    exclusion updates (`updateContentFilter`) keep the configuration and so the crop. A source
    switch or a re-pick (own chooser or system picker, single application) starts a new segment and
    re-crops.
  - An app with no such window on the display, or whose windows cover the whole display, records
    the whole display with no `sourceRect` (as before).
  - `SourceDescriptor::Application.crop_pt` (optional, display-local points) records the crop;
    projects without it still load. The UI sizes the preview the same way from the source list
    (`appBoundsPt`, `snappedRegionSizePx` in ui/src/lib/geometry.ts mirror the Rust rules).
  - Snapshots (`SCScreenshotManager`) set `scalesToFit` so a crop or window larger than the
    preview's maximum is scaled, not clipped.
- **Result** [live, 2026-11-20, CLI under the dev host]: `record --source app:com.apple.finder
  --aspect 9:16` on a 3840×1080 pt display at 2x with one 920×436 pt Finder window: `sourceRect`
  works with the `includingApplications` display filter (no SCStream error), the segment is
  1840×872 (`crop_pt` 1781,185 920×436, `content_px` the whole frame) instead of 7680×2160, and
  the export is 1840×3272 (9:16) with the window filling the canvas width (rows 1200–2072).

## ADR-040: Source lists show only windows a person would record
- **Status:** Accepted (bug fix for the owner's hands-on report, 2026-11-20: the Windows tab listed
  menu-bar status items, the WindowServer "Cursor" window and untitled windows; the Apps tab listed
  Control Center and Dock; "Pick window by clicking" always picked the Cursor window, which is
  frontmost and always under the pointer). The owner may revisit it at the ★ review.
- **Decision:** `list` (macOS, `sck::listable`, so the CLI `sources`, the panel tabs, the switcher
  and the pick overlay agree) keeps a window only if it is on screen and overlaps a display, is on
  an app content layer (0–23, as ADR-039), is at least 40 pt on each side (the pointer, status
  items, the menu bar, tooltips and status bubbles are smaller; a title bar alone is 28 pt), has a
  non-blank title or is at least 160 pt on each side (untitled helpers such as find bars and hover
  cards are small; some real windows, such as games and video players, are untitled), is not ours
  or in `hide`, and its owner is a regular app
  (`NSRunningApplication.activationPolicy`) or an unbundled program with the accessory policy.
  Dock, Control Center, Notification Center, SystemUIServer, loginwindow and menu-bar apps are
  bundled accessory or prohibited apps, and WindowServer is not an app. The unbundled exception
  keeps the devtools fixtures (accessory so they never take focus or show in the Dock) listable for
  the live tests and driver flows. An app is listed only if it owns a listed window.
- **Not changed:** the application crop (`app_window_frames`, ADR-039) still counts every window of
  the app on a content layer, 2 pt or more, titled or not, so the UI's preview size estimate from
  the list can differ when such a window lies outside the listed ones. Following app audio for a
  window source looks the owner up directly (`ScreenSource::window_app`), so it works for windows
  the list hides.
- **Result** [live, 2026-11-20, CLI `sources` under the dev host, one 3840×1080 pt display]: before,
  37 windows and 10 apps (WindowServer, Control Center, Dock, LanguageTool's full-display overlay at
  layer 3, Chrome's tooltip and status bubble); after, only titled layer-0 windows of Chrome, Slack,
  Claude, Messages, Notion, Asana and Moonlight, and those 7 apps. The frontmost listed window is a
  real app window, so the pick hit test lands on it.

## ADR-041: M2 camera bubble: placement, defaults and build approach

- **Status:** Accepted by the owner (2026-11-20); the placement part is superseded by ADR-043.
- **Context:** M2 adds the live camera bubble (SPEC Camera overlay). The camera position is stored
  in the output frame (canvas-normalized `CameraKey`), but the screen shows that frame truthfully
  only for a whole-display source with the Original frame.
- **Decision:**
  - Whole-display source, Original frame: the floating bubble is the placement; its spot on screen
    is its spot in the video.
  - Window, app and region sources, and letterboxed frames: a floating bubble stays on screen so
    the user sees themselves, and a stage preview window (live composed output, excluded from
    capture) is where the camera is placed exactly.
  - The camera is not mirrored by default; mirroring is a toggle. The preview always matches the
    output.
  - The camera session opens whenever a camera is selected and the recorder panel or a recording
    is active, so the bubble can be set up before recording.
  - M2 is built by one agent, package by package (docs/M2_PLAN.md §4), not a multi-agent workflow.
- **Consequences:** an app-level camera hub feeds the bubble, the stage preview and the recording
  from one device session (M2_PLAN §3.1).

## ADR-042: M2 camera implementation: geometry, shared camera, live frames, keyframes

- **Status:** Accepted (M2 build, 2026-11-20).
- **Context:** M2 needs the camera in the bubble, the stage preview, the panel still and the export,
  all pixel-consistent (SPEC "what you see is what you get"), plus keyframes while recording.
- **Decision:**
  - **Geometry** (`sc_geometry::camera`, mirrored in ui/src/lib/geometry.ts with one shared case
    table): `size_norm` is the shape's short side over the canvas short side; S/M/L = 0.18 / 0.26 /
    0.36; margin 0.03 of the short side for snapping and corner presets; zoom 1–4 from
    fill-the-shape; pan −1..1 inside the camera frame; corner radius as a fraction of half the
    short side (a circle is 1). The shape is always kept inside the canvas, so a position chosen
    for one frame shape is pulled in on another. Rounded = a rounded square.
  - **Compositor:** one camera layer (`sc_render::CameraLayer`, built by `camera_layer` for every
    consumer): rounded-rect signed distance in f32 quantized to 1/256 coverage, integer blending,
    an inner border and a soft shadow; the GPU blends by reading a copy of the canvas, so the CPU
    reference stays the golden (max difference 0 measured on the build Mac).
  - **Shared camera** (`sc_platform::SharedCamera`): one device session feeds the live preview and
    a recording, which attaches through the ordinary `CameraSource` trait (the record crate is
    unchanged). The app opens it while the panel is shown or a recording is on.
  - **Live frames:** the bubble and stage get compositor frames over `sc://live/<name>?after=<id>`
    (long poll, latest wins, raw premultiplied RGBA) drawn with WebGL2 — spike 2 mode B, ADR-007.
    The stage refreshes the source by snapshot twice a second; the camera is live.
  - **Keyframes:** `Session::camera_key` stamps the current recording time; the journal gets them
    in `camera_keys` batches at least every 200 ms (each append fsyncs); finalize and recovery
    write `camera_layout`. The app sends drags at most 30 times a second plus the final position.
- **Consequences:** a crash loses at most 0.2 s of camera motion. Keyframes are not editable until
  the M3 editor.
- **Update 2026-11-20 (CPU cut before the M2 hands-on):**
  - Live frames carry a format code in the header's fourth word: the Preview gets I420 (BT.709
    limited, what the encoder gets, 3/8 of the RGBA bytes), converted in its WebGL2 shader.
  - The renderer draws a frame only when the camera frame, camera key, background, source
    snapshot or window size changed; the snapshot refreshes once a second.
  - Camera frames that are IOSurface-backed `CVPixelBuffer`s (live camera, decoded camera track)
    are read in place through `CVMetalTextureCache` as `R8Uint` + `Rg8Uint` textures, so the M3
    import (SPEC "Frames into wgpu") is in use for the camera; output is bit-identical to the
    upload.
  - Measured (release, camera at about 25 fps): 29.5% of one core for the app plus its WebKit
    processes, against the 15% budget; 4.5% without a camera. About 5 points of that is macOS's own
    camera pipeline inside the app's process.
  - Superseded on macOS the same day by the native layer (ADR-007 update 2026-11-20): about 18%.
- **Update 2026-11-20 (review fixes before the owner's second hands-on):**
  - **Keyframe easing is decided when a key is recorded** (M2_PLAN §3.6): a key that arrives more
    than 100 ms after the previous one is a `Step` (the camera holds its place until then and
    jumps), a key within a drag (sent about every 33 ms) stays `Linear` (the motion is replayed).
    Keys are timed when the app sends them, not when the session's control thread gets to them.
    Before this, every key was `Linear`, so the export slid the camera across each idle stretch.
  - The end of a drag is always sent as a key (it was sometimes dropped by the 30 Hz throttle).
  - A camera attachment detaches when dropped, so a recording that fails to start no longer
    leaves the shared camera "recording"; a recording camera that fails to open gives the preview
    its camera back; an unplugged camera is closed (`DeviceLost`) and reopened when the devices
    change.
  - The export holds the camera's first frame until its first sample (frame 0 had no camera).
  - Settings are written under one lock; the app keeps `bar_at`/`preview_at` itself (the panel's
    copy no longer overwrites them) and announces changes in save order. The Preview's position
    is saved off the main thread once the window comes to rest, and the automatic first
    placement is not saved as the user's.
  - The panel tells the app it is closing when it hides itself to record, so the camera and the
    Preview turn off after that recording; the panel's still uses the camera key it shows.
  - Picking Rounded gives the camera rounded corners (it kept the circle's radius 0).

## ADR-043: The Preview window replaces the floating camera bubble

- **Status:** Accepted (owner's M2 hands-on, 2026-11-20: "Having the camera bubble as its own
  preview and the preview window w/ camera placement is confusing. The preview window is better.")
- **Decision:** there is no floating camera bubble on screen. One Preview window (the former stage)
  shows the composed output for every source and frame shape and is where the camera is placed,
  sized and reframed. It opens with the recorder panel and stays during a recording, with or
  without a camera; "Show the Preview window" in the panel (or its close button) turns it off; it
  remembers where it was moved. It is excluded from capture like every app window.
- **Consequences:** "the bubble matches the output" (SPEC) is met by showing the output itself.
  The bubble window, its renderer, `bubble_drag`/`bubble_state` and `OverlayRole::Bubble` are
  removed; `bubble_shown` in settings is read as `preview_shown`.

## ADR-044: A macOS look for every window

- **Status:** Accepted (owner 2026-11-20: "make the UI look better… more like a Mac tool-level UI
  in terms of refinement, fit and finish").
- **Decision:**
  - One design system in `ui/src/app.css`:
    - macOS semantic colours for light and dark, and the system accent colour (`AccentColor`) with
      white text on it;
    - controls drawn at AppKit sizes: push buttons, segmented controls, popup buttons with the
      blue chevron box, switches, checkboxes, sliders and search fields;
    - System Settings-style grouped forms (`.group` / `.grow`).
  - A small set of SF Symbols-style line icons (`ui/src/components/Icon.svelte`, inline SVG; no
    icon font or new dependency).
  - The panel, Saved, Permissions and switcher windows are laid out as grouped forms and dialogs:
    - the panel ends in a footer with the chosen source and a red Record button;
    - Saved and Permissions hide their window titles (`hiddenTitle`), since their pages carry the
      header.
  - Native materials where windows float: `sc_capture_macos::material` puts an
    `NSVisualEffectView` under the transparent webview:
    - the HUD material (always dark) behind the control bar, as a rounded pill;
    - the popover material behind the source switcher.

    It makes those windows non-opaque through public AppKit API; Tauri's `macOSPrivateApi` stays
    off (ADR-026).
  - The panel, Saved, Permissions and Preview windows keep the standard title bar.
- **Alternatives:** a component library (new dependency, still not native); Tauri's
  `windowEffects` (needs `transparent` windows); real AppKit controls (a rewrite of the UI).
- **Consequences:**
  - The browser mock shows a solid stand-in for the materials (`:root[data-native]` marks the app).
  - MANUAL_TEST uses the new labels: Region…, Click a Window, System…, Allow…, Show in Finder,
    Export Video.
  - Windows and Linux get the same CSS and no native material.

## ADR-045: M3 editor: edit model, playback, preview and export of edits
- **Status:** Accepted by the agent on 2026-11-21 under the owner's instruction "Build the rest of
  it out. I'll test when i get a chance." (no ★ stop between M3 and M7). Open to change at the
  owner's review.
- **Decision:**
  - **Edit model** (`sc_core::edit`): every edit is a pure function from one document (EDL, text,
    camera layout, audio mix, output canvas) to the next. Undo/redo keeps whole-document snapshots
    (a few KB each, unlimited); a slider or handle drag passes a coalesce key so it is one undo
    step. Edits that would empty the timeline are refused. Text moves with ripple edits: deleted
    ranges remove the text inside them and cut text across their edges; insertions shift text that
    starts at or after the insertion point; speed changes rescale text inside the clip.
  - **Seeking:** `Decoder::open_video_at` / `open_audio_at`. On macOS an AVAssetReader time range
    starting at the seek time; tested to start on the frame shown at that time even across a
    2.7 s gap with no frames, and AAC continues sample-for-sample (max difference < 0.01).
  - **New crate `player`** (`sc_player`): `TimelineAudio` (all audio tracks through the clips,
    per-track gain and mute, clip mute, pitch-kept speed with Signalsmith Stretch, a 5 ms
    raised-cosine fade on both sides of every cut that is not a continuation), `TimelineVideo`
    (the screen frame, camera frame and placement at any timeline time) and, with the `output`
    feature, playback through cpal. The editor preview, thumbnails and every export use the same
    two types, so the preview and the export agree.
  - **Playback clock** (ADR-025 as built): the frames the output device has played, minus the
    device's reported latency, plus the time since the last callback. Without an output device, a
    wall clock. Video picks the frame for that time; audio runs 250 ms ahead in a ring buffer.
  - **Editor window:** a normal (not panel) window, excluded from captures like every window of
    ours (`OverlayRole::Editor`). The app turns into a regular Dock app while it is open and back
    into a menu-bar app when it closes (not under the driver). The preview is drawn by the
    compositor into the shared native layer (`sink.rs`, the Preview window's ADR-007 path) at the
    preview's rectangle in the page; elsewhere webview frames.
  - **One `editor` command** with an `action` (open, state, edit, undo, redo, copy, cut, paste,
    play, pause, seek, rect, export, close, background_image) instead of 15 commands.
  - **Autosave:** project.json is rewritten 400 ms after the last edit, and on close, export and
    when another project opens.
  - **Thumbnails and waveform** are made in the background and stored in the project's `thumbs/`
    folder (SPEC: the project folder holds thumbnails), indexed by recording time so no edit makes
    them stale. Thumbnails: one per second, or fewer for long recordings (at most 400), made
    coarse-to-fine. Waveform: one byte per 10 ms, the loudest sample over the audio tracks on a
    square-root scale. Served as `sc://thumb/<step>/<n>.png` and `sc://peaks/v1`. Driver runs
    delete `thumbs/` with the media.
  - **After recording** the editor opens by default; a panel setting ("When Recording Stops: Open
    Editor / Show Summary") brings back the Saved window. The tray gains "Edit Last Recording";
    the Saved window an "Edit…" button.
  - **Export of edits:** the quick export (x264 MP4) renders and mixes the edited timeline. The
    full export dialog is M5.
  - **Shortcuts:** Space play/pause; K pause, L play, J back 2 s (no reverse playback); ←/→ one
    frame, ⇧←/⇧→ one second; ↑/↓ previous/next cut; S split; I/O range; ⌫ delete (ripple); ⌘X/C/V
    cut/copy/paste at the playhead; ⌘D duplicate; ⌘Z/⇧⌘Z undo/redo; ⌘A select all clips; ⌘+/⌘−
    or ⌘-scroll zoom; Shift-drag a range.
- **Dependencies added** (all in the SPEC tech stack): `cpal` 0.18.2 (Apache-2.0),
  `signalsmith-stretch` 0.1.3 (MIT; its C++ library is MIT; builds with `cc` and `bindgen`, which
  CI already has for Skia). Linux CI installs `libasound2-dev`. `rubato` was not needed: the
  decoders resample.
- **Not built / limits:** reverse shuttle (J jumps back); trimming shows the new length while
  dragging but the preview frame only after release; the waveform is one combined lane, not one
  per track; editor open time was measured only on short recordings.

## ADR-046: M4 text overlays as built
- **Status:** Accepted by the agent on 2026-11-21 under the owner's "Build the rest of it out"
  (no ★ stop); open to change at the owner's review.
- **Decision:**
  - **Fonts:** the ADR-020 final list (13 families, 20 files, Noto Color Emoji COLRv1) is
    committed in `assets/fonts` with license files (11 MB) and bundled in `Resources/fonts`.
    Weights are variable-font instances; static families pick the nearest face; an italic falls
    back to upright when a family has none. Installed families come through fontdb. Font fallback
    is off (deterministic output); missing characters are counted and the Text tab warns.
  - **Layout:** Skia paragraphs in 1080 units (1080 = the canvas's short side), wrapped at 90% of
    the canvas width, rasterized at the output scale times the item's scale, so text is crisp at
    any export size. The box layer and the text layer are separate images; reveals copy text tiles
    over the full box.
  - **Styles:** solid or two-stop linear gradient fill, outline, drop shadow, glow (two blurred
    strokes), per-line or merged (TikTok) boxes, inverted (box in the text colour, text in the box
    colour), uppercase, opacity, alignment, line height, letter spacing (1080 units).
  - **Animations** (pure functions of the time into the item, so every fps samples the same
    curve): in fade, pop, slide ×4, typewriter, word by word, bounce; out fade, pop, slide ×4;
    loops pulse, wiggle, float; durations capped at half the item; easings linear, ease in/out,
    ease in and out, overshoot.
  - **Compositing:** `TextLayer` (premultiplied image + affine + opacity) in the compositor, drawn
    after the camera by a WGSL pass with integer bilinear sampling and blending, mirrored by the
    CPU reference. GPU vs CPU max difference 2 (f32 coordinate maths may differ by one 1/256 step).
  - **UI:** Text tab, ten presets (Classic, Outline, Box, Inverted, Neon, Headline, Typewriter,
    Handwritten, Elegant, Marker), six recent styles (per-viewer local storage), the macOS
    eyedropper (`NSColorSampler`; elsewhere hidden), direct manipulation on the preview with
    centre/thirds/5% margin guides and 0/±15/±90° rotation snaps, and typing on the preview through
    a transparent text area that uses the bundled fonts served as `sc://font/<file>`.
- **Limits:** RTL and complex scripts untested; inline typing shows the browser's caret over
  Skia's glyphs, so the caret can sit a little off on fonts the page has to substitute (system
  fonts); per-character colours are not supported.

## ADR-047: M5 export as built
- **Status:** Accepted by the agent on 2026-11-21 under the owner's "Build the rest of it out"
  (no ★ stop); open to change at the owner's review.
- **Decision:**
  - **Settings** (`sc_export::settings`): format (MP4, WebM, GIF, MOV), video and audio codec
    chosen separately within the container (CODEC_MATRIX §2.4), encoder Best or Fast, quality
    Small / Balanced / High / Custom (CRF or kbps), fps from the per-format list, size (Original,
    a height class, a typed width or height, or a long side), upscale off by default, GIF settings
    (gifski quality, motion quality, lossy; palette colours and dither; loop forever / once / N),
    output folder and a file name template (`{title}`, `{date}`, `{time}`, `{size}`, `{fps}`, `{format}`).
    `normalized()` repairs a codec pair the container cannot hold; `less_compatible()` names the
    choices the UI labels.
  - **FPS:** the effective fps is the largest list value not above min(chosen, recording fps)
    with a 0.5 fps tolerance (Kap's rule), so a 30 fps recording never exports at 60.
  - **Recipes** (`sc_export::encode`): every video recipe tags BT.709 through `setparams` and the
    output flags; VP8 uses BT.601 (ADR-029); HEVC is tagged `hvc1`; MP4 and MOV get `+faststart`;
    ProRes is `yuv422p10le`. Custom kbps uses `-b:v`/`-maxrate` with a 2× buffer.
  - **GIF:** gifski reads Y4M on stdin. Palette mode runs two passes over the rendered frames:
    pass 1 writes a 16×16 raw RGB32 palette (the helper has no PNG decoder), pass 2 maps the frames
    onto it, so ffmpeg never buffers the whole clip (the single-pass graph needed about 1.9 GB per
    minute, CODEC_MATRIX §2.5). Frame delays alternate so the total length stays exact.
  - **Probe:** probe v1 records which encoders open on this machine (`lavfi` test pattern and
    tone); the export sheet only offers those. A probe cached before this field existed is redone.
  - **Sheet** (`app/src/exports.rs`, `ExportSheet.svelte`): settings remembered per format and
    the last format; named presets; an estimate (size range, time, output size, warnings for less
    compatible choices and the 4096 px hardware H.264 limit) refreshed as settings change;
    progress and Cancel in the editor toolbar; then Show in Finder, Copy (the file on the
    pasteboard) and Open.
- **Verified:** `export_formats` exports every offered combination and checks codec, size, frame
  count, duration, audio codec, `hvc1`, colour tags and the file extension; GIF frame delays at
  10, 15 and 25 fps from both GIF modes. `export_quality`: on a text-heavy clip x264 Best scored
  SSIM 0.9365 at 134 KB against VideoToolbox Fast 0.9341 at 233 KB. Live driver flow 17 exports
  MP4, a 240p GIF and a VP9 + Opus WebM from the editor.
- **Limits:** playback in Chrome, Firefox and VLC is still a manual check (MANUAL_TEST §10e);
  animated WebP/APNG (nice-to-have) are not offered; Windows and Linux helper builds come in M6.


## ADR-048: M6 Windows and Linux backends, helper tracks, our own helpers, packaging, self-test
- **Status:** Accepted by the agent on 2026-11-21 under the owner's "Build the rest of it out"
  (no ★ stop). **Needs the owner's OK** on the two architecture changes marked ⚑ below, which
  depart from the SPEC's media table.
- **Decision:**
  - ⚑ **Raw tracks on Windows and Linux go through the FFmpeg helper process**
    (`crates/tracks`), not Media Foundation MFTs or the LGPL FFmpeg libraries in-process. Video
    is written at a constant rate: a frame lands in the slot nearest its time and the previous
    image fills skipped slots, so frame `k` is at `k / fps` and readers need no index. Matroska
    with clusters of at most 1 s, flushed as written: a killed app leaves the helper to read EOF
    and finish the file; a killed helper leaves a file readable up to its last cluster (tested).
    The encoder is the first that opens on this machine: NVENC, QSV, AMF or Media Foundation
    (hardware only) on Windows, VAAPI or NVENC on Linux, else x264 (`veryfast`, `ultrafast`
    above 1440p). Audio is AAC. Reading back also goes through the helper (NV12 frames, f32 WAV),
    with `-hwaccel auto` and input seeking. Why: one code path for both OSes that can be tested
    on every OS (it passes on macOS with VideoToolbox and x264, on Linux in CI and Docker), no
    LGPL libraries to ship and load, and the hardware encoders still used through the helper.
    Cost: frames cross a pipe (NV12, about 370 MB/s at 4K30), and the same paths stay as
    capture → NV12 copies on the CPU.
  - **Windows** (`crates/capture-windows`): Windows.Graphics.Capture for displays, windows,
    regions and app crops (the crop is copied out of the frame on the GPU, then mapped and
    converted to NV12 on 4 threads; a resized window letterboxes into its first size), Media
    Foundation source readers for cameras (NV12, decoding MJPEG through MF), WASAPI mic and
    loopback system audio through cpal, `WDA_EXCLUDEFROMCAPTURE` on every app window, the QPC
    clock, `IFileOpenDialog`, `CF_HDROP` copy and Recycle Bin. Permissions read the privacy
    ConsentStore; Windows has no prompt API for desktop apps.
  - ⚑ **Linux** (`crates/capture-linux`): screens and windows come only from the
    xdg-desktop-portal ScreenCast picker (the app's System… button and a single "Choose Screen or
    Window…" button in the panel); frames arrive over PipeWire (BGRx/RGBx shared memory). The app
    hides its own windows while recording (exclusion `AutoHide`). Cameras through V4L2 (NV12 or
    YUYV; MJPEG-only modes are not offered yet). Mic and system audio through cpal's PipeWire host
    (system audio is a sink capture). The portal FileChooser and OpenDirectory, `gio`/freedesktop
    Trash. Raw tracks use x264 until a VAAPI helper is safe (VAAPI would make the static helper
    need libva to start at all).
  - **Shared** (`crates/capture-common`): cpal audio for both OSes and a polling device watcher.
  - **Helpers:** `third_party/ffmpeg/build-linux.sh` builds the Linux helper natively and the
    Windows helper with mingw-w64, from the same pinned sources as macOS, `--disable-network`,
    checking that only system libraries are linked (Linux: glibc family; Windows: KERNEL32,
    USER32, ADVAPI32, SHELL32, ole32, bcrypt, msvcrt). This settles LICENSES §3.3's networking
    question without BtbN builds. gifski is built for both. `scripts/stage-helpers.sh` stages
    helpers and every license text for the bundle.
  - **Packaging** (`.github/workflows/package.yml`): helpers per OS (cached), a source archive of
    the GPL/AGPL helpers' pinned sources and build scripts, then `.dmg`, NSIS `.exe` + `.msi`, and
    `.deb` + `.AppImage`, each with the helpers and the `licenses` resource folder. The Windows and
    Linux jobs first run the helper-track round trip with the built helper.
  - **Self-test** (`crates/diagnostics`, `simple-capture --self-test`, Diagnostics window): what
    the machine offers (capture APIs, permissions, devices, helpers and their network status,
    encoders, a recording-encoder benchmark), a synthetic record → cut + text → MP4 and GIF
    export, and 2 s of the main display recorded, decoded and exported when the screen grant is
    already there (it never asks; Linux skips it because the portal would ask). The report goes
    to the app data folder with the home folder redacted and holds no recordings.
- **Verified:** Windows code type-checks from macOS (cross `cargo clippy`) and builds and runs
  its tests on GitHub's Windows runner; Linux builds and runs its tests on Ubuntu (CI) and in
  `tools/linux-dev` (PipeWire consumer: 36 frames in 2 s from a GStreamer source, converted
  correctly). The self-test passes 7/7 on this Mac from the CLI and from the app.
- **Not verified:** any real Windows or Linux desktop (no machine here). The portal start never
  reached frames in the headless test session (xdg-desktop-portal-wlr could not read a format
  from wlr_screencopy on headless sway); the PipeWire side it hands over to is tested.

## ADR-049: M7 hardening and polish as built
- **Status:** Accepted by the agent on 2026-11-21 under the owner's "Build the rest of it out";
  open to change at the owner's review.
- **Decision:**
  - **Pause and resume:** while paused, the recorder's sink drops every frame and buffer, so the
    raw tracks hold the last frame and silence over the pause (the file clocks keep running).
    Pauses are journaled; the finished project lists them and starts with clips that leave the
    paused spans out, so the editor and every export skip them, and the person can still drag a
    paused stretch back in. Recording time is host time minus start, pauses included: tracks,
    segments, camera keys and the EDL share one clock (`ProjectV1::duration_ns` no longer
    subtracts pauses; nothing recorded before had pauses). A pause's silence is not reported as
    an audio gap. Recovery cuts pauses the same way.
  - **Recording bar:** Pause/Resume, Restart (discard this take, start again with the same
    request, after a confirmation), Stop, Discard; a 3-2-1 countdown before the start (setting:
    none, 3, 5 or 10 s; Escape or Cancel); optional frame rate, drops and drift. The timer and
    the tray title leave paused time out; the tray has Pause Recording.
  - **Shortcuts** (global): ⌥⇧R start/stop, ⌥⇧P pause/resume, ⌥⇧S switch source, ⌥⇧C camera
    on/off from now on, ⌥⇧O open the recorder. Not configurable yet.
  - **Library window:** every recording newest first, with a poster (the composed first second,
    rendered on demand and cached in the project's thumbs folder), date, length and size; Edit,
    Export (opens the editor's export sheet), Show in Finder/Explorer/Files, Move to Trash after a
    confirmation. Deleting always goes to the Trash or Recycle Bin, never straight to deletion.
  - **When Recording Stops** can also export with the export sheet's last settings. An opt-in
    setting (off by default, per the SPEC) moves the raw recording to the Trash after an export
    made from the summary or right after recording; editor exports never do.
  - **About window:** version, the no-network statement, and the license texts of the app, the
    FFmpeg and gifski helpers (with every library inside them), the fonts and the Rust crates.
    **Diagnostics window:** runs the self-test (ADR-048) and offers its report.
  - **Microphone level** in the recorder panel while it is open and nothing records.
- **Already in place from M1–M2 and kept:** crash recovery at launch with the recovered notice
  in the panel (now also a Recovered/Damaged badge in the Library), the low-disk notice and the
  automatic stop when the disk fills, device hot-plug notices and fallbacks.
- **Not done:** customizable shortcuts; a separate performance pass beyond the M2 CPU work (the
  30-minute soak numbers in STATUS stand); per-OS onboarding text for Windows and Linux beyond
  what the permission screens already say.
