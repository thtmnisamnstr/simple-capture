# Licenses

Status: M0 draft, 2026-11-19; M1 results added 2026-11-20 (§2, §10). This is the licensing policy for Simple Capture, how CI enforces it, and what every release must ship. Attribution for code or designs we adapt lives in docs/THIRD_PARTY.md; this file covers dependencies and bundled programs. It records engineering decisions, not legal advice.

Sources: S1-S4 are the M0 spikes (`spikes/01-helpers`, `02-preview-transport`, `03-macos-pipeline`, `04-skia-text`) and their quiet-machine verification passes; R-* are the notes in `docs/research/` (R-mit = cap-mit-crates.md, R-eco = ecosystem.md, R-kap = kap.md, R-rec = cap-recording.md). Each spike now has a REPORT.md (saved at M0 close-out). M1 facts cite the package that produced them as M1:<package> (for example M1:text = wp-text, M1:ci = wp-ci); the reports are not stored in the repo, and docs/STATUS.md §2 is the in-repo record of M1 results.

## 1. Policy

| What | Allowed licenses | How it ships |
|---|---|---|
| Simple Capture's own code | MIT | The app binary and CLI. |
| Everything linked into the app binary (Rust crates, C/C++ libraries compiled into crates, the TypeScript UI bundle) | MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unicode-3.0, CC0-1.0, plus MPL-2.0 only as a named per-crate exception | Statically or dynamically linked, no restrictions beyond notices. |
| LGPL code | LGPL-2.1-or-later (or LGPL-3.0-or-later) | **Linux:** only dynamically linked, the FFmpeg `lgpl-shared` libraries for in-process decode and hardware recording encode. Ship the `.so` files unmodified as separate files so users can replace them. **macOS (the DMG and the Mac App Store alike):** the `sc-ffmpeg` and `sc-ffprobe` executables, whose whole program is LGPL-2.1-or-later with its full source published (§3.4); never linked into the app. |
| GPL / AGPL code | GPL-2.0-or-later, GPL-3.0-or-later, AGPL-3.0-or-later | **Only as separate bundled executables** (Tauri sidecars) that we start as processes and talk to over pipes. Never linked into the app. Today, on Windows and Linux: `ffmpeg`, `ffprobe` and `gifski`, bundled as `sc-ffmpeg`, `sc-ffprobe` and `sc-gifski`. Never in any macOS build (§3.4, DECISIONS ADR-058). |
| Non-free code | none | Never. No `--enable-nonfree`, no fdk-aac, no BtbN `nonfree` variant. |
| Fonts | SIL OFL-1.1, Apache-2.0 | Bundled with their license files (section 6). |
| Icons | MIT, ISC | Section 7. |

Hard bans in the app binary: x264, x265, gifski, libimagequant (GPL-3.0-or-later), and any non-MIT Cap crate (section 8).

## 2. Enforcement in CI

All three run on every PR on all three OSes (spec, "Engineering and testing"), and fail the build.

### 2.1 cargo-deny (Rust)

Use cargo-deny 0.22.1 (MSRV 1.95; 0.21.0 is the fallback pin) (R-eco). Outline of `deny.toml`:

```toml
[graph]
all-features = true            # check optional features too
targets = ["aarch64-apple-darwin", "x86_64-apple-darwin",
           "x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu"]

[licenses]
allow = ["MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception",
         "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib",
         "Unicode-3.0", "CC0-1.0"]
confidence-threshold = 0.9
exceptions = [
  # Linux in-process FFmpeg bindings; the libraries they load are LGPL (section 3.3).
  { allow = ["WTFPL"], crate = "ffmpeg-next" },
  { allow = ["WTFPL"], crate = "ffmpeg-sys-next" },
  # Add MPL-2.0 here per crate, with a reason in DECISIONS.md, only if unavoidable.
]

[bans]
deny = [
  { crate = "gifski" }, { crate = "imagequant" },
  { crate = "x264" }, { crate = "x264-sys" }, { crate = "x265" }, { crate = "x265-sys" },
  # Cap crates that are AGPL (R-mit), in case anyone vendors them by name:
  { crate = "workspace-hack" }, { crate = "cap-d3d-adapter" },
  { crate = "cap-mediafoundation-utils" }, { crate = "cap-cursor-info" },
  { crate = "cap-cpal-ffmpeg" },
]

[sources]
unknown-registry = "deny"
unknown-git = "deny"           # no git dependencies on Cap or its forks
```

