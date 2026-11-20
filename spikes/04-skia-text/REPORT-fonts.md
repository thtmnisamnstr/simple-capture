# Spike 4 add-on: font specimen check for ADR-020

Date: 2026-11-19. Machine: M3 Pro, macOS 26.7.2. Code: `spikes/04-skia-text/src/specimen.rs`. Run it with `./target/ft/release/skia-text-spike --specimen [slug,...]`.

## What I ran
- `fonts/fetch.sh` now fetches all 13 ADR-020 families and 9 OFL replacement candidates from google/fonts at the pinned commit `cc5661d43cd4f9aad5567033a8e15ca28dc35c86`.
  - It records SHA-256 for every font file and its licence (OFL.txt or Apache LICENSE.txt), 56 checks in total.
  - A clean re-fetch into an empty folder gave 56/56 OK.
- The specimen mode uses the same text stack as the app:
  - FreeType `FontMgr::custom_empty`, `TypefaceFontProvider`, `FontCollection` with fallback disabled.
  - Text family first, then Noto-COLRv1 as the only fallback, used only on the emoji line.
- For each family it writes:
  - `<slug>_coverage.png`: name line; A–Z; a–z; 0–9; ASCII punctuation; ‘ ’ “ ” ’ – — … • € £ ¥; "Ærøskøbing, São Paulo, Kraków, Đặng, naïve café"; the full Latin-1 letter range (U+00C0–U+00FF); the full Latin Extended-A block (U+0100–U+017F); an NFD line made of base letters plus combining marks; an extras line (© ® ™ ° ± × ÷ ¢ ¿ ¡ « » ‹ › ‚ „ † ‡ ‰ ½ ¼ ¾ ² ³ µ ¶ § ′ ″ −); a mixed emoji line; and a stroke-only overlap diagnostic. Red boxes mark glyph id 0.
  - `<slug>_weights.png`: every weight instance (100–900 in steps of 100, clamped to the axis) and every italic.
  - `<slug>_styles_1080p.png`: a 1920x1080 frame with outline, drop shadow, merged TikTok line background, inverted box, glow, and gradient+stroke (rotated −6°).
  - `nfd_marks_all.png`: combining-mark placement compared across all 22 fonts.
- Three coverage measures per line:
  - `Typeface::unichar_to_glyph == 0`, plus glyphs whose outline is empty.
  - `Paragraph::unresolved_glyphs()`.
  - Glyph-id-0 count in the shaped runs.
  - Every character missing from the cmap is then shaped on its own, to separate real tofu from characters HarfBuzz builds from a base letter plus a combining mark.
- Weight check:
  - Measure: alpha difference on "Handgloves 0123 Kraków “Hamburg”" at 48 px. A pixel counts as different when |Δα| > 32, over the union of inked pixels. Ink ratio = sum of alpha (B) / sum of alpha (A).
  - Pass condition: every adjacent weight pair differs by more than 5% of pixels with ink ratio above 1.0, and Regular vs Bold also has ink ratio above 1.02.
- I looked at every PNG with the Read tool and zoomed into Fredoka, DM Serif Display, Pacifico, Sedgwick Ave Display and Special Elite.

## Results for the ADR-020 list

