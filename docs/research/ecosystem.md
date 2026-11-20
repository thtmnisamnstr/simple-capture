# Research: Rust/JS ecosystem and platform facts

M0 research note for Simple Capture. Everything here was checked on **2026-11-19**, unless a row says otherwise.

How I checked:
- Crate versions, dates, licenses, and MSRVs come from the crates.io API (`https://crates.io/api/v1/crates/<name>`) and `cargo info`.
- npm data comes from `https://registry.npmjs.org/<name>`.
- GitHub data comes from `gh api`.
- Platform facts come from vendor docs: Apple's documentation JSON, Microsoft Learn, and the freedesktop portal docs. Where only secondary sources (forums, blogs) exist, I mark the fact **secondary**.
- Anything I could not verify is marked **unverified**.

Nothing here is copied from Cap. Cap is cited only by path, to show which versions it pins. Nothing was adapted, so there is no `docs/THIRD_PARTY.md` entry.

No performance numbers appear in this note. This was a desk-research task, and I measured nothing.

---

## 0. Headlines (what changes the plan)

1. **Tauri 3.0 is in alpha** (`tauri 3.0.0-alpha.6`). Stay on **2.x stable (2.12.7)**. Since **2.12.1**, transparency and fullscreen on macOS no longer need `macOSPrivateApi`. The flag is now a no-op ([CHANGELOG 2.12.1](https://github.com/tauri-apps/tauri/blob/tauri-v2.12.7/crates/tauri/CHANGELOG.md), [config schema](https://raw.githubusercontent.com/tauri-apps/tauri/tauri-v2.12.7/crates/tauri-schema-generator/schemas/config.schema.json)).
2. **Tauri's `contentProtected` will not keep our UI out of macOS captures.** It maps to `NSWindow.sharingType = .none` (tao 0.37.3, `src/platform_impl/macos/window_delegate.rs`, [source](https://docs.rs/crate/tao/0.37.3/source/src/platform_impl/macos/window_delegate.rs)), which Apple now calls "a legacy constant that macOS no longer uses" and says not to use for hiding content ([Apple: NSWindow.SharingType.none](https://developer.apple.com/documentation/appkit/nswindow/sharingtype-swift.enum/none)). Exclusion on macOS must go through `SCContentFilter`, as the spec already says.
3. **On macOS 15+/26, periodic Screen Recording re-approval cannot be avoided by a normal app** unless it uses `SCContentSharingPicker`. The only entitlement Apple DTS names is meant for VNC-style screen-sharing apps. Details in section 2a.
4. **BtbN/FFmpeg-Builds has moved** to `github.com/FFmpeg-Builds/FFmpeg-Builds`, and the old URL redirects. Daily builds are kept only **14 days**. **The last build of each month is kept two years**, so pin those. Every release ships `checksums.sha256`, and the GitHub API exposes a per-asset `sha256` digest. BtbN's stated minimum Windows version is **Windows 10 22H2**, which is newer than the spec's "Windows 10 2004+". Details in section 2e.
5. **Global hotkeys on Linux Wayland:** `global-hotkey` (and so `tauri-plugin-global-shortcut`) is **X11-only**. Wayland needs the GlobalShortcuts portal. GNOME supports it from **48**, and KDE Plasma from **5.27** (secondary source).
6. **License-policy surprises for `deny.toml`:**
   - `ffmpeg-next` / `ffmpeg-sys-next` are **WTFPL**.
   - `cpal` and `nokhwa` are **Apache-2.0 only**, which is fine.
   - The objc2 framework crates are `Zlib OR Apache-2.0 OR MIT`.
   - `gifski` is **AGPL-3.0-or-later** and must be banned as a crate.
7. **`skia-safe` has a new experimental `wgpu` feature.** It runs Skia Graphite on our own `wgpu` device. It needs a from-source Skia build and downloads Dawn headers at build time, and there are no prebuilt binaries yet. The default path still downloads prebuilt Skia binaries at build time.
8. **`objc2` is heading for a breaking release.** `main` already carries unreleased breaking changes (Xcode 27.1 SDK regen, a `define_class!` ivar syntax change). The current framework crates (0.3.2) date from 2025-10-04. Tauri 2.12.7 itself depends on `objc2 ^0.6` / `objc2-app-kit ^0.3.2`, so using the same versions avoids duplicate copies in the build.

---

## 1. Crates and packages

The `Released` column is the date of the latest **stable** version. "Platforms" means what the crate targets, not something I tested. Cap pins come from `reference/Cap/Cargo.toml` and the crate manifests at @880ec89.

### 1.1 App shell (Tauri v2)

| Crate | Latest stable | Released | License | MSRV | Notes | Source |
|---|---|---|---|---|---|---|
| tauri | 2.12.7 (3.0.0-alpha.6 exists) | 2026-11-18 | Apache-2.0 OR MIT | 1.90 | macOS uses WKWebView, Windows uses WebView2 (`webview2-com ^0.39`), Linux uses WebKitGTK (`webkit2gtk ^2`). Deps include `objc2 ^0.6`, `objc2-app-kit ^0.3.2`, `windows ^0.62`. | [crates.io](https://crates.io/crates/tauri), [deps](https://crates.io/api/v1/crates/tauri/2.12.7/dependencies) |
| tauri-build | 2.7.6 | 2026-11-18 | Apache-2.0 OR MIT | 1.90 | | [crates.io](https://crates.io/crates/tauri-build) |
| tauri-cli | 2.12.6 | 2026-11-18 | Apache-2.0 OR MIT | 1.90 | npm `@tauri-apps/cli` 2.12.6 (next: 3.0.0-alpha.6) | [crates.io](https://crates.io/crates/tauri-cli), [npm](https://registry.npmjs.org/@tauri-apps/cli) |
| tauri-bundler | 2.10.6 | n/a | Apache-2.0 OR MIT | n/a | Signs sidecars; see section 2b | [crates.io](https://crates.io/crates/tauri-bundler) |
| tauri-plugin-global-shortcut | 2.4.1 | 2026-10-10 | Apache-2.0 OR MIT | 1.90 | Depends on **`global-hotkey ^0.8`** (the latest is 0.10). Inherits its X11-only Linux support. The docs page lists no Wayland caveat. | [crates.io](https://crates.io/crates/tauri-plugin-global-shortcut), [deps](https://crates.io/api/v1/crates/tauri-plugin-global-shortcut/2.4.1/dependencies), [docs](https://v2.tauri.app/plugin/global-shortcut/) |
| tauri-plugin-shell | 2.6.0 | 2026-10-28 | Apache-2.0 OR MIT | 1.90 | Sidecar spawn needs `shell:allow-spawn` (or `allow-execute`) with `"sidecar": true`. Rust `child.write()` writes to stdin. | [crates.io](https://crates.io/crates/tauri-plugin-shell), [sidecar docs](https://v2.tauri.app/develop/sidecar/) |
| tauri-plugin-dialog | 2.8.4 | 2026-10-28 | Apache-2.0 OR MIT | 1.90 | | [crates.io](https://crates.io/crates/tauri-plugin-dialog) |
| tauri-plugin-fs | 2.7.1 | 2026-10-14 | Apache-2.0 OR MIT | 1.90 | | [crates.io](https://crates.io/crates/tauri-plugin-fs) |
| tauri-plugin-opener | 2.7.3 | 2026-11-10 | Apache-2.0 OR MIT | 1.90 | Covers Reveal / Open | [crates.io](https://crates.io/crates/tauri-plugin-opener) |
| tauri-plugin-single-instance | 2.5.5 | 2026-10-28 | Apache-2.0 OR MIT | 1.90 | Must be registered first. On Linux it uses D-Bus (`org.{id}.SingleInstance`), and Flatpak/Snap need extra manifest entries. | [crates.io](https://crates.io/crates/tauri-plugin-single-instance), [docs](https://v2.tauri.app/plugin/single-instance/) |
| tauri-nspanel | 2.1.1 | 2026-10-08 | MIT OR Apache-2.0 | 1.75 | macOS only. Third-party (ahkohd). Built on `objc2-app-kit`. See section 2b. | [crates.io](https://crates.io/crates/tauri-nspanel), [repo](https://github.com/ahkohd/tauri-nspanel) |
| npm `@tauri-apps/api` | 2.12.5 (next 3.0.0-alpha.3) | 2026-11-16 | Apache-2.0 OR MIT | n/a | JS plugin packages match the Rust versions: global-shortcut 2.4.1, shell 2.6.0, dialog 2.8.4, fs 2.7.1, opener 2.7.3 | [npm](https://registry.npmjs.org/@tauri-apps/api) |

Notes:
- All plugins have a `3.0.0-alpha.x` line. Pin `2.x` explicitly with `^2` or `~2.N` so that no tool picks up the `next` tag.
- No plugin above makes network requests by itself. Updater and HTTP plugins are separate and must not be added.

### 1.2 Rendering, text, and the UI fallback

| Crate | Latest stable | Released | License | MSRV | Notes | Source |
|---|---|---|---|---|---|---|
| wgpu | 30.0.2 | 2026-11-12 | MIT OR Apache-2.0 | 1.87 | Default backends: `metal`, `dx12`, `vulkan`, `gles`. A `noop` feature exists. Cap pins **25.0.0** and vendors `wgpu-hal`. | [crates.io](https://crates.io/crates/wgpu) |
| skia-safe | 0.157.0 | 2026-11-03 | MIT (bindings). Skia itself is BSD-3-Clause. | 1.87 | Defaults: `binary-cache`, `embed-icudtl`, `pdf`, `jpeg`. Text needs `textlayout` (HarfBuzz + ICU, skparagraph). GPU backends: `metal`/`vulkan` (pick `ganesh` or `graphite`), `d3d`, `gl`. The experimental `wgpu` (or `wgpu-30`) feature is covered below. Cap pins 0.86.1. | [crates.io](https://crates.io/crates/skia-safe), [README](https://github.com/rust-skia/rust-skia/blob/master/README.md), [skia-safe README](https://github.com/rust-skia/rust-skia/blob/master/skia-safe/README.md) |
| fontdb | 0.24.0 | 2026-07-29 | MIT | 1.71 | System font enumeration | [crates.io](https://crates.io/crates/fontdb) |
| gpui (spec fallback) | 0.2.2 | 2025-10-22 | Apache-2.0 | n/a | Lives in the Zed monorepo (`crates/gpui`, LICENSE-APACHE). The crates.io release is over a year old. Default features: `font-kit`, `wayland`, `x11`, `windows-manifest`. See below. | [crates.io](https://crates.io/crates/gpui), [repo dir](https://github.com/zed-industries/zed/tree/main/crates/gpui) |
| gpui-component | 0.10.0 | 2026-11-16 | Apache-2.0 | n/a | Longbridge widget library. The GitHub repo now resolves to `longbridge/gpui-kit`. | [crates.io](https://crates.io/crates/gpui-component) |

**Skia notes**
- `skia-bindings`' `build.rs` downloads prebuilt binaries from GitHub (`rust-skia/skia-binaries`) with `curl` at build time.
  - Prebuilt targets: x86_64/aarch64 for Windows MSVC, Linux glibc ≥ 2.31, and macOS.
  - The aarch64 Windows binaries need Visual Studio 2026.
  - `FORCE_SKIA_BUILD=1`, or disabling `binary-cache`, builds from source. That needs LLVM, Python 3, and Ninja.
  - This is a build-time download only, which the hard rules allow. CI should cache it.
- The new **`wgpu` feature** runs Graphite on the app's `wgpu::Device`/`Queue` and renders into its `wgpu::Texture`.
  - It is experimental and has no prebuilt binaries.
  - It downloads Dawn's WebGPU headers, so it "does not work offline".
  - It could remove the CPU-to-GPU upload for text layers. Treat it as a spike candidate, not the default ([skia-safe README](https://github.com/rust-skia/rust-skia/blob/master/skia-safe/README.md)).
- The license audit must cover what Skia vendors: HarfBuzz, ICU, FreeType, libpng/jpeg. That is **unverified** here. Run cargo-about on the spike 4 build.

**GPUI as fallback**
- It is Apache-2.0 and usable outside Zed, but it has no predictable versioned releases. The crates.io `gpui` is 0.2.2 (2025-10).
- Cap's `apps/desktop-gpui/Cargo.toml` pins a **fork** (`wingleeio/zed` at a fixed git rev). It also keeps the app as a separate cargo workspace, and its own comment says the gpui dependency tree is very large.
- Community forks exist (`gpui-ce`, `xgpui`, and others), and a gpui-component discussion reports that upstream is not merging features outside Zed's needs (**secondary**: [discussion #1856](https://github.com/longbridge/gpui-component/discussions/1856)).
- Choosing GPUI therefore means pinning a git rev and owning upgrades.

### 1.3 macOS bindings

| Crate | Latest stable | Released | License | MSRV | Notes | Source |
|---|---|---|---|---|---|---|
| objc2 | 0.6.5 | 2026-10-05 | MIT | 1.71 | `main` has unreleased **breaking** changes (`define_class!` ivars syntax) and an "Update to Xcode 27.1" commit (2026-10-16), so expect a breaking bump. | [crates.io](https://crates.io/crates/objc2), [CHANGELOG](https://github.com/madsmtm/objc2/blob/main/crates/objc2/CHANGELOG.md) |
| objc2-screen-capture-kit | 0.3.2 | 2025-10-04 | Zlib OR Apache-2.0 OR MIT | 1.71 | Generated bindings. Every framework crate below is at 0.3.2 from the same date. | [crates.io](https://crates.io/crates/objc2-screen-capture-kit) |
| objc2-av-foundation | 0.3.2 | 2025-10-04 | Zlib OR Apache-2.0 OR MIT | 1.71 | Cap's MIT `cap-camera` uses it (0.3.1) | [crates.io](https://crates.io/crates/objc2-av-foundation) |
| objc2-core-media | 0.3.2 | 2025-10-04 | Zlib OR Apache-2.0 OR MIT | 1.71 | | [crates.io](https://crates.io/crates/objc2-core-media) |
| objc2-core-video | 0.3.2 | 2025-10-04 | Zlib OR Apache-2.0 OR MIT | 1.71 | | [crates.io](https://crates.io/crates/objc2-core-video) |
| objc2-metal | 0.3.2 | 2025-10-04 | Zlib OR Apache-2.0 OR MIT | 1.71 | | [crates.io](https://crates.io/crates/objc2-metal) |
| objc2-app-kit | 0.3.2 | 2025-10-04 | Zlib OR Apache-2.0 OR MIT | 1.71 | Same version Tauri uses | [crates.io](https://crates.io/crates/objc2-app-kit) |
| objc2-io-surface | 0.3.2 | 2025-10-04 | Zlib OR Apache-2.0 OR MIT | 1.71 | Needed for IOSurface → Metal | [crates.io](https://crates.io/crates/objc2-io-surface) |
| cidre | 0.30.1 | 2026-11-13 | MIT | 1.88 | Single main author (yury). Hand-written "rusty" Apple APIs. 226 stars, very active. Cap uses its own **fork** at a pinned rev for capture, encoding, and camera. | [crates.io](https://crates.io/crates/cidre), [repo](https://github.com/yury/cidre) |
| screencapturekit | 11.1.2 | 2026-11-02 | MIT OR Apache-2.0 | 1.82 | doom-fish. Cumulative `macos_13_0` … `macos_26_0` features. **Compiles a Swift bridge** (`swift-bridge/`) through `xcrun --sdk macosx` at build time. Has a picker, mic capture, IOSurface → Metal/wgpu helpers. Cap pins the old 0.3.5. | [crates.io](https://crates.io/crates/screencapturekit), [README](https://github.com/doom-fish/screencapturekit-rs) |

Notes:
- The macOS floor is 14, so the useful `screencapturekit` feature is `macos_14_0`. Anything newer must be gated at runtime.
- The Swift-bridge build was **not tested** here. It might matter because `xcode-select` points at the Command Line Tools.

### 1.4 Windows

| Crate | Latest stable | Released | License | MSRV | Notes | Source |
|---|---|---|---|---|---|---|
| windows (windows-rs) | 0.62.2 | 2025-10-06 | MIT OR Apache-2.0 | 1.82 | Same major version Tauri uses. Cap pins 0.60. | [crates.io](https://crates.io/crates/windows) |
| windows-capture | 2.0.1 | 2026-08-08 | MIT | n/a | WGC and DXGI Desktop Duplication. Has `DrawBorderSettings`, `CursorCaptureSettings`, `MinimumUpdateInterval`, dirty regions, and a built-in hardware video encoder. Cap pins 1.5.0. | [crates.io](https://crates.io/crates/windows-capture), [repo](https://github.com/NiiightmareXD/windows-capture) |

### 1.5 Linux

| Crate | Latest stable | Released | License | MSRV | Notes | Source |
|---|---|---|---|---|---|---|
| ashpd | 0.14.1 | 2026-11-19 | MIT | 1.89 | Default feature is `tokio`. Portal features include `screencast`, `global_shortcuts`, `camera`, `pipewire`, and `remote_desktop`. Cap pins 0.11. | [crates.io](https://crates.io/crates/ashpd) |
| pipewire | 0.10.1 | 2026-08-19 | MIT | 1.80 | `pipewire-sys` uses `bindgen` and `system-deps`, so it needs libpipewire-0.3 dev headers and libclang on the build machine. Cap pins 0.10.0. | [crates.io](https://crates.io/crates/pipewire), [sys deps](https://crates.io/api/v1/crates/pipewire-sys/0.10.1/dependencies) |

### 1.6 Audio, camera, hotkeys

| Crate | Latest stable | Released | License | MSRV | Notes | Source |
|---|---|---|---|---|---|---|
| cpal | 0.18.2 | 2026-08-16 | **Apache-2.0** | 1.85 | Hosts: CoreAudio, WASAPI, ALSA. Optional `pipewire`, `pulseaudio`, `jack`, `asio` features. The unreleased changelog has many PipeWire fixes. Loopback/monitor capture is **not documented**. Cap uses its own fork. | [crates.io](https://crates.io/crates/cpal), [CHANGELOG](https://github.com/RustAudio/cpal/blob/master/CHANGELOG.md) |
| rubato | 5.1.0 | 2026-11-12 | MIT OR Apache-2.0 | 1.87 | | [crates.io](https://crates.io/crates/rubato) |
| signalsmith-stretch | 0.1.3 | 2025-09-18 | MIT (wrapper). The C++ library is MIT. | n/a | Small wrapper (14 stars). Builds the C++ with a C++ toolchain. The upstream C++ repo is active (pushed 2026-10-22). Low bus factor, but it is thin enough to vendor if needed. | [crates.io](https://crates.io/crates/signalsmith-stretch), [wrapper](https://github.com/colinmarc/signalsmith-stretch-rs), [upstream](https://github.com/Signalsmith-Audio/signalsmith-stretch) |
| global-hotkey | 0.10.0 | 2026-11-09 | Apache-2.0 OR MIT | 1.77 | **"Linux (X11 Only)"**. On Windows, create it on the thread that runs the win32 event loop. On macOS, create it on the main thread. | [crates.io](https://crates.io/crates/global-hotkey), [README](https://github.com/tauri-apps/global-hotkey) |
| nokhwa | 0.10.11 | 2026-05-15 | **Apache-2.0** | n/a | Backends: `input-avfoundation`, `input-msmf`, `input-v4l` (plus opencv). Repo last pushed 2026-05-15. Cap uses its own **fork**. Given the spec's AVFoundation / MF / V4L2 plan, direct bindings may be preferable. | [crates.io](https://crates.io/crates/nokhwa), [repo](https://github.com/l1npengtul/nokhwa) |

### 1.7 FFmpeg bindings (Linux in-process, LGPL) and gifski

| Crate | Latest stable | Released | License | MSRV | Notes | Source |
|---|---|---|---|---|---|---|
| ffmpeg-next | 9.1.0 | 2026-11-11 | **WTFPL** | n/a | The crate's major.minor tracks FFmpeg's major.minor. The README calls it **"maintenance mode"**, and its supported-version text ("3.4 til 8.0") is stale relative to 9.1. Default features enable avdevice and avfilter too. Trim them for the LGPL build. Cap uses its own fork. | [crates.io](https://crates.io/crates/ffmpeg-next), [README](https://github.com/zmwangx/rust-ffmpeg) |
| ffmpeg-sys-next | 9.1.1 | 2026-11-11 | **WTFPL** | n/a | Links dynamically to the FFmpeg it finds (pkg-config, or `FFMPEG_DIR`). Point it at BtbN `lgpl-shared`. | [crates.io](https://crates.io/crates/ffmpeg-sys-next) |
| gifski (crate) | 1.34.0 | 2025-07-13 | **AGPL-3.0-or-later** | 1.74 | **Must not be linked.** Ban it (and `imagequant`) in `deny.toml`. Use only the CLI as a sidecar. The reference clone at `61ac230` already says `version = "1.35.0"` (unreleased). | [crates.io](https://crates.io/crates/gifski) |

### 1.8 License tooling

| Tool | Latest | Released | License | Notes | Source |
|---|---|---|---|---|---|
| cargo-deny | 0.22.0 | 2026-11-19 (today) | MIT OR Apache-2.0 | **MSRV 1.95**, which matches our toolchain. 0.21.0 (2026-11-16, MSRV 1.89) is the conservative pin if 0.22 misbehaves. | [crates.io](https://crates.io/crates/cargo-deny) |
| cargo-about | 0.9.3 | 2026-10-26 | MIT OR Apache-2.0 | MSRV 1.88 | [crates.io](https://crates.io/crates/cargo-about) |
| `pnpm licenses` (built in) | pnpm 12.21.0 local | n/a | MIT (pnpm) | `pnpm licenses list --prod --json`, checked against an allowlist by a small script. No extra dependency. Verified present locally via `pnpm help licenses`. | local CLI |
| license-checker-rseidelsohn | 5.0.1 (next 6.0.0-rc.1) | 2026-05-27 | BSD-3-Clause | Maintained fork of `license-checker`. Engines: node ≥ 24, npm ≥ 11. Whether it handles pnpm's symlinked `node_modules` is **unverified**. | [npm](https://registry.npmjs.org/license-checker-rseidelsohn) |
| license-checker | 25.0.1 | 2019-01-10 | BSD-3-Clause | Abandoned. Don't use. | [npm](https://registry.npmjs.org/license-checker) |
| generate-license-file | 4.2.5 | 2026-08-29 | ISC | Generates a notices file for the About → Licenses screen | [npm](https://registry.npmjs.org/generate-license-file) |
| license-report | 6.8.6 | 2026-10-09 | MIT | Alternative reporter | [npm](https://registry.npmjs.org/license-report) |

**Implications for the `deny.toml` allowlist**

Allowed licenses:
- MIT, Apache-2.0, `Apache-2.0 WITH LLVM-exception`
- BSD-2-Clause, BSD-3-Clause
- ISC
- Zlib (objc2 framework crates offer it as one option)
- Unicode-3.0 (ICU and unicode crates)
- CC0-1.0
- MPL-2.0, only as a named exception

Additions beyond the spec's list:
- WTFPL, as a **named exception for `ffmpeg-next`/`ffmpeg-sys-next`, Linux target only**.

Bans:
- `gifski`, `imagequant`, and any `x264`/`x265` crates.

---

## 2. Platform facts

### 2a. ScreenCaptureKit (macOS 15+ / 26)

| Topic | Fact | Source |
|---|---|---|
| Periodic re-authorization | Started in macOS 15. The interval moved from weekly to **about monthly** in 15.0 beta 6. Prompts after reboot were dropped. It applies to ScreenCaptureKit apps. Apple does not document the interval. | [9to5Mac 2024-08-14](https://9to5mac.com/2024/08/14/macos-sequoia-screen-recording-prompt-monthly/), [TidBITS 2024-08-19](https://tidbits.com/2024/08/19/apple-reduces-excessive-sequoia-permission-requests-shifts-to-monthly) (secondary) |
| Still present on macOS 26 | One vendor blog (2026-09-15) reports that macOS 26 keeps periodic re-authorization. It says the alert now offers only "Allow" and "Open System Settings" (no "for one month" wording), and that the pane is renamed "Screen & System Audio Recording". It also says approval is tied to the signed build, so frequent re-signing re-prompts. | [hop.tools blog](https://hop.tools/blog/the-screen-recording-permission-that-keeps-coming-back/) (**secondary**, not Apple) |
| What the alert is | Apps that capture without the system picker get "*App* is requesting to bypass the system private window picker and directly access your screen and audio". An Apple DTS engineer reproduced it with plain `SCShareableContent` + `SCScreenshotManager`. | [Apple Developer Forums 765103](https://developer.apple.com/forums/thread/765103) |
| Ways to avoid it | DTS named two: **(1)** the `com.apple.developer.persistent-content-capture` entitlement, described as meant for screen-sharing products, and **(2)** presenting `SCContentSharingPicker`. | [Forums 765103](https://developer.apple.com/forums/thread/765103) |
| The entitlement | "Persistent Content Capture": "a Boolean value that indicates whether a **Virtual Network Computing (VNC)** app needs persistent access to screen capture". macOS 14.4+. It is a managed capability, not something a screen recorder can expect to get. | [Apple docs](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.persistent-content-capture) |
| MDM opt-out | Restrictions payload key `forceBypassScreenCaptureAlert`, macOS 15.1+. Managed devices only. Community reports say it was unreliable after 15.2 (secondary). Not something we can use. | [Apple DeviceManagement Restrictions](https://developer.apple.com/documentation/devicemanagement/restrictions), [Jamf thread](https://community.jamf.com/t5/jamf-pro/monthly-tccc-prompts-macos-v15/m-p/331604) |
| `SCContentSharingPicker` | macOS 14.0+. The user picks content each time. | [Apple docs](https://developer.apple.com/documentation/screencapturekit/sccontentsharingpicker) |
| Microphone in `SCStream` | `SCStreamConfiguration.captureMicrophone` and `SCStreamOutputType.microphone` (both macOS 15.0+), plus `microphoneCaptureDeviceID` (15.0+). On macOS 14, the mic must come from AVFoundation (`AVCaptureSession`), which is the spec's plan anyway. | [captureMicrophone](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/capturemicrophone), [.microphone](https://developer.apple.com/documentation/screencapturekit/scstreamoutputtype/microphone), [microphoneCaptureDeviceID](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/microphonecapturedeviceid) |
| `excludesCurrentProcessAudio` | macOS 13.0+. "Exclude audio from your app during capture." Covers "never capture our own app's audio". | [Apple docs](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/excludescurrentprocessaudio) |
| `capturesAudio` | macOS 13.0+. Off by default. | [Apple docs](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/capturesaudio) |
| Per-app audio filtering | SCK audio filtering works **only at the app level**, through the content filter's app set. A single-window filter still captures audio from that whole app. A display filter that includes apps (`init(display:including:exceptingWindows:)`, macOS 12.3+) limits audio to those apps. | WWDC22 [10156](https://developer.apple.com/videos/play/wwdc2022/10156/) and [10155](https://developer.apple.com/videos/play/wwdc2022/10155/) (seen through search transcript snippets), [filter init](https://developer.apple.com/documentation/screencapturekit/sccontentfilter/init(display:including:exceptingwindows:)) |
| Alternative for per-app audio | Core Audio process taps: `AudioHardwareCreateProcessTap` (macOS **14.2+**) with `CATapDescription` (a mix of the chosen processes' output). They don't need a screen stream, but the TCC prompt is separate. | [AudioHardwareCreateProcessTap](https://developer.apple.com/documentation/coreaudio/audiohardwarecreateprocesstap(_:_:)), [CATapDescription](https://developer.apple.com/documentation/coreaudio/catapdescription) |
| Click highlight | `SCStreamConfiguration.showMouseClicks` is macOS **15.0+**. On 14 we'd draw clicks ourselves. | [Apple docs](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/showmouseclicks) |
| Other version gates | `showsCursor` 12.3, `ignoreShadowsSingleWindow` 14.0, `includeChildWindows` 14.2, `presenterOverlayPrivacyAlertSetting` 14.0, `SCRecordingOutput` 15.0 | Apple docs JSON for each symbol under [SCStreamConfiguration](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration) |

Consequence:
- With a normal `SCShareableContent` flow, expect the bypass alert on first use and a periodic re-prompt (about monthly) afterwards.
- `SCContentSharingPicker` avoids it, but it gives the user a system UI for choosing a display, window, or app. It doesn't fit our custom region or hotkey flow.
- Region capture could still work as "pick a display in the picker, then crop".
- Spike 3 should test both flows on this Mac. Ad-hoc signing will make re-prompts more frequent (hop.tools, secondary).
- *Update after M0:* spike 3 tested only the `SCShareableContent` flow, under the dev host, and the re-prompt was not observed. Neither flow comparison was done. The decision is open as ADR-028 (Proposed, needs owner input), with the test moved to M1 in the signed app.

### 2b. Tauri v2 on macOS: windows, panels, sidecars

| Topic | Fact | Source |
|---|---|---|
| `macOSPrivateApi` | Schema text: "No-op in Tauri 2.12.1+ because the APIs are always enabled now." Changelog 2.12.1: the flag "is no longer required to use transparency or fullscreen on macOS". | [schema @2.12.7](https://raw.githubusercontent.com/tauri-apps/tauri/tauri-v2.12.7/crates/tauri-schema-generator/schemas/config.schema.json), [CHANGELOG](https://github.com/tauri-apps/tauri/blob/tauri-v2.12.7/crates/tauri/CHANGELOG.md) |
| Window options | `transparent`, `alwaysOnTop`, `focus`, `focusable`, `skipTaskbar` (Windows/Linux), `shadow` (unsupported on Linux), `acceptFirstMouse` (macOS), `visibleOnAllWorkspaces` (**unsupported on Windows**), `contentProtected` | same schema |
| `contentProtected` on macOS | Not usable to exclude from capture. `NSWindow.SharingType.none` is "a legacy constant that macOS no longer uses… Don't use this value to hide or omit content from being captured." Use `SCContentFilter` with excepted windows or apps instead. | [Apple docs](https://developer.apple.com/documentation/appkit/nswindow/sharingtype-swift.enum/none) |
| All Spaces / over full-screen apps | `NSWindowCollectionBehavior.canJoinAllSpaces` (10.5+) and `.fullScreenAuxiliary` (10.7+, "displays on the same space as the full screen window"). Set them via objc2 on `WebviewWindow::ns_window()`. | [canJoinAllSpaces](https://developer.apple.com/documentation/appkit/nswindow/collectionbehavior-swift.struct/canjoinallspaces), [fullScreenAuxiliary](https://developer.apple.com/documentation/appkit/nswindow/collectionbehavior-swift.struct/fullscreenauxiliary) |
| Non-activating panel | `NSWindow.StyleMask.nonactivatingPanel` is valid only for `NSPanel` (or a subclass). A Tauri window is an `NSWindow`, so it must become a panel. `tauri-nspanel` 2.1.1 does this by swapping the window's Objective-C class at runtime to an NSPanel subclass (`object_setClass` in `src/panel.rs`). Its builder exposes the non-activating mask and the all-Spaces / full-screen-auxiliary behaviors (`src/builder.rs`). We can use the crate (MIT/Apache) or write the same idea ourselves with objc2. | [Apple docs](https://developer.apple.com/documentation/appkit/nswindow/stylemask-swift.struct/nonactivatingpanel), [tauri-nspanel 2.1.1 source](https://docs.rs/crate/tauri-nspanel/2.1.1/source/) |
| Sidecar naming | `bundle.externalBin` entries need one file per target named `name-<target-triple>[.exe]`. `rustc --print host-tuple` prints the triple (Rust ≥ 1.84). | [Tauri sidecar docs](https://v2.tauri.app/develop/sidecar/) |
| Sidecar signing (from source) | tauri-bundler 2.10.6 copies sidecars into `Contents/MacOS` and signs them **before** the app bundle ("inside out"). It uses the **same identity and the same entitlements file** as the app, and adds hardened runtime when `hardenedRuntime` is on. There is no per-sidecar entitlements option, so ffmpeg and gifski would inherit the camera/mic entitlements. With no identity, nothing is signed. | [tauri-bundler 2.10.6 `src/bundle/macos/app.rs`](https://docs.rs/crate/tauri-bundler/2.10.6/source/src/bundle/macos/app.rs), [`sign.rs`](https://docs.rs/crate/tauri-bundler/2.10.6/source/src/bundle/macos/sign.rs) |
| Ad-hoc and free accounts | `signingIdentity: "-"` gives ad-hoc signing. A free Apple account can't notarize. | [Tauri macOS signing](https://v2.tauri.app/distribute/sign/macos/) |
| Binary IPC | `Channel` payloads over 1024 bytes are fetched via the fetch API rather than eval'd (`MAX_RAW_DIRECT_EXECUTE_THRESHOLD`). Informal reports put 10 MB at about 5 ms on macOS and about 200 ms on Windows (**secondary, old**). Spike 2 must measure this itself. | [tauri `ipc/channel.rs`](https://docs.rs/crate/tauri/2.8.4/source/src/ipc/channel.rs), [discussion #11915](https://github.com/orgs/tauri-apps/discussions/11915) |

### 2c. Windows

| Topic | Fact | Source |
|---|---|---|
| `WDA_EXCLUDEFROMCAPTURE` | Introduced in **Windows 10 version 2004**. On earlier versions it behaves like `WDA_MONITOR` (the window shows as black, not hidden). Top-level windows of the **current process** only. Needs DWM composition. | [SetWindowDisplayAffinity](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowdisplayaffinity) |
| `IsBorderRequired` | Introduced in **10.0.20348** (UniversalApiContract v12). In practice that means Windows 11 / Server 2022, because consumer Windows 10 stops at 1904x. To disable the border, call `GraphicsCaptureAccess.RequestAccessAsync(Borderless)` (which prompts the user), and that needs the **`graphicsCaptureWithoutBorder` capability in the app's package manifest**. If access is denied, setting `false` silently does nothing. If another app sets `true`, the border shows. | [IsBorderRequired](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturesession.isborderrequired), [RequestAccessAsync](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscaptureaccess.requestaccessasync) |
| Border for an unpackaged (MSI/NSIS) app | **Unverified.** OBS moderators say OBS disables the border on Windows 11 automatically, controlled by a Windows privacy setting, and that it can come back with exclusive fullscreen (secondary). Test this on Windows 11 in M6. Plan for the border to possibly remain. | [OBS forum](https://obsproject.com/forum/threads/yellow-border.162264/) |
| Other WGC gates | `GraphicsCaptureSession` itself needs 1803 (17134). `IsCursorCaptureEnabled` needs **2004 (19041)**. | [GraphicsCaptureSession](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturesession) |
| Process loopback | `ActivateAudioInterfaceAsync` with the process-loopback init struct supports include-tree and exclude-tree modes, and isn't tied to one endpoint. If the target has no audio streams, you get silence. It needs **Windows 10 build 20348 or later**, which means Windows 11 for consumers, as the spec assumes. | [ApplicationLoopback sample](https://learn.microsoft.com/en-us/samples/microsoft/windows-classic-samples/applicationloopbackaudio-sample/) |
| MF hardware encoders | Hardware MFTs **must be asynchronous**. Enumerate with `MFTEnumEx` + `MFT_ENUM_FLAG_ASYNCMFT` (plain `MFTEnum` skips them). The client must set `MF_TRANSFORM_ASYNC_UNLOCK`, or most calls return `MF_E_TRANSFORM_ASYNC_LOCKED`. Input and output are driven by `METransformNeedInput` / `METransformHaveOutput` events, and calls outside those events fail (`MF_E_NOTACCEPTING` / `E_UNEXPECTED`). Drain ends with `METransformDrainComplete`. Async MFTs must support dynamic format changes. | [Hardware MFTs](https://learn.microsoft.com/en-us/windows/win32/medfound/hardware-mfts), [Asynchronous MFTs](https://learn.microsoft.com/en-us/windows/win32/medfound/asynchronous-mfts) |
| MF H.264 settings | The output type must be set before the input type. Inputs: NV12/I420/IYUV/YUY2/YV12. GOP: `CODECAPI_AVEncMPVGOPSize` (needed for our "keyframe about every second" rule). Rate control: CBR / PeakConstrainedVBR / Quality / UnconstrainedVBR. A certified hardware encoder is generally used instead of the inbox software one. | [H.264 Video Encoder](https://learn.microsoft.com/en-us/windows/win32/medfound/h-264-video-encoder) |

Not researched here: per-vendor limits on concurrent hardware encode sessions, and the exact maximum resolution per GPU. The capability probe has to find these by actually encoding (spec pitfall 6).

### 2d. Linux

| Topic | Fact | Source |
|---|---|---|
| ScreenCast persistence | `persist_mode` (added in **v4**): 0 none (default), 1 while the app runs, 2 until revoked. `restore_token` is returned from `Start()` only if persistence was granted, and it is **single-use**, so store the new token after every session. `cursor_mode`: 1 hidden, 2 embedded, 4 metadata (v2). `types`: 1 MONITOR, 2 WINDOW, 4 VIRTUAL. The docs describe interface v6. | [ScreenCast portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html) |
| Backend support | GNOME: xdg-desktop-portal-gnome **42** added restore tokens (secondary: [RHEL-4526](https://issues.redhat.com/browse/RHEL-4526), [42.0.1 notes](https://lists.gnome.org/archives/ftp-release-list/2022-March/msg00112.html)). KDE: **unverified**. | as listed |
| GlobalShortcuts portal | Interface v2. Methods: `CreateSession`, `BindShortcuts`, `ListShortcuts`, and `ConfigureShortcuts` (v2). Signals: `Activated`, `Deactivated`, `ShortcutsChanged`. | [GlobalShortcuts portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html) |
| GlobalShortcuts support | **GNOME 48**: "fully supported". **KDE Plasma 5.27** (Wayland) added support according to the 5.27 announcement (seen via search snippet, **secondary**). Users reported setup-dialog problems with early GNOME 48 builds. | [GNOME 48 developer notes](https://release.gnome.org/48/developers/), [Plasma 5.27](https://kde.org/announcements/plasma/5/5.27.0/), [GNOME Discourse](https://discourse.gnome.org/t/issue-with-global-shortcuts-setup-popup-in-gnome-48/27946) |
| Camera portal | `AccessCamera()`, then `OpenPipeWireRemote()` (which fails without permission). `IsCameraPresent` property. It mainly matters for sandboxed apps, since a non-Flatpak app can open V4L2 directly. | [Camera portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Camera.html) |
| System audio | PipeWire stream property **`stream.capture.sink`** (`PW_KEY_STREAM_CAPTURE_SINK`): "Try to capture the sink output instead of source output". Combine it with `target.object` (a node name or serial) for the default sink. `node.dont-reconnect` stops the stream from following device changes, so leave it unset. cpal's `pipewire` feature doesn't document monitor capture, so use the `pipewire` crate directly. | [keys.h](https://gitlab.freedesktop.org/pipewire/pipewire/-/blob/master/src/pipewire/keys.h), [pipewire-props(7)](https://docs.pipewire.org/page_man_pipewire-props_7.html) |
| WebKitGTK (Tauri) | Tauri's official NVIDIA workaround is `WEBKIT_DISABLE_DMABUF_RENDERER=1`, which "disables the faster rendering path". On Wayland, `__NV_DISABLE_EXPLICIT_SYNC=1` often fixes Error 71 with no performance cost. There are long-running upstream NVIDIA regressions (WebKit bug 261874). This supports the spec's rule: send small frames only, and measure preview transport on Linux. | [Tauri Linux graphics](https://v2.tauri.app/develop/debug/linux-graphics/), [WebKit 261874](https://bugs.webkit.org/show_bug.cgi?id=261874) |
| Global hotkeys | `global-hotkey` supports X11 only, so Wayland needs the portal through `ashpd` (`global_shortcuts` feature). | [global-hotkey README](https://github.com/tauri-apps/global-hotkey) |

### 2e. BtbN / FFmpeg-Builds

| Topic | Fact | Source |
|---|---|---|
| Repo location | `BtbN/FFmpeg-Builds` now redirects to **`FFmpeg-Builds/FFmpeg-Builds`** (MIT build scripts). Download URLs use the new org. | `gh api repos/BtbN/FFmpeg-Builds`, [repo](https://github.com/FFmpeg-Builds/FFmpeg-Builds) |
| Tags | Daily `autobuild-YYYY-MM-DD-HH-MM` (for example `autobuild-2026-11-19-12-39`), plus a floating `latest`. | [releases](https://github.com/FFmpeg-Builds/FFmpeg-Builds/releases) |
| Retention | "The last build of each month is kept for two years. The last 14 daily builds are kept." Today 38 releases exist: 14 dailies, monthly ones back to `autobuild-2024-12-31-13-02`, and `latest`. **Pin a month-end tag** (for example `autobuild-2026-10-31-12-59`) and mirror the archive in CI cache. | [README](https://github.com/FFmpeg-Builds/FFmpeg-Builds#release-retention-policy) |
| Checksums | Each release has a `checksums.sha256` asset. The GitHub API also exposes a per-asset `digest` (`sha256:…`). Record the sha256 in `third_party/`, and verify both. | `gh api repos/FFmpeg-Builds/FFmpeg-Builds/releases` |
| Asset names (dated release) | `ffmpeg-N-<rev>-g<sha>-{win64,winarm64,linux64,linuxarm64}-{gpl,gpl-shared,lgpl,lgpl-shared}.{zip,tar.xz}` for master. Release branches use `ffmpeg-n9.0.2-58-g8c7ef49002-<target>-<variant>-9.0.{zip,tar.xz}`. In `latest`, the same assets are named `ffmpeg-master-latest-…` and `ffmpeg-n9.0-latest-…-9.0…`. Branches currently built: master, **n9.0**, **n8.1**. | same |
| Variants | `gpl`: all dependencies. `lgpl`: lacks GPL-only libraries ("most prominently libx264 and libx265"). `nonfree`: adds fdk-aac (**never use**). `*-shared`: libav* as shared libraries. Example sizes (2026-11-19 `latest`): win64 gpl zip 205 MB, linux64 lgpl-shared tar.xz 67 MB. | [README](https://github.com/FFmpeg-Builds/FFmpeg-Builds) |
| Platform floor | "Windows builds are targetting Windows 7 and newer, provided UCRT is installed. **The minimum supported version is Windows 10 22H2**." Linux: RHEL 8 (glibc 2.28). | README |
| macOS | No macOS builds. The spec already says to build from source with a pinned script. | README |

### 2f. gifski CLI

| Topic | Fact | Source |
|---|---|---|
| Y4M from stdin | Added in **1.32.0** (2024-04-24): `ffmpeg … -f yuv4mpegpipe - \| gifski -o out.gif -`. 1.33.0 improved Y4M support. **1.34.0** (2025-07-13) added `--in-color-space` for ambiguous YUV4MPEG input. **Minimum 1.32.0. Recommend 1.34.0.** | [releases](https://github.com/ImageOptim/gifski/releases), `reference/gifski/README.md` |
| Accepted Y4M formats (from `reference/gifski/src/bin/y4m_source.rs` @61ac230) | 8-bit only: `C420` / `C420jpeg` / `C420paldv` / `C420mpeg2`, `C422`, `C444`, and mono. 10/12-bit input is rejected. Range comes from the Y4M `COLORRANGE=` tag (defaults to limited). The colour matrix defaults to **BT.601 if the frame is ≤ 720×480, otherwise BT.709**, unless `--in-color-space` is set. Frames larger than 65535 on a side are rejected. | reference clone |
| Options we need | `--fps`, `--fast`, `--extra`, `--quality`, `--motion-quality`, `--lossy-quality`, `--repeat` (−1 none, 0 forever, N), `--width`/`--height` | `reference/gifski/src/bin/gifski.rs` |
| Build | The default features (`gifsicle`, `binary`) are enough for Y4M. The `video` feature links FFmpeg, and we don't need it. | `reference/gifski/Cargo.toml` |

Recommendation: always write the `COLORRANGE` tag and pass `--in-color-space` explicitly (BT.709) so that small GIFs don't silently switch to BT.601 (spec pitfall 5).

---

## 3. Open items (not verified here)

- Whether `SCContentSharingPicker` sessions avoid the monthly re-prompt on macOS 26 specifically. DTS said so for 15. **Not tested in spike 3** (it never used the picker); moved to an M1 test in the signed app (ADR-028).
- Whether Windows 11 lets an unpackaged (MSI/NSIS) Tauri app turn off the WGC border.
- KDE Plasma restore-token support, and the exact first Plasma version with GlobalShortcuts.
- Whether the `screencapturekit` crate's Swift bridge builds with xcode-select pointing at the Command Line Tools.
- Which third-party licenses the prebuilt Skia binaries bring in. Check with cargo-about in spike 4.
- Whether `license-checker-rseidelsohn` handles pnpm's layout correctly.
- Tauri binary-IPC throughput on this machine. That is spike 2's job, and no numbers here.
