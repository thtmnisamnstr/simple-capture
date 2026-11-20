# Cap MIT crates: reuse audit (M0)

Status: research note, M0. Source audited: `reference/Cap` at CapSoftware/Cap `880ec89` (a shallow clone with a single commit, so there is no license history to inspect). Audited 2026-11-19 on the M3 Pro dev machine.

No Cap code was copied into this repository for this note. Everything below about AGPL crates is a description in my own words, with paths cited. I mention a few API names from crates confirmed MIT. Since nothing was adapted, no `docs/THIRD_PARTY.md` entry was needed for this note. The "Vendoring procedure" section says what entry would be needed if we do adapt something later.

## TL;DR

1. **Cap draws the MIT/AGPL line by crate-name family, not by directory.** The repo-root `LICENSE` says code in the `cap-camera*` and `scap-*` crate families is MIT (text in `licenses/LICENSE-MIT`) and everything else is AGPL-3.0. Ten crates also set `license = "MIT"` in their own `Cargo.toml`. No crate has its own LICENSE file, and no `.rs` file has an SPDX or license header.
2. **Two family members have no `license` field:** `scap-targets` and `cap-camera-effects`. Our hard rule asks for the crate's own LICENSE or Cargo.toml to say MIT, so **we treat `scap-targets` as not confirmed MIT** and do not adapt or quote it. `cap-camera-effects` was not in scope (it bundles ONNX models with their own license question). We ignore it.
3. **Every MIT crate depends on `workspace-hack`, which is AGPL by default (no license field).** It holds no code (a stub `lib.rs` and an empty `build.rs`), but its manifest pulls in Cap's whole unified dependency set. That set includes a **Cap fork of `reqwest` (HTTP client, cookies, rustls)**, `ort`/`ort-sys` with `download-binaries` (fetches ONNX Runtime at build time), `tauri-utils`, `clap` and more. It must be dropped. Dropping it changes nothing functionally. I checked this by building `scap-cpal` with the line removed.
4. **The other non-MIT edges are Windows-only and small.** `scap-direct3d` calls one function in `cap-d3d-adapter` (DXGI hardware-adapter selection). `cap-camera-windows` and `cap-camera-mediafoundation` use one RAII lock helper for `IMFMediaBuffer` from `cap-mediafoundation-utils`. All three are easy to rewrite from Microsoft's public docs.
5. **Five crates in the requested list are not MIT:** `cap-cpal-ffmpeg`, `cap-cursor-info`, `cap-d3d-adapter`, `cap-mediafoundation-utils` and `workspace-hack` sit outside the MIT families and have no license field, so they are AGPL-3.0. `cap-cursor-info` also ships cursor SVGs under an "Apple User Agreement" licence file. Do not use them.
6. **All external dependencies are permissive.** I checked this with `cargo metadata` against the exact versions in Cap's `Cargo.lock`. The exceptions: `ffmpeg-next`/`ffmpeg-sys-next` are WTFPL, but they link libav*, so FFmpeg's own LGPL/GPL applies. `cpal` is Apache-2.0 only. Cap uses **git forks** of `cidre`, `cpal` and `rust-ffmpeg` (the forks keep MIT, Apache-2.0 and WTFPL).
7. **Build blocker on this machine:** `cidre`, which every macOS crate here depends on, runs `xcodebuild` in its build script to compile an Objective-C helper project. With `xcode-select` pointing to the Command Line Tools, that fails. With `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer`, `xcodebuild` starts but fails to load plug-ins and asks for `xcodebuild -runFirstLaunch`, which changes the system, so I did not run it. **No cidre-based crate compiled here.** GitHub macOS runners have a set-up Xcode, so this is a local-environment issue. It is also a reason to prefer `objc2-*` crates, which need no Xcode build step.
8. **Recommendation:** do **not** depend on Cap through a git rev. **Vendor and adapt with attribution** only `cap-camera-windows`, `cap-camera-mediafoundation` and `cap-camera-directshow` (the Windows camera stack, about 2.8k lines, the most valuable and hardest to rewrite), plus the small ideas in `scap-cpal`. For macOS (ScreenCaptureKit, AVFoundation camera) and for display/window enumeration, **write our own** on `objc2-*` crates, using the MIT Cap crates as a design reference. Skip the `*-ffmpeg` glue crates on macOS and Windows: under SPEC, the app does not link FFmpeg there.