| Font | Licence | Verdict | Missing (tofu) / defects | Weights / italic | Recommendation |
|---|---|---|---|---|---|
| Inter | OFL-1.1 | pass (tiny gap) | ŉ U+0149 (deprecated) | 100–900 distinct (Reg vs Bold 79.0%, ink 1.54); italic 100–900 | keep. Default opsz 14; opsz 32 differs 81%, so decide whether to set opsz for large text |
| Montserrat | OFL-1.1 | pass | none | 100–900 (81.2%, 2.09); italic | keep. Axis default is wght 100 (Thin), so always instance explicitly |
| Oswald | OFL-1.1 | pass | none | 200–700 (76.8%, 1.49) | keep |
| Bebas Neue | OFL-1.1 | pass with gaps | ſ, ặ (Vietnamese), ′ ″ | single weight; caps-only design | keep |
| Playfair Display | OFL-1.1 | pass | none required (µ missing among extras) | 400–900 (80.4%, 1.55); italic | keep |
| DM Serif Display | OFL-1.1 | pass with gaps | tofu: Ħ ħ Ĳ ĳ ĸ ŉ Ŋ ŋ Ŧ ŧ ſ, ặ, ′ ″. 30 letters built from marks (Ĉ ĉ Ċ ċ Ĕ ĕ Ĝ ĝ Ġ ġ Ĥ ĥ Ĩ ĩ Ĭ ĭ Ĵ ĵ Ŏ ŏ Ŝ ŝ Ũ ũ Ŭ ŭ Ŵ ŵ Ŷ ŷ); the circumflex on Ŵ ŵ Ŷ ŷ sits to the right | single weight; italic | replace with Abril Fatface, or keep if Maltese, Sami and Welsh gaps are acceptable |
| Courier Prime | OFL-1.1 | pass with gaps | ĸ ŉ ſ, ặ; no combining marks (NFD input gives tofu) | 400 / 700 static (47.5%, 1.50); italic 400 and 700 | keep |
| Special Elite | Apache-2.0 | pass with gaps | ſ, ặ, ′ ″; no combining marks; blank .notdef | single weight | keep (Apache), or drop if OFL only (Courier Prime covers typewriter) |
| Caveat | OFL-1.1 | pass with gaps | ſ only (ặ is built correctly from marks) | 400–700 (79.6%, 1.51) | keep |
| Permanent Marker | Apache-2.0 | **fail** | Đ plus 118 of 128 Latin Extended-A characters (ą ć ę ń ś ź ż č ř š ž ğ ş ő ű …); ™ † ‡; blank .notdef, so letters vanish | single weight | **drop → Sedgwick Ave Display** (OFL) |
| Pacifico | OFL-1.1 | pass with gaps | Ĳ ĳ ſ, ′ ″ | single weight | keep |
| Tilt Neon | OFL-1.1 | pass with gaps | Ĳ ĳ ŉ ſ | no wght axis (XROT/YROT only) | keep |
| Fredoka | OFL-1.1 | **fail** | tofu: Đ đ Ħ ħ Ĳ ĳ ĸ Ŀ ŀ ŉ Ŋ ŋ Ŧ ŧ ſ, ặ, ′ ″. 103 more Latin Extended-A letters have no precomposed glyph (cmap of 321 code points); HarfBuzz composes them but the accent lands beside the letter (the styles frame shows "Łódz ´") | 300–700 (72.6%, 1.92) | **replace → Baloo 2** (or Nunito) |

Notes on the table:
- In every family, ASCII, the common punctuation (curly quotes, apostrophe, en/em dash, ellipsis, bullet, € £ ¥) and all Latin-1 letters resolved. The only exception is Đ in Fredoka and Permanent Marker.
- Emoji: 0 unresolved in every family, about 8.45–8.5k coloured pixels.
- The style effects showed no visible artifacts in any frame.

## Replacement candidates (all fetched and verified the same way, all OFL-1.1)

| Candidate | For | Verdict | Gaps | Weights |
|---|---|---|---|---|
| Baloo 2 | Fredoka (rounded) | pass with gaps | ſ | 400–800 (81.9%, 1.80) |
| Nunito | Fredoka (rounded) | pass with gaps | ĳ ſ | 200–1000 (80.4%, 1.49); italic |
| Quicksand | rounded / light | pass with gaps | ſ, ′ | 300–700 |
| Shantell Sans | handwriting / marker-ish | pass with gaps | ŉ ſ | 300–800; italic; extra BNCE/INFM/SPAC axes |
| Sedgwick Ave Display | Permanent Marker | pass with gaps | Ĳ ĳ ŉ ſ; ½ ¼ ¾ ² ³ µ missing among extras; Þ has an empty outline in my check; all-caps graffiti style | single |
| Caveat Brush | Permanent Marker (alternate) | pass with gaps | ſ, ặ, no combining marks, ′ ″ | single |
| Abril Fatface | DM Serif Display | pass with gaps | ặ, no combining marks, ′ ″; all of Latin Extended-A present | single |
| Knewave | marker | **fail** | Đ Þ plus 121 Latin Extended-A characters; blank .notdef | single |
| Gloock | DM Serif Display | rejected | ĸ Ŀ ŀ ŉ Ŧ ŧ ſ ặ; composed accents misplaced; + < = > \ ^ _ – drawn very light | single |

Proposed list (13 fonts plus emoji): Inter, Montserrat, Oswald, Bebas Neue, Playfair Display, **Abril Fatface** (or DM Serif Display with its gaps), Courier Prime, Special Elite, Caveat, **Sedgwick Ave Display**, Pacifico, Tilt Neon, **Baloo 2** (or Nunito).

## Findings that affect the app (all measured)
1. **Variable fonts must be instanced explicitly.**
   - Registering one variable face and asking for weight through `TextStyle::set_font_style(FontStyle::bold())` gives a different result from the true wght=700 instance: 73–85% of inked pixels differ (Montserrat ink ratio 0.376).
   - The result is darker than the default instance. It looks like synthetic emboldening (my inference, not checked in Skia source).
   - Two routes give exactly the true instance (0.0% difference): `TextStyle::set_font_arguments` with a wght coordinate, or `Typeface::clone_with_arguments` registered as its own alias.