Notes:
- `ffmpeg-next` / `ffmpeg-sys-next` must only appear in the Linux target graph. Add a CI check that `cargo tree --target aarch64-apple-darwin` and `--target x86_64-pc-windows-msvc` do not contain them.
- cargo-deny reads each crate's `license` field only. It does not see C/C++ code compiled inside a crate, so section 2.4 covers that by hand.
- Also run `cargo deny check advisories` and keep `sources` strict, so a networking crate cannot slip in through a git dependency.

**As built in M1** (`deny.toml`, M1:ci; ADR-024 M1 update):
- Five MPL-2.0 per-crate exceptions, unavoidable with Tauri 2 and used unmodified (MPL-2.0 is file-level copyleft; we ship no modified MPL files): `cssparser`, `cssparser-macros`, `selectors`, `dtoa-short` (tauri-utils → dom_query, HTML/CSP processing of the bundled UI) and `option-ext` (tauri → dirs → dirs-sys). Their notices come through cargo-about.
- HTTP clients and servers are banned: `reqwest`, `hyper`, `ureq`, `isahc`, `curl`, `curl-sys`, `attohttpc`, `surf`. reqwest and hyper sit in Cargo.lock only as Tauri's Android/iOS dependencies; `scripts/ci/deny.sh` proves with `cargo tree --target <each desktop target> -i <crate>` that no desktop target builds them, and that `ffmpeg-next` / `ffmpeg-sys-next` are absent from Cargo.lock (no Linux in-process FFmpeg yet). On Linux, `soup3` comes in through webkit2gtk (the system WebView) and is not banned (M1:scaffold).
- `[advisories]`: `unmaintained = "workspace"`, `yanked = "deny"`; private crates are ignored.
- The objc2-app-kit `default` feature is off in the app graph; `otool -L` shows neither CloudKit nor CoreData in `simple-capture-app`, `simple-capture` or `sc-devtools` (M1:scaffold).

### 2.2 cargo-about (Rust notices)

cargo-about 0.9.3 (R-eco) generates the third-party notices for the About → Licenses screen from the same `accepted` list as cargo-deny. Its output is a release artifact (section 5), generated in CI and not checked in.

- Install with `cargo install cargo-about --version 0.9.3 --features cli`; without `--features cli` no binary is installed (M1:ci).
- `scripts/ci/notices.sh` runs it with `--fail` and the template `scripts/ci/about.hbs`. M1 run (M1:ci-green): `THIRD_PARTY_RUST.html`, 395,424 bytes, about 448 entries, in 27.7 s. Licence counts as printed: MIT 194, Apache-2.0 5, MPL-2.0 3, Unicode-3.0 2, BSD-3-Clause 2, Zlib 1, ISC 1 (cargo-about groups crates that share a licence text, so these are texts, not crates).

### 2.3 npm license check (UI)

Use pnpm's built-in `pnpm licenses list --prod --json` and a small script that fails on anything outside the section 1 allow list (R-eco). `license-checker-rseidelsohn` 5.0.1 is the fallback; whether it handles pnpm's symlinked `node_modules` is unverified (R-eco). Only production dependencies end up in the shipped bundle; dev tools (Vite, svelte-check, TypeScript) are not shipped.

**As built in M1** (`scripts/ci/licenses.sh` + `npm-licenses.mjs`, M1:ci): no dependencies, a small SPDX AND/OR/WITH evaluator with its own node tests. `--prod` is strict. `--all` also checks dev packages, because svelte is a devDependency compiled into the bundle; the only dev exception is lightningcss (MPL-2.0, a Vite build tool, not shipped). Result (M1:ci-green): production 1 package ok, all 50 packages ok.

### 2.4 Native code the tools above cannot see

Kept as a hand-maintained list in `third_party/` and included in the notices.

**M1 audit of the Skia build** (M1:text; source `target/skia/debug/build/skia-bindings-488bc29a65ace2b8`: `args.gn`, `cargo:rustc-link-lib` lines, and `ar t` / `nm` over the static libraries). skia-safe 0.157.0 with features `textlayout` + `embed-freetype`; skia-safe's default features (`pdf`, `embed-icudtl`, `jpeg`, `binary-cache`) are also on, because the workspace dependency does not turn them off. Linked statically: `skparagraph`, `skshaper`, `skunicode_core`, `skunicode_icu`, `skia`, `skia-bindings`, plus the system `c++` and `ApplicationServices`. GN args: FreeType on and vendored (`skia_use_system_freetype2=false`), every `system_*` option off, libjpeg-turbo decode/encode on, libwebp off, PDF on, ICU and HarfBuzz bundled; Ganesh, Graphite, SVG, Skottie, XPS and DNG off.

