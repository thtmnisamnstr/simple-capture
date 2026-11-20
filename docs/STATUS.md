# Status

**Current milestone: M1 ★ (macOS recording): in progress.** M0 was approved by the owner on
2026-11-19. M1 now also includes two new requirements: switching the source while recording, and an
output aspect ratio (16:9, 4:3, 1:1, 9:16) with a letterbox background (SPEC → Capture).
Pre-work is done (below); the M1 build plan is being written (docs/M1_PLAN.md).
Updated 2026-11-19. Nothing is committed yet.

**M1 pre-work results (2026-11-19, each re-checked by an independent verifier):**
- **Native preview on Windows/Linux** ([spike 5](../spikes/05-native-preview-windows/REPORT.md),
  [spike 6](../spikes/06-native-preview-linux/REPORT.md)): Windows plausible but unproven (compiles,
  never run on Windows); Linux works on Wayland and only partly on X11 (software rendering in
  Docker). The owner's condition is not met yet, so **webview frames on every OS** (ADR-007).
- **System pickers** ([research](research/pickers-and-hotswap.md)): all three OSes have one, so
  the condition is met and a "System picker…" source is added everywhere (ADR-028).
- **Switching source while recording on macOS** ([spike 3b](../spikes/03-macos-pipeline/REPORT-swap.md)):
  works. Starting the new stream before stopping the old one gives a 0–28 ms gap with no wrong
  frames. System audio must move to its own stream, and audio gaps must be filled with silence
  (ADR-034, ADR-035).
- **Fonts** ([report](../spikes/04-skia-text/REPORT-fonts.md)): 3 of 13 did not display correctly
  and were replaced: Fredoka → Baloo 2, Permanent Marker → Sedgwick Ave Display, DM Serif Display →
  Abril Fatface (ADR-020).
- **Aspect ratio and background** ([design](research/canvas-and-source-switching.md)): the output
  becomes a canvas; each source is fit inside it; the background is a colour, gradient or image
  (ADR-033).

**Owner decisions on 2026-11-19** (recorded in DECISIONS.md and SPEC.md): Svelte 5 yes (ADR-005);
native preview and the system picker only if they also work on Windows and Linux (ADR-007, ADR-028,
conditional); keep the fonts but drop any that don't display correctly (ADR-020); VP8 BT.601
accepted on our recommendation (ADR-029); the two extra crates yes (ADR-030); Windows/Linux FFmpeg
built by us with networking disabled (ADR-013); no Apple Development identity for now (ADR-003
update: development runs through the Simple Capture Dev host). Every other Proposed ADR was
accepted with the M0 approval.

All numbers come from an Apple M3 Pro (18 GB, macOS 26.7.2). **Nothing was measured on the base M1
8 GB target.** The "constrained" proxies turned out to be noisy or did not constrain anything, so
treat every performance number as an upper bound until there is an M1 run.

---

## 1. What's done

| Item | Where |
|---|---|
| Spec (source of truth) | [docs/SPEC.md](SPEC.md) |
| Architecture | [docs/ARCHITECTURE.md](ARCHITECTURE.md) |
| Decisions (32 ADRs; the Proposed ones need your OK) | [docs/DECISIONS.md](DECISIONS.md) |
| Codec matrix, licenses | [docs/CODEC_MATRIX.md](CODEC_MATRIX.md), [docs/LICENSES.md](LICENSES.md) |
| Third-party attribution | [docs/THIRD_PARTY.md](THIRD_PARTY.md) (Kap, markus-perl, fonts, spike 3 note) |
| Research: Cap recording | [docs/research/cap-recording.md](research/cap-recording.md) |
| Research: Cap render/decode | [docs/research/cap-render-decode.md](research/cap-render-decode.md) |
| Research: Cap editor/export | [docs/research/cap-editor-export.md](research/cap-editor-export.md) |
| Research: Cap MIT crate audit | [docs/research/cap-mit-crates.md](research/cap-mit-crates.md) |
| Research: Kap | [docs/research/kap.md](research/kap.md) |
| Research: ecosystem versions, APIs, licenses | [docs/research/ecosystem.md](research/ecosystem.md) |
| Helper build tooling (pinned, hash-verified) | [third_party/README.md](../third_party/README.md), `third_party/versions.toml` |
| Permission dev host (one app, never renamed) | `tools/macos-dev-host/`, rules in [CLAUDE.md](../CLAUDE.md) |
| Spike 1: export helpers | [REPORT.md](../spikes/01-helpers/REPORT.md); raw: [results/latest.md](../spikes/01-helpers/results/latest.md) |
| Spike 2: preview transport | [REPORT.md](../spikes/02-preview-transport/REPORT.md); raw: `results/verify-r1..r3`, `verify-con1..2`, `evidence/*.png` |
| Spike 3: macOS pipeline | [REPORT.md](../spikes/03-macos-pipeline/REPORT.md); raw: `out/bench-quiet{,2,3}/summary.json` |
| Spike 4: Skia text | [REPORT.md](../spikes/04-skia-text/REPORT.md); raw: [out/bench-quiet.md](../spikes/04-skia-text/out/bench-quiet.md), `out/*.png` |

