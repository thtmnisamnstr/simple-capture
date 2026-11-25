# Mac App Store screenshots (en-US)

The screenshots for the English (U.S.) listing. `scripts/asc/asc.py prepare` uploads every
`.png` in this folder to one screenshot set with display type `APP_DESKTOP`, and reuses a
screenshot App Store Connect already has when its file is unchanged. This README is not
uploaded.

## Format

- PNG only (`asc.py` rejects other files), sRGB, with no alpha channel or transparency.
- 2880 × 1800, 2560 × 1600, 1440 × 900 or 1280 × 800 pixels (all 16:10). Use one size for the
  whole set; 2880 × 1800 is preferred.
- 1 to 10 files.

Check them with `python3 -I scripts/asc/asc.py check-metadata`.

## Order

Files are uploaded in filename order, and the first one or two appear in search results, so they
must show what the app is for. Name them with a two-digit prefix:

1. `01-record.png`: the recorder.
2. `02-edit.png`: the editor with a recording open.
3. `03-export.png`: the Export sheet.
4. `04-library.png`: the Library.

To add one, give it the next number, or renumber the files to change the order.

## Content rules

- Show the Mac app's UI: no Windows or Linux windows, and no export options the Mac lacks
  (x264/x265 "Best Quality" for H.264 or HEVC, or gifski for GIF).
- Use made-up content only: no real names, email addresses, file paths with a real user name,
  or third-party logos, apps or copyrighted video in the recorded frame.
- No notifications or other apps' windows overlapping ours.
- Captions on the images are optional. If used, keep them short and accurate to the listing
  (for example "Record your screen, camera and sound").