2. **Axis defaults are not "Regular".** Montserrat's default wght is 100 (Thin) and Fredoka's is 300. Inter's opsz defaults to 14 at every size.
3. **Missing glyphs can be invisible.** The .notdef glyph is blank in Permanent Marker, Knewave, Abril Fatface and Special Elite. The app must count unresolved glyphs (`unresolved_glyphs()` or glyph id 0) and warn the user, or add a text fallback family. It cannot rely on visible tofu boxes.
4. **NFC-normalize text before layout.** Courier Prime, Special Elite, Caveat Brush and Abril Fatface have no combining marks, so decomposed (NFD) input renders tofu even though the precomposed letters exist. This would need a normalization crate in the app; nothing was added in the spike.
5. **Composition fallback depends on the font's mark anchors.** HarfBuzz builds missing precomposed letters from base + mark. Placement is good in Caveat and most of DM Serif Display, and bad in Fredoka and Gloock, and for Ŵ ŵ Ŷ ŷ in DM Serif Display.
6. **METADATA subsets are not reliable.** Fredoka's METADATA claims latin-ext but the font file lacks the precomposed Latin Extended-A letters. Special Elite claims latin only but covers nearly all of Latin Extended-A. Only cmap checks plus shaping are trustworthy.

## Not measured / not done
- Windows and Linux runs.
- Cyrillic, Greek, RTL, and Latin Extended Additional beyond ặ.
- Synthetic bold on static single-weight fonts.
- Zoom checks of the stroke-only overlap diagnostic for every variable font.
- A native-reader review of composed accents.

---

## Independent verification (2026-11-19)

An adversarial verifier re-read the sources and re-ran the builds and measurements. Verdict: **partially confirmed**. Where this section and the text above disagree, this section wins.

### Discrepancies found

- REFUTED: 'Composition fallback placement is good in Caveat'. At 3x magnification, Caveat's ặ (U+1EB7) is misplaced. The breve sits to the upper right of the 'a', and in the sample line it lands on the following 'n' (the sample renders as 'Đạňg'). The ZOOM row also shows the dot-below and breve offset to the right of the base. Source: out/specimens/caveat_coverage.png, sample row and ZOOM row.
- REFUTED: 'DM Serif Display: most composed letters look right, only the circumflex on Ŵ ŵ Ŷ ŷ sits to the right'. At 3x magnification of the ZOOM row in dmserifdisplay_coverage.png, the marks are shifted right across the composed set, not only on Ŵ ŵ Ŷ ŷ. Ĉ's caret collides with the C terminal. ĉ's caret and ċ's dot sit past the base, over the next letter. Ĕ ĕ breves sit over the right edge. ġ's dot lands at the start of the following H. Ŏ ŏ breves sit over the next glyph. Ŝ ŝ carets are offset. Ŵ ŵ ŷ are clearly off. DM Serif's 30 composed Latin Extended-A letters should therefore count as displaying incorrectly, a smaller-scale version of the Fredoka problem. The 'keep DM Serif Display with gaps' option is weaker than stated.
- MINOR: 'A clean re-fetch into an empty folder gave 56/56 OK' does not reproduce in a truly empty folder. fetch.sh only sha-checks LICENSE-NotoColorEmoji-OFL.txt and never downloads it. In an empty directory the script stops after the 2 emoji fonts with 'LICENSE-NotoColorEmoji-OFL.txt: FAILED open or read' (exit 1). With that one licence file copied in, all 56 checks report OK.
- MINOR: Fredoka cmap size. My independent format-4 parser counts 320 mapped code points, not 321. The difference is probably the 0xFFFF sentinel. The other cmap facts reproduce: U+0106 and U+0110 absent, U+0301 present, GPOS present, 118 Latin Extended-A code points missing.
- MINOR: 'Fonts and specimens add about 26 MB to fonts' overstates what was added. 26 MB is the whole fonts/ directory. That includes the pre-existing NotoColorEmoji-CBDT.ttf (10.7 MB, gitignored) and Noto-COLRv1.ttf (5.0 MB), so the new font files are roughly 10 MB.
- MINOR wording: 'Permanent Marker: 118 of 128 Latin Extended-A characters are tofu, plus Đ'. Đ (U+0110) is itself one of the 118. It is listed again only because it appears in the sample line.
- NOT REPRODUCED: '5.0 s wall for the first 13 families'. I only timed the full 22-family run, which took 8.9 s.
- INFERENCE, not verified: that FontStyle::bold() on a variable face produces synthetic emboldening. What is measured: route B is darker than the default instance (ink ratio 1.35 to 3.09) and differs from the true wght=700 instance by 73.3 to 84.7%.

### Corrected summary

The specimen mode in spikes/04-skia-text (src/specimen.rs, `--specimen`) builds cleanly and reproduces byte-for-byte. It renders all 13 ADR-020 families plus 9 OFL candidates through FreeType FontMgr::custom_empty, TypefaceFontProvider, font fallback disabled, and Noto-COLRv1 as the emoji fallback. fetch.sh pins google/fonts cc5661d and sha-checks 31 font files and 22 licence files (56 checks with the emoji pins). One fix is needed: it does not download LICENSE-NotoColorEmoji-OFL.txt, so in a truly empty folder it fails until that file is present.