**Spike reports.** The agent harness would not let subagents write `REPORT.md`, so the
consistency pass returned drafts built only from files on disk plus its re-measurements, and the
orchestrator saved them verbatim. Their numbers were spot-checked against the saved files.

What the consistency pass re-measured (2026-11-19, 23:13–23:30 UTC, load average about 3.5):
- Spike 1: VideoToolbox H.264/HEVC size probe at 15 sizes, a 30 s clip run (x264, SVT-AV1, VT
  H.264, twice), and a SIGTERMed gifski partial. Results are in CODEC_MATRIX §2.1–2.2 and ADR-015.
- Spike 4: the quiet bench three more times (`--no-png`), and a timed clean Skia FreeType build in a
  scratch target folder (ADR-018, ADR-021).
- The raw outputs of these re-runs went to a scratch folder, not the repo. The numbers are quoted in
  the docs and in the REPORT drafts.

**Spike 3 evidence is now committable.** `spikes/03-macos-pipeline/.gitignore` still ignores the
recordings in `out/` (770 MB of real screen, camera and mic), but un-ignores every `summary.json`,
`analysis.json`, `psnr.txt`, `project.json` and run log (64 files, about 470 KB), so every [S3]
number has a source in the repo after a commit. `out/old-prehost/` stays ignored.

**Naming request ("stop giving the capture spike apps new names").** No spike app gets a new name
or bundle ID (ADR-002, CLAUDE.md):
- Spike 3 now runs only under `Simple Capture Dev` (`com.simplecapture.dev`). Its own Info.plist and
  camera `.app` were deleted, and three rebuilds caused no new prompts.
- Spike 2 still carries `productName` "Preview Transport Spike" and identifier
  `com.simplecapture.spike.preview` in `src-tauri/tauri.conf.json`. It was deliberately **not**
  renamed again: its release build embeds no Info.plist and asks for no permission. Fixed at
  close-out:
  - The `"tauri"` script in `package.json` now refuses to run, because `pnpm tauri dev` would embed
    an Info.plist with that name (a new app name to macOS).
  - `bench.sh` skips the window snapshot by default. `SC_SNAPSHOT=1` launches the binary through
    `tools/macos-dev-host/run.sh`, so the snapshot uses the Simple Capture Dev grant. Checked:
    `c-native-540` under the host, 60.5 fps, snapshot "video visible, HTML over it".
  - `run.sh` now forwards `SC_*` and `RUST_*` environment variables (`open --env`) and waits for
    the exit code if a child exits before `open -W` can attach.
- Spikes 1 and 4 need no permission.
- The old `com.simplecapture.spike.<name>` convention is retired. The M0 workflow's task text
  still mentioned it; that workflow is finished, and future agent briefs use the dev-host rule from
  CLAUDE.md and ADR-002.

---

## 2. Spike verdicts

Each spike was re-run by an independent verifier on a quieter machine; all four verdicts were
"partially confirmed". The verifiers' corrections were not saved to disk, so the consistency pass
re-read every number below from the saved result files, re-measured where a claim had no source,
and corrected the docs to match. All four pass on the technical question.