## How Cap declares MIT vs AGPL

| Source | What it says |
|---|---|
| `LICENSE` (repo root), preamble before the AGPL text | Code in the `cap-camera*` and `scap-*` crate families is MIT (see `licenses/LICENSE-MIT`). Third-party components keep their own licenses. Everything else is AGPLv3. |
| `licenses/LICENSE-MIT` | Standard MIT text, "Copyright (c) 2023 Cap Software, Inc." This is the notice we must keep with anything adapted. |
| `README.md` §License (line ~199) | Repeats the root preamble. |
| Per-crate `Cargo.toml` `license` | `MIT` on 10 crates (table below; corrected from 11 after a re-count). Absent on everything else, including `scap-targets` and `cap-camera-effects`. |
| Per-crate LICENSE files | None in any audited crate. |
| Per-file headers | None. `grep -i -E "spdx|agpl|gpl-|license|copyright"` over all `.rs` files in the 16 audited crates returns nothing. |
| Other | `crates/cursor-info/assets/mac/LICENCE` says those assets are "released under the Apple User Agreement" (taken from a third-party mac-cursors repo). |

Notes:
- The families are named by **crate** name (`cap-camera*`), not directory (`crates/camera*`). Crates whose directory starts with `camera` all have crate names starting with `cap-camera`, so the two agree for this audit. `crates/cpal-ffmpeg` is the crate `cap-cpal-ffmpeg`, which matches neither family, so it is AGPL.
- Cap's root preamble arguably makes `scap-targets` MIT. Its own manifest does not say so, and the rule for this project is to treat it as non-MIT unless the crate itself says MIT. If we ever want to adapt it, first get written confirmation from Cap, or a `license = "MIT"` upstream commit pinned by rev.

## Summary table

"Effective" is how this project treats the crate. LoC means `src/` lines without `src/bin`. "unsafe" counts lines with the `unsafe` keyword in `src/` (blocks and impls; on the COM-heavy Windows crates one block often wraps a whole function, so a low count does not mean little unsafe code). "Tests" counts `#[test]`/`#[tokio::test]` across src, tests and examples.