Coverage (measured by cmap lookup, paragraph unresolved_glyphs() and glyph-id-0; cmap facts checked with an independent parser). ASCII, curly quotes, apostrophe, en/em dash, ellipsis, bullet, € £ ¥ and all Latin-1 letters resolve in every family.
- Inter: missing ŉ only.
- Montserrat, Oswald: complete.
- Playfair Display: complete (µ missing among the extras).
- Bebas Neue: missing ſ, ặ, ′ ″; caps-only design.
- Courier Prime: missing ĸ ŉ ſ ặ; no combining marks.
- Special Elite: missing ſ ặ ′ ″; no combining marks; blank .notdef.
- Pacifico: missing Ĳ ĳ ſ ′ ″.
- Tilt Neon: missing Ĳ ĳ ŉ ſ.
- Caveat: missing ſ. ặ has no precomposed glyph and its composed marks are misplaced: the breve lands on the next letter.
- DM Serif Display: 11 tofu letters (Ħ ħ Ĳ ĳ ĸ ŉ Ŋ ŋ Ŧ ŧ ſ) plus ặ. 30 more letters have no precomposed glyph, and on visual inspection their composed marks are generally shifted right, often onto the next glyph (Ĉ ĉ Ċ ċ Ĕ ĕ ġ Ŏ ŏ Ŵ ŵ ŷ clearly). This is a real display defect for Esperanto, Maltese and Welsh text, not just Ŵ ŵ Ŷ ŷ.
- Fredoka: fails. 15 Latin Extended-A letters are tofu, including Đ đ. 103 more are composed with accents placed beside the letter. The font file lacks the precomposed glyphs even though METADATA claims latin-ext.
- Permanent Marker: fails. 118 of 128 Latin Extended-A characters are tofu (including Đ), plus ™ † ‡. Its .notdef is blank, so missing letters vanish.

Licences: Special Elite and Permanent Marker are Apache-2.0; the rest are OFL-1.1.

Weights. Every wght instance (clone_with_arguments) is distinct and heavier than the one before: adjacent steps differ by 36.7–82.1% of inked pixels, with ink ratios above 1.07. Regular vs Bold: Inter 79.0%/1.54, Montserrat 81.2%/2.09, Oswald 76.8%/1.49, Playfair Display 80.4%/1.55, Caveat 79.6%/1.51, Fredoka 72.6%/1.92, Courier Prime (static) 47.5%/1.50. Italic files differ from upright by 52–81%.

App-relevant measurements:
- TextStyle FontStyle::bold() on one registered variable face differs from the true wght=700 instance by 73.3–84.7%, and is darker than the default instance. That it is synthetic emboldening is inference.
- TextStyle::set_font_arguments(wght=700) matches the true instance exactly (0.0%).
- Axis defaults are not Regular: Montserrat defaults to wght 100, Fredoka to 300, Inter to opsz 14 (opsz 32 differs by 81.1%).
- Permanent Marker, Knewave, Abril Fatface and Special Elite have a blank .notdef, so gaps are only detectable by count, not by looking.
- Courier Prime, Special Elite, Caveat Brush and Abril Fatface lack combining marks, so NFD input shows tofu. NFC normalization avoids this.
- HarfBuzz composition from base + mark gave misplaced accents in every font where it was observed: Fredoka, DM Serif Display, Gloock and Caveat (ặ). Letters without a precomposed glyph should be treated as not displaying correctly.

Styles. Outline, shadow, merged line background, inverted box, glow and gradient+stroke showed no rendering artifacts at 1080p in the frames I viewed. Emoji resolved with 0 unresolved in all 22 families.

Replacements (all OFL, fetched and rendered the same way):
- Baloo 2 for Fredoka: missing ſ only; wght 400–800; Reg vs Bold 81.9%/1.80. Nunito is an alternative (missing ĳ ſ; wght 200–1000; italic).
- Sedgwick Ave Display for Permanent Marker: missing Ĳ ĳ ŉ ſ, plus ½ ¼ ¾ ² ³ µ among the extras.
- Abril Fatface for DM Serif Display: all of Latin Extended-A precomposed; missing ặ and combining marks; blank .notdef. Given the misplaced composed accents in DM Serif Display, replacing it is better supported than keeping it with gaps.
- Rejected: Knewave fails Latin Extended-A.

Housekeeping. The fonts/ directory totals 26 MB, of which about 16 MB are the pre-existing emoji fonts, and out/specimens is 21 MB. Neither the new fonts nor the specimens are gitignored. Not done: Windows and Linux runs, scripts beyond Latin, and a native-reader review.
