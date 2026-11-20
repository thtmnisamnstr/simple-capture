# M1 build plan: macOS recording ★

Written 2026-11-19 for a team of parallel coding agents; revised the same day after two critics'
reviews (what changed and why: §13). This plan changes no accepted decision: docs/SPEC.md is the
source of truth, docs/DECISIONS.md holds the decisions, and docs/ARCHITECTURE.md is the build
reference ("ARCHITECTURE §n" below). Where a research note or spike report has an "Independent
verification" section, its corrected figures are used. Choices this plan makes on its own are
listed in §10 so wp-docs-close can log them.

How to read this file:
- §0 how the team works: owner gates, worktrees, waves, merges, shared machine resources.
- §1 scope, what is not in M1, the exit checklist. §2 rules every package follows.
- §3–§6 are the **contracts** the stage 0 scaffold creates and every later package codes against:
  layout, Rust interfaces, CLI, app commands and events, the debug driver, test tiers.
- §7 spike code to port. §8 work packages with full briefs.
- §9 dependencies (with fallbacks if the owner says no), §10 plan-level choices, §11 owner
  questions, §12 risks, §13 revision log.

Nothing here has been built or run. Every number quoted comes from the M0 spikes on the M3 Pro
(no M1 8 GB data).

---

## Owner answers (2026-11-19) — every gate is open

- **Q1, Q3 dependencies and tools:** approved, all of them (crates, npm dev tools, GitHub
  actions, `cargo install` of cargo-deny/cargo-about, the ubuntu:24.04 Docker image).
- **Q2a, Q2b git:** any git operations needed: branches, commits, worktrees, merges, rebases, pushes
  to origin and PRs. Isolation model A. The repo is public: never commit recordings or secrets.
  The owner's words: "I want results that are stellar fast and economically."
- **Q7 soak:** an agent may run the 30-minute soak on the owner's real screen, camera, mic and
  system audio (dev host, files deleted after the numbers are taken). The owner keeps using the
  Mac meanwhile, so **nothing may steal focus** (CLAUDE.md "Don't steal the owner's focus").
