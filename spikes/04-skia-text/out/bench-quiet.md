# Skia text spike bench (quiet)

- Skia font manager for bundled fonts: FreeType (emoji file: Noto-COLRv1.ttf)
- bundled font load + provider setup: 3.5 ms
- fontdb: 838 faces / 388 families scanned in 76.1 ms; first families: [".ADT Slab Numeric", ".ADT Slab Soft Numeric", ".Al Bayan PUA", ".Al Nile PUA", ".Al Tarikh PUA", ".Apple Color Emoji UI", ".Apple SD Gothic NeoI", ".Aqua Kana"]
- fontdb pick (Georgia/serif/sans, bold): Some(("Georgia", "/System/Library/Fonts/Supplemental/Georgia Bold.ttf", 0))

## Per-item layout + raster (median of 30, single thread)

Warm = Skia glyph cache populated (re-raster after an edit). Cold = `purge_font_cache()` before every rep (first raster of new glyphs/size). Layout = paragraph build + shape + layout + rect/run extraction, paragraph cache off.

| item | lines | glyphs | unresolved | layout ms (warm/cold) | raster 1080p ms (warm/cold) | tex 1080p | raster 4K ms (warm/cold) | tex 4K |
|---|---|---|---|---|---|---|---|---|
| title_emoji | 2 | 51 | 0 | 0.178 / 0.205 | 3.118 / 5.230 | 1375x212 | 11.496 / 15.445 | 2750x424 |
| outline | 2 | 23 | 0 | 0.025 / 0.035 | 0.030 / 0.511 | 466x186 | 0.100 / 0.825 | 932x372 |
| gradient | 1 | 10 | 0 | 0.014 / 0.020 | 0.481 / 0.823 | 652x157 | 1.935 / 2.862 | 1303x314 |
| neon | 1 | 11 | 0 | 0.015 / 0.022 | 0.067 / 1.089 | 621x184 | 0.259 / 2.311 | 1241x368 |
| tiktok_center | 4 | 50 | 0 | 0.041 / 0.054 | 0.031 / 0.179 | 539x256 | 0.081 / 0.330 | 1077x512 |
| tiktok_left | 3 | 43 | 0 | 0.039 / 0.052 | 0.024 / 0.148 | 574x197 | 0.064 / 0.289 | 1148x394 |
| inverted | 2 | 30 | 0 | 0.029 / 0.039 | 0.022 / 0.103 | 467x147 | 0.059 / 0.204 | 934x294 |
| translucent_box | 3 | 47 | 0 | 0.037 / 0.044 | 0.064 / 0.117 | 368x215 | 0.227 / 0.334 | 736x430 |
| shadow_handwriting | 2 | 25 | 0 | 0.031 / 0.047 | 0.373 / 0.826 | 532x245 | 1.476 / 2.655 | 1064x490 |
| rotated | 2 | 25 | 0 | 0.021 / 0.031 | 0.028 / 0.409 | 460x162 | 0.097 / 0.781 | 919x324 |
| fontdb_system | 1 | 27 | 0 | 0.020 / 0.027 | 0.034 / 0.241 | 543x95 | 0.119 / 0.698 | 1085x190 |
| **sum (11 items)** | | | | 0.45 / 0.58 | 4.27 / 9.67 | | 15.91 / 26.73 | |

## Parallel layout+raster of all items at 4K scale (throughput proxy)

- 1 thread(s): 44 items in 136.7 ms -> 322.0 items/s (3.11 ms/item/thread)
- 4 thread(s): 176 items in 274.3 ms -> 641.6 items/s (6.23 ms/item/thread)

## GPU (Apple M3 Pro (Metal))

### 1080p (1920x1080), 11 text items

- upload all item textures (5.00 MB RGBA): 1.677 ms (create + write_texture + submit + wait)
- GPU composite (bg copy + 11 quads, submit + wait): 0.441 ms
- readback 1920x1080: 1.247 ms
- CPU reference composite: 17.8 ms
- GPU vs CPU per-channel diff: max 1 / mean 0.00059; channels >1: 0, >2: 0, >4: 0 of 6220800

### 4K (3840x2160), 11 text items

- upload all item textures (19.99 MB RGBA): 5.195 ms (create + write_texture + submit + wait)
- GPU composite (bg copy + 11 quads, submit + wait): 0.765 ms
- readback 3840x2160: 6.923 ms
- CPU reference composite: 75.8 ms
- GPU vs CPU per-channel diff: max 1 / mean 0.00030; channels >1: 0, >2: 0, >4: 0 of 24883200

- anim `typewriter` on `tiktok_center`: 4 draws at t=1, 37 of 50 glyph clusters revealed at t=0.75
- anim `words` on `title_emoji`: 16 draws at t=1, 38 of 51 glyph clusters revealed at t=0.75
- animation frames GPU vs CPU max channel diff: 1

## Emoji font format x Skia font manager

| combo | shaping | pixels | verdict | raster ms 1080p warm / cold / 4K cold |
|---|---|---|---|---|
| FreeType fontmgr + NotoColorEmoji-CBDT.ttf | unresolved glyphs: 0 | colored px: 12816 | renders | 0.02 / 0.73 / 1.14 |
| FreeType fontmgr + Noto-COLRv1.ttf | unresolved glyphs: 0 | colored px: 12770 | renders | 0.02 / 1.25 / 1.92 |
| System fontmgr + NotoColorEmoji-CBDT.ttf | unresolved glyphs: 30 | colored px: 0 | NO color emoji | 0.02 / 0.04 / 0.07 |
| System fontmgr + Noto-COLRv1.ttf | unresolved glyphs: 0 | colored px: 0 | NO color emoji | 0.00 / 0.01 / 0.02 |
