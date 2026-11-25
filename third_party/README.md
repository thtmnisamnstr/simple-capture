# third_party: bundled export helpers

Simple Capture ships its export helpers as **separate executables** (Tauri sidecars), never
linked into the MIT app binary. Every macOS build (the DMG and the Mac App Store edition, which
are the same app) ships an LGPL FFmpeg and ffprobe without x264 and x265 (H.264 and HEVC come
from VideoToolbox) and no gifski. Windows and Linux ship a GPL FFmpeg and ffprobe and the AGPL
gifski. Everything here is build tooling; build trees go to `third_party/build/` and finished
binaries to `third_party/out/<os>-<arch>/` (both gitignored).

| Path | What |
|---|---|
| `versions.toml` | Exact version, URL, SHA-256 and cross-check note for every source tarball |
| `lib.sh` | `pin` (read versions.toml), `fetch` (download once, always verify SHA-256), `unpack` |
| `fetch.sh` | Download and verify every pinned source |
| `ffmpeg/build-macos.sh [arm64\|x86_64]` | Static LGPL FFmpeg + ffprobe for macOS (min 14.0); `SC_FFMPEG_LICENSE=gpl` builds an unshipped GPL variant with x264/x265 for local experiments |
| `ffmpeg/build-linux.sh [linux\|windows]` | Static GPL FFmpeg + ffprobe for Linux and Windows |
| `gifski/build.sh [linux\|windows\|arm64\|x86_64]` | gifski CLI from the pinned crates.io source, `--locked` against `gifski/gifski-<version>.Cargo.lock` (shipped on Windows and Linux only) |
| `gifski/gifski-<version>.Cargo.lock` | The crate's own lockfile with security bumps (`lock_note` in versions.toml); replaces the crate's copy before the build |

```sh
third_party/fetch.sh
JOBS=8 third_party/ffmpeg/build-macos.sh arm64   # ~8 min on an M3 Pro under load
```

Outputs in `third_party/out/macos-arm64/` (built in `third_party/build/macos-arm64-lgpl/`):

- `ffmpeg`, `ffprobe` (stripped), `ffmpeg.configure.txt` (exact configure line),
  `ffmpeg.buildconf.txt` (`ffmpeg -buildconf`), `ffmpeg.otool.txt` (`otool -L`, checked by the
  script: anything outside `/System/Library/Frameworks`, `libSystem`, `libc++` or `libobjc` fails
  the build), `ffmpeg.vtool.txt` (minos), `ffmpeg.sources.txt` (versions + SHA-256),
  `licenses/ffmpeg/*` (license and patent files of FFmpeg and every linked library: FFmpeg's
  `COPYING.LGPLv2.1`, no x264 or x265 files). `ffmpeg -version` reads `9.1-simple-capture-lgpl`.
- No gifski: macOS GIFs use FFmpeg's palette encoder.

`SC_FFMPEG_LICENSE=gpl` writes the GPL variant (with x264 and x265, `9.1-simple-capture-gpl`) to
`third_party/out/macos-arm64-gpl/` from its own tree `third_party/build/macos-arm64-gpl/`.
Nothing stages or ships it; `scripts/stage-helpers.sh` refuses it on macOS.

Linux and Windows outputs (`third_party/out/linux-x86_64/`, `windows-x86_64/`) hold the GPL
`ffmpeg`, `ffprobe` and the same `ffmpeg.*.txt` files, plus `gifski`, `gifski.LICENSE` and
`gifski.deps.txt` (every Rust crate compiled in, with its license).

## Licensing rules enforced by the scripts

- FFmpeg on macOS: no `--enable-gpl`, `--enable-version3` or `--enable-nonfree`, and no
  x264/x265 (never built; each variant has its own build tree, so their libraries are never in
  the LGPL prefix). Every other library, codec, muxer and filter stays: none of the enabled
  filters needs `--enable-gpl`. The build fails unless configure reports
  `LGPL version 2.1 or later`. The binary is LGPL-2.1-or-later, and
  `scripts/ci/notices.sh --check-macos DIR` fails unless a staged or bundled `licenses` folder
  holds exactly the macOS set (no GPL/AGPL text, x264, x265 or gifski).
- FFmpeg on Windows and Linux (and the unshipped macOS gpl variant): `--enable-gpl`, never
  `--enable-nonfree`, no `--enable-version3` (no chosen library needs it: x264/x265 are
  GPL-2.0-or-later, libvpx/Opus/Ogg/Vorbis are BSD-3-Clause, SVT-AV1 is BSD-3-Clause-Clear +
  AOMedia patent license; all GPLv2-compatible). The resulting binary is GPL-2.0-or-later.
- `--disable-network` plus only the `file` and `pipe` protocols (and `fd` on macOS, so the
  sandboxed app can pass the helper an output file it opened): the helper cannot open sockets.
- `--disable-autodetect`, so nothing from the build machine (Homebrew, SDL, X11, iconv, zlib,
  SecureTransport...) is picked up implicitly. pkg-config is pointed only at the private prefix
  and CMake ignores `/opt/homebrew` and `/usr/local`.
- gifski: default features only (`binary`, `gifsicle`); the `video` feature (links FFmpeg) is
  never enabled. gifski is AGPL-3.0-or-later and pulls in libimagequant (GPL-3.0-or-later).
- Shipping either helper means shipping its license files (collected into `out/`) and pointing to
  the exact source (URLs + SHA-256 in `versions.toml`).

## Updating a pin

1. Change `version`/`url`/`file` in `versions.toml`, set `sha256` to the value you download.
2. Cross-check against an upstream checksum or signature and describe it in `cross_check`.
3. `CLEAN=1 third_party/ffmpeg/build-macos.sh arm64` and re-run the export tests (`SC_REQUIRE_HELPERS=1 cargo test -p simple-capture-export`).
   x264 or x265: the macOS build does not use them; check them with `third_party/ffmpeg/build-linux.sh`.
4. gifski: start the new `gifski/gifski-<version>.Cargo.lock` from the crate's own `Cargo.lock`,
   apply only the bumps `cargo audit -f` asks for (`cargo update -p <crate> --precise <version>`),
   and record `lock_file`, `lock_sha256` and `lock_note`.

## Known gaps (open, see docs/LICENSES.md §3.2)

- `gifski/build.sh` collects only `gifski.LICENSE` (AGPL-3.0). It does not yet collect the GPL-3.0
  text and `COPYRIGHT` (pngquant notice) of libimagequant 4.4.1, or the licence texts of the
  permissive crates listed in `gifski.deps.txt`. A release bundle built today would be missing
  required notices. Fix before M5 (cargo-about over gifski's `Cargo.lock`).
- The provenance of gifski's `gifsicle` feature (`src/gifsicle.rs`, a Rust port of lossy-LZW code
  from GPL-2.0 gifsicle, no licence header) is unreviewed.
- h264_videotoolbox (`-allow_sw 0`) cannot open when either dimension exceeds 4096 on the M3 Pro
  (re-measured 2026-11-19, docs/CODEC_MATRIX.md §2.2); the capability probe must check the real
  export size.