- Decided by the orchestrator (owner said "you figure this out" style; logged in DECISIONS):
  - **Q4:** plain folder with the `.sc` suffix (not a macOS package).
  - **Q5:** click highlight uses `showMouseClicks` on macOS 15+ only (hidden on 14); our own click
    drawing and `cursor.jsonl` clicks wait for M7.
  - **Q6:** the quick export caps the long side at 3840 (the full-size Original export comes with
    M5's resize presets); wp-export still measures x264 at 7680×2160 for the record.
  - **Q8:** asked at the M1 review (only the owner can run the never-granted release bundle).
  - **Q9:** PNG and JPEG backgrounds only; WebP waits.
  - **Q10:** if the Skia FreeType build fails on the Windows runner, Windows CI may temporarily use
    skia-safe's prebuilt binaries; log it in STATUS as an open item.

## 0. How the team works

### 0.1 Owner gates

No package starts while a question it depends on is open. The orchestrator writes each answer into
the affected briefs ("Owner answers: …") before starting them, so no agent sees an open question.

| Gate | Question (§11) | Blocks |
|---|---|---|
| G1 | Q1 dependencies | wp-scaffold (it writes `[workspace.dependencies]`; §9.4 lists the fallback for every "no") |
| G2 | Q2a local commits and worktrees | wp-scaffold (chooses isolation model A or B, §0.2) |
| G3 | Q3 local tools, Q10 Skia on Windows CI | wp-ci |
| G4 | Q4 project folder form | wp-core |
| G5 | Q2b pushing to GitHub | wp-ci-green (local equivalents otherwise) |
| G6 | Q7 agent 30-min soak | wp-soak-test's 30-min run (else the owner runs it from MANUAL_TEST) |

Q5, Q6, Q8 and Q9 only change details inside a package and can be answered while stage 1 runs;
until then the briefs use the recommended default.

### 0.2 Isolation model

**Model A (default; needs Q2a = yes).**
1. The orchestrator creates a local branch `m1` from `main`. wp-scaffold runs in the main working
   tree on `m1`; the orchestrator commits it locally (no push).
2. Every later package works in its own git worktree, `<repo>/.wt/<id>` (gitignored, inside the
   repo so the dev host's existing Documents-folder grant covers it), on branch `m1-<id>` created
   from the `m1` commit that already contains all of the package's `depends_on` merges.
3. Target directories: a package that does not link Skia sets
   `CARGO_TARGET_DIR=<repo>/target/wt/<id>`, so it never waits on another agent's build lock.
   Packages that link Skia (wp-text, wp-export, wp-cli, wp-app-shell, every stage 2/3 package) share
   `CARGO_TARGET_DIR=<repo>/target/skia`, which wp-scaffold warms, so the 3–8 min FreeType build
   (ADR-018) happens once; they accept Cargo's build lock between them. If Skia rebuilds per
   worktree anyway, the package reports it.
4. Merges: the orchestrator merges finished package branches into `m1` in wave order (§0.3) and
   runs `just ci-local-fast` after each merge (fmt check, clippy, tests). Owned paths are
   disjoint, so merge conflicts can only come from frozen files, which nobody edits.
5. `Cargo.lock` belongs to the orchestrator. Every cargo command runs with `--locked` (the justfile
   and scripts pass it). If a feature a package enables would add a crate to the lockfile,
   `--locked` fails and the package files a change request.
6. Formatting: packages run `rustfmt --edition 2024` on the `.rs` files they own, never
   `cargo fmt --all`; the orchestrator runs `cargo fmt --all --check` at merge.

**Model B (Q2a = no).** One working tree and one `target/`. Packages run strictly wave by wave;
inside a wave each package keeps its crate compiling after every edit-build cycle (larger rewrites
go into a new private module that is wired in last), and Cargo's build lock serialises builds
(expect slower waves). Rules 5 and 6 above still apply.

### 0.3 Waves

Each package's `depends_on` lists every package whose code its done criteria exercise.

| Wave | Packages | Why this order |
|---|---|---|
| 0 | wp-scaffold | creates everything |
| 1 | wp-core, wp-geometry, wp-platform-synth, wp-ui, wp-ci | need only the scaffold |
| 2 | wp-mac-sck (geometry), wp-mac-av (platform-synth), wp-decode, wp-text (geometry), wp-render (geometry, platform-synth), wp-helpers (core), wp-record-recovery (core, platform-synth), wp-cli (platform-synth) | |
| 3 | wp-record-session (core, geometry, platform-synth, mac-av), wp-export (core, geometry, platform-synth, mac-av, decode, render, text, helpers), wp-app-shell (platform-synth, mac-sck) | |
| stage 2 | wp-e2e-cli, wp-app-integration | integration against real crates |
| stage 3 | wp-sync-test, wp-soak-test, wp-kill-recovery-test, wp-manual-test, wp-ci-green, then wp-docs-close | verification |

wp-cli and wp-app-shell finish against whatever their dependencies provide at that point (stubs
return `Unsupported`); every criterion that needs real recording, export or recovery is in
wp-e2e-cli and wp-app-integration.

### 0.4 Shared machine resources

Live tests share one screen, one set of speakers, one mic and one camera with each other and with
the owner. `scripts/lib/live.sh` (frozen, scaffold) wraps every live run (test-live, e2e, sync,
soak, killtest, the app-integration driver runs):
- **Lock:** `mkdir target/live.lock` (macOS has no `flock(1)`), holding the pid; a lock whose pid is
  dead is stale; waits up to 30 min, then fails. Only one live run at a time on the machine.
- **Awake:** `caffeinate -dimsu -w $$` for the length of the run.
- **Screen check:** `sc-devtools envcheck` (no permission: `CGSessionCopyCurrentDictionary`
  screen-locked flag and `CGDisplayIsAsleep`) fails with exit 3 and "needs an unlocked, awake
  screen" before anything starts.
- **Fixture windows** (testapps) sit at floating level so other windows cannot cover them, and
  pixel checks look only inside fixture-window rects.
- Non-live work (builds, synthetic tests, the UI mock in the Browser pane) continues in parallel.
  Scripts print a banner when a live run starts and ends so the owner can stay off the screen.

### 0.5 Fixes after stage 1

- Stage 2: wp-e2e-cli may fix any non-frozen file under `crates/**` and `tools/devtools/**`, with a
  regression test in the owning crate's test files. wp-app-integration reports crate bugs to the
  orchestrator, which routes them to wp-e2e-cli.
- Stage 3 packages never edit crates outside their `owns` list. A bug they find goes to the
  orchestrator, which reopens the owning stage-1 package as `<id>-fix` with a narrowed brief, merges
  it, and reruns the stage-3 check.

---

## 1. Scope

### In M1 (SPEC milestone line, STATUS §6, ADR-033/034/035)

- **Workspace skeleton** with every crate below, compiling and tested on macOS, Windows and Linux.
- **macOS sources:** any connected display, window, application, region, and "System picker…"
  (`SCContentSharingPicker`, ADR-028), including "pick a display, then set a region" (ADR-028
  update). Own UI excluded on every stream: our registered windows by ID plus every window our
  process owns at stream start and when a menu opens (ADR-010, §10 item 9).
- **Permissions onboarding:** Screen Recording, Camera and Microphone status read **without
  prompting**; prompts only on an explicit click; ad-hoc re-prompt and macOS 15 re-prompt explained.
  Window and app lists are requested only after Screen Recording reads "granted".
- **Separate crash-safe tracks:** screen as segments (`tracks/screen-NNN.mov`, ADR-034); camera,
  mic and system audio as one continuous fragmented `.mov` each (ADR-008); all on the host clock
  with the start gate (ADR-011); `session.log` per project.
- **Switching source while recording:** make-before-break, routed by stream identity, journaled,
  with a first-frame timeout and the stop-then-start fallback (ADR-034, §10 item 12). Floating-bar
  button, tray item, ⌥⇧S hotkey and pick mode. A closed captured window or unplugged display ends
  its segment, holds the last frame and prompts "Switch source · Stop".
- **System audio** from its own audio-only SCStream; every audio timestamp gap filled with silence
  and every overlap trimmed, so audio stays on host time (ADR-035, §10 item 15).
- **project.json v1** with `recording.screen.segments` and `output.canvas`; `journal.jsonl`; atomic
  writes; recovery scan on launch and in the CLI (ARCHITECTURE §6, §12).
- **Canvas:** aspect Original / 16:9 / 4:3 / 1:1 / 9:16, contain fit per segment, background solid /
  linear / radial / image (PNG, JPEG) rasterized by Skia (ADR-033).
- **Quick export:** x264 medium CRF 22 + `aac_at` 160k MP4 through the bundled macOS FFmpeg helper,
  composited on the canvas at the reference size, `.part` then rename, cancel, ffprobe validation.
- **Capability probe v1** with the probe-size rule of §10 item 11.
- **Crash-safety parts of the disk guard and hot-plug** that STATUS §6 items 3–4 put in M1: free
  space checked before start, graceful stop before the disk is full; device lists update on
  hot-plug; a lost device finalizes its track. (SPEC's M7 line polishes both.)
- **Sleep, quit and signals:** sleep stops and finalizes the recording, with a notice after wake;
  Quit, SIGTERM and logout stop gracefully and cancel a running export (§10 items 13–14).
- **CLI** that drives a headless recording (scripted switches, duration, graceful stop), recovery,
  export, probe and validation. Every automated live test runs through it under the dev host.
- **A/V sync test** (screen flash + beep automated for system audio and mic; camera through
  full-screen flashes where detectable; clap assisted), **soak test** (30 min decides ADR-010),
  **kill/recovery tests**.
- **Minimal UI:** tray (Open Recorder, Start/Stop Recording, Switch Source…, Quit; elapsed time in
  the menu-bar title), recorder panel (source, Frame with the small-content warning, background,
  camera, mic with "Mic Mode…", system audio, fps including "60 fps at 1×", resolution, cursor,
  click highlight per Q5), onboarding, region overlay, pick-mode overlay, floating bar (timer, stop,
  switch source, discard with confirmation, notices including the source-lost prompt and a
  frames-dropping hint), switch popover, "saved" window with quick export and Reveal.
- **Release bundle config** (ad-hoc signature, hardened runtime, camera and audio-input
  entitlements, usage strings, signed sidecars), built and inspected but never launched by agents.
- **CI skeleton** for macOS arm64, Windows x64 and Ubuntu x64; **justfile**;
  **docs/MANUAL_TEST.md** (macOS section).

### Not in M1

| Item | Where it goes |
|---|---|
| Pause/resume, the 3-2-1 countdown, start/stop and pause hotkeys, the bar's Restart button | M7 (SPEC M7 line: "pause/resume, countdown, hotkeys"). ⌥⇧S switch stays in M1 (ADR-034). `Session::pause/resume` exist and return `Unsupported`; `RecClock` pause maths, the schema's `pauses` and the journal `pause`/`resume` events exist, so M7 needs no migration. ADR-012's points (re-stamp the newest frame dropped during the pause at resume, rewrite project.json at pause/resume, sync "including pauses", Pitfall 2) become M7 test items, recorded by wp-docs-close. |
| Camera bubble, live stage preview | M2 (the camera **track** is recorded in M1) |
| Editor, `player`/`preview` crates, zero-copy IOSurface → wgpu import | M3 (§10 item 1) |
| Text | M4 (`text` only rasterizes backgrounds) |
| Other export formats | M5 |
| Windows/Linux backends and helpers | M6 (placeholders compile and pass tests in M1) |
| Library, recovery UI beyond a notice, hotkey settings, segment rollover on window resize, disk-guard and hot-plug polish | M7 |
| Mic level meter (SPEC Audio, floating bar) and debug overlay (SPEC Engineering) | SPEC v1 but on no milestone line. This plan proposes M7 (polish and performance pass); wp-docs-close records the proposal in STATUS for the owner. |
| `cursor.jsonl`, our own click drawing | after Q5 (recommended: M7) |
| `updateConfiguration` in-place region moves | not used in M1 (§10 item 2) |

### M1 exit checklist (what the ★ review looks at)

1. `just run` starts the app under the Simple Capture Dev host with no new permission prompt.
2. Every source records (display, including a non-main display when one is connected; window;
   application; region; picker in MANUAL_TEST); every switch path works (bar, tray, ⌥⇧S, pick
   mode, picker: through the debug driver for agents, by hand in MANUAL_TEST); segment files tile
   the timeline; audio is continuous across switches; a closed window is handled as source lost.
3. Every aspect × background kind exports to a valid MP4 that plays in QuickTime with the right
   letterbox; colour patches decode within 3/255.
4. Kill tests at the three points of ARCHITECTURE §12 recover; a killed app recovers on next
   launch; quitting or SIGTERM during a recording finalizes the project (state `finalized`, not
   `recovered`).
5. Sync numbers at the start, after each switch and at the end, for system audio and mic against
   the screen flash, and for the camera where the flash is detectable (otherwise the report says
   "camera not automated" and the clap step in MANUAL_TEST is the camera check). Target: within one
   frame; any fixed system-audio offset is measured, not assumed. Sync across pauses is M7.
6. Soak: the ADR-010 frame-copy decision rests on a **30-minute** run, by an agent (Q7 = yes) or by
   the owner from MANUAL_TEST with the same metrics and thresholds: screen drops < 0.5 %, RSS growth
   ≤ 50 MB from minute 2 to the end, no `StreamStopped`, end sync within one frame, audio drift
   (count-based vs host time) reported per track. 10-minute agent runs are regression checks only.
7. CI green on all three OSes (Q2b), or the local equivalents of §6.4 with the exact blocker.
8. `cargo deny` and the pnpm license check pass; no networking code (source grep, cargo-deny bans,
   Tauri CSP, `--disable-network` in the helper's buildconf); only approved dependencies.
9. Release bundle built; `codesign` and `plutil` show the hardened runtime, both entitlements, the
   three usage strings and signed sidecars (not launched by agents).
10. docs/MANUAL_TEST.md macOS section; STATUS, DECISIONS, ARCHITECTURE updated from the package
    reports.

---

## 2. Rules for every package

1. **Permissions.** Any binary that needs Screen Recording, Camera or Microphone runs through
   `tools/macos-dev-host/run.sh <binary> [args]` (CLAUDE.md). Never create an `.app`, Info.plist or
   bundle ID to get a permission; never rebuild or re-sign `Simple Capture Dev.app`; never call
   `tccutil`, edit TCC databases or open System Settings. Never launch the bundled
   `Simple Capture.app` (it prompts as `com.simplecapture.desktop`); only the owner does that.
   Never click or call a permission "Request" path in a test.
2. **TCC tripwire.** In debug and test builds (`cfg(debug_assertions)`), every TCC-touching call in
   `sc_capture_macos` (SCShareableContent, SCStream start, SCScreenshotManager, the picker,
   AVCaptureSession/device open, every `request*` API) returns
   `Err(PermissionDenied/Other("TCC tripwire: run through tools/macos-dev-host/run.sh"))` unless
   `SC_LIVE=1` or `SC_DEV_HOST=1` is set (run.sh exports `SC_DEV_HOST=1`). Status reads
   (`CGPreflightScreenCaptureAccess`, `authorizationStatusForMediaType`), CoreGraphics display
   queries, AVAssetWriter/Reader and VideoToolbox are not TCC-touching. Plain `cargo test` can
   therefore never raise a prompt.
3. **Live tests** live in integration-test targets whose names start with the package's prefix
   (`sck_`, `av_`). They are either `#[ignore = "live"]` tests in a normal target, or bodies in the
   `sck_live` / `av_live` targets (harness = false, §4.4), which print `SKIPPED:` and pass unless
   `SC_LIVE=1`. Run them with `just test-live <package> <target-prefix>`, which selects test
   binaries by target name and **fails when 0 tests ran**.
4. **Ownership.** A package edits only the paths in its `owns` list. Frozen files (§3) change only
   through a change request.
5. **Frozen interfaces.** The public types and trait signatures in §4–§5 are created by the scaffold.
   A package may add private items and new public helpers in its own crate. Changing a frozen
   signature, adding a dependency or feature to the root manifest, or changing `Cargo.lock` is a
   **change request**: write it at the top of the final report (what, why, which packages it
   affects) and stop short of making it.
6. **Dependencies.** Only crates in root `[workspace.dependencies]` (§9). A crate manifest is frozen
   too; enabling a feature that adds no crate is a one-line change request the orchestrator applies
   at merge. No networking crates, no HTTP plugins, no git dependencies, no `std::net`.
7. **Docs.** Packages do not edit `docs/` (except wp-manual-test and wp-docs-close). Decisions,
   measurements and open questions go into the final report; every number names the file or command
   output it came from.
8. **Git.** As §0.2. No pushes except wp-ci-green after Q2b.
9. **Quality bar.** `rustfmt` on owned files; `cargo clippy --locked -p <package> --all-targets --
   -D warnings`; `cargo test --locked -p <package>`; the report gives "ran N tests" for every test
   command. No `todo!()`, `unimplemented!()` or `unwrap()` on fallible OS calls in library code;
   errors carry context. Bounded queues wherever frames flow (Pitfall 13).
10. **Threading.** Follow the threading contract in §4.3. Nothing pumps a run loop except the CLI's
    main thread and the live-test runner; nothing blocks the app's main thread.
11. **Cap and Kap.** Cap is read for design only; no Cap code. Our own spike code may be ported.
12. **Final report** (returned text, not a file): what was built, commands and results with test
    counts, measurements with sources, decisions taken (for DECISIONS.md), change requests, gaps.

---

## 3. Workspace layout and crate graph

```
simple-capture/
├─ Cargo.toml              [frozen] members, [workspace.dependencies], profiles, lints
├─ Cargo.lock              [orchestrator]
├─ rust-toolchain.toml     [frozen] 1.95.0, clippy rustfmt,
│                          targets aarch64-apple-darwin x86_64-pc-windows-msvc x86_64-unknown-linux-gnu
├─ .cargo/config.toml      [frozen] [env] FORCE_SKIA_BUILD=1, SKIA_GN_ARGS="skia_use_freetype=true
│                          skia_use_system_freetype2=false skia_enable_fontmgr_custom_empty=true" (ADR-018)
├─ justfile                [frozen] final target list; each target delegates to a script (below)
├─ deny.toml  about.toml   (wp-ci) from LICENSES §2.1–2.2
├─ .github/workflows/ci.yml  (wp-ci, then wp-ci-green)
├─ scripts/
│  ├─ test-live.sh         [frozen] select test binaries by target prefix, run each through run.sh
│  ├─ lib/live.sh          [frozen] lock, caffeinate, envcheck, fixture start/stop (§0.4)
│  ├─ ci/                  (wp-ci) lint.sh deny.sh licenses.sh npm-licenses.mjs ci-local.sh
│  │                       ci-local-fast.sh windows-check.sh linux-docker.sh netcheck.sh
│  ├─ e2e/run.sh           (wp-e2e-cli)      sync/run.sh (wp-sync-test)
│  ├─ soak/run.sh          (wp-soak-test)    killtest/run.sh (wp-kill-recovery-test)
├─ crates/
│  ├─ geometry/        simple-capture-geometry        lib sc_geometry
│  ├─ core/            simple-capture-core            lib sc_core
│  ├─ platform/        simple-capture-platform        lib sc_platform
│  ├─ capture-macos/   simple-capture-capture-macos   lib sc_capture_macos
│  ├─ capture-windows/ simple-capture-capture-windows lib sc_capture_windows (placeholder)
│  ├─ capture-linux/   simple-capture-capture-linux   lib sc_capture_linux   (placeholder)
│  ├─ record/          simple-capture-record          lib sc_record
│  ├─ decode/          simple-capture-decode          lib sc_decode
│  ├─ render/          simple-capture-render          lib sc_render
│  ├─ text/            simple-capture-text            lib sc_text
│  ├─ helpers/         simple-capture-helpers         lib sc_helpers
│  └─ export/          simple-capture-export          lib sc_export
├─ app/                simple-capture-app (Tauri 2.12.x; feature custom-protocol on by default)
│  ├─ tauri.conf.json          no externalBin, bundle.active = false, CSP (§5.2)
│  ├─ tauri.bundle.conf.json   bundle-only: externalBin ffmpeg + ffprobe, ad-hoc signing,
│  │                           hardened runtime, entitlements, usage strings (`just bundle --config`)
│  ├─ macos/entitlements.plist  macos/Info.bundle.plist
│  └─ icons/                   full generated set (icon.icns, icon.ico, PNGs, tray template)
├─ ui/                 Svelte 5 + TypeScript + Vite, pnpm
├─ cli/                simple-capture-cli, bin `simple-capture`
├─ tools/devtools/     sc-devtools (publish = false), bin `sc-devtools`: envcheck, testapps, sync,
│                      soak, killtest. Never shipped.
├─ tools/macos-dev-host/   run.sh extended by the scaffold (§8.0 item 9); host.c, Info.plist and
│                          install.sh untouched; the .app is never rebuilt
└─ third_party/            unchanged (helper build scripts)
```

All crates: `edition = "2024"`, `rust-version = "1.95"`, `license = "MIT"`, `publish = false`.
`[profile.dev.package."*"] opt-level = 3` (ADR-031). Lib names use the `sc_` prefix.

**Dependency graph** (our crates; no cycles):

| Crate | Depends on |
|---|---|
| geometry | – |
| core | geometry |
| platform | core, geometry |
| capture-macos / -windows / -linux | platform, core, geometry |
| text | core, geometry |
| helpers | core |
| record | platform, core, geometry; dev-dependency on capture-macos under `cfg(target_os = "macos")` |
| decode | platform, core, geometry |
| render | platform, core, geometry |
| export | render, decode, text, helpers, platform, core, geometry |
| cli | record, export (feature `export`, default on), helpers, platform, core, geometry + OS crate |
| app | everything above + OS crate |
| tools/devtools | record, decode, helpers, platform, core, geometry + capture-macos (macOS); no Skia |

Only `export`, `app` and `cli` (default features) link Skia. `capture-macos` is
`#![cfg(target_os = "macos")]` with its dependencies under `[target.'cfg(target_os = "macos")']`.
`cli --no-default-features` builds without Skia so its `cfg(not(macos))` code is checked locally
for Windows (§6.4).

**Frozen files created by the scaffold:** root `Cargo.toml`, `rust-toolchain.toml`,
`.cargo/config.toml`, `justfile`, `scripts/test-live.sh`, `scripts/lib/**`, every crate's
`Cargo.toml` and `src/lib.rs` (module declarations; every module of §4 pre-declared),
`crates/capture-macos/src/{util.rs, tcc.rs, mainloop.rs, platform_impl.rs}`,
`crates/capture-macos/tests/support/**`, `crates/record/src/lib.rs` re-exports,
`tools/devtools/{Cargo.toml, src/main.rs, src/envcheck/**}`, `tools/macos-dev-host/run.sh`.

---

## 4. Rust contracts (created by the scaffold, frozen)

Signatures below are normative for names, parameters and return types; the scaffold adds derives
(`Debug, Clone, PartialEq` where sensible, `serde` on data types), docs and private details. Stubs
return `Err(..Unsupported("<crate>: not implemented yet"))` or a valid empty value; no `todo!()`.
All time values are `i64` nanoseconds.

### 4.1 sc_geometry (pure; `serde` feature on by default)

```rust
pub struct SizePx { pub w: u32, pub h: u32 }
pub struct RectPx { pub x: i32, pub y: i32, pub w: u32, pub h: u32 }   // top-left origin
pub struct PointF { pub x: f64, pub y: f64 }
pub struct RectPt { pub x: f64, pub y: f64, pub w: f64, pub h: f64 }   // points, top-left origin
pub struct RectNorm { pub x: f64, pub y: f64, pub w: f64, pub h: f64 } // 0..1
pub struct Ratio { pub num: u32, pub den: u32 }                         // always reduced
pub enum Aspect { Original, R16x9, R4x3, R1x1, R9x16, R4x5 }
pub enum SizeClass { S2160, S1440, S1080, S720, S480, S360, S240 }
pub enum Handle { N, S, E, W, NE, NW, SE, SW, Move }

impl Aspect { pub fn ratio(self) -> Option<Ratio>; pub fn as_str(self) -> &'static str;
              pub fn parse(s: &str) -> Option<Aspect>; }
impl Ratio  { pub fn new(num: u32, den: u32) -> Ratio; pub fn of(size: SizePx) -> Ratio;
              pub fn as_f64(self) -> f64; }
pub fn even_round(x: f64) -> u32;                                      // 2·round(x/2), min 2
pub fn contain_fit(canvas: SizePx, content: SizePx) -> RectPx;         // ARCHITECTURE §8, both edges snapped
pub fn cover_src_rect(image: SizePx, dst: SizePx, focus: PointF) -> RectPt; // px units
pub fn natural_canvas(ratio: Ratio, content: SizePx) -> (f64, f64);
pub fn reference_canvas_size(aspect: Aspect, ratio: Ratio, segments: &[SizePx]) -> SizePx;
pub fn preset_size(class: SizeClass, ratio: Ratio, gif: bool) -> SizePx;
pub fn pt_to_px(r: RectPt, scale: f64) -> RectPx;
pub fn flip_y(r: RectPt, container_h_pt: f64) -> RectPt;               // Pitfall 3
pub fn snap_region_even_px(r: RectPt, scale: f64) -> RectPt;           // px size even
pub fn aspect_locked_resize(r: RectPt, ratio: Option<Ratio>, handle: Handle, to: PointF, bounds: RectPt) -> RectPt;
pub fn small_content(canvas: SizePx, content: SizePx) -> bool;         // dst.w < 25 % of canvas w or dst.h < 25 % of h
```

### 4.2 sc_core

```rust
pub mod error  { pub enum Error { Io(std::io::Error), Json(String), Invalid(String),
                   Locked(PathBuf), Unsupported(&'static str), Other(String) }
                 pub type Result<T> = std::result::Result<T, Error>; }
pub mod time   { pub struct HostTime(pub i64); pub struct RecTime(pub i64); pub struct TlTime(pub i64);
                 pub struct PauseSpan { pub start: HostTime, pub end: Option<HostTime> }
                 pub struct RecClock { pub start: HostTime, pub pauses: Vec<PauseSpan> }
                 pub enum ToRec { BeforeStart, Paused, At(RecTime) }
                 impl RecClock { pub fn to_rec(&self, h: HostTime) -> ToRec;
                                 pub fn pause_total_before(&self, h: HostTime) -> i64; } }
pub mod image  { pub struct RgbaImage { pub size: SizePx, pub stride: usize, pub data: Vec<u8> } } // premultiplied RGBA8, sRGB
pub mod project {
    // Every type of ARCHITECTURE §6 "ProjectV1 … TextItem", field for field, with serde rules that
    // reproduce the JSON (snake_case; tagged enums as shown). Each struct carries
    // `#[serde(flatten)] pub extra: serde_json::Map<String, Value>` so unknown fields survive.
    // Names: ProjectV1, ProjectState, Recording, ScreenSettings, ScreenSegment, StartReason,
    // EndReason, Via, SourceDescriptor, Canvas, Background, GradientStop, Rgba, Track, TrackKind,
    // VideoInfo, ColorInfo, AudioInfo, DeviceInfo, TrackHealth, GapRecord, CameraLayout, CameraKey,
    // Edl, Clip, Speed, AudioMixEntry, Output, ExportPresets, FontRef, TextItem, SystemAudioMode.
    // Deltas to the ARCHITECTURE §6 draft (wp-docs-close writes them back):
    //   SourceDescriptor::Application { display_id, app_bundle_id: Option<String>,
    //                                   app_pid: Option<i32>, app_name: Option<String>, scale }
    //   SourceDescriptor::Region.region_pt is DISPLAY-LOCAL points, top-left origin.
    impl ProjectV1 { pub fn new_recording(id: String, title: String, created_at: String,
                       app_version: String, clock: ClockKind) -> ProjectV1;
                     pub fn load(dir: &Path) -> Result<ProjectV1>;
                     pub fn save(&self, dir: &Path) -> Result<()>;           // atomic (fsio)
                     pub fn duration_ns(&self) -> Option<i64>;
                     pub fn segment_at(&self, t_rec: i64) -> Option<&ScreenSegment>; }
    pub fn copy_background_asset(src: &Path, project_dir: &Path) -> Result<Background>; // PNG/JPEG only
}
pub mod journal {
    #[serde(tag = "ev", rename_all = "snake_case")]
    pub enum JournalEvent {
        Start { start_host_ns: i64, clock: ClockKind, fps_requested: u32, resolution: String,
                system_audio: SystemAudioMode },
        SegmentPrepare { seg: String, file: String, host_ns: i64 },   // before a pre-created writer's file exists
        SegmentOpen { seg: String, track_id: String, start_rec_ns: i64, file: String, via: Via,
                      reason: StartReason, source: SourceDescriptor, frame_px: SizePx, content_px: RectPx },
        SwitchBegin { host_ns: i64, to: SourceDescriptor, via: Via },
        SegmentClose { seg: String, end_rec_ns: i64, reason: EndReason, trailing_dropped: u32 },
        SwitchFailed { host_ns: i64, error: String, next: SwitchNext },  // RetryStopThenStart | RestoreOld | SourceLost | KeepOld
        SourceLost { host_ns: i64, seg: String, reason: String },
        SourceResized { host_ns: i64, seg: String, content_rect_pt: Option<RectPt>, content_scale: Option<f64> },
        AudioGap { track: String, kind: GapKind, at_rec_ns: i64, dur_ns: i64 },  // GapKind: Gap | Overlap
        Pause { host_ns: i64 }, Resume { host_ns: i64 },                // written from M7; folded by recovery now
        Device { host_ns: i64, track_id: Option<String>, kind: DeviceKindJ, change: DeviceChange,
                 name: String, unique_id: String },                     // Camera|Mic; Lost|Connected|Disconnected
        TrackFinished { track_id: String, duration_ns: Option<i64>, samples: u64 },
        Health { host_ns: i64, rec_ns: i64, tracks: Vec<HealthEntry> }, // {track_id, frames, dropped, writer_not_ready, gaps}
        Stop { stop_host_ns: i64, reason: StopReason },                 // User|Duration|Signal|Quit|Disk|Sleep|Error
    }
    pub struct JournalWriter; impl JournalWriter {
        pub fn create(dir: &Path) -> Result<JournalWriter>;
        pub fn append(&mut self, ev: &JournalEvent) -> Result<()>; }   // write + fsync
    pub struct JournalRead { pub events: Vec<JournalEvent>, pub torn_tail: bool, pub unknown_lines: usize }
    pub fn read_journal(dir: &Path) -> Result<JournalRead>;
}
pub mod sessionlog { pub struct SessionLog; impl SessionLog {           // <project>/session.log, no tracing
        pub fn open(dir: &Path) -> Result<SessionLog>;                 // append; line-buffered + flush
        pub fn line(&mut self, level: Level, msg: &str) -> Result<()>; } // "<rfc3339> <host_ns> LEVEL msg"
    pub enum Level { Info, Warn, Error } }
pub mod fsio   { pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()>; pub fn fsync_dir(dir: &Path) -> Result<()>; }
pub mod folder { pub struct LocalDateTime { pub y: i32, pub mo: u8, pub d: u8, pub h: u8, pub mi: u8,
                                            pub s: u8, pub offset_min: i32 }
                 pub const PROJECT_SUFFIX: &str = ".sc";                // ADR-032; Q4 may change it
                 pub fn project_folder_name(t: &LocalDateTime, title: &str) -> String;
                 pub fn create_project_dir(root: &Path, t: &LocalDateTime, title: &str) -> Result<PathBuf>;
                 pub fn is_project_dir(p: &Path) -> bool;
                 pub struct ProjectLock; impl ProjectLock { pub fn acquire(dir: &Path) -> Result<ProjectLock>; } // Err(Locked) when held
                 pub fn rfc3339(t: &LocalDateTime) -> String; }
pub mod ids    { pub fn new_id() -> String; }        // UUID v4 text from OS-seeded std RandomState
pub mod sha    { pub fn sha256_file(p: &Path) -> Result<String>; pub fn sha256_bytes(b: &[u8]) -> String; }
pub enum ClockKind { MachHost, Qpc, ClockMonotonic }
```

### 4.3 sc_platform

**Threading contract (normative).** Every method of `ScreenSource`, `ScreenStream`,
`SystemAudioSource/Stream`, `AudioSource`, `CameraSource`, `DeviceStream`, `WriterFactory`,
`TrackWriter`, `Decoder` and the readers is callable from any non-main thread and **never pumps a
run loop**; it waits on completion handlers through channels or `dispatch2` semaphores with
timeouts. Only `SourcePicker`, `OverlayWindows`, `Desktop` and `Permissions::request` hop to the
main queue, and they require the host (Tauri's event loop, or the CLI/test runner calling
`sc_capture_macos::mainloop`) to be servicing it. `FrameSink` methods are called from capture
queues and must not block.

```rust
pub use sc_core::time::HostTime;
pub enum Error { Unsupported(&'static str), PermissionDenied(PermissionKind), NotFound(String),
                 Busy(String), Timeout(String), Os { code: i64, message: String },
                 Io(std::io::Error), Other(String) }
pub type Result<T> = std::result::Result<T, Error>;

pub struct StreamTag(pub u64);
pub struct WindowId(pub u32);          // CGWindowID on macOS
pub struct DisplayId(pub String);
pub struct AppRef { pub pid: i32, pub bundle_id: Option<String> }   // test apps have no bundle id

pub enum SourceSpec { Display { display: DisplayId }, Window { window: WindowId },
                      Application { display: DisplayId, app: AppRef },
                      Region { display: DisplayId, region_pt: RectPt },   // display-local points, top-left
                      Picked(PickedSource) }
pub struct PickedSource { pub descriptor: SourceDescriptor, pub handle: Arc<dyn Any + Send + Sync>,
                          pub region_pt: Option<RectPt> }   // "pick a display, then set a region"
pub struct DisplayInfo { pub id: DisplayId, pub name: String, pub frame_pt: RectPt /* global, top-left of main */,
                         pub scale: f64, pub size_px: SizePx, pub is_main: bool }
pub struct WindowInfo { pub id: WindowId, pub title: String, pub app_name: String,
                        pub app: AppRef, pub frame_pt: RectPt /* global */, pub display: Option<DisplayId>,
                        pub on_screen: bool, pub layer: i32,
                        pub z_index: u32 }       // 0 = frontmost, from CGWindowListCopyWindowInfo order
pub struct AppInfo { pub app: AppRef, pub name: String }
pub struct SourceList { pub displays: Vec<DisplayInfo>, pub windows: Vec<WindowInfo>, pub apps: Vec<AppInfo> }

pub enum Resolution { Native, OneX }
pub struct ScreenStreamConfig { pub fps: u32, pub resolution: Resolution, pub show_cursor: bool,
                                pub show_clicks: bool, pub queue_depth: u32, pub exclude: Vec<WindowId> }

pub trait NativeFrame: Send + Sync + 'static {
    fn as_any(&self) -> &dyn Any;
    fn size(&self) -> SizePx;
    fn with_nv12(&self, f: &mut dyn FnMut(Nv12View<'_>)) -> Result<()>;
}
pub struct Nv12View<'a> { pub size: SizePx, pub y: &'a [u8], pub y_stride: usize,
                          pub uv: &'a [u8], pub uv_stride: usize, pub full_range: bool }
pub struct CpuNv12Frame { pub size: SizePx, pub y: Vec<u8>, pub uv: Vec<u8>, pub full_range: bool } // impl NativeFrame
pub struct FrameBuffer(pub Arc<dyn NativeFrame>);
pub struct VideoFrame { pub stream: StreamTag, pub pts: HostTime, pub size: SizePx,
                        pub content_rect_pt: Option<RectPt>, pub content_scale: Option<f64>,
                        pub buffer: FrameBuffer }
pub trait NativeAudio: Send + Sync + 'static { fn as_any(&self) -> &dyn Any; }
pub enum AudioData { Native(Arc<dyn NativeAudio>), F32Interleaved(Arc<[f32]>) }
pub struct AudioBuffer { pub pts: HostTime, pub sample_rate: u32, pub channels: u16,
                         pub frames: u32, pub data: AudioData }
pub enum AudioTrackKind { Mic, SystemAudio }
pub enum DeviceKind { Camera, Mic }
pub enum SourceEvent { StreamStopped { stream: StreamTag, error: Option<String> },
                       SourceLost { stream: StreamTag, reason: String },
                       SourceResized { stream: StreamTag, content_rect_pt: Option<RectPt>, content_scale: Option<f64> },
                       DeviceLost { kind: DeviceKind, device_id: String },
                       DevicesChanged { kind: DeviceKind },
                       OwnWindowsChanged,                 // a menu or panel of ours opened: refresh exclusions
                       SystemWillSleep, SystemDidWake }
pub trait FrameSink: Send + Sync {
    fn on_screen(&self, f: VideoFrame); fn on_camera(&self, f: VideoFrame);
    fn on_audio(&self, track: AudioTrackKind, a: AudioBuffer); fn on_event(&self, e: SourceEvent);
}

pub trait ScreenSource: Send + Sync {
    fn displays(&self) -> Result<Vec<DisplayInfo>>;                // CoreGraphics/NSScreen only: never TCC
    fn list(&self, hide: &[WindowId]) -> Result<SourceList>;        // needs the grant; drops `hide` and
                                                                   // every window owned by this process
    fn describe(&self, spec: &SourceSpec, cfg: &ScreenStreamConfig) -> Result<(SourceDescriptor, SizePx)>;
    fn start(&self, tag: StreamTag, spec: &SourceSpec, cfg: &ScreenStreamConfig,
             sink: Arc<dyn FrameSink>) -> Result<Box<dyn ScreenStream>>;  // returns once started
    fn snapshot(&self, spec: &SourceSpec, max: SizePx) -> Result<RgbaImage>;
}
pub trait ScreenStream: Send {
    fn tag(&self) -> StreamTag;
    /// Excludes `exclude` ∪ every window this process owns now (fetched with onScreenWindowsOnly =
    /// false); waits for completion. Picked streams: Err(Unsupported) unless the rebuilt filter is
    /// known to keep picker consent (§10 item 10).
    fn update_exclusions(&self, exclude: &[WindowId]) -> Result<()>;
    fn stop(self: Box<Self>) -> Result<()>;
}
pub enum SystemAudioMode { Off, All, App }                          // = sc_core::project::SystemAudioMode
pub trait SystemAudioSource: Send + Sync {
    fn start(&self, mode: SystemAudioMode, exclude: &[WindowId], sink: Arc<dyn FrameSink>)
        -> Result<Box<dyn SystemAudioStream>>;
}
pub trait SystemAudioStream: Send {
    fn follow_app(&self, app: Option<&AppRef>) -> Result<()>;       // App mode: None = all
    fn stop(self: Box<Self>) -> Result<()>;
}
pub struct CaptureDevice { pub id: String, pub name: String, pub model_id: Option<String>,
                           pub is_default: bool, pub kind_hint: Option<String> }
pub trait DeviceStream: Send { fn format(&self) -> String; fn stop(self: Box<Self>) -> Result<()>; }
pub trait AudioSource: Send + Sync {
    fn devices(&self) -> Result<Vec<CaptureDevice>>;
    fn start(&self, device_id: &str, sink: Arc<dyn FrameSink>) -> Result<Box<dyn DeviceStream>>;
    fn show_mic_modes(&self) -> Result<()>;                         // main-queue hop
}
pub struct CameraLimit { pub max: SizePx, pub max_fps: u32 }        // default 1920×1080 @ 30
pub trait CameraSource: Send + Sync {
    fn devices(&self) -> Result<Vec<CaptureDevice>>;
    fn start(&self, device_id: &str, limit: CameraLimit, sink: Arc<dyn FrameSink>) -> Result<Box<dyn DeviceStream>>;
}
pub struct PickerRequest { pub exclude: Vec<WindowId> }            // modes are fixed per launch (§10 item 10)
pub trait SourcePicker: Send + Sync {
    fn present(&self, req: PickerRequest, done: Box<dyn FnOnce(Result<Option<PickedSource>>) + Send>);
    fn set_observer(&self, on_pick: Option<Box<dyn Fn(PickedSource) + Send + Sync>>); // menu-bar re-picks
    fn set_active(&self, active: bool) -> Result<()>;
}

pub enum VideoCodec { Hevc, H264 }
pub struct VideoWriterSpec { pub size: SizePx, pub fps: u32, pub codec: VideoCodec,
                             pub keyframe_interval_s: f64, pub fragment_interval_s: f64,
                             pub bits_per_pixel: f64 }               // colour always BT.709 limited
pub struct AudioWriterSpec { pub sample_rate: u32, pub channels: u16, pub bitrate: u32, pub fragment_interval_s: f64 }
pub enum Append { Written, NotReady }
pub struct FinishedTrack { pub path: PathBuf, pub duration_ns: Option<i64>, pub samples: u64 }
pub trait TrackWriter: Send {
    fn append_video(&mut self, f: &VideoFrame, file_ns: i64) -> Result<Append>;
    fn append_audio(&mut self, a: &AudioBuffer, file_ns: i64, skip_frames: u32) -> Result<Append>; // skip = trimmed overlap
    fn append_silence(&mut self, file_ns: i64, frames: u32) -> Result<Append>; // same format as last audio
    fn is_ready(&self) -> bool;
    fn finish(self: Box<Self>, end_file_ns: Option<i64>) -> Result<FinishedTrack>;
}
pub trait WriterFactory: Send + Sync {
    fn extension(&self) -> &'static str;                             // "mov" (macOS), "sctrk" (synthetic)
    fn video(&self, path: &Path, spec: &VideoWriterSpec) -> Result<Box<dyn TrackWriter>>; // file exists after this
    fn audio(&self, path: &Path, spec: &AudioWriterSpec) -> Result<Box<dyn TrackWriter>>;
    fn encode_benchmark(&self, size: SizePx, fps: u32, frames: u32, codec: VideoCodec) -> Result<f64>;
}
pub struct TrackProbe { pub readable: bool, pub duration_ns: Option<i64>, pub samples: u64,
                        pub size: Option<SizePx>, pub error: Option<String> }
pub struct DecodedFrame { pub file_ns: i64, pub frame: FrameBuffer }
pub trait VideoReader: Send { fn size(&self) -> SizePx; fn next_frame(&mut self) -> Result<Option<DecodedFrame>>; }
pub trait AudioReader: Send { fn first_sample_file_ns(&self) -> i64; fn read(&mut self, out: &mut [f32]) -> Result<usize>; }
pub trait Decoder: Send + Sync {
    fn open_video(&self, path: &Path) -> Result<Box<dyn VideoReader>>;
    fn open_audio(&self, path: &Path, rate: u32, channels: u16) -> Result<Box<dyn AudioReader>>;
    fn probe(&self, path: &Path) -> Result<TrackProbe>;
}
pub enum PermissionKind { Screen, Camera, Microphone }
pub enum PermissionStatus { Granted, Denied, NotDetermined, Restricted, Unknown }
pub trait Permissions: Send + Sync {
    fn status(&self, k: PermissionKind) -> PermissionStatus;        // NEVER prompts
    fn request(&self, k: PermissionKind, done: Box<dyn FnOnce(PermissionStatus) + Send>); // prompts
    fn settings_url(&self, k: PermissionKind) -> Option<String>;
}
pub enum OverlayRole { Panel, FloatingBar, Popover, RegionOverlay, PickOverlay, Saved, Onboarding }
pub trait OverlayWindows: Send + Sync {
    /// Main thread. Swaps the NSWindow to our NSPanel subclass (non-activating), forces creation of
    /// its window device, sets level/collection behaviour per role, registers it for exclusion and
    /// returns its windowNumber; Err if windowNumber <= 0.
    unsafe fn configure(&self, raw_window: *mut c_void, role: OverlayRole) -> Result<WindowId>;
    fn registered(&self) -> Vec<WindowId>;
}
pub trait Desktop: Send + Sync {
    fn reveal(&self, path: &Path) -> Result<()>;
    fn choose_image(&self, done: Box<dyn FnOnce(Option<PathBuf>) + Send>);   // PNG/JPEG
}
pub enum ExclusionKind { PerWindow, AutoHide, None }
pub struct Capabilities { pub system_picker: bool, pub application_source: bool, pub app_audio: bool,
                          pub show_clicks: bool, pub mic_modes: bool, pub switch_source: bool,
                          pub exclusion: ExclusionKind }
pub trait PreviewSink: Send + Sync {}                                // M3; kept for ADR-007

pub trait Platform: Send + Sync {
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> Capabilities;
    fn clock_kind(&self) -> ClockKind;
    fn host_now(&self) -> HostTime;
    fn host_duration(&self, ns: i64) -> std::time::Duration;         // real wait for a host-clock span
    fn local_now(&self) -> LocalDateTime;
    fn screen(&self) -> &dyn ScreenSource;
    fn system_audio(&self) -> &dyn SystemAudioSource;
    fn mic(&self) -> &dyn AudioSource;
    fn camera(&self) -> &dyn CameraSource;
    fn picker(&self) -> &dyn SourcePicker;
    fn writers(&self) -> Arc<dyn WriterFactory>;
    fn decoder(&self) -> Arc<dyn Decoder>;
    fn permissions(&self) -> &dyn Permissions;
    fn overlays(&self) -> &dyn OverlayWindows;
    fn desktop(&self) -> &dyn Desktop;
    fn watch_system(&self, sink: Arc<dyn FrameSink>) -> Result<Box<dyn DeviceStream>>; // devices, sleep/wake, own menus
    fn free_disk_bytes(&self, path: &Path) -> Result<u64>;
    fn default_projects_dir(&self) -> Result<PathBuf>;              // Movies/Simple Capture
    fn permission_host(&self) -> Option<String>;                    // "com.simplecapture.dev" when SC_DEV_HOST=1
}
```

**Synthetic platform (`sc_platform::synthetic`).** Synthetic PTS are **scripted** (intended)
values on the scenario's host clock, never the time a buffer happened to be delivered.

```rust
pub struct Scenario;  impl Scenario {
    pub fn new() -> Scenario;                                       // time_scale 1.0, permissions granted
    pub fn time_scale(self, s: f64) -> Self;                        // host clock runs s× real time
    pub fn screen(self, name: &str, s: ScreenScript) -> Self;       // unknown names are created on demand
    pub fn camera(self, c: Option<VideoScript>) -> Self;
    pub fn mic(self, a: Option<AudioScript>) -> Self;
    pub fn system_audio(self, a: Option<AudioScript>) -> Self;
    pub fn permissions(self, screen: PermissionStatus, camera: PermissionStatus, mic: PermissionStatus) -> Self;
    pub fn disk(self, d: DiskScript) -> Self;                       // Constant(bytes) | Linear{start, bytes_per_s}
    pub fn at(self, after_start_ns: i64, e: ScriptedEvent) -> Self; // WillSleep, DidWake, DeviceLost(kind),
}                                                                   // DevicesChanged(kind), OwnWindowsChanged
pub struct ScreenScript { pub size: SizePx, pub fps: u32, pub first_frame_latency_ms: u32,
    pub idle: Vec<(i64, i64)>, pub idle_from_start: bool, pub start_fails: StartFailure, // Never|Always|WhileOtherStreamRuns
    pub lost_after_ns: Option<i64>, pub trailing_after_new_stream: u32,
    pub first_frame_size: Option<SizePx> }                          // differs from describe() when set
impl Default for ScreenScript { /* 1280×720, 30 fps, 30 ms latency, nothing else */ }
pub struct VideoScript { pub size: SizePx, pub fps: u32 }
pub struct AudioScript { pub rate: u32, pub channels: u16, pub buffer_frames: u32, pub beeps_ns: Vec<i64>,
    pub gaps: Vec<(i64, i64)>, pub overlaps: Vec<(i64, i64)>, pub clock_ppm: f64 }   // (at, dur)
pub struct FrameCode { pub source_hash: u32, pub index: u32, pub pts_ms: u32 }
pub fn source_hash(name: &str) -> u32;
pub fn read_code(v: &Nv12View) -> Option<FrameCode>;               // 16×16 luma blocks, mid-grey threshold;
                                                                   // must survive HEVC at default bits_per_pixel
pub fn display_spec(name: &str, size: Option<SizePx>) -> SourceSpec; // DisplayId("synthetic:NAME[:WxH]")
pub struct SyntheticPlatform; impl SyntheticPlatform {
    pub fn new(s: Scenario) -> Arc<SyntheticPlatform>;
    pub fn with_writers(s: Scenario, w: Arc<dyn WriterFactory>, d: Arc<dyn Decoder>) -> Arc<SyntheticPlatform>;
    pub fn active_screen_streams(&self) -> usize; }
pub struct SctrkWriterFactory; pub struct SctrkDecoder;            // crash-tolerant `.sctrk` files
```

OS crates expose `sc_capture_macos::platform()`, `sc_capture_windows::platform()` and
`sc_capture_linux::platform()` returning `Arc<dyn Platform>`. The Windows and Linux ones return
`Unsupported` from every call and all-false `Capabilities`; the scaffold finishes them, with tests.

### 4.4 sc_capture_macos layout (frozen skeleton)

- `src/lib.rs`: `#![cfg(target_os = "macos")]`; `mod util; mod tcc; mod platform_impl; pub mod
  mainloop; pub mod sck; pub mod av;`; `pub fn platform() -> Arc<dyn Platform>`,
  `pub fn writers() -> Arc<dyn WriterFactory>`, `pub fn decoder() -> Arc<dyn Decoder>` (the last two
  need no TCC; record, decode, export and devtools use them on macOS).
- `src/util.rs`: ported from spike 3 `util.rs`: timebase, `mach_now`, host time ↔ ns, CMTime
  helpers, NSString/NSDictionary/NSNumber helpers, `wait_err`, and `wait_on(rx, timeout)` (channel
  wait **without** pumping).
- `src/mainloop.rs`: `pump(d)`, `pump_recv(rx, timeout)`, `run_until(&AtomicBool)`: only for the
  CLI's main thread and the live-test runner.
- `src/tcc.rs`: `pub(crate) fn guard(what: &str) -> Result<()>`, the tripwire of §2 rule 2.
- `src/platform_impl.rs`: `MacPlatform` delegating to `sck::{MacScreenSource, MacSystemAudio,
  MacPicker, MacPermissions, MacOverlays, MacDesktop, watch_system, free_disk_bytes,
  default_projects_dir, local_now, capabilities}` and `av::{MacMic, MacCamera, MacWriterFactory,
  MacDecoder, watch_devices}` (stubs in `src/sck/mod.rs` and `src/av/mod.rs`).
- `Cargo.toml` declares `[[test]] name = "sck_live", path = "tests/sck_live/main.rs", harness =
  false` and the same for `av_live`. `tests/support/live_runner.rs` (frozen) runs `NSApplication`
  on the main thread and each test body on a worker thread, accepts a name filter and
  `--list`, prints `SKIPPED:` and a zero-test summary unless `SC_LIVE=1`, and prints a libtest-style
  `test result: ok. N passed; M failed` line that `scripts/test-live.sh` parses. Bodies may create
  in-process windows only through `live_runner::on_main(|mtm| …)`.

### 4.5 sc_record

```rust
pub struct RecordRequest {
    pub projects_dir: PathBuf, pub title: String, pub source: SourceSpec, pub via: Via,
    pub fps: u32, pub resolution: Resolution, pub show_cursor: bool, pub click_highlight: bool,
    pub camera: Option<String>, pub mic: Option<String>, pub system_audio: SystemAudioMode,
    pub canvas: Canvas,                    // for aspect Original, `ratio` is overwritten when s0 opens
    pub background_image: Option<PathBuf>, pub exclude: Vec<WindowId>, pub tuning: Tuning }
pub struct Tuning { pub screen_queue: usize, pub camera_queue: usize, pub audio_queue: usize,
    pub gap_threshold_ns: i64, pub start_timeout_ms: u32, pub switch_first_frame_timeout_ms: u32,
    pub frames_dropping_pct: f64, pub disk_warn_bytes: u64, pub disk_stop_bytes: u64,
    pub fragment_interval_s: f64 }        // Default = the values wp-record-session chooses and reports
pub enum SessionState { Preparing, Recording, Switching, Stopping, Finalized { project_dir: PathBuf },
                        Failed { error: String } }
pub enum Notice { SwitchFailed { error: String, restored: bool }, SourceLost { segment: String },
                  DeviceLost { kind: DeviceKind, name: String }, DiskLow { bytes_left: u64 },
                  AudioGap { track: AudioTrackKind, dur_ns: i64 },
                  FramesDropping { track: String, pct: f64 },  // over the last 10 s; UI suggests 30 fps or 1×
                  StoppedForSleep, StoppedForDisk }
pub enum StopReason { User, Duration, Signal, Quit, Disk, Sleep, Error }
pub struct TrackHealthSnap { pub track_id: String, pub frames: u64, pub dropped: u64,
                             pub writer_not_ready: u64, pub gaps: u32, pub overlaps: u32, pub fps: f64,
                             pub drift_ns: i64 }   // audio: count-based end − host-time end
pub struct HealthSnapshot { pub rec_ns: i64, pub tracks: Vec<TrackHealthSnap>, pub free_disk_bytes: u64 }
pub trait SessionObserver: Send + Sync { fn on_state(&self, s: &SessionState);
    fn on_health(&self, h: &HealthSnapshot); fn on_notice(&self, n: &Notice); }
pub struct StopSummary { pub project_dir: PathBuf, pub duration_ns: i64, pub segments: usize,
                         pub health: HealthSnapshot }
pub struct Session;
impl Session {
    pub fn start(platform: Arc<dyn Platform>, req: RecordRequest, obs: Arc<dyn SessionObserver>) -> Result<Session>;
    pub fn switch_source(&self, spec: SourceSpec, via: Via) -> Result<()>;  // async; result via state/notices
    pub fn pause(&self) -> Result<()>;   // M1: Err(Error::Unsupported("pause/resume is M7"))
    pub fn resume(&self) -> Result<()>;  // M1: same
    pub fn set_exclusions(&self, exclude: Vec<WindowId>) -> Result<()>;
    pub fn project_dir(&self) -> &Path;
    pub fn state(&self) -> SessionState;
    pub fn stop(self, reason: StopReason) -> Result<StopSummary>;
    pub fn discard(self) -> Result<()>;
}
pub mod recovery {
    pub enum Outcome { Recovered, Damaged, AlreadyFinal, Locked, NotAProject }
    pub struct RecoveryReport { pub dir: PathBuf, pub outcome: Outcome, pub segments: usize,
                                pub dropped_segments: Vec<String>, pub notes: Vec<String> }
    pub fn recover_project(dir: &Path, decoder: &dyn Decoder) -> Result<RecoveryReport>;
    pub fn scan(projects_dir: &Path, decoder: &dyn Decoder) -> Result<Vec<RecoveryReport>>;
}
pub mod testhooks {  // feature "test-hooks"; SC_TEST_HOLD_AT=<point>
    pub enum HoldPoint { AfterStart, AfterSwitchBegin, AfterSegmentPrepare, AfterSegmentOpen }
    pub fn hold(p: HoldPoint);  // called AT the journaling site, on the thread that journals; prints
}                               // "SC_HOLD <point>" to stderr and blocks that thread for 30 s
pub type Result<T> = std::result::Result<T, Error>;
pub enum Error { Platform(sc_platform::Error), Core(sc_core::Error), State(String), Unsupported(&'static str) }
```

### 4.6 sc_decode, sc_text, sc_render, sc_helpers, sc_export

```rust
// sc_decode
pub struct SegmentFrameSource;   // sequential, monotonic t; hold rule of ARCHITECTURE §7
impl SegmentFrameSource { pub fn open(project: &ProjectV1, dir: &Path, decoder: Arc<dyn Decoder>) -> Result<Self>;
                          pub fn frame_at(&mut self, t_rec_ns: i64) -> Result<Option<SegmentFrame>>; }
pub struct SegmentFrame { pub segment_id: String, pub frame_px: SizePx, pub content_px: RectPx,
                          pub frame: FrameBuffer, pub held: bool }
pub struct TrackAudio;           // continuous audio in RecTime, silence before the first sample and after the end
impl TrackAudio { pub fn open(project: &ProjectV1, dir: &Path, track_id: &str, decoder: Arc<dyn Decoder>,
                              rate: u32, channels: u16) -> Result<Self>;
                  pub fn read(&mut self, out: &mut [f32]) -> Result<usize>; }
pub fn cfr_frame_times(duration_ns: i64, fps: u32) -> impl Iterator<Item = i64>;

// sc_text (Skia FreeType build, ADR-018)
pub struct BuildInfo { pub freetype: bool, pub skia_milestone: u32, pub jpeg_decode: bool, pub webp_decode: bool }
pub struct BackgroundCache;      // keyed by (stable hash of Background, w, h)
impl BackgroundCache { pub fn new(capacity: usize) -> Self;
                       pub fn get(&mut self, bg: &Background, size: SizePx, project_dir: &Path) -> Result<Arc<RgbaImage>>; }
pub fn rasterize_background(bg: &Background, size: SizePx, project_dir: &Path) -> Result<RgbaImage>;
pub fn encode_png(img: &RgbaImage) -> Result<Vec<u8>>;     // sRGB chunk
pub fn decode_image(bytes: &[u8]) -> Result<RgbaImage>;    // PNG/JPEG through Skia
pub fn build_info() -> BuildInfo;
pub mod layout;                                            // M4 placeholder: every fn Err(Unsupported)

// sc_render
pub enum Backend { Auto, Metal, Dx12, Vulkan, Gl, CpuOnly }        // env SC_WGPU_BACKEND overrides Auto
pub struct AdapterSummary { pub name: String, pub backend: String, pub device_type: String, pub software: bool }
pub struct Compositor;
impl Compositor { pub fn new(backend: Backend) -> Result<Compositor>;
                  pub fn adapter(&self) -> Option<AdapterSummary>;   // None = CPU only
                  pub fn render(&mut self, job: &FrameJob, out: OutputFormat) -> Result<RenderedFrame>; }
pub struct FrameJob<'a> { pub canvas: SizePx, pub background: &'a RgbaImage, pub screen: Option<ScreenLayer<'a>> }
pub struct ScreenLayer<'a> { pub frame: &'a FrameBuffer, pub src: RectPx, pub dst: RectPx }  // BT.709 limited input
pub enum OutputFormat { Rgba8, I420Bt709Limited }
pub struct RenderedFrame { pub size: SizePx, pub format: OutputFormat, pub data: Vec<u8> } // tightly packed
pub mod cpu { pub fn render_cpu(job: &FrameJob, out: OutputFormat) -> Result<RenderedFrame>; }
pub struct DiffStats { pub max: u8, pub mean: f64 }
pub fn diff(a: &RenderedFrame, b: &RenderedFrame) -> DiffStats;

// sc_helpers
pub enum HelperKind { Ffmpeg, Ffprobe, Gifski }
/// Order: $SC_FFMPEG / $SC_FFPROBE / $SC_GIFSKI → next to current_exe() (sidecar layout) →
/// debug builds only: concat!(env!("CARGO_MANIFEST_DIR"), "/../../third_party/out/<os>-<arch>/").
/// Never $PATH. Error names every path tried.
pub fn locate(kind: HelperKind) -> Result<PathBuf>;
pub struct SpawnSpec { pub program: PathBuf, pub args: Vec<String>, pub frame_bytes: usize, pub queue_frames: usize }
pub struct Progress { pub frame: u64, pub out_time_ns: i64, pub speed: Option<f64>, pub done: bool }
pub struct ExitReport { pub code: Option<i32>, pub stderr_tail: Vec<String>, pub frames_written: u64 }
pub struct CancelReport { pub escalated_to_kill: bool, pub waited_ms: u64 }
pub struct HelperProcess;
impl HelperProcess { pub fn spawn(spec: SpawnSpec) -> Result<HelperProcess>;
                     pub fn write_frame(&self, bytes: &[u8]) -> Result<()>;   // blocks when the queue is full
                     pub fn progress(&self) -> Option<Progress>;
                     pub fn finish(self) -> Result<ExitReport>;
                     pub fn cancel(self) -> CancelReport; }                    // SIGTERM → 1.5 s → SIGKILL
pub mod y4m  { pub struct Y4mHeader { pub size: SizePx, pub fps_num: u32, pub fps_den: u32 }
               pub fn header(h: &Y4mHeader) -> String; pub const FRAME: &[u8] = b"FRAME\n"; }
pub mod wav  { pub enum WavFormat { F32, S16 }
               pub struct WavWriter; impl WavWriter {
                   pub fn create(path: &Path, rate: u32, channels: u16, fmt: WavFormat) -> Result<WavWriter>;
                   pub fn write_f32(&mut self, interleaved: &[f32]) -> Result<()>;
                   pub fn frames(&self) -> u64;
                   pub fn finish(self) -> Result<u64>; } }                      // patches the header
pub mod part { pub struct PartFile; impl PartFile { pub fn create(final_path: &Path) -> Result<PartFile>;
                 pub fn path(&self) -> &Path; pub fn commit(self) -> Result<PathBuf>; pub fn discard(self); }
               pub fn sweep_stale(dir: &Path) -> Result<usize>; }
pub struct HelperInfo { pub path: PathBuf, pub sha256: String, pub version_line: String,
                        pub buildconf: Vec<String>, pub network_disabled: bool }
pub fn helper_info(path: &Path) -> Result<HelperInfo>;

// sc_export
pub enum ExportSize { Reference, Class(SizeClass) }
pub struct QuickExportRequest { pub project_dir: PathBuf, pub out_path: Option<PathBuf>, pub size: ExportSize, pub fps: Option<u32> }
pub enum ExportState { Preparing, Rendering, Finalizing, Done { path: PathBuf }, Canceled, Failed { error: String } }
pub struct ExportProgress { pub frames_done: u64, pub frames_total: u64, pub fps: f64, pub eta_s: Option<f64> }
pub trait ExportObserver: Send + Sync { fn on_state(&self, s: &ExportState); fn on_progress(&self, p: &ExportProgress); }
pub struct ExportJob;
impl ExportJob { pub fn start(platform: Arc<dyn Platform>, req: QuickExportRequest, obs: Arc<dyn ExportObserver>) -> Result<ExportJob>;
                 pub fn cancel(&self); pub fn wait(self) -> Result<PathBuf>; }
pub mod recipe   { pub fn x264_quick(size: SizePx, fps: u32, audio: bool, out: &Path) -> Vec<String>; }
pub mod audio    { pub fn mix_to_wav(project: &ProjectV1, dir: &Path, decoder: Arc<dyn Decoder>,
                                     fps: u32, frames: u64, wav: &Path) -> Result<bool>; }   // false = no audio
pub mod validate { pub struct Expect { pub size: SizePx, pub fps: u32, pub duration_ns: i64, pub audio: bool, pub vcodec: String }
                   pub struct ProbeReport { pub vcodec: String, pub size: SizePx, pub fps: f64, pub duration_ns: i64,
                       pub frames: Option<u64>, pub acodec: Option<String>, pub audio_duration_ns: Option<i64>,
                       pub color_primaries: Option<String>, pub color_transfer: Option<String>,
                       pub color_space: Option<String>, pub color_range: Option<String> }
                   pub fn ffprobe_check(file: &Path, e: &Expect) -> Result<ProbeReport>; }
pub mod probe {
    pub struct DisplayProbe { pub display_id: String, pub native_px: SizePx, pub one_x_px: SizePx,
                              pub hevc_fps_native: Option<f64>, pub hevc_fps_1x: Option<f64> }
    pub struct ProbeV1 { pub schema: u32 /* 1 */, pub created_at: String, pub os_version: String,
                         pub helper: Option<HelperInfo>, pub vt_h264_opens: Vec<(SizePx, bool)>,
                         pub displays: Vec<DisplayProbe>, pub quarantined: Vec<String> }
    impl ProbeV1 { pub fn allows_60fps(&self, display: &DisplayId, res: Resolution) -> Option<bool>; } // measured ≥ 1.3 × 60
    pub fn run(platform: &dyn Platform, displays: &[DisplayInfo]) -> Result<ProbeV1>;
    pub fn load_or_run(platform: &dyn Platform, cache_dir: &Path, displays: &[DisplayInfo]) -> Result<ProbeV1>;
}   // cache file <cache_dir>/probe-v1.json, keyed by helper sha256 + OS version + display sizes
```

---

## 5. CLI, app and UI contracts

### 5.1 CLI (`simple-capture`, cli/)

Hand-written parser (ported from spike 3's `Args`; no `clap`). All commands accept `--json` and
`--platform native|synthetic` (default native). Exit codes: 0 ok, 1 error, 2 usage, 3 unsupported
on this OS. On macOS the main thread runs `sc_capture_macos::mainloop::run_until` while the
session runs on its own threads.

```
simple-capture sources [--json]                 displays, windows, apps (needs the grant)
simple-capture displays [--json]                displays only, never needs a permission
simple-capture devices                          cameras and mics
simple-capture perms                            permission status (never prompts)
simple-capture record --source SPEC [--duration S] [--switch T=SPEC]...
        [--fps 30|60] [--resolution native|1x] [--no-cursor] [--clicks]
        [--camera NAME|default|none] [--mic NAME|default|none] [--system-audio off|all|app]
        [--aspect original|16:9|4:3|1:1|9:16] [--background BG] [--title T]
        [--projects-dir DIR] [--events] [--own-test-window T=SECONDS]   (debug builds: show an
        in-process window registered for exclusion, to check it never appears)
        Stops at --duration, on SIGINT/SIGTERM (graceful), or on a line "stop" on stdin.
        Stdin EOF is ignored (under run.sh stdin is /dev/null unless SC_DEV_STDIN is set).
        Prints the project folder. With feature "test-hooks", honours SC_TEST_HOLD_AT.
simple-capture recover [--projects-dir DIR]
simple-capture export PROJECT [--out FILE] [--size reference|2160|1440|1080|720|480] [--fps N]
simple-capture validate FILE --size WxH --fps N --duration S [--audio] [--vcodec h264]
simple-capture probe [--refresh]
simple-capture --self-test                      M1: prints "not implemented until M6", exit 3

SPEC := display[:ID|main|N] | window:ID | window-title:SUBSTRING
        | app:BUNDLE_ID[@DISPLAY] | app-pid:PID[@DISPLAY]
        | region:DISPLAY:X,Y,W,H   (display-local points, top-left)
        | synthetic:NAME[:WxH]     (synthetic platform only; unknown names created on demand)
BG   := solid:#RRGGBB | linear:DEG:#RRGGBB@0,#RRGGBB@1[,…] | radial:CX,CY,R:#RRGGBB@0,… | image:PATH
```

`export`, `validate` and `probe` sit behind the cli feature `export` (default on).

### 5.2 App commands and events (app/ ↔ ui/)

Rust handlers in `app/src/commands.rs`; typed wrappers in `ui/src/lib/api.ts`, DTOs in
`ui/src/lib/types.ts` (hand-written mirrors, snake_case), and a mock backend `ui/src/lib/mock.ts`.

| Command | Returns | Notes |
|---|---|---|
| `get_permissions` | `{screen, camera, mic}`: `granted\|denied\|not_determined\|restricted\|unknown` | never prompts |
| `request_permission(kind)` | status | prompts; only from a user click |
| `open_permission_settings(kind)` | – | user click only; agents never invoke it |
| `list_displays` | `DisplayInfo[]` | never needs a permission |
| `list_sources` | `SourceList` | **only after `get_permissions` reports screen = granted**; hides our windows |
| `list_devices` | `{cameras, mics}` | event `devices-changed` |
| `show_mic_modes` | – | only when `capabilities.mic_modes`; user click |
| `get_settings` / `set_settings(RecorderSettings)` | settings | app data `settings.json` |
| `get_capabilities` | `Capabilities` + probe per display: `{display_id, can_60fps_native, can_60fps_1x}` | |
| `preview_still(source, canvas)` | `sc://still/<id>.png` URL | compositor still (ARCHITECTURE §9) |
| `choose_background_image` | path or null | `Desktop::choose_image` |
| `begin_region_select(purpose, display_id?)` | – | region overlay on that display (default: display under the cursor); events `region-selected` / `region-cancelled` |
| `begin_pick_mode(purpose)` | – | pick overlay on the display under the cursor, follows the cursor across displays; `source-picked` / `pick-cancelled` |
| `open_system_picker(purpose)` | – | `source-picked` (via `system_picker`) or `pick-cancelled`; a picked display may continue with `begin_region_select(purpose, picked)` |
| `start_recording(RecordRequestDto)` | – | events `recording-state`, `health` (1 Hz), `notice` |
| `stop_recording` / `discard_recording` | – | discard only after a UI confirmation |
| `switch_source(SourceSpecDto)` | – | |
| `quick_export(project_dir, size)` | job id | events `export-progress`, `export-state`; `cancel_export(id)` |
| `reveal(path)` | – | |
| `recovery_report` | last scan results | scan runs at launch |

`purpose` is `"start" | "switch"`. Windows (labels): `panel`, `onboarding`, `bar`, `switcher`,
`region`, `pick`, `saved`. All are created **hidden at launch** and configured through
`OverlayWindows::configure`; the region and pick windows are moved onto the target display before
they are shown (same window IDs, so exclusions still hold). One Vite build, hash routes.

Security: `app.security.csp` allows only `'self'`, the `sc:` scheme, Tauri IPC and `data:`/`blob:`
images; no remote `connect-src`, no remote fonts. No HTTP, updater, shell, dialog or opener plugins.

**Quit and signals.** Tray Quit, `RunEvent::ExitRequested` (logout, Cmd-Q) and SIGTERM/SIGINT (a
signal thread sets a flag the app polls) all run one exit path: an active recording is stopped with
`StopReason::Quit`/`Signal` and finalized; an active export is cancelled (`.part` deleted, helper
gone); then the app exits.

### 5.3 Hotkeys and tray

- ⌥⇧S: switch source (pick mode on the display under the cursor), registered with
  tauri-plugin-global-shortcut (no Accessibility grant); a registration failure shows a notice.
  No other global hotkeys in M1.
- Tray menu: Open Recorder · Start Recording / Stop Recording · Switch Source… (opens the switch
  popover, an excluded window; no submenu) · Quit. While recording, the status-item title shows
  the elapsed time. Our status item and its menu are windows of our process, so display recordings
  exclude them (§10 item 9); the menu bar shows a gap where our item is.

### 5.4 Debug app driver (debug builds only)

`SC_APP_SCRIPT=<file>` (forwarded by run.sh) makes the app run a line-based script after `SC_READY`,
calling **the same functions** the commands, tray items and hotkey use; results go to stderr as JSON
lines (`{"step":n,"ok":true,…}`) and the app exits with the script's status. Commands: `wait MS`,
`cmd NAME JSON` (any §5.2 command), `tray open-menu` (in-process `performClick` on the status-item
button), `tray item "Switch Source…"`, `hotkey switch`, `region set DISPLAY X,Y,W,H`,
`pick window ID|display ID`, `show WINDOW`, `log-window-rects`, `sim sleep|wake` (injects
`SystemWillSleep`/`SystemDidWake`), `quit`. No synthetic OS input events are posted, so no
Accessibility or Input Monitoring grant is involved. The driver is compiled out of release builds.

---

## 6. Test strategy

### 6.1 Tiers

| Tier | Runs where | Needs | Command |
|---|---|---|---|
| Unit + synthetic | CI all three OSes, locally | nothing | `just test` |
| macOS non-TCC | CI macOS, locally | AVFoundation writers/readers on synthetic frames, VideoToolbox | `just test` on macOS; `SC_ALLOW_NO_HW_ENCODER=1` (CI only) turns a missing hardware HEVC encoder into a visible `SKIPPED:` instead of a failure |
| Helper | CI macOS (cached helper), locally | `third_party/out/macos-arm64/{ffmpeg,ffprobe}` | `SC_REQUIRE_HELPERS=1 just test` |
| GPU golden | locally (Metal); CI where an adapter exists | wgpu adapter | `SC_REQUIRE_GPU=1 just test` |
| Live | locally, dev host | Simple Capture Dev's grants; live lock | `just test-live <package> <prefix>` |
| End-to-end | locally, dev host | CLI + fixture apps | `just e2e` |
| App driver | locally, dev host | app debug build | `just app-driver <script>` |
| Verification | locally, dev host | CLI + devtools | `just sync-test`, `just soak N`, `just killtest` |

A test that cannot run (no helper, no GPU, no grant, one display) fails when its `SC_REQUIRE_*`
variable is set and prints a `SKIPPED:` line otherwise; CI sets every variable it can satisfy.

### 6.2 Synthetic platform

Deterministic sources that behave like SCK where it matters (§4.3 Scenario): coded frames,
scripted PTS, first-frame latency, idle periods, idle-from-start, trailing old frames, start
failures (always, or only while another stream runs), lost-at, a first frame whose size differs
from `describe()`, audio with beeps, gaps, overlaps and clock skew in ppm, scripted permissions,
disk, devices and system events, and a host clock that can run faster than real time. Logic that
needs long simulated spans (gap filler drift, router cut rules) is unit-tested as pure components
with scripted inputs; Session integration tests run a few real seconds with tolerances of at least
one frame, and CI runs the record tests with `--test-threads=2`.

### 6.3 What M1 tests prove

- geometry tables and property tests (ARCHITECTURE §8/§10).
- record (synthetic): segments sorted and non-overlapping; every frame inside its segment; audio
  sample counts continuous; camera unaffected by switches; gaps filled and overlaps trimmed; ±100
  ppm audio skew over 30 simulated minutes stays within one frame of host time; stop extends the
  last segment; switch failure, retry and restore; first-frame timeout; source lost then switch;
  Original ratio from the first frame; permission denied fails fast; sleep stops; disk stop; lock
  held for the whole recording.
- recovery: torn journal, missing `segment_close`, prepared-but-never-opened file, unreadable
  segment, already-final, locked, idempotence.
- macOS live: every source kind on the main display and, when present, a second display; region
  sizes (odd, Retina); application filter with a window opening mid-recording; exclusion of
  hidden-then-shown and of our process's windows; system audio All/App with two tone processes;
  make-before-break gap and wrong-content count; switch to a static window; closing the captured
  window; `SCShareableContent.currentProcess`.
- export: every aspect × background kind GPU vs CPU; ffprobe size/fps/duration/audio per size
  class; decoded colour patches within 3/255; cancel leaves no `.part` and no process.
- e2e under the dev host (§8.2), app driver flows (§8.2), sync, soak, kill tests (§8.3).
- Manual only (MANUAL_TEST): real clicks, the ⌥⇧S key press, region drags, picker picks (including
  `excludedWindowIDs` on a picked display), device hot-plug, display unplug, sleep/wake, release
  bundle.

### 6.4 CI and local equivalents

GitHub workflow (wp-ci): a **fast job** per OS (fmt, clippy and tests for every crate that does not
link Skia or Tauri, plus `cli --no-default-features`) and a **full job** per OS (Skia FreeType
build, Tauri app, UI build, all tests) with `timeout-minutes`, the Skia build output cached
separately from `target/` and keyed by skia-safe version + `SKIA_GN_ARGS` + OS, a short
`CARGO_TARGET_DIR` (`C:\t`) on Windows against MAX_PATH, `libfontconfig1-dev` on Ubuntu; a
`licenses` job (cargo-deny, pnpm license script, network check). `just ci-local`: fmt, clippy,
tests, deny, pnpm licenses, `scripts/ci/netcheck.sh`, `scripts/ci/windows-check.sh` (`cargo clippy
--target x86_64-pc-windows-msvc` for core, geometry, platform, capture-windows, record, decode,
helpers and `cli --no-default-features`), and `scripts/ci/linux-docker.sh`, which builds and tests
in Docker on **aarch64** Linux (the M3's native arch; it is a dry run, not the x86_64 CI target)
from an image that adds clang, ninja, python3 and the Ubuntu packages to spike 6's base. The Windows
Skia and Tauri builds can only be proven on GitHub's runners.

---

## 7. Spike code to port (all ours, no Cap code)

| Spike file | Goes to | Package |
|---|---|---|
| 03 `util.rs` | time/CMTime/NS helpers → `capture-macos/src/util.rs`; `pump*` → `src/mainloop.rs`; process CPU/RSS → `tools/devtools/src/soak/` | scaffold; wp-soak-test |
| 03 `capture.rs` | `capture-macos/src/sck/` (StreamOutput class, `fetch_content`, filters, 420v/queueDepth config, `frame_info`); `extend_screen_to` idea → `record/src/session/` | wp-mac-sck; wp-record-session |
| 03 `swap.rs` | `sck/` (overlap start/stop, audio-only SCStream, picker configuration, `frame_geom`); router logic → `record/src/session/`; `swaphelper` tones → `tools/devtools/src/testapps/` | wp-mac-sck; wp-record-session |
| 03 `avcap.rs` | auth status → `sck/permissions.rs`; discovery, `best_format`, camera/mic, delegate → `av/` (replace `pump_recv` with `util::wait_on`) | wp-mac-sck; wp-mac-av |
| 03 `writer.rs`, `decode.rs` (reader), `encbench.rs` | `av/writer.rs`, `av/reader.rs`, `av/encbench.rs`; BT.709 shader maths → `render` | wp-mac-av; wp-render |
| 03 `record.rs` | `perms` → `sck/permissions.rs` (status only); `list` → `sck/content.rs`; kill pattern → `scripts/killtest/` | wp-mac-sck; wp-kill-recovery-test |
| 03 `testwin.rs` | `tools/devtools/src/testapps/` and the live runner's `on_main` windows | wp-mac-sck |
| 01 `helper.rs`, `sys.rs`, `frames.rs`, `wav.rs`, `probe.rs` (helper info) | `helpers/src/` (FFI → `libc`) | wp-helpers |
| 01 `jobs.rs`, `probe.rs` (VT size probe) | `export/src/recipe.rs`, `export/src/probe/` | wp-export |
| 02 `src-tauri/src/main.rs` | `app/src/protocol.rs`, window builder settings | wp-app-shell |
| 04 `gpu.rs`, `composite.rs` | `render/src/` | wp-render |
| 04 build setup | `.cargo/config.toml`, `text/` | scaffold; wp-text |

Not ported in M1: spike 2's `native_mac.rs` (native preview) and spike 4's text layout.

---

## 8. Work packages

| Stage | Wave | Id | Owns | Depends on |
|---|---|---|---|---|
| 0 | 0 | wp-scaffold | everything (creates the workspace) | – (gates G1, G2) |
| 1 | 1 | wp-core | `crates/core/**` (non-frozen) | scaffold (G4) |
| 1 | 1 | wp-geometry | `crates/geometry/**` | scaffold |
| 1 | 1 | wp-platform-synth | `crates/platform/**` | scaffold |
| 1 | 1 | wp-ui | `ui/**` | scaffold |
| 1 | 1 | wp-ci | `.github/**`, `deny.toml`, `about.toml`, `scripts/ci/**` | scaffold (G3) |
| 1 | 2 | wp-mac-sck | `crates/capture-macos/src/sck/**`, `crates/capture-macos/tests/sck_*.rs`, `crates/capture-macos/tests/sck_live/**`, `tools/devtools/src/testapps/**` | scaffold, geometry |
| 1 | 2 | wp-mac-av | `crates/capture-macos/src/av/**`, `crates/capture-macos/tests/av_*.rs`, `crates/capture-macos/tests/av_live/**` | scaffold, platform-synth |
| 1 | 2 | wp-decode | `crates/decode/**` | core, geometry, platform-synth |
| 1 | 2 | wp-text | `crates/text/**` | scaffold, geometry |
| 1 | 2 | wp-render | `crates/render/**` | geometry, platform-synth |
| 1 | 2 | wp-helpers | `crates/helpers/**` | core |
| 1 | 2 | wp-record-recovery | `crates/record/src/recovery/**`, `crates/record/tests/recovery_*.rs` | core, platform-synth |
| 1 | 2 | wp-cli | `cli/**` | scaffold, platform-synth |
| 1 | 3 | wp-record-session | `crates/record/src/session/**`, `crates/record/src/testhooks.rs`, `crates/record/tests/session_*.rs` | core, geometry, platform-synth, mac-av |
| 1 | 3 | wp-export | `crates/export/**` | core, geometry, platform-synth, mac-av, decode, render, text, helpers |
| 1 | 3 | wp-app-shell | `app/**` | scaffold, platform-synth, mac-sck |
| 2 | – | wp-e2e-cli | `cli/**`, `scripts/e2e/**`; fixes: non-frozen `crates/**`, `tools/devtools/**` | every stage-1 package except ui, ci, app-shell |
| 2 | – | wp-app-integration | `app/**`, `ui/**` | app-shell, ui, mac-sck, mac-av, record-session, record-recovery, export, render, text, helpers |
| 3 | – | wp-sync-test | `tools/devtools/src/sync/**`, `scripts/sync/**` | e2e-cli |
| 3 | – | wp-soak-test | `tools/devtools/src/soak/**`, `scripts/soak/**` | e2e-cli (G6 for 30 min) |
| 3 | – | wp-kill-recovery-test | `tools/devtools/src/killtest/**`, `scripts/killtest/**`, `crates/record/src/bin/sc-record-synth.rs`, `crates/record/tests/kill_*.rs` | e2e-cli |
| 3 | – | wp-manual-test | `docs/MANUAL_TEST.md` | e2e-cli, app-integration |
| 3 | – | wp-ci-green | `.github/**`, `deny.toml`, `about.toml`, `scripts/ci/**` | e2e-cli, app-integration, ci (G5) |
| 3 | – | wp-docs-close | `docs/STATUS.md`, `docs/DECISIONS.md`, `docs/ARCHITECTURE.md`, `docs/LICENSES.md`, `docs/THIRD_PARTY.md`, `docs/CODEC_MATRIX.md` | every other stage-3 package |

### 8.0 Stage 0

#### wp-scaffold: create the whole workspace

- **Gates:** starts only after Q1 and Q2a are answered; the answers are written here. If Q1 is "no"
  for a crate, use its fallback in §9.4.
- **Goal:** every crate, the app, UI, CLI, devtools, CI, scripts and the justfile exist and build,
  with every public type and trait of §4–§5 in place and every function stubbed. After this
  package, nobody edits the frozen files of §3.
- **Create:**
  1. Root `Cargo.toml`: members (12 crates + app + cli + tools/devtools); `[workspace.package]`;
     `[workspace.dependencies]` with every crate of §9 at the listed version (path entries for our
     crates); `[profile.dev.package."*"] opt-level = 3`; `[profile.release] debug = 1`;
     `[workspace.lints]` (`unsafe_op_in_unsafe_fn` warn; clippy `unwrap_used` warn in libs).
     objc2 framework crates with spike 3's cross-framework features
     (`spikes/03-macos-pipeline/Cargo.toml`); `objc2-app-kit` with `default-features = false` and
     `std`, `objc2-core-foundation`, `objc2-core-graphics`, `block2`, plus the modules we use
     (NSApplication, NSWindow, NSPanel, NSView, NSResponder, NSScreen, NSWorkspace,
     NSRunningApplication, NSOpenPanel, NSSavePanel, NSColor, NSGraphics, NSEvent, NSImage, NSMenu,
     NSStatusItem, NSStatusBar, NSButton) per ADR-006; `skia-safe = "=0.157.0"` with `textlayout`;
     `wgpu = "30.0.2"`; `tauri` with `tray-icon`.
  2. `rust-toolchain.toml`, `.cargo/config.toml`, `.gitignore` additions (`.wt/`, `ui/dist/`,
     `ui/node_modules/`, `app/binaries/`).
  3. Every crate with the §4 types, traits, modules and stubs; complete `sc_core::project` and
     `journal` data types with a unit test that the ARCHITECTURE §6 example (gaps filled in) and
     one line of every `JournalEvent` deserialize; finished Windows/Linux placeholders with tests.
     Crate manifests list every dependency each crate will need (they are frozen), including
     `record`'s macOS dev-dependency on capture-macos and the `test-hooks` feature.
  4. capture-macos: `util.rs`, `mainloop.rs`, `tcc.rs` (tripwire, with a unit test that a guarded
     call fails without `SC_LIVE`/`SC_DEV_HOST`), `platform_impl.rs`, `sck/mod.rs` and `av/mod.rs`
     stubs, the `sck_live`/`av_live` `[[test]]` targets and `tests/support/live_runner.rs` (§4.4).
  5. record: `session/mod.rs`, `recovery/mod.rs`, `testhooks.rs` stubs; `src/bin/sc-record-synth.rs`
     stub (usage, exit 2).
  6. cli: the §5.1 surface with the hand-written parser and "not implemented" bodies; features
     `export` (default) and `test-hooks`.
  7. app: Tauri 2.12.x shell (identifier `com.simplecapture.desktop`, productName "Simple
     Capture", `frontendDist ../ui/dist`, feature `custom-protocol` default on, no
     `macOSPrivateApi`, no HTTP/updater/shell/dialog/opener plugins, the §5.2 CSP), every §5.2
     command as a stub, the seven windows declared hidden, the §5.4 driver skeleton, and:
     - `tauri.conf.json` with `bundle.active = false` and **no** `externalBin`, so `cargo build -p
       simple-capture-app` works on all three OSes without helpers;
     - `tauri.bundle.conf.json` (used only by `just bundle --config`): `externalBin`
       `binaries/ffmpeg`, `binaries/ffprobe`; macOS `signingIdentity "-"`, hardened runtime on,
       `entitlements` → `macos/entitlements.plist` (`com.apple.security.device.camera`,
       `com.apple.security.device.audio-input`), usage strings from `macos/Info.bundle.plist`
       (NSCameraUsageDescription, NSMicrophoneUsageDescription, NSAudioCaptureUsageDescription)
       merged through Tauri 2.12's bundle-only macOS Info.plist option (check the key in the
       tauri-utils schema; the plist must not be embedded into `just run` builds);
     - the full icon set in `app/icons/` (icon.icns, icon.ico, 32/128/256 PNGs, a tray template
       PNG), generated once from a 1024 px PNG with `pnpm exec tauri icon` and committed.
  8. ui: Svelte 5 + Vite 8 + TypeScript 6 with pnpm lockfile, hash routes for the seven windows,
     `src/lib/{api.ts,types.ts,mock.ts}` covering §5.2; scripts `dev`, `build`, `check`, `test`
     (`node --test tests/`), `licenses`; no `tauri dev` script.
  9. `tools/macos-dev-host/run.sh` (script only; the bundle, host.c, Info.plist and install.sh are
     untouched): always pass a pid file (a temp one when `SC_DEV_PID_FILE` is unset); trap
     INT/TERM/HUP and send the signal to the recorded child pid (host.c already forwards signals to
     its child, but signals to run.sh never reached the host); add `--env SC_DEV_HOST=1`; when
     `SC_DEV_STDIN=<fifo>` is set, pass `--stdin <fifo>` to `open`. Test: `run.sh /bin/sleep 30`
     then SIGTERM to run.sh ends the child within 1 s (pgrep shows no `sleep 30`).
  10. devtools: bin `sc-devtools` dispatching `envcheck|testapps|sync|soak|killtest`; `envcheck`
      implemented (§0.4); the others stubs. Its manifest lists objc2-app-kit (window, colour, screen
      features), objc2-avf-audio (`AVAudioPlayer`), objc2-core-graphics, libc, serde_json,
      anyhow, and our crates per §3.
  11. justfile (final, frozen; every target passes `--locked`): `setup`, `helpers`
      (`third_party/fetch.sh` + `third_party/ffmpeg/build-macos.sh arm64`, copy ffmpeg and ffprobe to
      `app/binaries/<name>-aarch64-apple-darwin`), `build`, `run` (UI build → `cargo build -p
      simple-capture-app` → `scripts/lib/live.sh run.sh target/debug/simple-capture-app`),
      `app-driver SCRIPT`, `bundle`, `test`, `test-live PKG PREFIX` → `scripts/test-live.sh`, `e2e`
      → `scripts/e2e/run.sh`, `sync-test` → `scripts/sync/run.sh`, `soak N` →
      `scripts/soak/run.sh`, `killtest` → `scripts/killtest/run.sh`, `lint`, `deny`, `licenses`,
      `netcheck`, `ci-local`, `ci-local-fast` → `scripts/ci/*.sh`. The scaffold creates every
      delegated script as a stub that prints "not implemented yet" and exits 1; the owning package
      replaces it.
  12. `scripts/test-live.sh` (frozen): `cargo test --locked -p PKG --no-run --message-format=json`;
      pick test binaries whose `target.name` starts with PREFIX; `cargo build -p sc-devtools` and
      export `SC_DEVTOOLS=<abs path>`; for each binary `cd` into the package's manifest dir and run
      it through `scripts/lib/live.sh` + run.sh with `SC_LIVE=1 --ignored --test-threads=1`
      (harness = false targets get the filter only); sum the `test result:` lines and **fail if 0
      tests ran** or any failed. `scripts/lib/live.sh` (frozen): lock, caffeinate, envcheck, and
      `fixture start|stop` (runs `$SC_DEVTOOLS testapps fixture` **outside** the dev host, writes
      pids, titles and window IDs to `target/live/fixture.json`, exports `SC_FIXTURE`).
  13. `.github/workflows/ci.yml` skeleton (wp-ci completes it), `deny.toml`, `about.toml`,
      `scripts/ci/*` stubs. Actions pinned by commit SHA (Q1).
- **Do not:** implement behaviour beyond stubs and the items above; add any crate not in §9; create
  bundle IDs; launch the app bundle.
- **Commands:** `cargo build --locked --workspace`, `cargo clippy --locked --workspace
  --all-targets -- -D warnings`, `cargo test --locked --workspace`, `pnpm --dir ui check && pnpm
  --dir ui build && pnpm --dir ui test`, `cargo clippy --target x86_64-pc-windows-msvc` for the
  §6.4 list, `cargo tree -e features -i objc2-app-kit -p simple-capture-app`, `just --list`,
  the run.sh signal test, `CARGO_TARGET_DIR=target/skia cargo build -p simple-capture-app` (warms
  the shared Skia dir).
- **Done when:** all commands pass on this Mac; `cargo tree -p simple-capture-app` shows no HTTP
  client; the report lists the resolved version of every dependency, whether CloudKit/CoreData are
  still linked (ADR-006, `otool -L`), clean and incremental dev build times (ADR-031), and the
  exact §4 signatures as created (the orchestrator compares them with this plan).

### 8.1 Stage 1

#### wp-core: project model, journal, session log, atomic files

- **Owns** `crates/core/**` (non-frozen). **Implements** `sc_core::{project, journal, sessionlog,
  fsio, folder, ids, sha, time, image}` (§4.2). Owner answer to Q4 goes here.
- **Build:** `ProjectV1::load/save` with unknown-field preservation, `save` through
  `fsio::atomic_write` (temp in the same folder, fsync, rename, fsync folder). `JournalWriter`
  (O_APPEND, fsync per event) and `read_journal` (torn last line and unknown events tolerated and
  counted). `SessionLog` (append, flush per line, no fsync). `RecClock::to_rec` with pauses
  (ARCHITECTURE §5). `folder`: `<YYYY-MM-DD HH.MM.SS> <title><PROJECT_SUFFIX>` (ADR-032; title
  sanitized for macOS, Windows and Linux; collision suffix " 2", " 3"); `ProjectLock` (exclusive
  create of `.lock` holding the pid; stale when the pid is dead on unix; conservative on Windows).
  `ids::new_id`. `sha` with `sha2` (or the std-only fallback, §9.4). `copy_background_asset`
  (PNG/JPEG by magic bytes, `assets/background-<sha256[0..12]>.<ext>`, `fit: cover`, focus 0.5/0.5).
- **Tests:** ARCHITECTURE §6 example round-trips semantically and keeps an unknown field; every
  `JournalEvent` variant round-trips with the exact `ev` names; torn tail; atomic write leaves old
  or new file; `to_rec` table with two pauses, inside a pause and before start; folder names for
  `/`, `:`, emoji and 300-char titles; lock contention between two processes (re-spawn the test
  binary as a helper); SHA-256 test vectors (empty, "abc", 1 MB).
- **Commands:** `cargo test --locked -p simple-capture-core`; clippy; Windows clippy check.
- **Done when:** tests pass on macOS and the Windows check passes.

#### wp-geometry: fit, sizes, coordinates

- **Owns** `crates/geometry/**`. **Implements** §4.1.
- **Build:** ARCHITECTURE §8 contain fit (both edges snapped); reference canvas size rules (fixed
  aspects: short side = largest natural short side capped at 2160; Original single segment = that
  size rounded down to even; Original multi-segment: long side capped at 8192; sides even); §10
  preset table (GIF not even-rounded); `cover_src_rect`; pt/px conversions; `flip_y`; even-px
  region snapping; aspect-locked resize (Free, 16:9, 9:16, 4:3, 1:1, "Match frame" = canvas ratio);
  `small_content`.
- **Tests:** worked examples (7680×2160 into 1920×1080 → (0, 270, 1920, 540); into 1080×1920 → s
  0.140625, (0, 808, 1080, 304); 7680×2160 → 3840×2160 in 16:9, 2160×3840 in 9:16, 7680×2160 in
  Original; 3024×1964 in 16:9 → 3492×1964); the full preset table; seeded-xorshift property tests
  (dst inside canvas, aspect error ≤ 1 px, centred within 0.5 px, both edges snapped, 100k cases);
  region resize keeps ratio and bounds; `small_content(1080×1920, 7680×2160)` is true.
- **Commands:** `cargo test --locked -p simple-capture-geometry`; Windows check.
- **Done when:** tables match ARCHITECTURE §8/§10 exactly.

#### wp-platform-synth: synthetic platform

- **Owns** `crates/platform/**` (traits frozen). **Implements** `sc_platform::synthetic` and
  `CpuNv12Frame` (§4.3).
- **Build:** `SyntheticPlatform` implementing every trait from a `Scenario` with scripted PTS on a
  host clock that runs `time_scale`× real time (`host_now`, `host_duration` consistent with it);
  `with_writers` for real writers/decoders; on-demand named sources (`display_spec`); coded frames
  (`read_code` with a mid-grey threshold); `.sctrk` writer/decoder (header line, length-prefixed
  records, flush per record, torn tail ignored, `probe` duration and sample count); scripted
  permissions, disk, devices and `at()` events delivered through `watch_system`; `start_fails:
  WhileOtherStreamRuns`; `first_frame_size`; audio `clock_ppm`.
- **Tests:** code round-trip through `.sctrk`; torn-file probe; trailing frames after a new stream;
  injected gap and overlap PTS; `clock_ppm` moves PTS by the expected amount over 60 s of scripted
  buffers; `time_scale(10.0)` delivers 10 s of 30 fps PTS in about 1 s; a stream that fails only
  while another runs.
- **Commands:** `cargo test --locked -p simple-capture-platform`; Windows check.
- **Done when:** tests pass; the crate docs give one example per Scenario feature.

#### wp-mac-sck: ScreenCaptureKit, picker, permissions, AppKit glue, test apps

- **Owns** `crates/capture-macos/src/sck/**`, `tests/sck_*.rs`, `tests/sck_live/**`,
  `tools/devtools/src/testapps/**`.
- **Implements** `MacScreenSource`, the screen stream, `MacSystemAudio`, `MacPicker`,
  `MacPermissions`, `MacOverlays`, `MacDesktop`, `watch_system` (sleep/wake via NSWorkspace
  notifications, `NSMenuDidBeginTrackingNotification` → `OwnWindowsChanged`, plus the av device
  watcher), `free_disk_bytes`, `default_projects_dir`, `local_now`, `capabilities` (§4.3–4.4).
  Follows the threading contract: no run-loop pumping; main-queue hops only for picker, overlays,
  desktop and requests.
- **Port:** spike 3 `capture.rs`, `swap.rs`, `record.rs` (`perms`, `list`), `testwin.rs`,
  `swaphelper`.
- **Build:**
  - `displays()`: `CGGetActiveDisplayList` + display modes + NSScreen names (no TCC).
  - `list(hide)`: SCShareableContent (on-screen, no desktop windows), minus `hide` and our process's
    windows; `z_index` from `CGWindowListCopyWindowInfo` order; frames in global top-left points.
  - `start`: filters for display, window (`desktopIndependentWindow`), application (display + the
    app found by `AppRef.pid`, our windows excepted), region (display filter + `sourceRect` in
    display-local points, even px via `snap_region_even_px`), `Picked` (picker filter; `sourceRect`
    when `region_pt` is set). Exclusions at every start = `cfg.exclude` ∪ every window our pid owns,
    from a fetch with `onScreenWindowsOnly = false` (try `SCShareableContent.currentProcess`, 14.4+,
    first, since it needs no grant; report what it lists). `SC_EXCLUSION=app` (any build) instead
    excludes our application by pid, for the owner's release-bundle re-test (ADR-010). Config:
    420v, BT.709, queueDepth from cfg, fps, cursor, `showMouseClicks` only on macOS 15+ when asked,
    **video only**. Output callback: retain the sample buffer as a `NativeFrame`, read
    `SCStreamFrameInfo` (skip idle/blank), content rect and scale, `sink.on_screen`.
    `didStopWithError` → `StreamStopped`; window closed / display gone → `SourceLost` (report which
    callback or status actually fires); content rect/scale change → `SourceResized`.
  - `update_exclusions` as §4.3 (waits for completion).
  - `describe` for every kind; `snapshot` with `SCScreenshotManager`.
  - System audio: one audio-only SCStream for the recording (ADR-035), `excludesCurrentProcessAudio`,
    48 kHz stereo; `follow_app(Some(app))` switches its filter to an application-based filter for
    that pid, `None` back to the display filter; measure any gap a filter change causes.
  - Picker: `SCContentSharingPicker` configured **once per launch** (all supported modes; the
    research verification says `allowedPickerModes` cannot be reset to "all" once narrowed),
    `maximumStreamCount = 0`, `excludedWindowIDs` = our registered windows; `present()` on the main
    queue; observer forwards `didUpdateWithFilter:forStream:` and cancel; `set_active`. Switching
    uses a new stream from the picked filter (§10 item 10), not `present(for:)`.
  - Permissions: preflight/status APIs only in `status`; `request` calls
    `CGRequestScreenCaptureAccess` / `requestAccessForMediaType`; Privacy pane URLs.
  - Overlays (`configure`, main thread): runtime class swap to an NSPanel subclass defined in
    `sck/` (ecosystem note §2b), `nonactivatingPanel`, force the window device so
    `windowNumber > 0` (else Err), `canJoinAllSpaces | fullScreenAuxiliary`, level per role (bar
    and popovers above normal windows; region and pick at screen-saver level), register the ID.
  - Desktop: `activateFileViewerSelectingURLs`; `NSOpenPanel` (PNG/JPEG) on the main queue.
  - testapps: `sc-devtools testapps window --color C --title T [--size WxH] [--floating]
    [--close-on-signal USR1] [--second-window-on-signal USR2]`, `testapps tone --hz F [--window]`,
    `testapps fixture` (the standard set: SC-TEST-RED 640×480, SC-TEST-GREEN, SC-TONE-440 with a
    window, SC-TONE-660 without, SC-STATIC (never repaints); prints JSON with pids and window IDs).
- **Decide and report (spike-sized tests first):** region by `sourceRect` vs crop (617×283.5 pt,
  1×1 pt, 1121×641 px on the Retina display: delivered size = even-snapped px, content = requested
  area; implement `method: "crop"` if `sourceRect` misbehaves); application capture with a second
  window opened mid-recording; hidden-at-launch windows excluded after being shown (else the
  fallback: update exclusions and wait before showing); App audio with SC-TONE-440 vs SC-TONE-660
  (Goertzel: 440 present, 660 absent); which callback a closed window produces; what
  `currentProcess` returns.
- **Live tests** (`sck_live`, fixture apps started by test-live outside the host, IDs from
  `SC_FIXTURE`): each source kind delivers frames of the expected size within 1 s; the same on a
  non-main display (`SKIPPED:` with one display); make-before-break display ↔ window ↔ region:
  gap ≤ 34 ms at 30 fps, 0 wrong-content frames (colour classification inside fixture rects);
  switch to SC-STATIC: first frame within 1 s (report latency); close the captured SC-TEST-RED
  (USR1) mid-stream: `SourceLost` or `StreamStopped` arrives within 1 s (report which); our own
  in-process window (via `on_main`) and a hidden-then-shown window excluded; a window of ours
  created after start is excluded after `update_exclusions`; App audio; picker configuration
  read-back (not presented); `status` returns `Granted` for all three with no prompt.
- **Commands:** `cargo test --locked -p simple-capture-capture-macos`, `just test-live
  simple-capture-capture-macos sck_`, clippy.
- **Done when:** all live tests pass under the dev host with zero new permission prompts (stated),
  "ran N tests" > 0 for both commands, and each decision above is reported with evidence.

#### wp-mac-av: AVFoundation devices, writers, readers

- **Owns** `crates/capture-macos/src/av/**`, `tests/av_*.rs`, `tests/av_live/**`.
- **Implements** `MacMic`, `MacCamera`, `MacWriterFactory` + `TrackWriter`, `MacDecoder` +
  readers, `watch_devices` (§4.3); exposed through `sc_capture_macos::{writers, decoder}`.
- **Port:** spike 3 `avcap.rs`, `writer.rs`, `decode.rs` (reader), `encbench.rs`; replace
  `pump_recv` with `util::wait_on`.
- **Build:** device discovery and `best_format` ≤ 1080p30 420v; Continuity Camera detection as in
  the spike; `watch_devices` via connect/disconnect notifications → `DevicesChanged`, a running
  device disappearing → `DeviceLost`; host-time timestamps; `show_mic_modes`
  (`showSystemUserInterface(.microphoneModes)`, main queue). Writers (ADR-008): fragmented `.mov`,
  HEVC `hvc1` 420v (H.264 only when asked and both sides ≤ 4096), BT.709 limited tags, keyframe
  interval, bitrate from `bits_per_pixel`; the file is created and `startWriting` +
  `startSession(0)` called inside `video()`/`audio()` (so record can pre-create); `append_video`
  for SCK/AV sample buffers (retimed to `file_ns`) and `CpuNv12Frame` (copied into a pool buffer),
  `NotReady` instead of blocking; `append_audio` (AAC LC 48 kHz; `skip_frames` drops leading
  samples); `append_silence` (zeroed LPCM in the last input format); `finish(end)` with a timeout.
  Decoder: `AVAssetReader` 420v IOSurface-backed video, interleaved f32 audio at the requested
  format, `probe` that works on a killed fragmented file. `encode_benchmark` from `encbench.rs`.
- **Tests (non-TCC, CI macOS):** synthetic `CpuNv12Frame`s → writer → reader round trip (codes
  readable with `read_code` after HEVC at the default bits_per_pixel, PTS within 1 ms, size, BT.709
  tags via ffprobe when `SC_REQUIRE_HELPERS`); 200 ms audio PTS gap + `append_silence` → decoded
  length = host span within 1 ms; `skip_frames` trims exactly; writer SIGKILLed mid-file (child
  process) → `probe` readable with ≤ 1 s loss; `encode_benchmark` 1920×1080 > 0. All real-writer
  tests honour `SC_ALLOW_NO_HW_ENCODER`. **Live (`av_live`):** camera and mic deliver host-time
  samples; device lists non-empty; `show_mic_modes` returns without error (do not interact).
- **Commands:** `cargo test --locked -p simple-capture-capture-macos`, `just test-live
  simple-capture-capture-macos av_`.
- **Done when:** both pass with "ran N tests" > 0.

#### wp-decode: segment-aware frames and track audio

- **Owns** `crates/decode/**`. **Implements** §4.6 `sc_decode`; consumes `Decoder` (synthetic
  `SctrkDecoder` in tests).
- **Build:** `SegmentFrameSource` for monotonic `t_rec` (binary-search the segment; one open
  reader per active segment; newest frame at or before `t − start_rec_ns`; hold across gaps, before
  a segment's first frame and over dropped segments; `held` flag; before any frame → `None`).
  `TrackAudio` pads silence before `first_sample_offset_ns` and after the end. `cfr_frame_times` =
  `round(n·1e9/fps)` up to the duration.
- **Tests (synthetic):** three segments with a gap; idle periods → CFR holds; the boundary picks
  the new segment exactly at `start_rec_ns`; audio offset padding; a dropped segment holds.
- **Commands:** `cargo test --locked -p simple-capture-decode`; Windows check.
- **Done when:** tests pass.

#### wp-text: Skia FreeType build and background raster

- **Owns** `crates/text/**`. **Implements** §4.6 `sc_text`. Uses the shared `target/skia` dir.
- **Build:** ADR-018 FreeType build; `build_info` (FreeType on, JPEG decode on, WebP off);
  `rasterize_background`: solid (alpha forced to 1), linear (angle, 2–4 stops) and radial (centre,
  radius of half the diagonal) with dither, image (`Image::from_encoded`, cover via
  `sc_geometry::cover_src_rect`, high-quality sampling); premultiplied RGBA8 sRGB; `BackgroundCache`;
  `encode_png` (sRGB chunk), `decode_image`; `layout` placeholder.
- **Tests:** each kind at 1920×1080 and 1080×1920 (solid exact; gradient endpoints within 1/255;
  radial centre; image cover crop picks the right quadrant of a PNG and a JPEG); cache hit returns
  the same `Arc`; PNG round trip; `build_info`.
- **Commands:** `cargo test --locked -p simple-capture-text`; list what the Skia build vendors
  (`args.gn`, linked objects) for LICENSES §2.4.
- **Done when:** tests pass; report clean build time and the vendored library list.

#### wp-render: canvas compositor and CPU reference

- **Owns** `crates/render/**`. **Implements** §4.6 `sc_render`.
- **Port:** spike 4 `gpu.rs`, `composite.rs`; spike 3 `decode.rs` BT.709 limited YUV→RGB maths.
- **Build:** wgpu device (`Auto`: hardware, then software adapter, then CPU-only; `SC_WGPU_BACKEND`
  override); NV12 planes uploaded from `with_nv12` (CPU upload in M1, §10 item 1); `src` → `dst`
  over the background texture, gamma-space premultiplied blending (ADR-021); one downscale filter
  for large contain ratios (for example a GPU mip chain + trilinear, or a box prefilter then
  bilinear), mirrored exactly in `cpu::render_cpu`; `Rgba8` and `I420Bt709Limited` output with
  256-byte padded readback, packed result.
- **Tests:** GPU vs CPU on a test pattern for every aspect at reference sizes and scale factors 1,
  0.5, 0.25, 0.140625: max ≤ 2, mean ≤ 0.01 on Metal, max ≤ 3 on software adapters; YUV→RGB→YUV
  within 2; letterbox edges exactly at `dst`; `SKIPPED:` without an adapter unless `SC_REQUIRE_GPU=1`.
- **Commands:** `SC_REQUIRE_GPU=1 cargo test --locked -p simple-capture-render`.
- **Done when:** tests pass on Metal; report render + readback ms at 1080p and 2160p and the filter.

#### wp-helpers: sidecar process management

- **Owns** `crates/helpers/**`. **Implements** §4.6 `sc_helpers`.
- **Port:** spike 1 `helper.rs`, `sys.rs` (FFI → `libc`), `frames.rs`, `wav.rs`, `probe.rs`.
- **Build:** `locate` exactly as §4.6 (never `$PATH`); spawn in its own process group (unix);
  stdin writer thread with a bounded queue (default 3 frames, ADR-015); stderr parsed for
  `-progress pipe:2` blocks and the last error lines; `finish`; `cancel` = SIGTERM → 1.5 s →
  SIGKILL (Windows: `kill`); `PartFile` (exclusive create of `<final>.part`, `commit` renames
  without overwriting, `sweep_stale`); `WavWriter`; `helper_info` (sha256, `-version`,
  `-buildconf`, `network_disabled` = `--disable-network` present). Unix-only parts `cfg`-gated.
- **Tests:** a fake helper test binary (slow reader, hang, exit 1) for backpressure, cancel timing
  (1.5 s ± 0.3 s), no leftover process, EOF exit; Y4M header bytes; PartFile semantics; WAV header
  after `finish`; with `SC_REQUIRE_HELPERS=1`: a 2 s x264 encode of a Y4M pattern and
  `helper_info(ffmpeg).network_disabled == true`.
- **Commands:** `cargo test --locked -p simple-capture-helpers`; the same with
  `SC_REQUIRE_HELPERS=1` after `just helpers`; Windows check.
- **Done when:** tests pass; `pgrep -f third_party/out` is empty after the suite.

#### wp-record-recovery: recovery scan

- **Owns** `crates/record/src/recovery/**`, `crates/record/tests/recovery_*.rs`. **Implements**
  `recovery::{recover_project, scan, RecoveryReport, Outcome}`.
- **Build (ARCHITECTURE §12 step 3):** take `ProjectLock` (`Locked` when held); rebuild segments
  from `project.json` + `segment_open`/`segment_close`; a `segment_prepare` with no `segment_open`
  is ignored (file kept, noted); `Decoder::probe` every track; a segment without `segment_close`
  ends at start + measured duration (`end_reason: "recovered"`); an unreadable segment is dropped
  from the list (file kept, `SessionLog` line); fold pauses, gaps, devices, `track_finished` and
  health into `project.json`; `stop_host_ns` from the longest track; state `recovered` or
  `damaged`; never modify or delete originals; idempotent; `scan` walks the folder for project
  dirs in state `recording`.
- **Tests:** hand-built fixtures with `SctrkWriterFactory` and `JournalWriter`: missing
  `segment_close`; torn journal; prepared-never-opened file; garbage segment → `damaged`; first
  segment unreadable; `AlreadyFinal`; lock held by a live process → `Locked`; second run
  byte-identical.
- **Commands:** `cargo test --locked -p simple-capture-record`; Windows check.
- **Done when:** tests pass; the report lists every journal field recovery relies on.

#### wp-cli: the `simple-capture` command

- **Owns** `cli/**`. **Implements** §5.1 against `sc_record::{Session, recovery}`, `sc_export`,
  `sc_platform` (native: `sc_capture_macos::platform()` on macOS, placeholders elsewhere;
  `synthetic` via `SyntheticPlatform::new(Scenario::new())` and `synthetic::display_spec`).
- **Build:** parser (port spike 3 `Args`), SPEC and BG parsing, `record` with the macOS main thread
  in `mainloop::run_until` and the session on worker threads; scripted `--switch`; SIGINT/SIGTERM →
  graceful stop (`libc` signal flag); a `stop` line on stdin (EOF ignored); `--events` JSON lines;
  `--own-test-window` (debug builds); prints the project folder; `recover`, `export` with progress,
  `validate`, `probe` (displays from `displays()`), `perms`, `sources`, `displays`, `devices`,
  `--self-test`; `window-title:` resolves through `list`. The cfg-specific platform selection lives
  in one module so `--no-default-features` builds without Skia.
- **Tests:** parser and spec tests; `record --platform synthetic --source synthetic:a --switch
  1=synthetic:b --duration 3` written as an integration test marked
  `#[ignore = "integration: un-ignored by wp-e2e-cli"]` (it needs the real Session).
- **Commands:** `cargo test --locked -p simple-capture-cli`; `cargo clippy --target
  x86_64-pc-windows-msvc -p simple-capture-cli --no-default-features`;
  `tools/macos-dev-host/run.sh target/debug/simple-capture perms` and `displays` (no recording).
- **Done when:** tests pass; `perms` under the dev host prints three statuses without a prompt;
  `simple-capture sources` run directly (not through run.sh) fails with the tripwire message.

#### wp-record-session: recording orchestration

- **Owns** `crates/record/src/session/**`, `crates/record/src/testhooks.rs`,
  `crates/record/tests/session_*.rs`. **Implements** `Session`, `RecordRequest`, `Tuning::default`,
  states, notices, health (§4.5). **Consumes** every `sc_platform` trait, `sc_core::{project,
  journal, sessionlog, folder, time}`, `sc_geometry`.
- **Port (logic):** spike 3 `record.rs` start/stop and `extend_screen_to`; spike 3b `swap.rs` router.
- **Build (ARCHITECTURE §4–§6, §12):**
  - **Start:** check `permissions().status(Screen)` (and camera/mic when requested): not granted →
    `Err(Platform(PermissionDenied))` before any folder exists. Check disk (`disk_stop_bytes × 2`).
    Create the folder, acquire `ProjectLock` and hold it until finalize or discard, write
    `project.json` (state `recording`), `journal.jsonl` `start`, open `session.log`, copy the
    background image. All time comes from `Platform::host_now`/`host_duration`.
  - **Start gate:** start every source; wait until each required source delivered a sample (timeout
    `start_timeout_ms` → clear error, everything stopped, folder removed); `start_host` = now after
    warm-up; discard earlier samples; restamp the newest pre-start screen frame at `start_host`;
    record first-sample offsets per track. For aspect Original, set `canvas.ratio =
    Ratio::of(s0 content_px)` from the **first frame**, not from `describe()`, when s0 opens, then
    write `project.json`.
  - **Queues and threads:** bounded `sync_channel`s (defaults: screen 3 with SCK queueDepth 6,
    camera 8, audio 256); video drops the newest frame when full and counts; audio overflow counted,
    logged, notice. Threads: control (commands, events, switch controller, `project.json`
    rewrites), screen writer/router, finisher, one writer per continuous track, health (1 Hz:
    journal `health`, disk poll, `FramesDropping` when a track's drops over the last 10 s exceed
    `frames_dropping_pct`).
  - **Switch (make-before-break):** journal `switch_begin` → journal `segment_prepare` and
    pre-create the new writer at `describe()`'s frame px (file exists only after the journal line)
    → start the new stream (new tag) → the router cuts on its first complete frame `t1`: journal
    `segment_open`, append at file time 0 (if the frame size differs from the pre-created writer,
    finish and delete that empty prepared file and create the writer at the real size, counted),
    old-stream frames with PTS ≥ `t1` dropped and counted, journal `segment_close` with
    `trailing_dropped`; the router hands the old writer to the finisher (retimed copy of the last
    frame at `cut − 1/fps` when older, `finish(cut)`) and tells the control thread, which stops the
    old stream and rewrites `project.json`.
  - **Switch failure (ADR-034 order):** (1) the new stream fails to start while the old one runs →
    journal `switch_failed{next: retry_stop_then_start}`, stop the old stream (its segment stays
    open; the hold covers the gap), start the new one again; (2) that fails → `switch_failed{next:
    restore_old}`, start the old source again as a new segment; (3) that fails →
    `switch_failed{next: source_lost}` and the source-lost path. (4) The new stream starts but no
    complete frame arrives within `switch_first_frame_timeout_ms` (default 1500) → stop it, delete
    the empty prepared file, `switch_failed{next: keep_old}`, notice; the old segment continues.
    Never two streams running after a switch settles.
  - **Gap filler (mic and system audio), a pure component `session::gapfill`:** expected next PTS
    from cumulative samples anchored at the first buffer; a buffer later than expected by more than
    `gap_threshold_ns` (default 2 ms; report the distribution) → `append_silence` first, journal
    `audio_gap{kind: gap}`; earlier by more than the threshold → trim the overlapping leading
    samples via `skip_frames` (a buffer entirely inside the overlap is dropped), journal
    `audio_gap{kind: overlap}`. This re-anchors to host time in both directions, so device clock
    skew is corrected in small steps; `drift_ns` in health reports what remains.
  - **Stop(reason):** `stop_host` = now; re-append the last screen frame at stop when older than one
    frame; finish all writers; journal `stop{reason}`; `project.json` finalized (segments with
    `end_rec_ns`, tracks with durations, health, `edl` = one clip, state `finalized`); release the
    lock. `pause`/`resume` return `Unsupported` (M7).
  - **Events:** `SourceLost` → segment ends `source_lost`, hold, notice, continue until a switch or
    stop; `DeviceLost` → that writer finishes cleanly, journal `device` and `track_finished`,
    continue; `SystemWillSleep` → `stop(Sleep)` + `StoppedForSleep`; `OwnWindowsChanged` →
    `update_exclusions` on every running stream; disk below `disk_warn_bytes` → `DiskLow`, below
    `disk_stop_bytes` → `stop(Disk)` + `StoppedForDisk` (defaults 2 GB / 500 MB; report).
  - **System audio App mode:** on each segment open, `follow_app` with the segment's `AppRef`
    (application or window segment), `None` for display/region.
  - **session.log:** key lines (start, every journal event in readable form, errors, notices) via
    `SessionLog`; no tracing subscriber in this crate.
  - **testhooks:** `hold(point)` called at the journaling site on the journaling thread
    (`after_segment_prepare`/`after_segment_open` on the router or control thread that writes them).
- **Tests (synthetic, CI all OSes; `--test-threads=2`):** every §6.3 "record" item, including: a
  switch every 500 ms for 6 s across three sizes; trailing frames dropped and counted; start fails
  only while the old stream runs → retry succeeds (gap ≤ stop + start latency of the scenario);
  retry fails → restore old; new source idle from start → `keep_old` after the timeout and
  `active_screen_streams() == 1`; source lost then switch; device lost; sleep event → finalized with
  `stop.reason == sleep`; disk stop; denied screen permission → `PermissionDenied`, no folder left;
  `describe()` ≠ first frame → Original ratio from the frame; `recover_project` on a live session →
  `Locked`; journal order (`segment_prepare` before the file exists, `segment_open` before the
  first append, `segment_close` at the cut); gap filler units: scripted gaps and overlaps, and
  ±100 ppm skew over 30 simulated minutes (pure unit, no threads) stays within 33 ms of host time.
  **macOS non-TCC:** the switch test with `SyntheticPlatform::with_writers(sc_capture_macos::
  writers(), sc_capture_macos::decoder())` (real AVAssetWriter files; decoded codes match
  segments; honours `SC_ALLOW_NO_HW_ENCODER`).
- **Commands:** `cargo test --locked -p simple-capture-record -- --test-threads=2`; clippy;
  Windows check.
- **Done when:** tests pass on macOS and the Windows check passes; the report lists the `Tuning`
  defaults with reasons and the cut sequence timings from the macOS non-TCC test.

#### wp-export: quick export and probe v1

- **Owns** `crates/export/**`. **Consumes** `sc_render::Compositor`, `sc_decode`, `sc_text::
  BackgroundCache`, `sc_helpers`, `sc_geometry`, `sc_core::project`,
  `sc_capture_macos::{writers, decoder}` on macOS. Uses `target/skia`.
- **Port:** spike 1 `jobs.rs` (x264 + aac_at args, colour tags), `probe.rs` (VT size probe).
- **Build (ARCHITECTURE §10–§11):** plan (reference canvas or a size class, no upscaling beyond the
  reference; fps = `fps_requested` unless given; `cfr_frame_times`; per-frame `contain_fit` of the
  active segment's `content_px`; background from the cache); audio mix (mic + system audio at 0 dB
  into 48 kHz stereo f32 WAV via `WavWriter`, mono upmixed, clamped, frame N gets samples up to
  `floor((N+1)·48000/fps)`; no audio → `-an`); video as Y4M I420 through `HelperProcess` with
  `libx264 -preset medium -profile:v high -pix_fmt yuv420p -crf 22`, BT.709 limited via `setparams`
  and output tags, `aac_at` 160k, `-movflags +faststart`, `-f mp4` into the `.part`; never
  `-ss/-to/-r`/scaling; default output next to the project folder (`<name>.mp4`, " 2" on
  collision); cancel deletes the `.part`; progress and states; `ffprobe_check`. Probe v1:
  helper info; `h264_videotoolbox -allow_sw 0` open test at 3840×2160, 4096×2160, 4098×2160;
  `encode_benchmark` HEVC at 60 fps for each display's native px and 1× px (from `displays`);
  `allows_60fps` = measured ≥ 1.3 × 60 (ADR-009); in-progress marker and quarantine; cache JSON.
- **Tests:** synthetic project → export (CI: plan, render and mix without the helper; with
  `SC_REQUIRE_HELPERS=1`: full MP4 + ffprobe) for every aspect × background kind and every size
  class; three segments of different sizes and a gap; cancel (no `.part`, no process); the WAV has
  exactly `floor(frames·48000/fps)` samples, and in the MP4 the audio and video durations differ by
  at most one AAC frame (21.3 ms) plus one video frame; **colour patches** (Pitfall 5): export a
  synthetic colour-patch project, decode the MP4 to RGB with the helper's ffmpeg, every patch
  within 3/255; probe on this Mac returns a `DisplayProbe` per display.
- **Commands:** `cargo test --locked -p simple-capture-export`; `SC_REQUIRE_HELPERS=1
  SC_REQUIRE_GPU=1 cargo test --locked -p simple-capture-export`.
- **Done when:** both pass locally; report export speed at 1080p and 3840×2160 for a 30 s synthetic
  project, and at 7680×2160 (for Q6).

#### wp-app-shell: Tauri shell

- **Owns** `app/**`. **Implements** §5.2–§5.4 on top of the crate APIs; real data where the merged
  crates provide it, stub-backed otherwise. Uses `target/skia`.
- **Port:** spike 2 `src-tauri/src/main.rs` (async URI scheme, window builder options,
  transparency without `macOSPrivateApi`).
- **Build:** activation policy Accessory; the seven windows created hidden at launch and
  configured through `overlays().configure`, their IDs in app state and in every
  `RecordRequest.exclude`; region/pick windows moved to the target display before showing; tray
  (§5.3) with the elapsed timer; ⌥⇧S; app state machine (Idle → Recording ⇄ Switching → Stopping
  → Saved → Exporting); the `sc://still` protocol route (real compositor PNG once render/text/
  export are merged; a placeholder PNG before); settings JSON; launch tasks on a background thread
  (recovery scan, stale `.part` sweep, probe v1 from `displays()` only); `list_sources` only after
  screen = granted; quit/signal exit path (§5.2); `watch_system` events forwarded to the session;
  picker, overlays and NSOpenPanel through `run_on_main_thread`; tracing subscriber (stderr + app
  log file) owned here; the §5.4 driver; `just run` works (no Info.plist embedded:
  `otool -s __TEXT __info_plist target/debug/simple-capture-app` is empty).
- **Tests:** state machine, settings round trip, DTO conversions, driver script parser; `cargo build
  -p simple-capture-app` on macOS; `just run` under the dev host prints `SC_READY`; a driver script
  that queries `get_permissions`, `list_displays`, `log-window-rects` and quits; `just bundle`
  (after `just helpers`), then without launching: `codesign -dv --entitlements - <app>` shows
  `adhoc,runtime` and both entitlements, `plutil -p <app>/Contents/Info.plist` shows the three usage
  strings, `codesign -dv <app>/Contents/MacOS/ffmpeg` (and ffprobe) shows ad-hoc + runtime.
- **Done when:** the app launches under the dev host with every window hidden and configured
  (window IDs > 0 in the log), the tray present, no permission prompt, and the bundle inspection
  passes.

#### wp-ui: Svelte 5 UI

- **Owns** `ui/**`. **Implements** the screens over `api.ts` (§5.2) against `mock.ts` in a browser.
- **Build:**
  - Recorder panel: source picker (displays always; windows and apps only when screen = granted,
    otherwise "Grant Screen Recording to list windows" + System picker; "Select region…", "Pick
    window by clicking", "System picker…"); Frame control (Original/16:9/4:3/1:1/9:16, "Original =
    first source's shape" hint, and a warning when `small_content` holds for the chosen source);
    Background popover (solid palette + custom, gradient presets with 2 stops, angle, linear/radial,
    image Choose…); still mini preview; camera ("recorded now, shown in M2"), mic with a
    "Mic Mode…" button (only when `capabilities.mic_modes`), system audio Off/All/App; fps 30 / 60 /
    "60 fps at 1×" enabled per the probe for the source's display, with the reason when disabled;
    resolution; cursor; click highlight (hidden when unsupported); Record.
  - Onboarding: three statuses, explanations, Request buttons, "Open System Settings", the ad-hoc
    re-prompt note and the macOS 15 re-prompt / system picker note (ARCHITECTURE §13).
  - Floating bar: timer, Stop, Switch source, Discard (confirmation dialog), notices; on
    `SourceLost` a persistent "Source closed — Switch source · Stop" prompt; on `FramesDropping`
    "Frames are dropping: try 30 fps or 1× next time"; `StoppedForSleep` / `StoppedForDisk` shown in
    the Saved window.
  - Switch popover: Recent (last 5), displays, windows (search), Select region…, Pick by clicking,
    System picker….
  - Region overlay: drag, eight handles, numeric W×H, presets Free/16:9/9:16/4:3/1:1/Match frame,
    Esc cancels, remembers the last region per display; display-local points; `src/lib/geometry.ts`
    mirrors `sc_geometry` (unit-tested against the same tables).
  - Pick-mode overlay: hover highlight of the topmost window under the cursor using `z_index`,
    click picks, D = display, drag = region, P = system picker, Esc cancels, hint card.
  - After a system-picker display pick: "Whole display / Select region…".
  - Saved window: duration, size, Export MP4 (progress, cancel), Reveal.
  - Accessibility: focus order and labels; no network requests, no external fonts.
- **Tests:** `pnpm check`, `pnpm build`, `pnpm test` (geometry tables, small-content rule, DTO
  guards, recent-sources list, z-order hit test).
- **Done when:** all pass; every screen renders against the mock (Browser pane screenshots
  allowed; no TCC involved).

#### wp-ci: CI, license and network gates

- **Owns** `.github/**`, `deny.toml`, `about.toml`, `scripts/ci/**`. Owner answers to Q3 and Q10
  go here. The justfile is frozen; its CI targets call these scripts.
- **Build:** the §6.4 workflow (fast and full jobs per OS, timeouts, separate Skia cache, macOS
  helper cache keyed by `third_party/versions.toml` + script hashes then `SC_REQUIRE_HELPERS=1`,
  `SC_ALLOW_NO_HW_ENCODER=1` on macOS runners with a visible warning, `SC_REQUIRE_GPU` only where an
  adapter exists, Ubuntu packages webkit2gtk-4.1, gtk3, ayatana-appindicator, rsvg, xdo, clang,
  ninja, python3, libfontconfig1-dev, mesa-vulkan-drivers; Windows LLVM, ninja, python, short
  target dir); cargo-deny (licenses, sources, advisories, LICENSES §2.1 bans, plus bans on
  reqwest, hyper, ureq, isahc, curl, attohttpc, surf; if they appear only as optional Tauri
  dependencies, prove with `cargo tree --target all -i <crate>` that nothing builds them and
  document a narrow exception instead); a check that `ffmpeg-next` is absent; `netcheck.sh`
  (grep `crates/ app/ cli/ tools/` for `std::net`, `TcpStream`, `UdpSocket`, `TcpListener`,
  `tokio::net`, `fetch(`, `XMLHttpRequest`, `WebSocket`; check the Tauri CSP has no remote
  `connect-src`); `npm-licenses.mjs` (no dependency); `ci-local.sh`, `ci-local-fast.sh`,
  `windows-check.sh`, `linux-docker.sh` (§6.4).
- **Commands:** `just ci-local`; YAML reviewed by hand. No push.
- **Done when:** `just ci-local` passes, or the report names each step that could not run and why;
  the workflow is ready to push.

### 8.2 Stage 2

#### wp-e2e-cli: record → switch → stop → export under the dev host

- **Owns** `cli/**`, `scripts/e2e/**`; fixes in non-frozen `crates/**` and `tools/devtools/**`.
- **Build:** un-ignore wp-cli's synthetic integration test (and make it pass: record with a switch,
  `recover` finds nothing, `export` when the helper exists). `just e2e` (`scripts/e2e/run.sh`): via
  `scripts/lib/live.sh`, fixture apps started outside the host, every recording through
  `run.sh target/debug/simple-capture record …` into a temp projects folder, stops through
  `--duration` or `SC_DEV_PID_FILE` + `kill -TERM`:
  1. main display 10 s, mic + system audio + camera → export reference size → `validate`;
  2. window SC-TEST-RED 8 s; 3. application by `app-pid:` (SC-TONE-440) with a second window
     opened at 3 s (USR2); 4. region 617×283.5 pt and 1121×641 px;
  5. display → window → region → display every 3 s, system audio All; report per switch: request
     host time → cut (from the journal) against the 500 ms target, `trailing_dropped`, the recorded
     gap (decoded last frame of segment n vs first frame of n+1), and peak RSS (`ps` every 200 ms)
     with two native-size streams overlapping;
  6. system audio App: switch between SC-TONE-440 and SC-TONE-660's windowed variant; Goertzel per
     span;
  7. source lost: window SC-TEST-RED closed (USR1) at 4 s, `--switch 7=display:main`, stop at 10 s:
     `end_reason source_lost`, held frame until the new segment, audio continuous, s1 opens;
  8. switch to SC-STATIC (succeeds; first-frame latency) and to a minimized fixture window
     (`switch_failed{keep_old}` after the timeout, old segment continues);
  9. `--fps 60` at native and at 1× (nominal fps, delivered rate, frame size);
  10. `--resolution 1x --no-cursor` (frame px = frame_pt size; cursor off in the config log);
  11. non-main display: display and region (`SKIPPED:` with one display);
  12. `--own-test-window 2=…`: its colour is absent from every frame;
  13. every aspect × background kind (export only, from one recording);
  14. graceful stop by SIGTERM to run.sh (forwarded) and by a `stop` line through `SC_DEV_STDIN`.
  Checks per run: state `finalized`; segments tile `[0, duration)`; every file decodes (helper's
  ffprobe `-count_frames`, never `$PATH`); segment sizes match descriptors; audio file length =
  recording length ± one frame; export valid.
- **Done when:** `just e2e` passes twice in a row with no new permission prompt; bugs found are fixed
  with regression tests in the owning crate; the report gives the run-5 numbers.

#### wp-app-integration: the app end to end (debug driver)

- **Owns** `app/**`, `ui/**`.
- **Build:** wire the UI to the real commands; real `sc://still`; then run driver scripts with
  `just app-driver` (under `scripts/lib/live.sh`, fixture apps outside the host):
  onboarding statuses; start from each source kind (`cmd start_recording`); switch via the bar
  action, `tray item "Switch Source…"`, `hotkey switch` + `pick window`, `region set`, and
  `cmd switch_source`; **tray menu exclusion**: during a display recording, `tray open-menu`, log
  the menu window rect, stop, and compare that rect in frames during the menu against frames before
  it (equal within noise = excluded; report how many frames, if any, show the menu); bar and popover
  visible during a recording, same comparison; quick export with progress and cancel; `sim sleep`
  during a recording → finalized with `stop.reason == sleep` and the notice; `quit` during a
  recording, relaunch → project `finalized`, not `recovered`; SIGTERM to run.sh during a recording
  → finalized; `kill -9` via the pid file, relaunch → recovery notice and state `recovered`; quit
  during an export → no `.part`, no ffmpeg process; probe cache file written; CSP active (no
  console CSP errors in the app log).
- **Done when:** every flow passes under the dev host, logged with the project folders produced;
  crate bugs reported (not fixed here). Real clicks, key presses, drags, picker picks and hot-plug
  are MANUAL_TEST items, not agent criteria.

### 8.3 Stage 3

#### wp-sync-test: flash + beep (+ camera) + clap

- **Owns** `tools/devtools/src/sync/**`, `scripts/sync/**`.
- **Build:**
  - `sc-devtools sync flashbeep --schedule …` (outside the host; no permission): an 800×600
    floating window SC-SYNC-WIN that turns white for 100 ms at each event while playing a 1 kHz
    100 ms tone (AVAudioPlayer, generated WAV), plus `--fullscreen-at` events that flash a
    borderless full-display white window for 300 ms (for the camera, which sees the room light up);
    logs issued host times.
  - `sc-devtools sync analyze PROJECT --log flashbeep.json`: decode segments (`sc_decode` +
    `sc_capture_macos::decoder()`), flash onsets from the window area's mean luma, camera onsets
    from the camera track's mean luma at full-screen events (report the step size and whether it
    clears the noise floor), 1 kHz onsets in system audio and mic (Goertzel, 2 ms hop); report per
    event video→system audio, video→mic and camera→audio offsets.
  - `just sync-test`: 70 s recording (display; switch to SC-SYNC-WIN at 35 s; back to display at
    50 s; mic + system audio + camera) with events at the start, after each switch and at the end,
    full-screen events at 5 s and 65 s.
  - `sc-devtools sync clap PROJECT` (assisted): loudest mic transient and the camera frame with the
    most motion energy near it; the owner confirms by eye (MANUAL_TEST).
- **Done when:** 3 runs reported (start / after each switch / end; system audio, mic, camera; ms
  against the 33 ms target) with a recommendation for any fixed system-audio compensation
  (ADR-011). If the mic hears nothing or the camera step is below noise, say so; never change the
  volume. States that sync across pauses is M7.

#### wp-soak-test: long run

- **Owns** `tools/devtools/src/soak/**`, `scripts/soak/**`.
- **Build:** `just soak N` (N = 10 by default; 30 for the ADR-010 decision, by an agent only with
  Q7): native display + 1080p camera + mic + system audio at 30 fps, a display ↔ window switch every
  5 min, flashbeep at 5 s and at N − 1 min; samples every 5 s (our RSS and CPU, replayd and
  VTEncoderXPCService CPU; port spike 3 proc functions); after stop: per-track health, dropped %,
  RSS slope from minute 2, gaps and overlaps, `drift_ns` per audio track, sync at start and end,
  `StreamStopped` count. Output a summary in the MANUAL_TEST format so an owner run is comparable.
- **Done when:** a report with the exit-checklist thresholds (§1 item 6) and a recommendation on
  ADR-010 frame copy (copy if SCK stalls, `StreamStopped` or delivery collapse appear; otherwise
  no copy), marked "regression only" unless the run lasted 30 min. Recordings are deleted after the
  summary unless the owner asked to keep them.

#### wp-kill-recovery-test: crash safety

- **Owns** `tools/devtools/src/killtest/**`, `scripts/killtest/**`,
  `crates/record/src/bin/sc-record-synth.rs`, `crates/record/tests/kill_*.rs`.
- **Build:** CI version: `sc-record-synth` (feature `test-hooks`) records with the synthetic
  platform; the test kills it (SIGKILL / TerminateProcess) mid-segment, at `after_switch_begin`,
  `after_segment_prepare` and `after_segment_open` (waits for `SC_HOLD <point>` on stderr), then
  runs `recover_project` and asserts, **from the journal content**, that the kill landed between
  the intended events (for example the last line is `segment_open` with no matching
  `segment_close`), then checks segments, durations and states. Dev-host version (`just killtest`):
  `simple-capture` with `test-hooks` through run.sh with `SC_DEV_PID_FILE`, killed at the same
  points plus a plain 6 s kill with camera, mic and system audio; then `simple-capture recover`,
  ffprobe every file, export.
- **Done when:** every kill point recovers (≤ 1 s loss per track as in [S3]), the recovered project
  exports, both versions pass 3 runs each.

#### wp-manual-test: MANUAL_TEST.md (macOS)

- **Owns** `docs/MANUAL_TEST.md`.
- **Build:** an owner checklist, each step with an expected result and where to look: `just run`;
  onboarding; every source, including a second display at a different scale (display, region, pick
  mode); real region drags; pick mode clicks; the ⌥⇧S key press; switching from bar and tray by
  hand; Mic Mode… opens the system panel; system picker under the dev host (pick a display, a
  window, an app; cancel; switch with it; pick a display then a region; **pick a display with the
  floating bar visible and check the recording for our pixels**, i.e. `excludedWindowIDs` on a
  picked display); aspect × background exports opened in QuickTime; our UI and the tray menu
  absent from recordings; close a captured window → "Switch source · Stop" prompt; unplug the
  display being recorded; unplug camera/mic mid-recording; device lists update on hot-plug; sleep
  the Mac mid-recording → finalized + notice after wake; Quit during a recording; low disk
  (optional disk image); 60 fps gating; clap sync; kill and relaunch recovery; **soak 30 min** (the
  same metrics and thresholds as §1 item 6, with the soak summary format) if Q7 = no; the
  **release-bundle test** (owner only, ADR-028, Q8): never-granted bundle records through the
  picker (note any prompt or bypass alert, whether system audio works, behaviour after a rebuild),
  then the whole-app exclusion re-test (`open -n --env SC_EXCLUSION=app -a "Simple Capture"`,
  record a display with the bar visible, check for our pixels), and the camera/mic prompts appear
  with our usage strings; logging every Screen Recording prompt seen, with the path used. Short
  Windows/Linux sections: "placeholders until M6; CI only".
- **Done when:** every step has an expected result and where to look.

#### wp-ci-green: all three OSes green

- **Owns** `.github/**`, `deny.toml`, `about.toml`, `scripts/ci/**`.
- **Build:** with Q2b, push a branch and iterate until macOS, Windows and Ubuntu are green (fast and
  full jobs, deny, pnpm licenses, netcheck, Skia FreeType builds, cached macOS helper). Code
  failures go to the orchestrator (§0.5); config failures are fixed here. Measure clean and
  incremental dev build times (ADR-031) and CI durations; generate cargo-about notices once.
- **Done when:** the three jobs are green on the final commit (run URLs), or the report shows the
  local results and the exact blocker.

#### wp-docs-close: fold results into the docs

- **Owns** `docs/STATUS.md`, `docs/DECISIONS.md`, `docs/ARCHITECTURE.md`, `docs/LICENSES.md`,
  `docs/THIRD_PARTY.md`, `docs/CODEC_MATRIX.md`.
- **Build:** from every package report: the §10 choices as ADR entries or updates (with the
  measured results), ARCHITECTURE deltas (journal field list, `segment_prepare`, `app_pid`,
  display-local `region_pt`, probe-size rule, picker switching, exclusion rule, quit/sleep rules),
  STATUS (milestone state, measured numbers with sources, open risks, the M7 test items carried
  from pause, the mic meter and debug overlay milestone proposal, the ★ review list), LICENSES
  (Skia vendored set, CI license results), CODEC_MATRIX (export speeds). If the tray-menu check
  showed leaking frames, add an owner question: accept the menu exception or change the tray.
- **Done when:** every number in the updated docs names its source; STATUS ends with the ★ review
  list.

---

## 9. Dependencies

"Approved by" names the accepted ADR or SPEC line; "spike-only" = used by a spike but named by no
ADR; "new" = needs the owner's OK (Q1). Versions are those resolved in the spikes or recorded in
docs/research/ecosystem.md; the scaffold pins them.

### 9.1 Rust

| Crate | Version | License | Used by | Approved by |
|---|---|---|---|---|
| tauri (`tray-icon`; `custom-protocol` via app) | 2.12.7 | Apache-2.0 OR MIT | app | SPEC Tech stack, ADR-004 |
| tauri-build | 2.7.6 | Apache-2.0 OR MIT | app (build) | ADR-004 |
| tauri-plugin-global-shortcut | 2.4.1 | Apache-2.0 OR MIT | app | SPEC Tech stack |
| objc2 | 0.6.5 | MIT | capture-macos, devtools | ADR-006 |
| block2 | 0.6.2 | MIT | capture-macos, devtools | ADR-006 (spike 3) |
| dispatch2 | 0.3.1 | Zlib OR Apache-2.0 OR MIT | capture-macos | ADR-006 (spike 3) |
| objc2-foundation, -core-foundation, -core-graphics, -core-media, -core-video, -core-audio-types, -io-surface, -screen-capture-kit, -av-foundation, -avf-audio | 0.3.2 | Zlib OR Apache-2.0 OR MIT | capture-macos, devtools | ADR-006 |
| objc2-app-kit (`default-features = false` + §8.0 list) | 0.3.2 | Zlib OR Apache-2.0 OR MIT | capture-macos, devtools | ADR-006 (trim features) |
| wgpu | 30.0.2 | MIT OR Apache-2.0 | render | SPEC Tech stack |
| skia-safe (`textlayout`, FreeType source build) | =0.157.0 | MIT (Skia BSD-3-Clause; vendored libs LICENSES §2.4) | text | ADR-018 |
| serde (`derive`) | 1.0.229 | MIT OR Apache-2.0 | all crates | ARCHITECTURE §1 ("project.json v1 (serde)"); spikes |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | core, record, export, app, cli, devtools | spike-only (ARCHITECTURE §1 implies it) |
| tracing | 0.1 (latest patch, pinned by the scaffold) | MIT | all crates | SPEC Engineering ("tracing plus a session.log") |
| tracing-subscriber (`fmt`, `env-filter`) | 0.3 (pinned) | MIT | app, cli (stderr/app log only; session.log is written by core/record) | new |
| sha2 | 0.10 (pinned) | MIT OR Apache-2.0 | core (asset names, helper hashes via core::sha) | new |
| libc | 0.2.191 | MIT OR Apache-2.0 | helpers, cli, core (lock pid check), app (signals), capture-macos, devtools | spike-only (S2, S3) |
| anyhow | 1.0.104 | MIT OR Apache-2.0 | cli, app, devtools (binaries only) | spike-only (S3) |
| pollster | 1.0.3 | Apache-2.0 OR MIT | render | spike-only (S2, S4) |
| bytemuck (`derive`) | 1.25.2 | Zlib OR Apache-2.0 OR MIT | render | spike-only (S2, S4) |

Not used, on purpose: `clap`, `thiserror`, `uuid`, `proptest`, `tempfile`, `png`, `crossbeam`,
`tauri-plugin-dialog`/`-opener`/`-shell`, `wgpu-hal`, `objc2-metal` (M3), `fontdb` (M4). cargo-deny
keeps the LICENSES §2.1 bans (gifski, imagequant, x264/x265 crates, the Cap crates of ADR-023) and
adds the HTTP-client bans of wp-ci.

### 9.2 npm (ui/)

| Package | Version | License | Used by | Approved by |
|---|---|---|---|---|
| svelte | 5.58.0 | MIT | ui | ADR-005 |
| vite | 8.3.8 | MIT | ui (dev) | ADR-005 |
| typescript | 6.0.3 | Apache-2.0 | ui (dev) | ADR-005 |
| @sveltejs/vite-plugin-svelte | 7.3.1 | MIT | ui (dev) | ADR-005 (spike 2) |
| @tauri-apps/api | 2.12.5 | Apache-2.0 OR MIT | ui | ADR-005 |
| svelte-check | 4.7.8 | MIT | ui (dev) | spike-only (S2) |
| @tsconfig/svelte | 5.0.9 | MIT | ui (dev) | spike-only (S2) |
| @tauri-apps/cli | 2.12.6 | Apache-2.0 OR MIT | `just bundle`, icon generation (dev) | ADR-004 (ecosystem; spike 2) |

UI unit tests use Node 26's built-in `node --test`; the license check is a script over
`pnpm licenses list --prod --json` (ADR-024).

### 9.3 Tools and CI

| Tool | Version | License | Used by | Approved by |
|---|---|---|---|---|
| cargo-deny | 0.22.0 (fallback 0.21.0) | MIT OR Apache-2.0 | CI, `just deny` | ADR-024 (local install: Q3) |
| cargo-about | 0.9.3 | MIT OR Apache-2.0 | CI notices | ADR-024 (local install: Q3) |
| just, cmake, nasm, ninja, pkgconf, meson, autotools | Homebrew | various | builds | ADR-027 |
| actions/checkout, actions/cache, actions/setup-node | current major, pinned by commit SHA | MIT | CI | new |
| Docker image (`ubuntu:24.04` base + clang, ninja, python3, build packages) | pinned digest | – | `scripts/ci/linux-docker.sh` | spike 6 used Docker; pulling: Q3 |
| FFmpeg 9.0.2 static GPL helper (macOS arm64), ffmpeg + ffprobe | `third_party/versions.toml` | GPL-2.0-or-later (sidecar) | export, app bundle | ADR-013 |

### 9.4 Fallbacks if Q1 is "no" (the scaffold uses them; no contract changes)

| Item | Fallback |
|---|---|
| sha2 | std-only SHA-256 in `sc_core::sha` (about 100 lines; NIST vectors in wp-core's tests) |
| tracing-subscriber | a small hand-written `tracing::Subscriber` in app and cli (stderr lines, level filter from `SC_LOG`); session.log never depended on it |
| libc | process groups via std's `CommandExt::process_group`; helper SIGTERM via `/bin/kill -TERM <pid>`; the lock's pid check via `/bin/kill -0`. Catching SIGTERM/SIGINT in cli and app needs libc, so without it scripts stop recordings through the stdin FIFO (`SC_DEV_STDIN`) and the app through the driver's `quit`; SIGTERM then ends the process without finalizing (recovery covers it) |
| anyhow | `Box<dyn std::error::Error + Send + Sync>` in binaries |
| pollster | a 20-line `block_on` (thread park/unpark waker) in render |
| bytemuck | explicit `to_ne_bytes` packing in render |
| serde_json | no practical fallback (project.json, journal, IPC); without it M1 cannot proceed as planned |
| svelte-check / @tsconfig/svelte | `tsc --noEmit` on `.ts` files only, an inline tsconfig |
| GitHub actions | `git clone`/`git checkout` in `run:` steps, no cache action (slow Skia builds) |

---

## 10. Plan-level choices (logged in DECISIONS.md by wp-docs-close)

1. **CPU upload in M1 export.** Decoded NV12 planes are uploaded to wgpu from mapped buffers; the
   zero-copy IOSurface import proven in [S3] moves to M3. Same path on every OS and the synthetic
   platform; keeps `wgpu-hal`/`objc2-metal` out of M1.
2. **Region changes during a recording are switches** (new segment, make-before-break); in-place
   `updateConfiguration` is not used in M1, which ADR-034 permits.
3. **`ScreenSource::start` returns after start**; cuts and the start gate use first-frame PTS only.
4. **No new utility crates where std suffices** (§9.1 "Not used").
5. **Live tests** run through `scripts/test-live.sh` under the dev host, fail when 0 tests ran, and a
   debug-build TCC tripwire keeps plain `cargo test` from touching TCC.
6. **Camera track recorded in M1** without a bubble, so soak and sync cover four tracks.
7. **`cursor.jsonl` deferred** until the click-highlight method is chosen (Q5).
8. **M7 items stay in M7:** pause/resume, countdown, start/stop and pause hotkeys, Restart (SPEC
   M7 line). The mic level meter and debug overlay are on no milestone line; proposed for M7.
9. **Own-UI exclusion rule:** every stream excludes our registered window IDs plus every window our
   process owns at start (fetched with `onScreenWindowsOnly = false`); `OwnWindowsChanged` (a menu
   of ours begins tracking) refreshes every stream's exclusions; the tray's Switch Source item opens
   the excluded popover instead of a submenu; our status item is excluded, leaving a gap in the menu
   bar. Leaking frames, if the app-integration check finds any, go to the owner (wp-docs-close).
   Hidden-at-launch overlays are tested first; the fallback is update-and-wait before showing.
10. **Picker:** modes set once per launch; switching through the picker = `present()` + a new
    make-before-break stream from the picked filter (not `present(for:)` with an in-place filter
    update, which [S3b] showed delivers wrong-size frames); "pick a display, then region" =
    `PickedSource.region_pt` + `sourceRect`; `update_exclusions` on a picked stream is unsupported
    until the release-bundle test shows a rebuilt filter keeps picker consent; `excludedWindowIDs` on
    a picked display is verified manually.
11. **Probe-size rule:** probe v1 measures HEVC at 60 fps for every connected display's native and
    1× pixel size (from CoreGraphics, no TCC) at launch; a source may use 60 fps (native or 1×) only
    if its display's probe passed (≥ 1.3 × 60); windows and regions are never larger than their
    display, so switching never re-probes; a new display connected later is probed on the next
    launch, and until then 60 fps is not offered for it. Sustained drops raise `FramesDropping`.
12. **Switch timing and failures:** the new segment's writer is pre-created (journal
    `segment_prepare` first) before the new stream starts; `project.json` is rewritten on the
    control thread after the cut; a first-frame timeout (1.5 s) abandons a stream that never
    delivers; on start failure the ADR-034 fallback order is retry stop-then-start → restore old →
    source lost.
13. **Sleep** stops and finalizes the recording (`stop.reason = sleep`) with a notice after wake;
    with no pause in M1 there is no resume across sleep.
14. **Quit, SIGTERM and logout** stop a recording gracefully and cancel an export; a project is
    `recovered` only after a real crash or kill.
15. **Audio overlap trimming** is symmetric with gap filling, which re-anchors audio to host time
    and corrects device clock skew in small steps (each ≤ the threshold plus one buffer).
    Resampling is not used; the soak's `drift_ns` decides whether M7 needs it.
16. **App identity by `AppRef { pid, bundle_id? }`**; project schema gains `app_pid` and makes
    `app_bundle_id` optional; `region_pt` is display-local.
17. **session.log** is written by `sc_core::sessionlog` from record and recovery, not through a
    tracing subscriber; app and cli own their subscribers.
18. **run.sh** forwards signals to the child, exports `SC_DEV_HOST=1` and supports a stdin FIFO
    (script change only; the signed host bundle is untouched).
19. **Whole-app exclusion re-test** (ADR-010) moves to the owner's release-bundle test via
    `SC_EXCLUSION=app`; per-window exclusion stays the default.
20. **Overlays are repositioned** onto the target display before showing (no per-display windows).
21. **Release bundle config** lives only in `tauri.bundle.conf.json` so plain builds work everywhere.

---

## 11. Owner questions

1. **Dependencies (G1).** Approve the new crates `tracing-subscriber` and `sha2`, the spike-proven
   crates no ADR names (`libc`, `anyhow`, `pollster`, `bytemuck`, `serde_json`), the dev tools
   `svelte-check` and `@tsconfig/svelte`, and GitHub's own CI actions? Each "no" has the fallback in
   §9.4 (`serde_json` has none).
2. **Git (G2, G5).** (a) May agents make **local** commits on a branch `m1` and use git worktrees
   inside the repo (`.wt/`, gitignored) so packages build in isolation? Without it, packages run
   strictly wave by wave in one tree (§0.2 model B, slower). (b) May the orchestrator push `m1` to
   `origin` so GitHub Actions runs on macOS, Windows and Linux? Without it the Windows Skia/Tauri
   build stays unproven.
3. **Local tools (G3).** May agents run `cargo install --locked cargo-deny@0.22.0 cargo-about@0.9.3`
   and pull an `ubuntu:24.04` Docker image for the Linux dry run?
4. **Project folders (G4, ADR-032).** Keep the `.sc` suffix, and make the folder a plain folder or a
   macOS package? Recommended: plain folder with `.sc`.
5. **Click highlight.** ScreenCaptureKit's `showMouseClicks` on macOS 15+ only (hidden on 14), with
   our own drawing and `cursor.jsonl` deferred to M7? Our own drawing needs a global mouse monitor,
   which may bring an Input Monitoring or Accessibility prompt.
6. **Large Original quick exports.** Export a 7680×2160 Original canvas at full size, or cap the
   quick export's long side at 3840? (x264 speed at 7680×2160 is measured in wp-export.)
7. **30-minute soak on your real screen (G6).** May an agent record your screen, camera, mic and
   system audio for 30 minutes under the dev host (files local, deleted after the summary)? If not,
   you run `just soak 30` from MANUAL_TEST.
8. **Release-bundle test (ADR-028, ADR-010).** It runs the never-granted, ad-hoc-signed
   `com.simplecapture.desktop` bundle, which creates real permission prompts, so only you can run
   it. Will you run it at the M1 review (picker without a grant, system audio on the picker path,
   whole-app exclusion, usage strings)? And should the self-signed code-signing certificate option
   of ADR-003 be tried?
9. **WebP backgrounds.** PNG and JPEG only in M1 (recommended), or also WebP (skia-safe
   `webp-decode` plus a libwebp licence entry)?
10. **Skia on Windows CI (G3).** If the FreeType source build cannot work on the Windows runner in
    M1, may Windows CI temporarily use skia-safe's prebuilt binaries (backgrounds only), or must M1
    wait (ADR-018)?

---

## 12. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Skia FreeType build fails or is very slow on Windows/Linux CI | wp-ci from wave 1; separate cache; fast job independent of Skia; Q10 fallback |
| VideoToolbox HEVC missing in GitHub's macOS VMs | `SC_ALLOW_NO_HW_ENCODER=1` in CI only, with a visible `SKIPPED:`; full coverage locally |
| Parallel agents break each other's builds or serialise on Cargo locks | worktrees and per-package target dirs (§0.2); lockfile and fmt rules |
| A test triggers a TCC prompt | debug-build tripwire; `displays()` for anything before onboarding; `list_sources` only after "granted"; release bundle never run by agents |
| Live tests disturbed by other agents or the owner | live lock, caffeinate, envcheck, floating fixture windows, pixel checks only inside fixture rects |
| Hidden-at-launch windows or menus not excluded | tested in wp-mac-sck and app integration; our-pid rule + refresh on menu tracking; owner decision if frames leak |
| "Only the captured app" audio leaks or gaps on filter changes | live test with two tone processes; if it fails, App mode is hidden in the UI for M1 and reported |
| SCK starvation on long runs (ADR-010) | screen queue 3; 30-min soak decides the frame copy |
| Two overlapping native-size streams spike memory/encoder load; slow cut | pre-created writer; measured in e2e run 5 (latency, drops, RSS) and the soak |
| A switch target never delivers a frame | first-frame timeout, `keep_old` |
| Audio drift on long recordings | symmetric gap/overlap handling; `drift_ns` in health and soak |
| `sourceRect` odd sizes / Retina rounding | even-px snapping; crop fallback if needed |
| Synthetic tests flaky on CI runners | scripted PTS, pure-unit long-span tests, `--test-threads=2`, ≥ one-frame tolerances |
| Mic does not hear the beep; camera flash too weak | analyzer says so; clap step in MANUAL_TEST |
| x264 at 7680×2160 too slow | measured in wp-export; Q6 |
| Interfaces drift between packages | frozen contracts, the scaffold's report of the exact signatures, change requests only |

---

## 13. Revision log (2026-11-19, after two critics' reviews)

- **Team mechanics:** added §0 (owner gates before stage 0/1, worktrees with per-package target
  dirs and a shared Skia dir, waves recomputed from real dependencies, merges by the orchestrator,
  lockfile and rustfmt rules, live-resource lock, fix ownership in stages 2–3). wp-cli and
  wp-app-shell now finish against stubs; their integration criteria moved to stage 2.
- **Dev host:** run.sh forwards signals, exports `SC_DEV_HOST`, supports a stdin FIFO; the CLI
  ignores stdin EOF; helpers never use `$PATH`; test-live.sh selects by target prefix, `cd`s per
  package, builds devtools, starts fixture apps outside the host and fails on 0 tests; TCC tripwire;
  `displays()` without TCC.
- **Threading:** explicit contract (§4.3); `mainloop` module; harness-free live runner on the main
  thread.
- **Contracts completed:** full journal fields (`segment_prepare`, `trailing_dropped`, `next`,
  `kind`, `track_finished`, stop reasons), helper/export/probe/render/text structs, Scenario API,
  `AppRef`, `z_index`, `PickedSource.region_pt`, display-local `region_pt`, `writers()`/`decoder()`
  as `Arc`, `host_duration`, `watch_system`, `OwnWindowsChanged`, sleep events, `FramesDropping`,
  `StopReason`.
- **Scope:** pause, countdown, ⌥⇧R and Restart moved to M7 (SPEC); meter and debug overlay marked
  unscheduled with an M7 proposal; added Mic Mode command and button, Discard, the small-content
  warning, the source-lost prompt, sleep/wake, quit/SIGTERM rules, multi-display overlays.
- **Recording correctness:** first-frame timeout; ADR-034 fallback order; symmetric overlap trimming
  with a ±100 ppm test; pre-created writers; project.json rewrite off the router; ProjectLock for the
  whole recording; Original ratio from the first frame; permission-denied fast failure.
- **Tests added:** live source-lost and static-window switches; e2e 60 fps, 1×/no-cursor, non-main
  display, run-5 latency/drops/RSS; tray-menu and bar exclusion checks through the debug driver;
  colour-patch export test; camera sync via full-screen flashes; 30-minute soak thresholds in the
  exit checklist and MANUAL_TEST; kill tests verified from journal content.
- **Release and CI:** bundle-only config with hardened runtime, entitlements, usage strings and
  signed sidecars, inspected by `codesign`/`plutil`; full icon set; CI fast/full split, Skia cache,
  Windows short target dir, fontconfig, `cli --no-default-features`, aarch64 Docker label;
  networking checks (grep, cargo-deny bans, CSP, `--disable-network`); justfile frozen with
  per-package scripts.