| Spike | Verdict | Key verified result | Main corrections / caveats |
|---|---|---|---|
| 1 Helpers | **Pass** | Static FFmpeg 9.0.2 (GPL-2.0+, no network) and gifski 1.34.0. All 29 outputs pass ffprobe, 3 runs out of 3. 1080p30: x264 medium 197 fps, x265 120, SVT-AV1 p8 189, VP9 64, VideoToolbox H.264 191 / HEVC 177. gifski 41 fps at 960×540. Cancel is clean with no leftover processes. | The original speed table was taken under load (x264 was reported at 122.8 fps). The 4-thread proxy is noisy. FFmpeg palette GIF runs at about realtime (14.2 fps median), not "faster". A SIGTERMed gifski partial is **truncated** (no trailer, last frame undecodable) but ffprobe reads 18–20 frames from it; the earlier word "corrupt" had no saved evidence and was re-checked in this pass. VideoToolbox H.264 (hardware, `-allow_sw 0`) fails when **either dimension exceeds 4096**: only 8192×4320 had been probed; 4098×2160, 5120×2880, 6016×3384, 7680×2160 and 2160×4098 were probed in this pass. The 30 s figures: VT H.264 207–208 fps confirmed; "SVT-AV1 153.6 fps" did **not** reproduce (182–185). |
| 2 Preview | **Pass, keep Tauri** | Every transport: 58.5–60.1 of 60 fps, including full 1080 through the webview. Native CAMetalLayer under the webview: 60.1 fps, about 20% CPU, HTML composites on top. | The constrained pass did not constrain anything. 2D canvas costs 1.3–1.7× the WebKit GPU-process CPU of WebGL2 (1.3× at 540, 1.7× at 720 and 1080), not 2×. The URI pull (B) is cheaper than the Channel (A) at 1080. "0 drops" on the native path holds by construction. Photon latency, WebKitGTK and WebView2 are untested. |
| 3 macOS pipeline | **Pass** | SCK, camera, mic and system audio each write a fragmented HEVC/AAC .mov. A SIGKILL loses ≤0.86 s and every file stays readable. All clocks are on host time. Decode is zero-copy into wgpu (39 µs import). HEVC encodes 46 fps at 7680×2160. | 60 fps at full native size reached only 44.5 fps. Whole-app UI exclusion fails under the dev host (per-window exclusion works). Keyframe gaps reach 2.8 s on idle screens. Mic sync is unverified. The "Documents hang" did not reproduce. Writer "not ready" drops were 0 in two quiet runs but **6 camera frames in `bench-quiet3`** (capture drops 0 in all three); earlier docs said "0". Only display and single-window capture were tested: region, application, "only this app" audio, device hot-plug, permission onboarding and Mic Mode were not (§3 item 14). Summary evidence under `out/` is now committable (§1). |
| 4 Skia text | **Pass** | Every text style plus colour emoji works. GPU and CPU outputs differ by at most 1/255 (saved quiet run, three re-runs and the E-core proxy all agree). Sum over 11 items, warm/cold: layout 0.44–0.45 / 0.55–0.58 ms, raster 1080p 3.91–4.27 / 9.10–9.67 ms, raster 4K 15.2–15.9 / 26.1–26.7 ms (`out/bench-quiet.md` plus three re-runs in this pass). | Stock macOS skia-safe (CoreText) cannot draw bundled emoji, so Skia needs a FreeType source build. Build time: the saved log (`target/ft-build.log`, machine under load) says 7 min 55 s; a clean build in this pass took 2 min 42 s wall (skia-bindings build script 144 s). The earlier "2 min 54 s" and the verifier's numbers (layout 0.21 ms, raster 3.82 / 14.5 ms, "identical in all 4 runs") were never saved and are withdrawn. The typewriter animation was broken by a Skia m157 quirk and is fixed. `FORCE_SKIA_BUILD`, not `SKIA_GN_ARGS`, forces the source build. |

---

## 3. Open risks

