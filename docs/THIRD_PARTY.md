# Third-party notices

Attribution for third-party code, recipes, designs and assets adapted into or committed to Simple Capture. The licensing policy, dependency enforcement and bundled helper programs (FFmpeg, gifski) are covered in docs/LICENSES.md; this file is the record of **what we took from whom**.

How to add an entry: append under the project's section (or add a new section) with the project, exact commit or version, license and copyright line, the file(s) it came from, and the file(s) in this repo it went into. Never remove an existing entry; mark it "removed" with a date if the code goes away.

## Index

| Project | License | What | Where in this repo | Section |
|---|---|---|---|---|
| Kap (wulkano/Kap @c42692f) | MIT | FFmpeg GIF palette arguments (adapted); planned adaptations K1-K7; recipes quoted in research | `spikes/01-helpers/src/jobs.rs`; `docs/research/kap.md` | [Kap](#kap) |
| markus-perl/ffmpeg-build-script @9243847 | MIT | Build recipe knowledge for the static FFmpeg; pin cross-checks | `third_party/ffmpeg/build-macos.sh`, `third_party/versions.toml` | [markus-perl](#markus-perlffmpeg-build-script) |
| Noto Color Emoji, Oswald, Fredoka, Caveat | OFL-1.1 | Font files | `spikes/04-skia-text/fonts/` | [Fonts](#fonts-spike-4-spikes04-skia-text) |
| Cap (CapSoftware/Cap @880ec89) | AGPL-3.0, MIT for `cap-camera*`/`scap-*` | **Nothing adapted.** Read for design only | – | [Cap](#cap-read-only-nothing-adapted) |

## Kap

- Project: Kap, https://github.com/wulkano/kap, commit c42692fa63ac71ed192e01684beb78a1b864aa88 (v3.6.0)
- License: MIT
- Copyright (c) Wulkano hello@wulkano.com (https://wulkano.com)

### Adapted so far

- **GIF palette arguments** (spike 1). Adapted in `spikes/01-helpers/src/jobs.rs` `gif_palette_args` from Kap's `convertToGif` in main/converters/h264.ts (`fps=N,scale=W:H:flags=lanczos` then `palettegen` / `paletteuse`, `-loop 0` for forever). Changes: a single-pass `split` graph instead of Kap's two processes with a temporary palette PNG (we stream frames over a pipe and cannot read them twice), `palettegen=stats_mode=full`, `paletteuse=dither=sierra2_4a`, and a `setparams` stage that tags the incoming Y'CbCr as BT.709 limited range (otherwise swscale converts with BT.601). This is item K1 below.

### Quoted in research

- `docs/research/kap.md` quotes Kap's exact FFmpeg argument recipes and describes its algorithms.

### Planned adaptations (K1-K7, section 7 of docs/research/kap.md)

When code derived from these lands, add its file path(s) under "Adapted so far".

- K1 FFmpeg argument recipes (main/converters/h264.ts). GIF palette part adapted, see above.
- K2 conditional argv builder pattern (main/converters/utils.ts)
- K3 progress/ETA heuristic and cancel handling (main/converters/utils.ts, main/converters/process.ts, main/conversion.ts, main/export.ts)
- K4 per-format fps memory capped at source fps (main/remote-states/editor-options.ts, renderer/components/editor/options-container.tsx)
- K5 aspect-locked size inputs (renderer/components/editor/options/left.tsx, renderer/components/keyboard-number-input.js, renderer/utils/inputs.js)
- K6 cropper aspect-lock interaction (renderer/containers/cropper.js, renderer/containers/action-bar.js, renderer/utils/inputs.js, renderer/pages/cropper.js, renderer/components/cropper/handles.js)
- K7 even-size rounding helper (main/converters/utils.ts)

### License text

```
MIT License

Copyright (c) Wulkano hello@wulkano.com (https://wulkano.com)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## markus-perl/ffmpeg-build-script

- Project: https://github.com/markus-perl/ffmpeg-build-script, commit 9243847768f19d868fd59134d2d6d029f782aab5
- License: MIT, Copyright (c) 2016-2026 Markus Perl (full text in `reference/ffmpeg-build-script/LICENSE`; the terms are the MIT text reproduced under Kap above, with this copyright line)
- Used in: `third_party/ffmpeg/build-macos.sh` (spike 1). No code copied verbatim; adapted recipe knowledge:
  - stripping `-force_cpusubtype_ALL` from libvorbis' configure on Darwin (we patch the shipped `configure` instead of re-running autogen),
  - building libvpx with `--disable-examples --disable-unit-tests --disable-shared` and an explicit darwin target,
  - building x265 with CMake, `ENABLE_SHARED=OFF`, `ENABLE_CLI=OFF`,
  - `--pkg-config-flags=--static` for the final FFmpeg link.
- Every non-free option of that script (fdk-aac, `--enable-nonfree`, `--enable-version3` libraries) is deliberately not used.
- The pinned versions and hashes in that script's `src/10-versions.sh` were used only to cross-check our own downloaded SHA-256 values (see `third_party/versions.toml`, `cross_check`).

## Fonts (spike 4: spikes/04-skia-text)

Font files stored under `spikes/04-skia-text/fonts/` (intended for commit), each with its license file next to it. Downloaded and SHA-256-checked by `spikes/04-skia-text/fonts/fetch.sh`. No Cap or Kap code was adapted for spike 4.

| File | Source | License and copyright | SHA-256 |
|---|---|---|---|
| `Noto-COLRv1.ttf` (Noto Color Emoji, COLRv1 build) | googlefonts/noto-emoji tag v2.051, commit 8998f5dd683424a73e2314a8c1f1e359c19e8742, `fonts/Noto-COLRv1.ttf` | SIL Open Font License 1.1, Copyright 2022 Google Inc. (per the font name table); `LICENSE-NotoColorEmoji-OFL.txt` | 0ae57fe58645638523ba35f388d93739d292539a9acb84df5700c81b1e1a28d2 |
| `Oswald[wght].ttf` | google/fonts commit cc5661d43cd4f9aad5567033a8e15ca28dc35c86, `ofl/oswald` | OFL-1.1, Copyright 2016 The Oswald Project Authors; `LICENSE-Oswald-OFL.txt` | 5b38c246e255a12f5712d640d56bcced0472466fc68983d2d0410ec0457c2817 |
| `Fredoka[wdth,wght].ttf` | same commit, `ofl/fredoka` | OFL-1.1, Copyright 2016 The Fredoka Project Authors; `LICENSE-Fredoka-OFL.txt` | 2ba02e68b152868aef9ba28e24b3648c7d457fe6f25c761f2c2c53fb61a73fc8 |
| `Caveat[wght].ttf` | same commit, `ofl/caveat` | OFL-1.1, Copyright 2014 The Caveat Project Authors; `LICENSE-Caveat-OFL.txt` | 0bdb6b660482d31531b3945849fba5916b3ef8695da7024a9e6b9ee3c4157988 |

The CBDT emoji build (`NotoColorEmoji.ttf` from the same noto-emoji commit, saved as `NotoColorEmoji-CBDT.ttf`, SHA-256 72a635cb3d2f3524c51620cdde406b217204e8a6a06c6a096ff8ed4b5fd6e27b) is fetched on demand for comparison only and is gitignored. The full bundled font set for the app is proposed in docs/LICENSES.md section 6; add each one here when it is committed.

## Cap (read only, nothing adapted)

- Project: Cap, https://github.com/CapSoftware/Cap, commit 880ec89. AGPL-3.0, except crates in the `cap-camera*` and `scap-*` families, which Cap's root LICENSE places under MIT (`licenses/LICENSE-MIT`, Copyright (c) 2023 Cap Software, Inc.). docs/LICENSES.md section 8 lists which crates qualify.
- **No Cap code, MIT or AGPL, is in this repository.** If any MIT Cap code is adapted later, add an entry here with the crate, file and the MIT notice above.
- Research notes (`docs/research/cap-recording.md`, `cap-render-decode.md`, `cap-editor-export.md`, `cap-mit-crates.md`) describe Cap's design in our own words with path citations; none adapted code, so none added an entry.
- Spike 1 (export helpers): verified to contain no Cap code (compared against enc-ffmpeg, enc-gif and export).
- Spike 2 (preview transport): no Cap or Kap code adapted.
- Spike 3 (macOS capture pipeline, `spikes/03-macos-pipeline`): no third-party source code was copied or adapted. Cap's MIT crates `crates/scap-screencapturekit` and `crates/camera-avfoundation` (both Cargo.toml files say `license = "MIT"`) were read for orientation only; the spike uses the objc2 crates instead of cidre, so no code from them was reused. Cap's AGPL crates (enc-avfoundation, video-decode, rendering, recording) were read for design only and nothing from them is in the repo. Rust dependencies of the spike (118 packages, all permissive): objc2 family (objc2, objc2-foundation, objc2-app-kit, objc2-core-foundation, objc2-core-graphics, objc2-core-media, objc2-core-video, objc2-core-audio-types, objc2-io-surface, objc2-metal, objc2-screen-capture-kit, objc2-av-foundation, objc2-avf-audio, block2, dispatch2; MIT OR Apache-2.0 OR Zlib), wgpu and wgpu-hal 30 (MIT OR Apache-2.0), png (MIT OR Apache-2.0), pollster (Apache-2.0 OR MIT), serde/serde_json, anyhow, libc (MIT OR Apache-2.0). Transitive: unicode-ident adds Unicode-3.0, zerocopy adds BSD-2-Clause as an alternative; no copyleft.
- Spike 4 (Skia text): Cap's text layer (`crates/rendering/src/layers/text.rs`) uses glyphon/cosmic-text, not Skia; it was read only. The spike's `ease_out_back` is the standard published easeOutBack formula, written independently.
