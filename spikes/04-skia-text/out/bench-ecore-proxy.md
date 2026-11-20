# Skia text spike bench (ecore-proxy)

- Skia font manager for bundled fonts: FreeType (emoji file: Noto-COLRv1.ttf)
- bundled font load + provider setup: 4.6 ms
- fontdb: 838 faces / 388 families scanned in 87.3 ms; first families: [".ADT Slab Numeric", ".ADT Slab Soft Numeric", ".Al Bayan PUA", ".Al Nile PUA", ".Al Tarikh PUA", ".Apple Color Emoji UI", ".Apple SD Gothic NeoI", ".Aqua Kana"]
- fontdb pick (Georgia/serif/sans, bold): Some(("Georgia", "/System/Library/Fonts/Supplemental/Georgia Bold.ttf", 0))

## Per-item layout + raster (median of 15, single thread)

Warm = Skia glyph cache populated (re-raster after an edit). Cold = `purge_font_cache()` before every rep (first raster of new glyphs/size). Layout = paragraph build + shape + layout + rect/run extraction, paragraph cache off.

| item | lines | glyphs | unresolved | layout ms (warm/cold) | raster 1080p ms (warm/cold) | tex 1080p | raster 4K ms (warm/cold) | tex 4K |
|---|---|---|---|---|---|---|---|---|
| title_emoji | 2 | 51 | 0 | 0.546 / 0.597 | 15.313 / 31.411 | 1375x212 | 48.914 / 87.244 | 2750x424 |
| outline | 2 | 23 | 0 | 0.060 / 0.089 | 0.090 / 1.497 | 466x186 | 0.358 / 2.414 | 932x372 |
| gradient | 1 | 10 | 0 | 0.037 / 0.046 | 1.213 / 2.690 | 652x157 | 5.252 / 6.895 | 1303x314 |
| neon | 1 | 11 | 0 | 0.136 / 0.059 | 0.198 / 3.790 | 621x184 | 2.047 / 17.227 | 1241x368 |
| tiktok_center | 4 | 50 | 0 | 0.114 / 0.478 | 0.276 / 2.110 | 539x256 | 0.758 / 3.620 | 1077x512 |
| tiktok_left | 3 | 43 | 0 | 0.292 / 0.442 | 0.205 / 1.518 | 574x197 | 0.195 / 1.453 | 1148x394 |
| inverted | 2 | 30 | 0 | 0.100 / 0.133 | 0.073 / 0.544 | 467x147 | 0.284 / 2.094 | 934x294 |
| translucent_box | 3 | 47 | 0 | 0.305 / 0.356 | 0.837 / 1.394 | 368x215 | 3.179 / 4.338 | 736x430 |
| shadow_handwriting | 2 | 25 | 0 | 0.245 / 0.442 | 2.518 / 5.780 | 532x245 | 8.157 / 15.958 | 1064x490 |
| rotated | 2 | 25 | 0 | 0.177 / 0.264 | 0.299 / 3.920 | 460x162 | 0.536 / 2.756 | 919x324 |
| fontdb_system | 1 | 27 | 0 | 0.094 / 0.115 | 0.174 / 1.459 | 543x95 | 0.702 / 3.565 | 1085x190 |
| **sum (11 items)** | | | | 2.11 / 3.02 | 21.19 / 56.11 | | 70.38 / 147.56 | |

## Parallel layout+raster of all items at 4K scale (throughput proxy)

- 1 thread(s): 44 items in 724.1 ms -> 60.8 items/s (16.46 ms/item/thread)
- 4 thread(s): 176 items in 1274.6 ms -> 138.1 items/s (28.97 ms/item/thread)

## GPU (Apple M3 Pro (Metal))

### 1080p (1920x1080), 11 text items

- upload all item textures (5.00 MB RGBA): 5.627 ms (create + write_texture + submit + wait)
- GPU composite (bg copy + 11 quads, submit + wait): 1.429 ms
- readback 1920x1080: 3.993 ms
- CPU reference composite: 61.1 ms
- GPU vs CPU per-channel diff: max 1 / mean 0.00059; channels >1: 0, >2: 0, >4: 0 of 6220800

### 4K (3840x2160), 11 text items

- upload all item textures (19.99 MB RGBA): 14.511 ms (create + write_texture + submit + wait)
- GPU composite (bg copy + 11 quads, submit + wait): 1.600 ms
- readback 3840x2160: 20.030 ms
- CPU reference composite: 568.5 ms
- GPU vs CPU per-channel diff: max 1 / mean 0.00030; channels >1: 0, >2: 0, >4: 0 of 24883200

- anim `typewriter` on `tiktok_center`: 4 draws at t=1, 37 of 50 glyph clusters revealed at t=0.75
- anim `words` on `title_emoji`: 16 draws at t=1, 38 of 51 glyph clusters revealed at t=0.75
- animation frames GPU vs CPU max channel diff: 1