1. **No M1 data.** Encoder headroom, preview CPU, memory and helper speed are all M3 Pro numbers.
   The constrained proxies (4 threads, E-cores only, CPU hogs) were noisy or ineffective.
2. **60 fps at native Retina size is not sustainable**, even on the M3 Pro (44.5 fps delivered at
   7680×2160). The default stays 30 fps, and 60 fps is gated by the probe (ADR-009).
3. **Own-UI exclusion.** Whole-app filtering fails under the dev host. M1 must re-test it in the
   signed `com.simplecapture.desktop` app, and keep the per-window filter up to date as windows
   change.
4. **A/V sync is unproven.** Only screen flash to system-audio onset was measured (39–58 ms).
   The mic did not hear the test sounds. The flash/beep/clap test is an M1 deliverable.
5. **SCK buffer starvation.** Cap copies every frame into its own pool. We have not done that and
   saw no problem in 10 s runs, but long recordings are untested.
6. **Pause** is designed (ADR-012) but untested.
7. **Skia FreeType build on Windows and Linux** is untested, and it is the first CI risk.
8. **Linux preview on WebKitGTK** and **Windows WebView2** are untested. Real Linux hardware is
   needed (see owner actions).
9. **macOS 15+ periodic screen-capture re-prompt and `SCContentSharingPicker`: open, needs owner
   input (ADR-028).** M0 did desk research only. Spike 3 never used the picker, and the re-prompt
   was never observed (it needs weeks of wall-clock time and a stable signed app). No decision
   exists on adopting the picker, offering it as an optional source path, or rejecting it, nor on
   "pick a display in the picker, then crop" for regions. ADR-028 proposes a default and an M1 test.
10. **Licensing loose ends:**
    - gifski's `gifsicle` feature (lossy LZW port) has unreviewed provenance: `src/gifsicle.rs` in
      the pinned crate has no licence header, and gifsicle upstream is GPL-2.0.
    - The gifski release bundle is missing notice texts: only `gifski.LICENSE` (AGPL-3.0) is
      collected. libimagequant 4.4.1 (GPL-3.0-or-later, plus the pngquant/Poskanzer-Roelofs notice
      in its `COPYRIGHT` file) and about 50 permissive crates (`gifski.deps.txt`) have no texts in
      `third_party/out/` yet (LICENSES §3.2, §5).
    - BtbN's Windows minimum (10 22H2) is above the spec's (10 2004), and its builds probably
      contain network protocols.
    - The FFmpeg GPG signature is unverified (no gpg).
    - The spec says "OFL fonts", but two suggested fonts are Apache-2.0.
11. **Helper gaps:**
    - The PNG decoder is silently missing (zlib is disabled).
    - There is no AV1 decoder.
    - gifski x86_64 has not been built.
    - The Small and High quality tiers are untested.
12. **objc2-app-kit default features** link CloudKit and CoreData into the binary. Trim them in M1.
13. **M0 deliverable gaps:** closed at close-out (reports saved, spike 3 evidence un-ignored,
    spike 2 snapshot routed through the dev host, `pnpm tauri` guarded; §1).
14. **M1 sources that M0 did not test.** Spike 3 tested display capture, single-window delivery
    and own-UI exclusion only. These spec items are untested and are real work, not porting:
    - region capture (`SCStreamConfiguration.sourceRect` on a display stream vs. a full-display
      capture cropped later; Retina scale and odd sizes);
    - application capture (an `SCContentFilter` built from an app's windows; new windows appearing
      mid-recording);
    - system audio "Only the captured app" (SCK filters audio per app, not per window
      [ecosystem]), and Core Audio process taps as the alternative;
    - camera and mic lists that update on hot-plug (device connect/disconnect notifications);
    - reading Screen Recording, Camera and Mic status **without prompting** for onboarding
      (spike 3 read camera/mic status with `authorizationStatusForMediaType`, and spike 2 used
      `CGPreflightScreenCaptureAccess`, but no onboarding flow was built or tested);
    - the macOS "Mic Mode" button (`showSystemUserInterface(.microphoneModes)`);
    - click highlight: `showMouseClicks` needs macOS 15, so on 14 we draw clicks ourselves from
      `cursor.jsonl`. Which one to use is undecided.
