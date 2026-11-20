# Licenses

Status: M0 draft, 2026-11-19. This is the licensing policy for Simple Capture, how CI enforces it, and what every release must ship. Attribution for code or designs we adapt lives in docs/THIRD_PARTY.md; this file covers dependencies and bundled programs. It records engineering decisions, not legal advice.

Sources: S1-S4 are the M0 spikes (`spikes/01-helpers`, `02-preview-transport`, `03-macos-pipeline`, `04-skia-text`) and their quiet-machine verification passes; R-* are the notes in `docs/research/` (R-mit = cap-mit-crates.md, R-eco = ecosystem.md, R-kap = kap.md, R-rec = cap-recording.md). None of the spikes has a REPORT.md yet: the harness blocked those writes, so facts here come from the spike folders, `third_party/` and the verification results.

## 1. Policy

| What | Allowed licenses | How it ships |
|---|---|---|
| Simple Capture's own code | MIT | The app binary and CLI. |
| Everything linked into the app binary (Rust crates, C/C++ libraries compiled into crates, the TypeScript UI bundle) | MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unicode-3.0, CC0-1.0, plus MPL-2.0 only as a named per-crate exception | Statically or dynamically linked, no restrictions beyond notices. |
| LGPL code | LGPL-2.1-or-later (or LGPL-3.0-or-later) | **Linux only**, and **only dynamically linked**: the FFmpeg `lgpl-shared` libraries for in-process decode and hardware recording encode. Ship the `.so` files unmodified as separate files so users can replace them. |
| GPL / AGPL code | GPL-2.0-or-later, GPL-3.0-or-later, AGPL-3.0-or-later | **Only as separate bundled executables** (Tauri sidecars) that we start as processes and talk to over pipes. Never linked into the app. Today: `ffmpeg`, `ffprobe` (if shipped) and `gifski`. |
| Non-free code | none | Never. No `--enable-nonfree`, no fdk-aac, no BtbN `nonfree` variant. |
| Fonts | SIL OFL-1.1, Apache-2.0 | Bundled with their license files (section 6). |
| Icons | MIT, ISC | Section 7. |

Hard bans in the app binary: x264, x265, gifski, libimagequant (GPL-3.0-or-later), and any non-MIT Cap crate (section 8).

## 2. Enforcement in CI

All three run on every PR on all three OSes (spec, "Engineering and testing"), and fail the build.

### 2.1 cargo-deny (Rust)

Use cargo-deny 0.22.0 (MSRV 1.95, matching our toolchain; 0.21.0 is the fallback pin) (R-eco). Outline of `deny.toml`:

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

### 2.2 cargo-about (Rust notices)

cargo-about 0.9.3 (R-eco) generates the third-party notices for the About → Licenses screen from the same `accepted` list as cargo-deny. Its output is a release artifact (section 5), generated in CI and not checked in.

### 2.3 npm license check (UI)

Use pnpm's built-in `pnpm licenses list --prod --json` and a small script that fails on anything outside the section 1 allow list (R-eco). `license-checker-rseidelsohn` 5.0.1 is the fallback; whether it handles pnpm's symlinked `node_modules` is unverified (R-eco). Only production dependencies end up in the shipped bundle; dev tools (Vite, svelte-check, TypeScript) are not shipped.

### 2.4 Native code the tools above cannot see

Kept as a hand-maintained list in `third_party/` and included in the notices:

| Component | Comes in through | License |
|---|---|---|
| Skia | `skia-safe` / `skia-bindings` (statically linked) | BSD-3-Clause |
| FreeType | Skia build (we use FreeType on every OS, S4) | FreeType License (FTL); requires a credit in the documentation/About |
| HarfBuzz | Skia build | MIT-style ("Old MIT") |
| ICU | Skia build | Unicode-3.0 |
| expat, libjpeg-turbo, libpng, zlib | Skia build | MIT; IJG + BSD-3-Clause + Zlib; libpng license; Zlib |

S4 confirmed Skia, FreeType, HarfBuzz and ICU are statically linked into the spike binary (`otool -L` shows only system libraries) but did **not** run cargo-about or audit the exact set Skia vendors (R-eco flagged this). Do that audit in M1 when the text crate lands.

