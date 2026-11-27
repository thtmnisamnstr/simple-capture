# Mac App Store app previews

Three previews for the English (U.S.) listing, rendered from the real UI on its mock backend:

1. `01-record.mp4`: pick a source and shape, record, stop; the editor opens.
2. `02-edit.mp4`: play back, add text and restyle it.
3. `03-export.mp4`: export presets and formats, then export.

Render them with `appstore/previews/tools/make.sh` (or `make.sh 2` for one). They land in
`en-US/`, which git ignores: upload them by hand in App Store Connect (the version page → App
Previews and Screenshots → Mac). `asc.py` does not upload previews.

Apple's format: 1920 × 1080, 15 to 30 seconds, at most 30 fps, H.264 with a stereo AAC track
(silent here), under 500 MB. The scenes follow the screenshot rules in
`../screenshots/en-US/README.md`.
