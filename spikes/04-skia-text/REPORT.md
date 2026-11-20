# Spike 4 report: Skia text layout and raster
Status: pass. Written 2026-11-19 from `out/bench-quiet.md` (one saved run), `out/bench-ecore-proxy.md`, `out/*.png`, and three `--no-png` re-runs plus one timed clean build in the consistency pass.
Machine: M3 Pro, Metal. No M1, Windows or Linux data.

## Question
Can skia-safe draw every spec text style plus colour emoji identically in preview and export, fast enough, with a CPU reference for golden tests?

## Results
- **Emoji:**
  - Stock (CoreText) skia-safe: CBDT → 30 unresolved glyphs; COLRv1 → 0 coloured pixels.
  - FreeType build: both formats render, 12.8k coloured pixels (`emoji_matrix.png`).
- **Per-item cost**, sum over 11 items, warm / cold, median of 30:

  | | bench-quiet.md | re-runs |
  |---|---|---|
  | layout | 0.45 / 0.58 ms | 0.44 / 0.55–0.56 ms |
  | raster 1080p | 4.27 / 9.67 ms | 3.91 / 9.10–9.13 ms |
  | raster 4K | 15.91 / 26.73 ms | 15.18–15.33 / 26.09–26.33 ms |

- **GPU:**
  - 1080p: upload 1.68 ms, composite 0.441 ms (re-runs 0.413–0.638), readback 1.247 ms (1.230–1.749), CPU reference 17.8 ms.
  - 4K: upload 5.20 ms, composite 0.765 ms (0.625–0.789), readback 6.923 ms (5.464–6.703), CPU reference 75.8 ms.
- **GPU vs CPU:** max 1, mean 0.00059 at 1080p and 0.00030 at 4K; animation frames max 1. Identical in the saved run, the E-core proxy and all three re-runs.
- **E-core proxy** (`taskpolicy -c background`): layout 2.11 / 3.02 ms, raster 1080p 21.2 / 56.1 ms, raster 4K 70.4 / 147.6 ms. It shows the slow-core penalty, but it is not an M1 measurement.
- **Animations:** typewriter and word-by-word take their order from cluster/rect queries. Fixed a Skia m157 quirk where `utf8Starts` is wrong on continuation lines.
- **Build:**
  - The saved first build (`target/ft-build.log`, under load) took 7 min 55 s.
  - A clean build in a fresh target folder this pass took 2 min 42 s wall; the skia-bindings build script took 144 s. Whether the source download was cached was not checked.
  - `FORCE_SKIA_BUILD=1` is required; `SKIA_GN_ARGS` alone is ignored when a prebuilt archive matches.
  - The earlier "2 min 54 s" and the verifier's lower timings were never saved and are withdrawn.

## Verdict
Pass. Feeds ADR-018–022.

## Open
- Windows and Linux FreeType builds (first CI risk).
- RTL and complex scripts.
- Font list licences: 2 of the fonts are Apache-2.0 (ADR-020).