| Component (compiled into the app binary) | Comes in through | License |
|---|---|---|
| Skia | `skia-safe` / `skia-bindings` | BSD-3-Clause |
| FreeType | Skia build (FreeType on every OS, ADR-018) | FreeType License (FTL); requires a credit in the documentation/About |
| HarfBuzz | Skia build (`textlayout`) | MIT-style ("Old MIT") |
| ICU, plus the embedded `icudtl.dat` | Skia build (`skunicode_icu`, default feature `embed-icudtl`) | Unicode-3.0 |
| libjpeg-turbo | Skia build (default feature `jpeg`) | IJG + BSD-3-Clause + Zlib |
| libpng | Skia build | libpng license (PNG Reference Library License v2) |
| zlib (Chromium's copy) | Skia build | Zlib |
| expat | Skia build, pulled in by the default `pdf` feature | MIT |
| Wuffs (GIF decode) | Skia build | Apache-2.0 (to confirm against the vendored LICENSE when notices are generated) |
| skcms | Skia build | BSD-3-Clause (Google; to confirm against the vendored LICENSE) |

- Checked out under Skia's externals but **not** compiled in: libwebp, brotli and others (no symbols in the archives).
- WebP decode is off (`build_info().webp_decode = false`), matching ADR-033/Q9 (PNG and JPEG backgrounds only).
- Optional, for the owner or orchestrator: `default-features = false, features = ["textlayout", "embed-freetype", "jpeg"]` would drop PDF and expat (and the binary-cache feature). It is a manifest change that rebuilds Skia everywhere, so it was not made in M1.
- macOS: `otool -L` shows only system libraries and frameworks in the app, CLI and devtools binaries (M1:scaffold, as in S4).

## 3. Bundled helpers

### 3.1 FFmpeg, macOS (built by us)

Built from source by `third_party/ffmpeg/build-macos.sh` (S1). Pins, URLs and SHA-256 values are in `third_party/versions.toml`; each build records its inputs in `third_party/out/macos-<arch>/`.

- **Variants (DECISIONS ADR-058):** the default, `SC_FFMPEG_LICENSE=lgpl`, is what every macOS build ships: LGPL-2.1-or-later, into `third_party/out/macos-<arch>` (§3.4). `SC_FFMPEG_LICENSE=gpl` adds x264 and x265 into `third_party/out/macos-<arch>-gpl` for local experiments only; `scripts/stage-helpers.sh` refuses it. The rest of this section describes both unless it says GPL.
- **Version:** FFmpeg 9.1, `--extra-version=simple-capture` (reports `9.1-simple-capture`).
- **Result license (GPL variant):** **GPL-2.0-or-later** (`--enable-gpl`, no `--enable-version3`, no `--enable-nonfree`; every linked library is GPLv2-compatible) (S1).
- **Targets:** arm64 (built, benchmarked, validated), x86_64 (cross-built, smoke-tested under Rosetta only; last built for 9.0.2). Deployment target macOS 14.0, built against SDK 26.5 (`vtool`).
- **Links to:** only libSystem, libc++, and the AudioToolbox, VideoToolbox, CoreFoundation, CoreMedia, CoreVideo and CoreServices frameworks (`otool -L`; the build script fails on anything else). These are operating-system components.
- **No networking:** `--disable-network`, only the `file` and `pipe` protocols, no socket or getaddrinfo imports (S1 verification).

| Library | Version | License | Source (exact URL and SHA-256 in versions.toml) | Checksum cross-check |
|---|---|---|---|---|
| FFmpeg | 9.1 | LGPL-2.1-or-later code, GPL-2.0-or-later as configured | https://ffmpeg.org/releases/ffmpeg-9.1.tar.xz | Upstream GPG signature verified (release key `FCF986EA…58D8`); Homebrew formula matches; contents equal git tag n9.1. |
| x264 | 0.165.r3222, stable branch commit b35605ac | GPL-2.0-or-later | code.videolan.org archive of b35605ac | No upstream checksum; tarball identical to a git clone (`diff -r`). |
| x265 | 4.3 | GPL-2.0-or-later | github.com/Multicorewareinc/x265 release 4.3 | GitHub asset digest and Homebrew match. |
| SVT-AV1 | 4.3.1 | BSD-3-Clause-Clear AND AOM Patent License 1.0 | github.com/AOMediaCodec/SVT-AV1 tag v4.3.1 (the project's canonical home since GitLab stopped at v4.3.0) | No upstream checksum; tarball identical to a git clone (`diff -r`); v4.3.0 is the same commit on GitLab and GitHub. |
| libvpx | 1.17.0 | BSD-3-Clause (plus Google's PATENTS grant) | github.com/webmproject/libvpx tag v1.17.0 | Homebrew and markus-perl match. |
| Opus | 1.6.1 | BSD-3-Clause | downloads.xiph.org | Upstream SHA256SUMS, Homebrew, markus-perl match. |
| libogg | 1.3.6 | BSD-3-Clause | downloads.xiph.org | Upstream SHA256SUMS, markus-perl match. |
| libvorbis | 1.3.7 | BSD-3-Clause | downloads.xiph.org | Upstream SHA256SUMS, Homebrew match. Darwin configure patched to drop `-force_cpusubtype_ALL`. |
| libwebp | 1.6.0 (with libwebpmux and libsharpyuv) | BSD-3-Clause (plus Google's PATENTS grant) | storage.googleapis.com/downloads.webmproject.org | Homebrew matches. **GPG signature not verified.** For animated WebP. |

Configure flags of the GPL variant (full line in `ffmpeg.configure.txt` next to the binaries, `ffmpeg -buildconf` in `ffmpeg.buildconf.txt`); the LGPL variant drops `--enable-gpl`, `--enable-libx264`, `--enable-libx265` and those two encoders. The licensing-relevant ones:

```
--enable-gpl --disable-shared --enable-static --disable-autodetect --disable-network
--disable-ffplay --disable-doc --disable-debug --disable-sdl2 --disable-xlib
--disable-securetransport --disable-iconv --disable-zlib --disable-bzlib --disable-lzma
--disable-appkit --disable-avfoundation --disable-coreimage --disable-metal
--enable-videotoolbox --enable-audiotoolbox --enable-pthreads
--enable-libx264 --enable-libx265 --enable-libsvtav1 --enable-libvpx --enable-libopus --enable-libvorbis --enable-libwebp
--disable-everything --enable-protocol=file,pipe
--enable-encoder=libx264,libx265,libsvtav1,libvpx_vp8,libvpx_vp9,h264_videotoolbox,hevc_videotoolbox,prores_videotoolbox,prores_ks
--enable-encoder=aac,aac_at,libopus,libvorbis,flac,alac,pcm_s16le,pcm_s24le,pcm_f32le
--enable-encoder=gif,libwebp_anim,rawvideo,wrapped_avframe
(plus explicit decoder, muxer, demuxer, parser, hwaccel and filter allow lists)
```

Known gaps: there is no AV1 decoder (S1). Sizes (9.1): arm64 ffmpeg 20,970,216 B and ffprobe 20,761,512 B; x86_64 ffmpeg 29,354,544 B (9.0.2). Shipping ffprobe is optional (decide in M5).

### 3.2 gifski (built by us, Windows and Linux; not shipped on macOS)

- **Version:** gifski 1.34.0 from the crates.io `.crate` (SHA-256 `c246c795…1190`, matches the registry checksum; git tag 1.34.0, commit 1060eab4), built with `cargo build --release --locked --bin gifski` (`third_party/gifski/build.sh`) (S1).
- **Lockfile:** the build replaces the crate's `Cargo.lock` with `third_party/gifski/gifski-1.34.0.Cargo.lock` (SHA-256 pinned in versions.toml), which differs only in crossbeam-epoch 0.9.18 → 0.9.20 for RUSTSEC-2026-0204. `cargo audit -f` on it reports no advisories (2026-11-24). Neither Dependabot nor cargo-deny reads this lockfile, so re-run `cargo audit -f` on it when touching the helpers.
- **Features:** defaults only (`binary`, `gifsicle`); the `video` feature, which links FFmpeg, is never enabled.
- **Result license:** **AGPL-3.0-or-later**. It contains libimagequant 4.4.1 (**GPL-3.0-or-later**); every other compiled crate is permissive (list with licenses in `third_party/out/macos-arm64/gifski.deps.txt`, 52 lines).
- **Open question:** the default `gifsicle` feature compiles a Rust port of gifsicle's lossy-LZW code (`src/gifsicle.rs` in the pinned crate; it has no licence header of its own, and the crate declares only `AGPL-3.0-or-later`), and gifsicle is GPL-2.0 upstream. The combined binary is still distributed under AGPL-3.0-or-later, but check that module's provenance (who wrote the port, from which gifsicle/giflossy code, under which licence) before M5, or build with `--no-default-features --features binary` if we drop "lossy" (S1 verification). Not resolved in M0.
- **Notice gap (open, must be fixed before any release):** the build collects only `gifski.LICENSE` (the AGPL-3.0 text). Missing from `third_party/out/<os>-<arch>/`:
  - the **GPL-3.0** text for libimagequant 4.4.1, and its `COPYRIGHT` file, which also carries the original pngquant licence notice (Jef Poskanzer, Greg Roelofs) that must be reproduced;
  - the licence texts and copyright notices of the ~50 permissive crates compiled in (`gifski.deps.txt`: MIT, Apache-2.0, BSD-2-Clause for `yuv`, Zlib for `lodepng`, `zlib-rs`, `libz-rs-sys`, CC0 for `imgref`, and so on). MIT and BSD require the notice to ship with the binary.
  - Fix in `third_party/gifski/build.sh` (M5 at the latest): run cargo-about (ADR-024) over gifski's own `Cargo.lock` and write `licenses/gifski/*`, plus copy imagequant's `COPYRIGHT` and the GPL-3.0 text.
- Links only libSystem and the system libiconv on macOS, where it was built in M0 but is no longer shipped (ADR-058).
- Never linked as a crate: banned in cargo-deny (section 2.1).

### 3.3 FFmpeg, Windows and Linux (prebuilt, pinned)

From FFmpeg-Builds (formerly BtbN/FFmpeg-Builds, now `github.com/FFmpeg-Builds/FFmpeg-Builds`; build scripts MIT) (R-eco):

- **Export helper:** the `gpl` variant (static) for win64 and linux64.
- **Linux in-process libraries:** the `lgpl-shared` variant (libavcodec, libavformat, libavutil, libswscale, libswresample as `.so`), loaded through `ffmpeg-sys-next` (WTFPL bindings).
- **Pinning:** daily builds disappear after 14 days, so pin a **month-end tag** (for example `autobuild-2026-10-31-12-59`, kept two years), verify each archive against both `checksums.sha256` and the GitHub per-asset digest, record the SHA-256 in `third_party/versions.toml`, and cache the archive in CI (R-eco).
- **Before adopting, check `ffmpeg -buildconf` of the pinned build and record it**, because three things are unverified here:
  1. **License version.** If the build uses `--enable-version3`, the `gpl` binary is GPL-3.0-or-later and the `lgpl` libraries are LGPL-3.0-or-later. Either is allowed by section 1, but the notices must say which.
  2. **Networking.** These are general-purpose builds. If they keep FFmpeg's network protocols (likely), the shipped helper and the Linux in-process libraries contain networking code, which conflicts with the spec's "no networking code anywhere". The app never passes them a URL, but the code is there. Options: build our own Windows/Linux helpers with FFmpeg-Builds' MIT scripts plus `--disable-network`, or record an explicit exception in DECISIONS.md. **This needs the owner's decision.**
  3. **Windows floor.** FFmpeg-Builds states Windows 10 22H2 as its minimum, newer than the spec's Windows 10 2004 (R-eco). Either update the spec or build our own.

### 3.4 macOS helper set: the DMG and the Mac App Store

The App Store's usage rules add restrictions that GPL-2.0 §6 and GPL-3.0/AGPL-3.0 §10 forbid (the FSF's position; GNU Go was removed in 2010 and VLC for iOS in 2011 after copyright holders complained). So the store build (DECISIONS ADR-057) ships no GPL or AGPL code, and since ADR-058 the DMG and Homebrew cask are the same app with the same helpers:

- **`sc-ffmpeg` and `sc-ffprobe`: LGPL-2.1-or-later.** The pinned FFmpeg sources and `build-macos.sh`'s default variant, built without `--enable-gpl`, `--enable-libx264`, `--enable-libx265` and those encoders. No `--enable-version3`, so LGPL-3.0's installation-information terms don't arise for a signed store bundle. Linked in: SVT-AV1 (BSD-3-Clause-Clear and the AOM Patent License), and libvpx, Opus, Ogg, Vorbis and libwebp (BSD-3-Clause; libvpx and libwebp with Google's patent grants), plus the same Apple frameworks as §3.1. Networking stays off.
- **Not shipped:** x264, x265, gifski and libimagequant. H.264 and HEVC exports use VideoToolbox (no "Best Quality" for them on macOS) and GIFs use palette mode.
- **Notices:** FFmpeg's LGPL-2.1 text (`COPYING.LGPLv2.1`) and `LICENSE.md` and the notices of the permissive libraries above (the §5 set without x264, x265 and gifski) are bundled in `Contents/Resources/licenses` (staged by `scripts/stage-helpers.sh` into `app/gen/licenses`, with the Rust notices from `scripts/ci/notices.sh --macos`) and shown in About → Licenses. `scripts/ci/notices.sh --check-macos DIR` fails unless DIR holds exactly this set and no GPL/AGPL text; the package workflow runs it on every macOS build.
- **Corresponding source:** the GitHub release of the same tag, `https://github.com/thtmnisamnstr/simple-capture/releases/tag/v<version>`. Its `simple-capture-helper-sources.tar.gz` holds every pinned tarball and `third_party/` (the scripts that build both variants), kept for at least three years like every release (§5). The DMG is downloaded from that page, which gives "equivalent access"; the store build is not, so for it the FFmpeg entry in About → Licenses, which says its complete source is attached to this version's GitHub release and has a **Source code** button that opens that release page (`releases/tag/v<version>` of the running version), is the equivalent of a written offer. Both Mac builds show the same entry.
- **Residual risk:** the LGPL also forbids further restrictions (§10), so the store terms are not settled for it either; LGPL FFmpeg ships in many store apps without enforcement. A custom licence agreement in App Store Connect that leaves the LGPL rights intact would lower the risk (open).

## 4. Patents

- **H.264, HEVC and AAC** are covered by patent pools (Via LA, Access Advance and others). The spec accepts this for personal use; wider or commercial distribution may need licenses. Using an OS encoder (VideoToolbox, Media Foundation) for Fast exports does not settle the patent question for files people distribute, so treat both paths the same.
- **AV1:** SVT-AV1 ships under the AOMedia Patent License 1.0, which is included with the helper's license files (`svt-av1-PATENTS.md`).
- **VP8/VP9:** libvpx ships Google's patent grant (`libvpx-PATENTS`), included.
- **Opus, Vorbis, FLAC, ALAC, GIF:** treated as royalty-free.
- The About → Licenses screen states this plainly. Not legal advice.

## 5. What every release ships

For each OS and architecture, next to the helpers inside the app bundle and in the About → Licenses screen:

1. **License texts** for every helper and every library linked into it (`third_party/out/<os>-<arch>/licenses/ffmpeg/*`: FFmpeg COPYING.GPLv2 and LICENSE.md, x264 and x265 COPYING, SVT-AV1 LICENSE and PATENTS, libvpx LICENSE and PATENTS, Opus, Ogg and Vorbis COPYING; plus `gifski.LICENSE`, **and, not yet collected**, libimagequant's GPL-3.0 text and `COPYRIGHT` (pngquant notice) and the notices of every permissive crate in `gifski.deps.txt`; see section 3.2). For BtbN builds, their bundled license files.
2. **Version and build record:** `ffmpeg.configure.txt`, `ffmpeg.buildconf.txt`, `ffmpeg.sources.txt` (versions and SHA-256), `gifski.buildinfo.txt`, `gifski.deps.txt`.
3. **Corresponding source.** GPL/AGPL require us to provide it, not just link upstream. Attach to every GitHub release a source archive containing every pinned tarball (FFmpeg, x264, x265, SVT-AV1, libvpx, Opus, Ogg, Vorbis, the gifski crate with its vendored dependencies) **plus our build scripts** (`third_party/`, which count as the scripts that control compilation). Distributing it from the same release page as the binaries satisfies "equivalent access". Also include a written offer in the notices as a fallback. For BtbN binaries, mirror their matching source and scripts the same way. Keep these for at least three years per release. Every build's About → Licenses also has a **Source code** button on the FFmpeg entry (and, on Windows and Linux, the gifski entry) that opens the release of the running version. The Mac App Store build is not downloaded from that page, so for it that button is the source offer (§3.4).
4. **Rust notices** from cargo-about, **npm notices** from the pnpm license list, and the **native components list** from section 2.4 (including the FreeType credit).
5. **Font licenses** for every bundled font (section 6).
6. **Patent note** (section 4).

Every macOS build (DMG and Mac App Store) ships the §3.4 set instead (no x264, x265 or gifski texts). The spec's definition of done also requires a check that the app binary contains no GPL/AGPL code: CI runs cargo-deny and the npm check, and a script confirms no `x264`, `x265`, `gifski` or `imagequant` symbols in the app binary.

macOS signing note: tauri-bundler signs sidecars with the app's identity **and the app's entitlements** (no per-sidecar option), so the helpers would inherit camera/mic entitlements (ADR-003 accepted that for v1, R-eco). `scripts/macos-sign.sh`, the one signing path for the DMG and the Mac App Store build, now signs the helpers itself, before the app, with only `app-sandbox` and `inherit` (`app/macos/helper.entitlements.plist`), so they run inside the app's sandbox and hold no camera or microphone entitlement.

## 6. Fonts

- **Policy:** bundle only SIL OFL-1.1 or Apache-2.0 fonts, each pinned by commit and SHA-256 and shipped with its license file. System fonts are used through fontdb (path + face index) and never redistributed. Projects store font hashes so export can warn when a font is missing (S4).
- **Emoji:** Noto Color Emoji **COLRv1** v2.051 (OFL-1.1, 5.0 MB), not the 10.7 MB CBDT build (S4).
- **Suggested set (S4, all checked in google/fonts @ cc5661d):**

| Font | License | Path in google/fonts |
|---|---|---|
| Inter | OFL | ofl/inter |
| Montserrat | OFL | ofl/montserrat |
| Oswald | OFL | ofl/oswald |
| Bebas Neue | OFL | ofl/bebasneue |
| Playfair Display | OFL | ofl/playfairdisplay |
| DM Serif Display | OFL | ofl/dmserifdisplay |
| Courier Prime | OFL | ofl/courierprime |
| Special Elite | **Apache-2.0** | apache/specialelite |
| Caveat | OFL | ofl/caveat |
| Permanent Marker | **Apache-2.0** | apache/permanentmarker |
| Pacifico | OFL | ofl/pacifico |
| Tilt Neon | OFL | ofl/tiltneon |
| Fredoka | OFL | ofl/fredoka |
| Noto Color Emoji (COLRv1) | OFL | googlefonts/noto-emoji v2.051 |

Decision needed: the spec's Licensing section allows "OFL/Apache", but its Text section says "curated bundled OFL fonts". Two suggestions are Apache-2.0 (S4 verification). Either accept Apache-2.0 fonts or swap those two.

- In the repo so far (spike 4 only, not yet committed): Noto Color Emoji COLRv1, Oswald, Fredoka, Caveat, each with its license file. See docs/THIRD_PARTY.md.
- OFL fonts may not be sold on their own and modified versions may not use reserved names. We bundle them unmodified; rendered text in exported videos is not a font redistribution.

## 7. Icons

Use MIT- or ISC-licensed icon sets only (for example Lucide, ISC; Heroicons or Phosphor, MIT). Nothing is chosen yet. Ship the set's license in the notices. The app icon is our own work under the project's MIT license.

## 8. Cap and Kap

**Cap (CapSoftware/Cap @880ec89) is AGPL-3.0 except the `cap-camera*` and `scap-*` crate families**, which its root LICENSE puts under MIT (`licenses/LICENSE-MIT`, "Copyright (c) 2023 Cap Software, Inc."). Rules:

- Any Cap code may be **read** to understand the design. Non-MIT Cap code must never be copied, translated or closely paraphrased into this repo, including into notes. Describe ideas in our own words and cite paths.
- A Cap crate counts as MIT only if **its own Cargo.toml (or a file header) says MIT**, not only the root preamble. MIT crates may be adapted with an entry in docs/THIRD_PARTY.md that keeps the MIT notice.
- Never depend on Cap through a git rev or path: every MIT crate depends on Cap's AGPL `workspace-hack`, whose manifest pulls in a reqwest fork and other networking crates (R-mit).

MIT audit (R-mit, re-checked with `grep` over `reference/Cap/crates/*/Cargo.toml` for this file):

| Status | Crates |
|---|---|
| **MIT** (own Cargo.toml says `license = "MIT"`) | scap-screencapturekit, scap-direct3d, scap-cpal, scap-ffmpeg, cap-camera, cap-camera-avfoundation, cap-camera-windows, cap-camera-mediafoundation, cap-camera-directshow, cap-camera-ffmpeg |
| **Treat as AGPL** (in an MIT family by name, but no license field) | scap-targets, cap-camera-effects |
| **AGPL** (outside the families, no license field) | cap-cpal-ffmpeg, cap-cursor-info (also ships cursor SVGs under an "Apple User Agreement" licence), cap-d3d-adapter, cap-mediafoundation-utils, workspace-hack, and every other Cap crate (recording, enc-*, video-decode, rendering, editor, project, export, ...) |

Ten crates carry `license = "MIT"` (the list above; R-mit originally said eleven and has been corrected). Separately, Cap's `crates/rnnoise` declares BSD-3-Clause (a third-party component); we don't use it.

Non-MIT edges inside the MIT crates, to strip if any are vendored: `workspace-hack` (all), `cap-d3d-adapter` (scap-direct3d, one call), `cap-mediafoundation-utils` (cap-camera-windows and cap-camera-mediafoundation, one buffer-lock helper) (R-mit).

Current use: **no Cap code is in this repository.** The MIT crates `scap-screencapturekit` and `cap-camera-avfoundation` were read for orientation in spike 3 only. Vendoring the Windows camera stack is a candidate for M6 (R-mit).

**Kap (wulkano/Kap @c42692f) is MIT** and may be adapted with attribution. One adaptation exists so far (the GIF palette arguments in spike 1); planned adaptations K1-K7 are listed in docs/THIRD_PARTY.md. Unlike Kap, we build helpers from pinned, checksum-verified source and ship full notices; Kap ships no third-party notices and downloads an unpinned GPL ffmpeg at install time (R-kap). We also don't ship gifsicle (GPL-2.0) (R-kap).

## 9. M0 spike compliance (dev-only code, nothing ships)

| Spike | Finding |
|---|---|
| S1 helpers | Spike crate is std-only. No Cap code (verifier compared against enc-ffmpeg, enc-gif, export). Kap and markus-perl attributions recorded. |
| S2 preview | All dependencies MIT or Apache-2.0. `reqwest` and `hyper` are in Cargo.lock only as optional Tauri dependencies and are not compiled. No Cap or Kap code. |
| S3 macOS pipeline | 118 crates, all permissive. objc2-app-kit's default features link CloudKit and CoreData (no networking called); trim those features in M1. No Cap code. |
| S4 Skia text | skia-safe MIT, fontdb MIT, wgpu/png/pollster MIT OR Apache-2.0, bytemuck Zlib/MIT/Apache-2.0. Skia's vendored C libraries listed in section 2.4. Skia is built from source with `FORCE_SKIA_BUILD=1` (S4). No Cap code. |

## 10. M1 compliance (2026-11-20)

| Check | Result | Source |
|---|---|---|
| cargo-deny (licenses, bans incl. HTTP clients, advisories, sources) | pass | `licenses` CI job, runs 44695891492 and 44705677113; `scripts/ci/deny.sh` locally (M1:ci-green) |
| pnpm license gate (prod strict, all with the lightningcss exception) | pass: prod 1 package, all 50 packages | same |
| cargo-about notices | generated, about 448 entries (§2.2) | `scripts/ci/notices.sh` (M1:ci-green) |
| No networking code: source grep of crates/app/cli/tools and `ui/src`, both Tauri CSPs local-only, helper built with `--disable-network` | pass (`netcheck: ok`); negative tests (an injected `std::net` line, a WebSocket in `ui/src`, a remote `connect-src`) were caught | `scripts/ci/netcheck.sh` (M1:ci, M1:ci-green); the merge fix ece22aa renamed a private `fetch(` function the grep flagged |
| Skia vendored set audited | §2.4 | M1:text |
| No Cap or Kap code in M1 | none adapted; the only "ported" code is our own spike code | THIRD_PARTY.md |
| Release bundle sidecars | ffmpeg and ffprobe signed `adhoc,runtime` with the app's entitlements (inherited, as §5 notes) | M1:app-shell (bundle inspected, never launched) |

Still open from M0 (unchanged in M1): gifski notice texts (libimagequant GPL-3.0 text and COPYRIGHT, the permissive crates in `gifski.deps.txt`), the gifsicle-feature provenance question, BtbN for Windows/Linux (M6), and the Apache-2.0 fonts question (§6). M1 ships no GIF export and bundles no fonts, so none of these blocks M1. Since M4 the app bundles the ADR-020 fonts with their license files (THIRD_PARTY.md, "Fonts (app)"); Special Elite is the one Apache-2.0 font, accepted under the spec's Licensing section. Since the FFmpeg 9.1 pin (2026-11-24) the FFmpeg tarball's GPG signature is verified (versions.toml `cross_check`).
