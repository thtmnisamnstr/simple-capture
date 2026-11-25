# third_party: bundled export helpers

Simple Capture ships two GPL/AGPL programs as **separate executables** (Tauri sidecars). They are
never linked into the MIT app binary. The Mac App Store edition ships an LGPL FFmpeg and ffprobe
instead, and no gifski. Everything here is build tooling; build trees go to `third_party/build/`
and finished binaries to `third_party/out/<os>-<arch>[-lgpl]/` (both gitignored).

| Path | What |
|---|---|
| `versions.toml` | Exact version, URL, SHA-256 and cross-check note for every source tarball |
| `lib.sh` | `pin` (read versions.toml), `fetch` (download once, always verify SHA-256), `unpack` |
| `fetch.sh` | Download and verify every pinned source |
| `ffmpeg/build-macos.sh [arm64\|x86_64]` | Static GPL FFmpeg + ffprobe for macOS (min 14.0); with `SC_FFMPEG_LICENSE=lgpl`, the LGPL Mac App Store variant |
| `gifski/build.sh [arm64\|x86_64]` | gifski CLI from the pinned crates.io source, `--locked` against `gifski/gifski-<version>.Cargo.lock` |
| `gifski/gifski-<version>.Cargo.lock` | The crate's own lockfile with security bumps (`lock_note` in versions.toml); replaces the crate's copy before the build |

```sh
third_party/fetch.sh
JOBS=8 third_party/ffmpeg/build-macos.sh arm64   # ~8 min on an M3 Pro under load
third_party/gifski/build.sh arm64                # ~2 min
SC_FFMPEG_LICENSE=lgpl JOBS=8 third_party/ffmpeg/build-macos.sh arm64   # Mac App Store helper
```

Outputs in `third_party/out/macos-arm64/`:

- `ffmpeg`, `ffprobe` (stripped), `ffmpeg.configure.txt` (exact configure line),
  `ffmpeg.buildconf.txt` (`ffmpeg -buildconf`), `ffmpeg.otool.txt` (`otool -L`, checked by the
  script: anything outside `/System/Library/Frameworks`, `libSystem`, `libc++` or `libobjc` fails
  the build), `ffmpeg.vtool.txt` (minos), `ffmpeg.sources.txt` (versions + SHA-256),
  `licenses/ffmpeg/*` (license and patent files of FFmpeg and every linked library).
- `gifski`, `gifski.LICENSE`, `gifski.deps.txt` (every Rust crate compiled in, with its license).

Outputs in `third_party/out/macos-arm64-lgpl/` (built in `third_party/build/macos-arm64-lgpl/`):
the same `ffmpeg`, `ffprobe` and `ffmpeg.*.txt` files, with `licenses/ffmpeg/*` holding FFmpeg's
`COPYING.LGPLv2.1` in place of `COPYING.GPLv2` and no x264 or x265 files. No gifski.
`ffmpeg -version` reads `9.1-simple-capture-lgpl`.

## Licensing rules enforced by the scripts

- FFmpeg: `--enable-gpl`, never `--enable-nonfree`, no `--enable-version3` (no chosen library
  needs it: x264/x265 are GPL-2.0-or-later, libvpx/Opus/Ogg/Vorbis are BSD-3-Clause, SVT-AV1 is
  BSD-3-Clause-Clear + AOMedia patent license; all GPLv2-compatible). The resulting binary is
  GPL-2.0-or-later.
- FFmpeg with `SC_FFMPEG_LICENSE=lgpl`: no `--enable-gpl`, `--enable-version3` or
  `--enable-nonfree`, and no x264/x265 (never built; the variant has its own build tree, so
  their libraries are never in its prefix). Every other library, codec, muxer and filter stays:
  none of the enabled filters needs `--enable-gpl`. The build fails unless configure reports
  `LGPL version 2.1 or later`. The binary is LGPL-2.1-or-later, and
  `scripts/ci/notices.sh --check-mas DIR` fails unless a staged or bundled `licenses` folder
  holds exactly the Mac App Store set (no GPL/AGPL text, x264, x265 or gifski).
- `--disable-network` plus only the `file` and `pipe` protocols (and `fd` in the LGPL variant,
  so the sandboxed app can pass the helper an output file it opened): the helper cannot open
  sockets.
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
   FFmpeg or one of its libraries: also `CLEAN=1 SC_FFMPEG_LICENSE=lgpl third_party/ffmpeg/build-macos.sh arm64`.
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
