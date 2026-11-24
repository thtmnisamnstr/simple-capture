# third_party: bundled export helpers

Simple Capture ships two GPL/AGPL programs as **separate executables** (Tauri sidecars). They are
never linked into the MIT app binary. Everything here is build tooling; build trees go to
`third_party/build/` and finished binaries to `third_party/out/<os>-<arch>/` (both gitignored).

| Path | What |
|---|---|
| `versions.toml` | Exact version, URL, SHA-256 and cross-check note for every source tarball |
| `lib.sh` | `pin` (read versions.toml), `fetch` (download once, always verify SHA-256), `unpack` |
| `fetch.sh` | Download and verify every pinned source |
| `ffmpeg/build-macos.sh [arm64\|x86_64]` | Static GPL FFmpeg + ffprobe for macOS (min 14.0) |
| `gifski/build.sh [arm64\|x86_64]` | gifski CLI from the pinned crates.io source, `--locked` |

```sh
third_party/fetch.sh
JOBS=8 third_party/ffmpeg/build-macos.sh arm64   # ~8 min on an M3 Pro under load
third_party/gifski/build.sh arm64                # ~2 min
```

Outputs in `third_party/out/macos-arm64/`:

- `ffmpeg`, `ffprobe` (stripped), `ffmpeg.configure.txt` (exact configure line),
  `ffmpeg.buildconf.txt` (`ffmpeg -buildconf`), `ffmpeg.otool.txt` (`otool -L`, checked by the
  script: anything outside `/System/Library/Frameworks`, `libSystem`, `libc++` or `libobjc` fails
  the build), `ffmpeg.vtool.txt` (minos), `ffmpeg.sources.txt` (versions + SHA-256),
  `licenses/ffmpeg/*` (license and patent files of FFmpeg and every linked library).
- `gifski`, `gifski.LICENSE`, `gifski.deps.txt` (every Rust crate compiled in, with its license).

## Licensing rules enforced by the scripts

- FFmpeg: `--enable-gpl`, never `--enable-nonfree`, no `--enable-version3` (no chosen library
  needs it: x264/x265 are GPL-2.0-or-later, libvpx/Opus/Ogg/Vorbis are BSD-3-Clause, SVT-AV1 is
  BSD-3-Clause-Clear + AOMedia patent license; all GPLv2-compatible). The resulting binary is
  GPL-2.0-or-later.
- `--disable-network` plus only the `file` and `pipe` protocols: the helper cannot open sockets.
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