| Dir (crate) | Cargo.toml license | Effective | Platforms | Non-MIT Cap deps (normal) | LoC / unsafe / tests | Recommendation |
|---|---|---|---|---|---|---|
| `scap-screencapturekit` | MIT | **MIT** | macOS | `workspace-hack` | 447 / 13 / 1 | Design reference. Write our own on `objc2-screen-capture-kit`. Vendoring is possible but needs the cidre fork. |
| `scap-targets` | *(none)* | **Treat as AGPL** (ambiguous) | macOS, Windows, Linux (X11 + Wayland) | `workspace-hack` | 4,781 / 85 / 33 | Write our own. Do not adapt or quote. |
| `scap-direct3d` | MIT | **MIT** | Windows | `workspace-hack`, `cap-d3d-adapter` (Windows) | 1,229 / 19 / 11 | Candidate for vendor+adapt in the Windows milestone. Replace adapter selection with our own code. Otherwise use `windows-capture` (MIT). |
| `scap-cpal` | MIT | **MIT** | all (loopback is Windows-only in practice) | `workspace-hack` | 169 / 1 / 0 | Adapt the ideas (with attribution if any code is reused): loopback buffer sizing and a silent keepalive stream. |
| `scap-ffmpeg` | MIT | **MIT** | macOS, Windows (Linux is a stub) | `workspace-hack` + transitively `cap-d3d-adapter` via `scap-direct3d` | 407 / 8 / 1 | Skip. It links FFmpeg in-process, which SPEC rules out on macOS and Windows. |
| `camera` (`cap-camera`) | MIT | **MIT** | macOS, Windows, Linux (V4L2) | `workspace-hack` + transitively `cap-mediafoundation-utils` via `cap-camera-windows` | 1,301 / 6 / 5 | Write our own facade. Its API shape is a good reference. |
| `camera-avfoundation` | MIT | **MIT** | macOS | `workspace-hack` | 335 / 11 / 1 | Reference. Write our own on `objc2-av-foundation`. |
| `camera-windows` | MIT | **MIT** | Windows | `workspace-hack`, `cap-mediafoundation-utils` | 905 / 12 / 3 | **Vendor+adapt** (Windows milestone) with a clean-room buffer-lock helper. |
| `camera-mediafoundation` | MIT | **MIT** | Windows | `workspace-hack`, `cap-mediafoundation-utils` | 683 / 28 / 0 | **Vendor+adapt** with the same clean-room helper. |
| `camera-directshow` | MIT | **MIT** | Windows | `workspace-hack` | 1,192 / 61 / 0 | **Vendor+adapt** (fallback path for older and virtual cameras). |
| `camera-ffmpeg` | MIT | **MIT** | all | `workspace-hack` + transitively via `cap-camera-windows` | 1,726 / 3 / 13 | Skip on macOS and Windows. On Linux, consider only the MJPEG reduced-resolution decode idea, if we decode MJPEG with the LGPL libraries. |
| `cpal-ffmpeg` (`cap-cpal-ffmpeg`) | *(none)* | **AGPL** | all | `workspace-hack` | 45 / 0 / 0 | Do not use. An MIT near-equivalent lives in `scap-ffmpeg/src/cpal.rs`, but we don't need either. |
| `cursor-info` (`cap-cursor-info`) | *(none)* | **AGPL** + Apple-licensed assets | macOS, Windows | `workspace-hack` | 1,080 / 1 / 10 | Do not use. Write our own if cursor shapes are needed. |
| `d3d-adapter` (`cap-d3d-adapter`) | *(none)* | **AGPL** | Windows | `workspace-hack` | 153 / 1 / 0 | Strip. Rewrite from DXGI docs. |
| `mediafoundation-utils` (`cap-mediafoundation-utils`) | *(none)* | **AGPL** | Windows | `workspace-hack` | 108 / 9 / 0 | Strip. Rewrite from Media Foundation docs. |
| `workspace-hack` | *(none)* | **AGPL** (generated, no code) | n/a | n/a | 3 / 0 / 0 | Strip. Delete the dependency line. |

No `TODO`/`FIXME` markers were found in `src/` of any MIT crate. The only marker-like match is a `b"XXXX"` fourcc in a `camera-ffmpeg` Linux unit test, which is not a TODO. Panicking `unwrap`/`expect` calls in `src/`: `scap-targets` 76, `camera-ffmpeg` 39, `camera-directshow` 11, `camera-mediafoundation` 4, and 0 to 2 in the others. `scap-ffmpeg/src/cpal.rs` and `cpal-ffmpeg` call `panic!` on an unsupported sample format.

## Internal dependency tree (normal deps; dev-deps noted)

```
scap-screencapturekit ── workspace-hack*            (dev: scap-targets†)
scap-targets† ────────── workspace-hack*
scap-direct3d ────────── workspace-hack*, cap-d3d-adapter* [windows]   (dev: scap-targets†, scap-ffmpeg)
scap-cpal ────────────── workspace-hack*
scap-ffmpeg ──────────── workspace-hack*, scap-cpal (optional, unused feature),
                         scap-screencapturekit [macos], scap-direct3d [windows] → cap-d3d-adapter*
cap-camera ───────────── workspace-hack*, cap-camera-avfoundation [macos],
                         cap-camera-windows [windows] → cap-mediafoundation-utils*, cap-camera-mediafoundation, cap-camera-directshow
cap-camera-avfoundation ─ workspace-hack*
cap-camera-windows ───── workspace-hack*, cap-mediafoundation-utils* [windows], cap-camera-mediafoundation, cap-camera-directshow
cap-camera-mediafoundation ─ workspace-hack*, cap-mediafoundation-utils* [windows]
cap-camera-directshow ── workspace-hack*
cap-camera-ffmpeg ────── workspace-hack*, cap-camera, cap-camera-avfoundation [macos], cap-camera-windows [windows]
cap-cpal-ffmpeg* ─────── workspace-hack*
cap-cursor-info* ─────── workspace-hack*
cap-d3d-adapter* ─────── workspace-hack* [windows]
cap-mediafoundation-utils* ─ workspace-hack*
```
`*` = not MIT (AGPL). `†` = treated as non-MIT (no license field).