15. **Dev host and `~/Documents`.** Spike 3 once saw `run.sh` hang on a hidden "Documents folder"
    prompt. At close-out, `run.sh` ran a binary with its working folder inside this repo without
    any prompt, so the grant is in place. Spike 3's bench still stages binaries in a temp folder,
    which is harmless.

---

## 4. What to look at now

Allow about 20 minutes. Nothing here needs a rebuild or a new permission.

1. **Read** the Proposed ADRs in [DECISIONS.md](DECISIONS.md): 005, 006, 007, 009, 012, 013
   (**three** sub-points: Windows floor, PNG/AV1 decoders, and the BtbN **networking** exception),
   016–021, 023–026, 028 (SCContentSharingPicker), 029 (VP8 BT.601 exception to Pitfall 5),
   030–032 (new crates, dev-profile opt-level, project-folder naming), plus the Proposed parts of
   010 (frame copy) and 011 (start gate).
2. **Recordings (spike 3).** Open these in QuickTime:
   - `spikes/03-macos-pipeline/out/bench-quiet/main/screen.mov` and `camera.mov`.
   - The killed recording, `spikes/03-macos-pipeline/out/bench-quiet/kill/screen.mov`. It should
     still play.

   Check colour, sharpness and that our test window is not visible.
3. **Text (spike 4).** Look at these PNGs in `spikes/04-skia-text/out/`:
   - `composite_gpu_1080p.png`: all text styles.
   - `emoji_matrix.png`: why CoreText fails.
   - `anim_typewriter.png` and `anim_words.png`.

   Sign off on the merged-box shape and the emoji look.
4. **Preview (spike 2).** `spikes/02-preview-transport/evidence/c-native-1080.png` shows HTML
   drawn over the native Metal layer.
5. **Export outputs (spike 1).** Open the files in
   `spikes/01-helpers/results/run-20261119T214545Z/out/` in QuickTime and Chrome. They are
   synthetic test patterns.
6. **Optional re-runs** on your machine, from the repo root, on a quiet machine:
   - `spikes/01-helpers/bench.sh` (about 2.5 min).
   - `spikes/04-skia-text/bench.sh` (14 s when already built).
   - `spikes/02-preview-transport/bench.sh` (about 4 min; opens a test window per mode). Add
     `SC_SNAPSHOT=1` to also check that HTML draws over the video; it runs under Simple Capture
     Dev.
   - `spikes/03-macos-pipeline/bench.sh spikes/03-macos-pipeline/out/bench-owner` (about 3 min).
     This records your real screen, camera and mic, and runs under Simple Capture Dev, which
     already has its grants.

---

## 5. Owner actions required

| # | Action | Why / when |
|---|---|---|
| 1 | **Create a free Apple Development signing identity**: open Xcode 27.2 → Settings → Accounts → add your Apple ID → Manage Certificates → "+" → Apple Development. Opening Xcode once also runs its first-launch setup. | **Before M1.** Without it, every build of `com.simplecapture.desktop` is a "new app" to macOS, and you would re-approve permissions after each build (ADR-003). |
| 2 | Approve or reject each **Proposed** ADR, especially Svelte 5 (005), the native preview using a private WebKit key (007), the Skia source build (018), the font list (020) and the GIF "Fast" rename (017). | Before M1 starts |
| 4 | Choose: raise the spec's Windows minimum to **10 22H2**, or keep 10 2004 and record a helper limitation (BtbN). | Before M6 |
| 4b | Choose: build our own Windows and Linux FFmpeg with `--disable-network`, or accept BtbN's general-purpose builds, which probably include network protocols, as a documented exception to "no networking code" (ADR-013 point 3). | Before M6 |
| 5 | Run the benches on a **base M1 8 GB** if you can get one (spikes 1–4 bench commands above). | Before M2 (encoder headroom) and M3 (preview, memory) |
| 6 | Arrange a **physical Linux machine** (Intel/AMD iGPU, Wayland and X11) for the WebKitGTK preview bench. | End of M1, so a GPUI fallback decision does not land in M6 |
| 7 | **Do not** rebuild or re-sign `~/Applications/Simple Capture Dev.app` (`install.sh --force`). It holds the Screen Recording, Camera and Microphone grants. | Always |
| 8 | If macOS ever shows "Simple Capture Dev would like to access files in your Documents folder" again, allow it. Otherwise `run.sh` from this repo can hang. | Only if it appears |
| 9 | Review the Small and High quality tiers on a real screen recording (ADR-016). | Before M5 |
| 10 | Decide the **SCContentSharingPicker** question (ADR-028): adopt it, offer it as an optional source path, or reject it, and whether "pick a display, then crop" is acceptable for regions. | Before M1 source work (M1 plan item 3) |
| 11 | OK or reject the **VP8 BT.601 exception** to Pitfall 5 (ADR-029), and the architecture additions in ADR-030–032 (new `geometry`/`preview` crates, dev-profile `opt-level = 3`, `.sc` project-folder naming). | Before M1 starts |

