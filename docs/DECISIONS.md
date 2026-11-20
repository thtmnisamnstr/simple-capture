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
- **Status:** **Conditional, condition not yet met** (owner 2026-11-19: "Yeah, but only if you can figure out ways to do this in Windows and Linux"). Until native paths are proven on real Windows and Linux hardware, **every OS, macOS included, uses webview frames** behind `PreviewSink`. See the 2026-11-19 update below.
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
  - Fragment interval vs. overhead is still to tune: at 1 s, loss stays under 1 s.

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