Exact non-MIT symbols the MIT crates use, from grepping `cap_*::` and `use cap_*`:
- `scap-direct3d/src/lib.rs` (~line 272): one call to `cap_d3d_adapter::select_capture_adapter(None)`. In my words, that function enumerates DXGI adapters, skips software and known virtual-display adapters, prefers an optional LUID, and otherwise picks the one with the most dedicated VRAM. The adapter is then used to create the D3D11 device for capture.
- `camera-mediafoundation/src/lib.rs` (~line 501) and `camera-windows/src/lib.rs` (~lines 476, 483): a glob import of `cap_mediafoundation_utils`. The only items used are an extension trait that locks an `IMFMediaBuffer` and a guard type that unlocks on drop and derefs to the byte slice. The crate also has a COM thread-init helper and an MMCSS audio-thread registration guard, which these crates don't use.
- `workspace-hack`: no symbols are used. Every MIT crate lists it only so Cargo unifies features.

`workspace = true` inheritance: the MIT crates take `cidre`, `cpal`, `ffmpeg`, `tokio`, `specta`, `windows`, `windows-core`, `thiserror` (1.x), `tracing`, `futures` and `[lints]` from Cap's root `Cargo.toml`. That file pins `cidre`, `cpal`, `ffmpeg-next`, `nokhwa` and a patched `reqwest` to CapSoftware git forks, and vendors a patched `wgpu-hal`. Cap pins Rust 1.88 (`rust-toolchain.toml`), and the crates use edition 2024, which our Rust 1.95 builds.

### Why "depend via git rev" doesn't work