---

## 6. M1 plan outline (macOS recording) ★

Goal (spec): sources, permissions, separate crash-safe tracks, the project file, a quick x264 MP4
export, the sync test, and a CI skeleton for all three OSes.

1. **Workspace skeleton** (ARCHITECTURE §1):
   - `core`, `geometry`, `platform`, `capture-macos`, `record`, `helpers`, `export` (minimal),
     `app`, `ui`, `cli`.
   - Placeholder Windows and Linux capture crates that compile.
   - justfile targets.
2. **Signing.** Sign the app as `com.simplecapture.desktop` with the Apple Development identity,
   using the hardened runtime and the camera and mic entitlements.
3. **capture-macos.** Only part of this is a port of spike 3; the rest is new, untested work
   (§3 item 14), so each item below gets its own test before it is called done:
   - *ported:* display and window sources; per-window exclusion of our UI, re-testing whole-app
     exclusion in the signed app; trimmed objc2 features;
   - *new, untested in M0:* application source; region source (decide `sourceRect` vs. crop
     after capture with a spike-sized test first); system audio Off / All / App (per-app audio
     filtering); camera and mic lists that update on hot-plug; the Mic Mode button;
   - *new, depends on owner input:* the `SCContentSharingPicker` path (ADR-028) and the click
     highlight method (`showMouseClicks` on 15+, our own drawing on 14).
4. **record:**
   - start gate, bounded queues with drop counters, and writer threads;
   - fragmented `.mov` tracks with 1 s fragments;
   - stop extension, `journal.jsonl`, and atomic `project.json` v1;
   - disk-space guard and finalizing on device unplug.
5. **Recovery scan on launch** (state `recording` → `recovered` / `damaged`), plus a kill-test in
   the test suite using the dev host.
6. **Soak test:** 30 min at native resolution + camera + mic + system audio. Measure drops, memory
   and A/V offsets. Decide the SCK frame-copy question (ADR-010).
7. **Sync test:** flash + beep + clap, measured at both the start and the end. Set any
   system-audio compensation from it.
8. **Quick export:**
   - Composite the screen layer only (the camera bubble comes in M2).
   - Pipe Y4M to the bundled FFmpeg: x264 medium CRF 22 + aac_at, `.part` then rename, cancel.
   - Validate with ffprobe.
9. **Capability probe v1:** helper hashes, VideoToolbox H.264/HEVC size limits, and native HEVC
   headroom at the capture size, which gates 60 fps.
10. **Minimal UI:** recorder panel, permission onboarding including the macOS 15 re-prompt note,
    and a floating stop bar. Onboarding needs a status check that never prompts
    (`CGPreflightScreenCaptureAccess`, `AVCaptureDevice.authorizationStatus`); untested as a flow.
11. **CI** on macOS arm64, Windows x64 and Ubuntu x64:
    - build, clippy, tests;
    - cargo-deny and the pnpm license check;
    - cached helper builds and cached Skia FreeType builds (the Windows FreeType build is the first
      thing to prove).
12. **Docs:** MANUAL_TEST.md (macOS section) and STATUS updates.

At the end of M1, stop for owner review (★).