## 3. Bundled helpers

### 3.1 FFmpeg, macOS (built by us)

Built from source by `third_party/ffmpeg/build-macos.sh` (S1). Pins, URLs and SHA-256 values are in `third_party/versions.toml`; each build records its inputs in `third_party/out/macos-<arch>/`.

- **Version:** FFmpeg 9.0.2, `--extra-version=simple-capture` (reports `9.0.2-simple-capture`).
- **Result license:** **GPL-2.0-or-later** (`--enable-gpl`, no `--enable-version3`, no `--enable-nonfree`; every linked library is GPLv2-compatible) (S1).
- **Targets:** arm64 (built, benchmarked, validated), x86_64 (cross-built, smoke-tested under Rosetta only). Deployment target macOS 14.0, built against SDK 26.5 (`vtool`).
- **Links to:** only libSystem, libc++, and the AudioToolbox, VideoToolbox, CoreFoundation, CoreMedia, CoreVideo and CoreServices frameworks (`otool -L`; the build script fails on anything else). These are operating-system components.
- **No networking:** `--disable-network`, only the `file` and `pipe` protocols, no socket or getaddrinfo imports (S1 verification).

| Library | Version | License | Source (exact URL and SHA-256 in versions.toml) | Checksum cross-check |
|---|---|---|---|---|
| FFmpeg | 9.0.2 | LGPL-2.1-or-later code, GPL-2.0-or-later as configured | https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz | Homebrew formula matches. **GPG signature not verified** (gpg not installed). |
| x264 | 0.165.r3222, stable branch commit b35605ac | GPL-2.0-or-later | code.videolan.org archive of b35605ac | No upstream checksum; tarball identical to a git clone (`diff -r`). |
| x265 | 4.3 | GPL-2.0-or-later | github.com/Multicorewareinc/x265 release 4.3 | GitHub asset digest and Homebrew match. |
| SVT-AV1 | 4.3.0 | BSD-3-Clause-Clear AND AOM Patent License 1.0 | gitlab.com/AOMediaCodec/SVT-AV1 tag v4.3.0 | Matches markus-perl pin. |
| libvpx | 1.17.0 | BSD-3-Clause (plus Google's PATENTS grant) | github.com/webmproject/libvpx tag v1.17.0 | Homebrew and markus-perl match. |
| Opus | 1.6.1 | BSD-3-Clause | downloads.xiph.org | Upstream SHA256SUMS, Homebrew, markus-perl match. |
| libogg | 1.3.6 | BSD-3-Clause | downloads.xiph.org | Upstream SHA256SUMS, markus-perl match. |
| libvorbis | 1.3.7 | BSD-3-Clause | downloads.xiph.org | Upstream SHA256SUMS, Homebrew match. Darwin configure patched to drop `-force_cpusubtype_ALL`. |

Configure flags (full line in `third_party/out/macos-arm64/ffmpeg.configure.txt`, `ffmpeg -buildconf` in `ffmpeg.buildconf.txt`). The licensing-relevant ones:

```
--enable-gpl --disable-shared --enable-static --disable-autodetect --disable-network
--disable-ffplay --disable-doc --disable-debug --disable-sdl2 --disable-xlib
--disable-securetransport --disable-iconv --disable-zlib --disable-bzlib --disable-lzma
--disable-appkit --disable-avfoundation --disable-coreimage --disable-metal
--enable-videotoolbox --enable-audiotoolbox --enable-pthreads
--enable-libx264 --enable-libx265 --enable-libsvtav1 --enable-libvpx --enable-libopus --enable-libvorbis
--disable-everything --enable-protocol=file,pipe
--enable-encoder=libx264,libx265,libsvtav1,libvpx_vp8,libvpx_vp9,h264_videotoolbox,hevc_videotoolbox,prores_videotoolbox,prores_ks
--enable-encoder=aac,aac_at,libopus,libvorbis,flac,alac,pcm_s16le,pcm_s24le,pcm_f32le
--enable-encoder=gif,rawvideo,wrapped_avframe
(plus explicit decoder, muxer, demuxer, parser, hwaccel and filter allow lists)
```

Known gaps: the PNG decoder is listed but silently dropped because zlib is disabled, so the recorded configure line and the binary disagree; there is no AV1 decoder (S1). Sizes: arm64 ffmpeg 20,273,336 B and ffprobe 20,097,672 B; x86_64 ffmpeg 29,354,544 B. Shipping ffprobe is optional (decide in M5).

### 3.2 gifski (built by us, all OSes)

- **Version:** gifski 1.34.0 from the crates.io `.crate` (SHA-256 `c246c795…1190`, matches the registry checksum; git tag 1.34.0, commit 1060eab4), built with `cargo build --release --locked --bin gifski` (`third_party/gifski/build.sh`) (S1).
- **Features:** defaults only (`binary`, `gifsicle`); the `video` feature, which links FFmpeg, is never enabled.
- **Result license:** **AGPL-3.0-or-later**. It contains libimagequant 4.4.1 (**GPL-3.0-or-later**); every other compiled crate is permissive (list with licenses in `third_party/out/macos-arm64/gifski.deps.txt`, 52 lines).
- **Open question:** the default `gifsicle` feature compiles a Rust port of gifsicle's lossy-LZW code (`src/gifsicle.rs` in the pinned crate; it has no licence header of its own, and the crate declares only `AGPL-3.0-or-later`), and gifsicle is GPL-2.0 upstream. The combined binary is still distributed under AGPL-3.0-or-later, but check that module's provenance (who wrote the port, from which gifsicle/giflossy code, under which licence) before M5, or build with `--no-default-features --features binary` if we drop "lossy" (S1 verification). Not resolved in M0.
- **Notice gap (open, must be fixed before any release):** the build collects only `gifski.LICENSE` (the AGPL-3.0 text). Missing from `third_party/out/<os>-<arch>/`:
  - the **GPL-3.0** text for libimagequant 4.4.1, and its `COPYRIGHT` file, which also carries the original pngquant licence notice (Jef Poskanzer, Greg Roelofs) that must be reproduced;
  - the licence texts and copyright notices of the ~50 permissive crates compiled in (`gifski.deps.txt`: MIT, Apache-2.0, BSD-2-Clause for `yuv`, Zlib for `lodepng`, `zlib-rs`, `libz-rs-sys`, CC0 for `imgref`, and so on). MIT and BSD require the notice to ship with the binary.
  - Fix in `third_party/gifski/build.sh` (M5 at the latest): run cargo-about (ADR-024) over gifski's own `Cargo.lock` and write `licenses/gifski/*`, plus copy imagequant's `COPYRIGHT` and the GPL-3.0 text.
- Links only libSystem and the system libiconv on macOS. Built for macOS arm64 only so far.
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
3. **Corresponding source.** GPL/AGPL require us to provide it, not just link upstream. Attach to every GitHub release a source archive containing every pinned tarball (FFmpeg, x264, x265, SVT-AV1, libvpx, Opus, Ogg, Vorbis, the gifski crate with its vendored dependencies) **plus our build scripts** (`third_party/`, which count as the scripts that control compilation). Distributing it from the same release page as the binaries satisfies "equivalent access". Also include a written offer in the notices as a fallback. For BtbN binaries, mirror their matching source and scripts the same way. Keep these for at least three years per release.
4. **Rust notices** from cargo-about, **npm notices** from the pnpm license list, and the **native components list** from section 2.4 (including the FreeType credit).
5. **Font licenses** for every bundled font (section 6).
6. **Patent note** (section 4).

The spec's definition of done also requires a check that the app binary contains no GPL/AGPL code: CI runs cargo-deny and the npm check, and a script confirms no `x264`, `x265`, `gifski` or `imagequant` symbols in the app binary.

macOS signing note: tauri-bundler signs sidecars with the app's identity **and the app's entitlements** (no per-sidecar option), so ffmpeg and gifski would inherit camera/mic entitlements. Narrower entitlements would need a custom signing step; ADR-003 accepts the inherited ones for v1 (R-eco).

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