A `git = "https://github.com/CapSoftware/Cap", rev = "880ec89"` dependency on any of these crates would:
1. bring in `workspace-hack`, an AGPL crate, plus its graph, including the forked `reqwest` (networking code in the shipped binary's dependency tree, against our no-networking rule), `ort-sys` with `download-binaries`, `tauri-utils` with `build`, `clap`, `specta`, `schemars` and others;
2. on Windows, also bring in `cap-d3d-adapter` and `cap-mediafoundation-utils` (AGPL, linked in);
3. clone the whole Cap monorepo, which is large.

`[patch."https://github.com/CapSoftware/Cap"]` could, in principle, swap `workspace-hack`, `cap-d3d-adapter` and `cap-mediafoundation-utils` for our own crates with the same names and APIs. That is brittle, since it silently depends on Cap's internal API, and it leaves AGPL crate names in `Cargo.lock`, which makes cargo-deny auditing confusing. Vendoring is cleaner.

## External dependencies and licenses

I checked these with `cargo metadata` in a scratch project pinned to the exact versions in Cap's `Cargo.lock` (crates.io), plus the three CapSoftware git forks at Cap's pinned revs. Across the resolved transitive graph for all target platforms (274 packages for the crates.io set), every license is MIT, Apache-2.0, BSD-2/3, ISC, Zlib, Unlicense, 0BSD or Unicode-3.0, with two exceptions: `ffmpeg-next`/`ffmpeg-sys-next` (WTFPL) and `r-efi` (`MIT OR Apache-2.0 OR LGPL-2.1-or-later`, so permissive is available). Transitive versions resolved to the latest compatible ones, not Cap's lockfile, so a cargo-deny run on our real lockfile is still needed.

| Dependency (version in Cap lock) | Used by | License |
|---|---|---|
| `cidre` 0.11.0, **git fork** CapSoftware/cidre `bf84b67` (`LICENSE.txt` present) | scap-screencapturekit, scap-targets, scap-ffmpeg, cap-camera, cap-camera-avfoundation, cap-camera-ffmpeg | MIT (`cidre-macros` 0.3.0 MIT) |
| `cpal` 0.15.3, **git fork** CapSoftware/cpal `6013cb5` | scap-cpal, scap-ffmpeg, cap-cpal-ffmpeg | Apache-2.0 |
| `ffmpeg-next` 7.1.0, **git fork** CapSoftware/rust-ffmpeg `49db1fe`; `ffmpeg-sys-next` 7.1.3 | scap-ffmpeg, cap-camera-ffmpeg, cap-cpal-ffmpeg | WTFPL (bindings). The linked libav* carries FFmpeg's LGPL/GPL. |
| `objc2` 0.6.2, `objc2-foundation` 0.3.1 | scap-screencapturekit | MIT |
| `objc2-app-kit` 0.3.1, `objc2-av-foundation` 0.3.1 | scap-screencapturekit, cap-camera | Zlib OR Apache-2.0 OR MIT |
| `core-graphics` 0.24.0, `core-foundation` 0.10.1, `cocoa` 0.26.1 | scap-targets | MIT OR Apache-2.0 (`cocoa` is deprecated upstream in favour of objc2) |
| `objc` 0.2.7 | scap-targets | MIT (legacy, superseded by objc2) |
| `windows` 0.60.0, `windows-core` 0.60.1, `windows-numerics` 0.2.0 | Windows crates | MIT OR Apache-2.0 |
| `x11rb` 0.13.2 | scap-targets (Linux) | MIT OR Apache-2.0 |
| `wayland-client` 0.31.11, `wayland-protocols` 0.32.9 | scap-targets (Linux) | MIT |
| `rustix` 1.1.2 | scap-targets (Linux) | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `uuid` 1.18.1 | scap-targets (Linux) | Apache-2.0 OR MIT |
| `v4l` 0.14.0 | cap-camera (Linux) | MIT |
| `image` 0.24.9 | scap-targets (app icons) | MIT OR Apache-2.0 |
| `specta` 2.0.0-rc.20 | scap-targets, cap-camera (optional), cap-cursor-info | MIT (pre-release, Tauri-specta typing) |
| `tokio` 1.51.6 | scap-targets (macOS), dev-deps | MIT |
| `serde` 1.0.228, `thiserror` 1.0.69, `tracing` 0.1.41, `futures` 0.3.31, `parking_lot` 0.12.4 | various | MIT OR Apache-2.0 / MIT |
| `sha2` 0.10.9, `strum` 0.27.2 | cap-cursor-info | MIT OR Apache-2.0 / MIT |
| `inquire` 0.7.5, `clap` 4 | examples and dev-deps (`cap-camera-windows` wrongly lists `inquire` as a normal dependency) | MIT / MIT OR Apache-2.0 |

What the forks change, per comments in Cap's root `Cargo.toml`: the `cpal` fork has an unreleased fix so streams stop and release on drop on macOS, and zeroes WASAPI capture packets flagged silent (otherwise loopback of an idle endpoint can return garbage). The `cidre` fork is pinned to a rev that is API-incompatible with crates.io `cidre` 0.30.x (version 0.11.0 on the fork vs 0.30.1 on crates.io). Anything vendored from Cap that uses cidre would tie us to that fork.

## Build probe (this machine)

These ran in the scratchpad, outside the repo. I copied `scap-screencapturekit`, `scap-cpal`, `camera` and `camera-avfoundation`, deleted the `workspace-hack` line from each, removed the Windows-only `cap-camera-windows` path dependency, and wrote a minimal workspace root with the same pinned forks.
- `cargo build -p scap-cpal`: **builds** (Rust 1.95, about 38 s cold including fetching the cpal fork). 23 unique crates in its normal-dependency tree. This shows that removing `workspace-hack` is harmless.
- Anything using `cidre` (`scap-screencapturekit`, `cap-camera-avfoundation`, `cap-camera`): **fails in cidre's `build.rs`.** It runs `xcodebuild` on its bundled `pomace` Objective-C project. With xcode-select on CommandLineTools: "tool 'xcodebuild' requires Xcode". With `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer`, Xcode 27.2 (27B52) reports a missing `DVTDownloads` symbol while loading plug-ins and asks for `xcodebuild -runFirstLaunch`. I did not run it because it installs system components. **I did not compile or run any macOS MIT crate or its tests.** cidre's build script skips the native build only when `DOCS_RS` is set, so there is no supported way around it.
- Not attempted: any Windows or Linux build (only the aarch64-apple-darwin target is installed).

## Per-crate notes

### scap-screencapturekit (MIT, confirmed: Cargo.toml + root family rule)
- **What it does:** a thin ScreenCaptureKit wrapper on top of `cidre`. It exposes a `Capturer` built from an `sc::ContentFilter` and an `sc::StreamCfg`, with a closure called for every sample buffer (screen, system audio, or mic variant) and another for stream errors. It has async `start()`/`stop()`, a `StreamCfgBuilder` (size, source rect, cursor, fps via minimum frame interval, queue depth clamped to 8, audio on macOS 13+), and permission helpers that call `CGPreflightScreenCaptureAccess`/`CGRequestScreenCaptureAccess`.
- **API shape:** builder plus callbacks, cidre types exposed directly (`cm::SampleBuf`, `cv::ImageBuf`, `sc::*`). There is no Rust-owned abstraction over the target. Display/window filters come from `scap-targets`.
- **Quality:** small and readable. Callbacks are wrapped in `catch_unwind` so panics don't cross the Objective-C boundary. A custom `dealloc` works around the pinned cidre macro leaking the delegate's Rust payload, and one unit test checks drop counts over 100 cycles. Things I noticed:
  - fps becomes an integer-millisecond interval, so 60 fps turns into 16 ms (about 62.5 fps cap) and 30 fps into 33 ms. Use a rational `CMTime` in our version.
  - It registers only the Screen and (optionally) Audio outputs, never the macOS 15 Mic output, even though the enum has a Mic variant.
  - Frames without an image buffer (idle or status-only samples) are dropped silently. Our implementation should read `SCStreamFrameInfo` status to tell idle from blank, as SPEC requires.
  - `unsafe impl Send/Sync` on the delegate.
  - There is no backpressure or buffer-pool policy. The callback runs on a serial dispatch queue, and holding frames is up to the caller (SPEC rule 13: never hold capture buffers).
- **Recommendation:** **write our own** on `objc2-screen-capture-kit` (Zlib/Apache/MIT, no Xcode build step, already in our registry cache), treating this crate as a checklist of gotchas. Vendoring would tie us to the cidre fork and its `xcodebuild` build step. If we later decide to vendor, it needs only `workspace-hack` stripped.

### scap-targets (no license field: treated as non-MIT)
- **What it does (my description):** cross-platform enumeration of displays and windows. It reports IDs (serde-serialisable, typed for specta), names, logical and physical sizes, refresh rate, the display under the cursor, the window at the cursor, owner app name, PID, bundle ID, app icon as PNG bytes, notch geometry on macOS, and display-relative window bounds. macOS uses CoreGraphics window lists and NSScreen. Windows uses Win32/DWM with DPI awareness. Linux uses X11 (randr) and a Wayland path built on xdg-output, with some portal-window special cases. It includes benchmark binaries and integration tests for Linux and Windows, which Cap CI runs (`.github/workflows/ci.yml`, `windows-targets.yml`).
- **Quality:** the largest crate (about 4.8k lines). It relies on the deprecated `cocoa` and `objc` 0.2 crates alongside cidre, and has many `unwrap`s (76).
- **Recommendation:** **write our own.** The license is ambiguous, the macOS dependencies are legacy, and on Linux SPEC uses the xdg-desktop-portal ScreenCast picker rather than enumerating windows ourselves. If we want it later, ask Cap to add `license = "MIT"` and re-audit at that rev.

### scap-direct3d (MIT)
- **What it does:** Windows.Graphics.Capture capture into D3D11 textures. A `Capturer::new(item, settings, callbacks…)` with `Settings` for border, cursor, minimum update interval and pixel format. There are capability probes (`can_is_border_required`, etc.) and Windows-version detection, a `StagingTexturePool` for CPU readback, and a `Frame` that exposes the texture, device, context and `as_buffer()` mapping. It detects software rendering.
- **Non-MIT coupling:** one call to the AGPL `cap-d3d-adapter` to choose the DXGI adapter.
- **Quality:** substantial (about 1.2k lines, plus a 774-line `tests/window_capture.rs` that needs Windows). Defensive about older Windows builds.
- **Recommendation:** for the Windows milestone, either use `windows-capture` (MIT, listed in SPEC) or **vendor+adapt** this crate, replacing adapter selection with our own clean-room function. Spec for that function, written from DXGI docs: `IDXGIFactory1::EnumAdapters1`, skip `DXGI_ADAPTER_FLAG_SOFTWARE`, optional LUID preference, fallback policy. Re-check the choice when the Windows spike runs.

### scap-cpal (MIT)
- **What it does:** builds a cpal *input* stream on the default *output* device, which is WASAPI loopback on Windows. It picks a buffer size of about 80 ms, clamped to 256 to 16384 frames and to the device range. On Windows it also opens a silent output stream on the same endpoint so loopback packets keep arriving during silence. Without that, the first packet comes only at the first sound and silent stretches produce no frames. The Cap comment says OBS does the same.
- **Quality:** small and clear. Has `unsafe impl Send` for the capturer. On macOS, building an input stream on an output device is not loopback; Cap uses ScreenCaptureKit audio there.
- **Recommendation:** **adapt the ideas** for our Windows system-audio path: the keepalive render stream and buffer-size clamping. If we copy any code, add a THIRD_PARTY.md entry. Also note we need the drop/stop and silent-packet fixes from Cap's cpal fork. Check whether upstream cpal has released them before we pin cpal.

### scap-ffmpeg (MIT)
- **What it does:** `AsFFmpeg` trait impls that turn ScreenCaptureKit and Direct3D frames into `ffmpeg::frame::Video` by copying planes (with a frame pool on Windows), plus a `cpal::Data` to FFmpeg audio-frame conversion.
- **Recommendation:** **skip.** SPEC keeps FFmpeg out of the app on macOS and Windows (native encoders in-process, the FFmpeg helper only at export, over a pipe), and the Linux branch is a stub.

### cap-camera (MIT)
- **What it does:** a cross-platform camera facade: `list_cameras()` gives `CameraInfo` (device ID, Chromium-style `ModelID` built from vendor and product ID, display name) and `formats()` gives `Format` (width, height, frame rate, pixel-format name, plus access to the native format). `start_capturing(format, callback)` returns a handle with `stop_capturing()`, and `CapturedFrame::native()` exposes the platform frame. Backends: AVFoundation (through `cap-camera-avfoundation`), Windows (through `cap-camera-windows`) and Linux V4L2 (through `v4l`). Optional serde/specta derives.
- **Quality:** decent. The macOS backend serialises session start and teardown through a global mutex, falls back when the requested native pixel format is not offered, and sets the active format under the device configuration lock. A comment cites a Stack Overflow answer for that technique, so if we adapt that part, write it ourselves rather than lift it, because Stack Overflow content is CC BY-SA. Has 5 unit tests for model-ID parsing.
- **Recommendation:** **write our own facade**, using this API shape as a reference (it's a good one), with macOS on `objc2-av-foundation` and Windows on the vendored crates below. Linux in SPEC prefers V4L2 or the portal camera, so the `v4l` path is a reasonable reference.

### cap-camera-avfoundation (MIT)
- **What it does:** cidre-based AVFoundation helpers. `list_video_devices()` runs a discovery session over built-in, desk-view (13+), external and continuity (14+) device types inside an autorelease pool, which fixes a leak when polled from tokio threads. It also has a closure-backed `AVCaptureVideoDataOutputSampleBufferDelegate`, YCbCr matrix parsing, and an RAII pixel-buffer base-address lock with per-plane slices.
- **Recommendation:** **reference only.** Write our own on `objc2-av-foundation` and keep the autorelease-pool and device-type lessons.

### cap-camera-windows, cap-camera-mediafoundation, cap-camera-directshow (MIT)
- **What they do:** `cap-camera-windows` merges devices from Media Foundation and DirectShow. It prefers MF, adds DirectShow-only devices such as some virtual cameras and capture cards, de-duplicates formats, classifies devices (virtual, capture card, high bandwidth), picks formats by preference (resolution, fps, pixel-format priority, with presets for hardware encoding and capture cards), handles bottom-up RGB, and gives one `Frame`/`FrameBytes` API over both backends. `cap-camera-mediafoundation` covers device-source enumeration, an `IMFCaptureEngine`-based capture with an event channel, and a `SourceReader` path. `cap-camera-directshow` covers moniker enumeration, `IAMStreamConfig` format listing, and a custom sink filter implemented as a COM object (the most `unsafe`-dense code in the set: 61 marked sites).
- **Non-MIT coupling:** the `IMFMediaBuffer` lock helper from `cap-mediafoundation-utils`. Replacing it means writing about 30 lines of our own code against `IMFMediaBuffer::Lock`/`Unlock` from the MS docs.
- **Quality:** few tests (3 in `camera-windows`, none in the other two). There is an `enumeration_leak.rs` example, which suggests real leak hunting. `inquire` (a CLI prompt crate) is wrongly a normal dependency of `cap-camera-windows`, so drop it.
- **Recommendation:** **vendor+adapt with attribution** when the Windows milestone starts. These encode a lot of hard-won Windows camera behaviour that would be expensive to rediscover. Strip `workspace-hack` and `inquire`, swap in our clean-room buffer-lock helper, and add tests.

### cap-camera-ffmpeg (MIT)
- **What it does:** `CapturedFrameExt::as_ffmpeg()` / `as_ffmpeg_covering()` turn native camera frames into FFmpeg frames. On Windows there is an H.264 decoder for cameras that output H.264. On Linux, MJPEG can decode at reduced resolution through FFmpeg's `lowres` IDCT scaling when the target is smaller (`mjpeg_idct.rs`, unit-tested). 13 tests.
- **Recommendation:** **skip on macOS and Windows** (no in-process FFmpeg there). On Linux, where we load the LGPL FFmpeg libraries, the reduced-resolution MJPEG decode is a useful idea for camera preview. Adapt it with attribution if we take it.

### cap-cpal-ffmpeg (AGPL)
- Converts cpal sample buffers into FFmpeg audio frames. **Do not use.** We have no need for it, and the MIT `scap-ffmpeg/src/cpal.rs` does nearly the same job anyway.

### cap-cursor-info (AGPL; assets "Apple User Agreement")
- In my words: it maps the current system cursor to a known shape. On macOS it hashes the cursor bitmap, because NSCursor objects can't be compared directly. On Windows it compares `HCURSOR` handles against the loaded system cursors. Each shape maps to an SVG and a hotspot for re-rendering in the editor. **Do not use** the code or the assets. If we need cursor re-rendering, design our own, and draw or source cursor art under OFL/MIT/ISC terms.

### cap-d3d-adapter, cap-mediafoundation-utils (AGPL)
- Described above. **Strip.** If needed, write our own replacements from Microsoft documentation, ideally without having the Cap source open.

### workspace-hack (AGPL by default; generated by cargo-hakari)
- A stub `lib.rs` plus an empty `build.rs`. The manifest is generated by `cargo hakari` (`.config/hakari.toml`) for the whole Cap workspace on four target triples. **Strip** the dependency line from every vendored crate. Add `workspace-hack` and the AGPL crate names to a cargo-deny `bans` list so they can't come back.

## Vendoring procedure (if and when we adapt the Windows camera crates or scap-cpal)

1. Copy from Cap at the pinned commit `880ec89` into a clearly separate path (for example `third_party/cap-mit/<crate>/` or `crates/vendor/…`, to be decided in DECISIONS.md). Keep a copy of `licenses/LICENSE-MIT` ("Copyright (c) 2023 Cap Software, Inc.") next to it.
2. Append an entry to `docs/THIRD_PARTY.md` naming the crate, the upstream path, the commit, the license and our changes.
3. Remove `workspace-hack`, any `cap-d3d-adapter` or `cap-mediafoundation-utils` dependency, and misplaced dev tools (`inquire`). Replace the removed helpers with code we write from vendor documentation.
4. Make `[workspace.dependencies]` explicit. Prefer upstream crates.io `cpal`/`windows` where the forks' fixes have landed, and record any fork we keep.
5. CI: `cargo deny check licenses bans` must pass. Add tests, since the Windows camera crates have almost none.

## Open questions and risks
- **`scap-targets` license ambiguity:** the root preamble says MIT, the manifest says nothing. Resolve with Cap only if we want it, which we likely don't.
- **cidre build environment:** any cidre user needs a working full Xcode. On this machine that means someone running `sudo xcodebuild -runFirstLaunch` (or opening Xcode once), which is the user's call. Using `objc2-*` avoids the issue.
- **cpal fork fixes:** the macOS stream-release-on-drop and WASAPI silent-packet fixes are not in cpal 0.15.3 on crates.io. I could not verify whether a newer upstream release includes them (not checked).
- **Nothing here was run on hardware.** No capture, camera or permission behaviour was tested. Every quality judgement comes from reading the code.
