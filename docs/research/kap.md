# Kap research: export pipeline and UI patterns

Status: M0 research note. Source: `reference/Kap` (wulkano/Kap @ c42692f, v3.6.0, Feb 2024).
License: Kap is **MIT** (`reference/Kap/LICENSE.md`, "Copyright (c) Wulkano hello@wulkano.com"). Exact
FFmpeg arguments and small snippets below are quoted from Kap under that license; the attribution
entry is in `docs/THIRD_PARTY.md` under "Kap (export pipeline and UI patterns research)".

Items we plan to adapt are marked **[ADAPT]**. Each one is listed again in section 7 and covered by the
THIRD_PARTY entry.

Checks were run on this machine (Apple M3 Pro, macOS 26.7.2) with Homebrew FFmpeg 9.0.2. We used it
only to validate Kap's recipes; it is not the helper we ship. Kap itself ships FFmpeg **4.4.1**
(`ffmpeg-static@4.4.1`). I did not run 4.4.1, so behavior differences between versions are marked as
such.

---

## 0. Architecture in one paragraph

Kap records to an H.264 (or HEVC) `.mp4` with Aperture. Export is a separate step:
`Export` (main/export.ts) owns a `Conversion` (main/conversion.ts), which calls
`convertTo(format, options)` (main/converters/index.ts). That function looks up a per-format function
in `main/converters/h264.ts`. Each function builds an argv list with a small helper,
`conditionalArgs(...)` (main/converters/utils.ts). The helper accepts strings, arrays, or `{args, if}` objects, which keeps
optional flags readable. `convert()` (main/converters/process.ts) runs the bundled ffmpeg through
`execa` and turns stderr into progress. The result is wrapped in a `PCancelable` promise. The
**input is always a file on disk**, and trimming, scaling and fps conversion all happen inside ffmpeg. That is the
biggest difference from our design: our Rust renderer applies the EDL, scale, and fps, then streams finished
frames to the helper over a pipe.

**[ADAPT]** the `conditionalArgs` pattern (a typed argv builder with conditional groups). It is
trivial, but it makes recipes easy to read and diff. In Rust it becomes a small `Args` builder with
`.push(..)` and `.push_if(cond, [..])`.

---

## 1. FFmpeg argument recipes per format

All recipes come from `main/converters/h264.ts`, except where noted. Placeholders: `IN` is the recording, `OUT` is a temp
file in a `tempy` directory, `FPS` is the export fps, `W`/`H` are the chosen size, and `S`/`E` are the trim
start and end in seconds. `makeEven(n) = 2 * Math.round(n / 2)` (main/converters/utils.ts).

Important context: `shouldCrop` is hard-coded to `true` in the only UI that starts conversions
(`renderer/components/editor/options/right.tsx`, `ConvertButton`: `const shouldCrop = true;`). So
in practice the `-s`/`-ss`/`-to` groups below are **always** present, and trimming is done with
output-side `-ss`/`-to`. Output-side seeking decodes from the start of the file and discards frames, so it is
frame-accurate but slow for late trims.

### 1.1 Recipe table (exact args)

| Format (Kap name) | Exact args (in order) | Notes from Kap's source |
|---|---|---|
| **GIF pass 1: palette** (`convertToGif`) | `-i IN -vf fps=FPS,scale=W:H:flags=lanczos,palettegen -ss S -to E PALETTE.png` | Not tracked for progress (`shouldTrack:false`, no `onProgress`). If the palette file doesn't exist afterwards, Kap falls back to the non-palette path (comment: "Sometimes if the clip is too short or fps too low, the palette is not generated"). |
| **GIF pass 2: encode**, palette exists | `-i IN -i PALETTE.png -filter_complex "fps=FPS,scale=W:H:flags=lanczos[x]; [x][1:v]paletteuse" -loop L -ss S -to E OUT.gif` | `L` = `0` (forever) if the `loopExports` setting is on (default **true**), else `-1` (no loop). |
| **GIF pass 2**, no palette | `-i IN -vf fps=FPS,scale=W:H:flags=lanczos -loop L -ss S -to E OUT.gif` | ffmpeg's default GIF palette, so quality is noticeably worse. |
| **GIF pass 3: gifsicle** (`compress`, process.ts) | `gifsicle --batch OUT.gif` (+ `--lossy=50` if the `lossyCompression` setting is on, default **false**) | In-place optimization with the separate **gifsicle** binary. Shown as "Compressing", but gifsicle prints no `time=`, so progress stays empty. |
| **MP4 / H.264** (`convertToMp4`) | `-i IN -r FPS [-an] -s makeEven(W)xmakeEven(H) -ss S -to E OUT.mp4` | No `-c:v`/`-c:a`, so it uses ffmpeg's defaults for `.mp4`: libx264 (default CRF 23 / preset medium) and the native AAC encoder. `-an` if muted. The size group is also added whenever dims are odd. |
| **HEVC** (`convertToHevc`, saved as `.mp4`) | `-i IN -r FPS -c:v libx265 -c:a libopus -preset medium -tag:v hvc1 [-an] -s makeEven(W)xmakeEven(H) -ss S -to E OUT.mp4` | Comment on `-tag:v hvc1`: "Metadata for macOS". Opus audio inside MP4. |
| **AV1** (`convertToAv1`, saved as `.mp4`) | `-i IN -r FPS -c:v libaom-av1 -c:a libopus -crf 34 -b:v 0 -strict experimental -cpu-used 4 -row-mt 1 -tiles 2x2 [-an] -s makeEven(W)xmakeEven(H) -ss S -to E OUT.mp4` | Constant-quality mode (`-crf 34 -b:v 0`). Row-MT and 2x2 tiles for parallelism, per the trac AV1 wiki. |
| **WebM / VP9** (`convertToWebm`) | `-i IN -threads max(cpus-1,1) -deadline good -b:v 1M -codec:v vp9 -codec:a vorbis -ac 2 -strict -2 -r FPS [-an] -s makeEven(W)xmakeEven(H) -ss S -to E OUT.webm` | Comments: `best` is "twice as slow and only slightly better". 1M bitrate is described as "same as the MP4", but the MP4 recipe sets no bitrate. `-strict -2` because the native `vorbis` encoder is experimental. `-codec:v vp9` resolves to libvpx-vp9. |
| **APNG** (`convertToApng`) | `-i IN -vf fps=FPS,scale=W:H:flags=lanczos -plays P [-an] -ss S -to E OUT.apng` | `P` = `0` (forever) if looping, else `1`. Comment: APNG uses `-plays`, not `-loop`. Dimensions are **not** made even. |
| Edit-plugin pre-crop (`crop`) | `-i IN -s makeEven(W)xmakeEven(H) -ss S -to E OUT.<same ext>` | Despite the name, this scales and trims. It only runs before third-party "edit service" plugins. |
| Mute (`mute`, process.ts) | `-i IN -an -vcodec copy OUT` | Stream-copy that drops audio. |
| HEVC to H.264 pre-conversion (`convertToH264`, main/utils/encoding.ts) | `-i IN -vcodec libx264 -crf 27 -preset veryfast -c:a copy OUT` | Kap's converter map only handles H.264 inputs, so HEVC recordings are transcoded first. |
| Probe fps (`main/utils/fps.ts`) | `ffmpeg -i IN` (expected to fail), then regex `/.*, (.*) fp.*/` on stderr | Parses the "NN fps" token from the human-readable stream line. Fragile. |
| Probe codec (`main/utils/encoding.ts`) | `ffmpeg -i IN`, then regex `/.*: Video: (.*?) \(.*/` | Same approach. |

Scaling: GIF and APNG use the `scale` filter with `flags=lanczos`. MP4, HEVC, AV1 and WebM use the `-s` output
option, which inserts swscale's default (bicubic) scaler. Neither path sets a pixel format or color tags.

### 1.2 What I verified on this machine (Homebrew FFmpeg 9.0.2, not Kap's 4.4.1)

These are smoke runs on a synthetic 4 s, 1280x720 @ 60 fps `testsrc2` + sine source. Each run trims 0.5 to 3.5 s and
outputs 854x480 @ 30 (GIF 640x360 @ 15). They are **not** benchmarks.

| Recipe | Result |
|---|---|
| MP4 | OK: h264 High, yuv420p, 30/1, 3.000 s. **Color tags all `unknown`.** Top-level atoms `ftyp free mdat moov`, so **moov at the end (no faststart)**. Exactly-even `-s` honoured. |
| HEVC | OK: `hvc1` tag, yuv420p, Opus audio. 1.67 s wall. |
| WebM | OK: vp9 + native vorbis. 6.99 s wall. |
| APNG | OK: rgb24, frame rate stored as 100000/6667 (APNG delays are fractions). 3.64 s wall, 4.4 MB for 3 s. |
| AV1 | **Could not test**: this Homebrew build has no libaom (`Unrecognized option 'tiles'`). It has `libsvtav1`. |
| GIF pass 1 | **Produces no palette** when `S > 0`: "Output file is empty, nothing was encoded". palettegen emits its single frame at EOF with a timestamp that the output-side `-ss` then drops. With `-ss 0`, or with `-ss/-to` moved **before** `-i`, the palette is produced. So on FFmpeg 9 every trimmed-start GIF silently falls back to the low-quality no-palette path (625 KB vs 1.06 MB with palette for this clip). I did not check whether 4.4.1 behaves the same, but Kap's "sometimes the palette is not generated" comment suggests it might. This doesn't affect us because our frames arrive already trimmed. It is still a good example of why every recipe needs a test that checks its intermediate outputs. |
| Color tags on a Y4M pipe | Output-side `-color_primaries bt709 -color_trc bt709 -colorspace bt709 -color_range tv` with libx264 tagged only `color_space` and `color_range`; **trc and primaries stayed `unknown`**. Passing the same four options **input-side** (before `-f yuv4mpegpipe -i -`), or using `-vf setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv`, tagged all four for libx264, libx265 and libvpx-vp9. |

### 1.3 What we'd change, per format

General changes that apply to every video format:

- **Input is a pipe, not a file.** Our renderer applies the EDL, speed, scale and fps, so the helper never
  needs `-ss/-to`, `-r`, `-s`, `scale` or `fps`. Video comes in as
  `-f yuv4mpegpipe -i pipe:0` (the Y4M header carries size, rate, chroma and range), or as raw NV12:
  `-f rawvideo -pix_fmt nv12 -video_size WxH -framerate N/D -i pipe:0`. Mixed audio is a second input,
  `-i mix.wav` (or a second pipe/FIFO). Add `-map 0:v:0 -map 1:a:0` and an explicit `-t` or `-shortest`.
- **Color tags.** Put `-color_primaries bt709 -color_trc bt709 -colorspace bt709 -color_range tv`
  **before the pipe input** (verified above), or use `setparams`. The renderer must output BT.709
  limited-range YUV to match the tags. Displays are Display P3, so the decision of which primaries to tag
  belongs in DECISIONS.md. This note only records that output-side flags are not enough on FFmpeg 9.
- **Even dimensions.** The renderer produces even W/H for 4:2:0 formats, and the UI shows the even numbers
  (section 4), instead of silently running `makeEven` inside the converter the way Kap does.
- **Explicit codecs and pixel format.** Never rely on ffmpeg's defaults (Kap's MP4 does). Always pass
  `-c:v`, `-pix_fmt yuv420p` (or `nv12` for hardware), `-c:a`, and a bitrate or quality value.
- **Faststart.** Use `-movflags +faststart` for MP4/MOV (Kap's output has moov at the end, verified above).
- **Progress.** Use `-nostats -progress pipe:2` (or a dedicated fd) instead of scraping the human stats line (section 2).
- **Overwrite and logging.** Use `-y -hide_banner -loglevel error`, so stderr only carries errors and `-progress` key/values.
- **Atomic output.** Write to `OUT.partial` in the destination folder, then rename on success (section 2.3).

| Our format | Our recipe sketch (to be confirmed by spike 1 and the capability probe) | What changes compared with Kap and why |
|---|---|---|
| MP4 / H.264 Best | `<color in-opts> -f yuv4mpegpipe -i pipe:0 -i mix.wav -map 0:v -map 1:a -c:v libx264 -preset slow -crf Q -pix_fmt yuv420p -profile:v high -tag:v avc1 -c:a aac_at -b:a 160k -movflags +faststart OUT.mp4` | Explicit encoder, CRF and preset (Small/Balanced/High map to CRF). Color tags, avc1, faststart. `aac_at` on macOS per SPEC (native `aac` elsewhere). |
| MP4 / H.264 Fast | `... -f rawvideo -pix_fmt nv12 ... -c:v h264_videotoolbox -b:v RATE (or -q:v) -tag:v avc1 ...` | Hardware path. NV12 avoids a conversion. Check the ~4K limit (SPEC). |
| MP4 / HEVC | `... -c:v libx265 -preset medium -crf Q -pix_fmt yuv420p -tag:v hvc1 -c:a aac_at ... -movflags +faststart` (Fast: `hevc_videotoolbox -tag:v hvc1`) | **Keep `-tag:v hvc1`** (verified that it produces `hvc1`). Default audio is **AAC**, not Opus: SPEC makes AAC the MP4 default, and Opus-in-MP4 support in players varies (the probe and QuickTime test decide whether Opus/FLAC/ALAC show up). |
| MP4 / AV1 | `... -c:v libsvtav1 -preset P -crf Q -pix_fmt yuv420p ...` | SPEC picks SVT-AV1 over libaom. Kap's libaom flags (`-cpu-used 4 -row-mt 1 -tiles 2x2`) are a reasonable fallback if we ever ship libaom. The `-strict experimental` flag is probably obsolete for libaom in current ffmpeg (not verified). |
| WebM / VP9 | `... -c:v libvpx-vp9 -crf Q -b:v 0 -deadline good -cpu-used 2..4 -row-mt 1 -tile-columns N -pix_fmt yuv420p -c:a libopus -b:a 128k OUT.webm` | Replace Kap's fixed `-b:v 1M`, which ignores resolution (far too low for 4K, wasteful for small exports), with constrained quality. Keep `-deadline good` (Kap's speed/quality note matches the libvpx docs). Opus is the SPEC default. If Vorbis is chosen, use **libvorbis**, not the experimental native `vorbis`. (This Homebrew build lists only native `vorbis`, so our helper build must enable libvorbis.) Thread count: set an explicit `-threads`, plus a constrained-proxy run, in spike 1. |
| WebM / VP8 | `-c:v libvpx -crf Q -b:v RATE -deadline good ...` | Not in Kap. libvpx VP8 requires a `-b:v` ceiling alongside CRF. |
| GIF / Fast (palette) | Two passes over the pipe: pass 1 `-f yuv4mpegpipe -i pipe:0 -vf palettegen=max_colors=C:stats_mode=diff -frames:v 1 -update 1 PAL.png`, pass 2 `-f yuv4mpegpipe -i pipe:0 -i PAL.png -lavfi "[0:v][1:v]paletteuse=dither=D:diff_mode=rectangle" -loop L OUT.gif` | Kap's two-pass structure works for files. For a pipe we must either render twice (pass 1 can use reduced fps, since palette statistics don't need every frame) or use a single-pass `split` graph. Single pass buffers every frame inside ffmpeg until palettegen finishes. For example, 960x540 RGBA is ~2 MB/frame, so 60 s @ 15 fps is ~1.9 GB, which is too much for an 8 GB M1. So prefer **two passes**. `stats_mode=diff` and `diff_mode=rectangle` suit screen content (static backgrounds). The UI's "colors" and "dithering" settings map to `max_colors` and `dither`. Loop: SPEC wants forever/once/N, which is `-loop 0` / `-loop -1` / `-loop N` (N = number of **repeats**, so "play 3 times" = `-loop 2`). No even-dimension rounding. **Drop gifsicle**: it is another GPL-2.0 helper, and gifski already covers "lossy". |
| GIF / Best | gifski reads Y4M on stdin (SPEC) | Not in Kap. |
| APNG (nice-to-have) | `-f yuv4mpegpipe -i pipe:0 -pix_fmt rgb24 -plays P -f apng OUT.apng` | **[ADAPT]** `-plays 0` = forever, `-plays 1` = once (APNG counts *plays*, GIF counts *repeats*: easy to get wrong). Large files (4.4 MB for 3 s at 854x480 in the smoke test), so show a size estimate. |

**[ADAPT]** from this section: the per-format flag choices we keep (hvc1 tag, VP9 `-deadline good`
rationale, APNG `-plays` vs GIF `-loop` semantics, lanczos for downscales, palettegen/paletteuse
two-pass structure, the libaom AV1 parallelism flags if libaom is ever used).

---

## 2. Progress parsing and cancel

### 2.1 How Kap parses progress (`extractProgressFromStderr`, main/converters/utils.ts)

- Every stderr `data` chunk is decoded as UTF-8 and appended to a full `stderr` string, which is kept for error
  messages. The **chunk** (not the accumulated buffer) is then passed to the parser.
- Parser:
  - `speed` comes from regex `speed=\s*(-?\d+(,\d+)*(\.\d+(e\d+)?)?)` via `parseFloat`.
    `processed` comes from regex `time=\s*(\d\d:\d\d:\d\d.\d\d)`, converted to ms with moment.
  - If `speed <= 0` or it is missing (for example `speed=N/A` early on), the chunk yields nothing.
  - `progress = processedMs / durationMs`, where `durationMs = (endTime - startTime) * 1000`. This is the
    trimmed duration, because the output timeline starts at 0 after output-side `-ss`.
  - An ETA appears only once the encoder has processed more than 2 s of media **or** more than 15 s of wall
    time has passed. The comment says this lets speed stabilise, and the 15 s case covers slow encoders like AV1.
    `remainingMs = (durationMs - processedMs) / speed`, floored at 1 s and formatted with
    `pretty-ms` compact ("~3m").
- `Conversion.onConversionProgress` clamps progress to [0, 1] and builds text such as
  `"Converting — 3s remaining"` or `"Converting…"`. Multi-stage exports relabel the stage
  ("Cropping", "Converting", "Compressing"), but **the bar restarts from 0 each stage**. The GIF palette
  pass reports nothing, and gifsicle has no `time=`, so its stage shows no progress at all.
- On FFmpeg 9.0.2 the stats line is still `frame=… time=HH:MM:SS.cc … speed=N.NNx elapsed=…`,
  separated by `\r` (verified). Kap's regexes still match.

Weaknesses to avoid:

- **Chunk boundaries.** A stats line split across two chunks is missed, so updates are only
  occasionally lost, but it shows the approach is fragile.
- **Locale and format coupling.** Parsing depends on the human stats line. `time=` with hours ≥ 100 or a negative
  start time doesn't match. A comma in the speed value is truncated by `parseFloat`.
- **Speed from ffmpeg** is media-time/wall-time. With a pipe input, the "speed" is limited by our
  renderer as much as by the encoder. That's fine for an ETA, but it isn't encoder speed.

What we'd do:

- Run the helper with `-nostats -progress pipe:2` (or an extra fd), which emits `key=value` lines ending in
  `progress=continue|end`. Keys verified on 9.0.2: `frame`, `fps`, `out_time_us`, `out_time`,
  `dup_frames`, `drop_frames`, `speed`, `progress`. Read line by line (buffered), and use
  `out_time_us / total_us`.
- Better still, since **we** produce the frames: progress = frames written to the pipe / total frames.
  This needs no parsing and works the same for gifski and ffmpeg. Use the helper's `-progress` output only
  for the encoder-lag/flush phase at the end, and to detect `progress=end`.
- **[ADAPT]** Kap's ETA warm-up rule (show an ETA only after enough work has been done for the rate to settle, with a
  wall-clock fallback for slow encoders) and the 1 s floor on the ETA.
- Show one bar for the whole export (weight the passes, e.g. GIF palette pass 1 = 20%) instead of
  restarting at 0 for each stage.

### 2.2 How Kap cancels

- Each stage returns a `PCancelable` (p-cancelable, MIT). `createProcess` registers
  `onCancel(() => runner.kill())`. `runner` is an `execa@5` child, so `kill()` sends **SIGTERM**, and
  execa's default `forceKillAfterTimeout` (5 s) escalates to SIGKILL.
- Multi-stage functions (GIF, edit plugins) register an `onCancel` for each stage they start and keep an
  `isCanceled` flag so later stages don't start.
- `Export.cancel()` cancels both the share-plugin action and the `Conversion`. It sets the status to
  `canceled` with the text "Export canceled". Errors carrying `isCanceled` count as a cancel, not a failure.
- A non-zero exit becomes `Error("<program> exited with code: N\n\n<entire stderr>")`, which feeds the error dialog.
- `app.on('before-quit')` asks "Do you want to continue exporting?" while an export is in progress.
- **Not done:** partial output files in the temp directory are not deleted on cancel or failure. The
  palette PNG is never cleaned up.
- A failed `Conversion` resets its promise so Retry re-runs it, and conversions are cached by a hash of
  (file, format, options), so exporting the same settings twice reuses the file.

What we'd do:

- Cancel = stop writing frames → kill the helper (SIGTERM, then SIGKILL after a short grace period;
  `TerminateProcess` on Windows) → **delete `OUT.partial`**. Never close stdin as a way to
  cancel: closing the pipe tells ffmpeg the input has ended, so it finishes a valid but truncated file. ffmpeg also
  treats SIGTERM/SIGINT as "stop and finalise", so a killed run can still leave a playable partial file.
  That's another reason to write to `.partial` and only rename on success. ffmpeg's `q`-on-stdin
  doesn't work either, because stdin carries the video.
- Handle a broken pipe in the renderer (EPIPE on write) as "the helper died": read the helper's
  stderr and exit code and report them.
- **[ADAPT]** the quit-while-exporting confirmation, the "cancelled is not failed" status
  distinction, and caching by a hash of export settings (for us, a hash of project revision + export settings) so
  "Copy file" or "Open" after export doesn't re-encode.

---

## 3. Per-format fps memory, capped at source fps

Files: `main/remote-states/editor-options.ts`, `renderer/components/editor/options-container.tsx`,
`renderer/hooks/editor/use-editor-options.tsx`, `renderer/components/editor/options/slider.tsx`,
`main/video.ts`.

How Kap does it:

1. **Storage.** There is one persisted number per format (an electron-store `fps-usage-history`), and every format defaults
   to **60**.
2. **Source fps.** For a new recording it is the fps Aperture recorded at. For an opened file it is
   `Math.round(parseFloat(...))` of the "NN fps" token from `ffmpeg -i` (so 29.97 becomes 30).
3. **Effective fps when a format is selected**, both on editor load (the first format, which is the
   most-recently-used one) and on every format switch: `min(sourceFps, history[format])`.
4. **Picker.** A text field that opens a range slider with `min=5`, `max=sourceFps`, `step=1`. Typed values
   are parsed as integers and clamped into [5, sourceFps], and an invalid entry shakes the field and reverts.
5. **Persisting.** Every fps change, including the automatic capped value applied when switching
   format, calls `updateFpsUsage({format, fps})`. This is debounced by 1 s, and the pending debounce is
   flushed before switching format so the value is saved to the right format.
6. **Format order** is also remembered. Formats are sorted by a `lastUsed` timestamp per format (with seeded
   defaults GIF > MP4 > WebM > HEVC > AV1 > APNG), so the editor opens on the last format you exported.
7. **Side effect.** Switching to GIF or APNG mutes the preview, remembering whether it was already muted.
   Switching back unmutes only if it wasn't muted before.

Flaw to fix: because step 5 also persists the **capped** value, opening one 30 fps recording overwrites a
remembered 60 with 30 for that format. Later 60 fps recordings then export at 30. Memory should only change when the
user explicitly picks a value.

Our algorithm (our design; it adapts Kap's idea, not its code):

- Store `preferred_fps[format]`, which is changed **only** by an explicit user choice. Defaults:
  video formats 30 (or 60, per DECISIONS), GIF 15.
- Each format has its allowed list (SPEC: video 15/24/25/30/50/60; GIF 5/8/10/12/15/20/25/30/50).
- `source_fps` = the recording's **nominal capture rate** from project.json. Screen tracks are VFR, so
  never use a measured average, and treat 59.94 as 60 by comparing with a tolerance (e.g. ≤ 0.5%).
- `effective = max { v in list : v <= min(preferred, source_fps) }`. If nothing qualifies (source slower than
  the smallest list value), use the smallest list value ≤ the source, or the source rate itself.
- The UI shows only list values ≤ `source_fps`, and shows the effective one as selected. If the cap lowered the
  value, show a hint like "60 → 30 (recording is 30 fps)" without overwriting `preferred`.
- Persist the remembered format order and preferred fps through our settings store (local file), and flush on
  format switch and on export.
- **[ADAPT]** remembering per-format fps, capping it at the source, remembering the last used format, and the GIF/APNG
  auto-mute-with-restore behavior.

---

## 4. Aspect-locked width/height inputs (export size)

Files: `renderer/components/editor/options/left.tsx`, `renderer/components/keyboard-number-input.js`,
`renderer/utils/inputs.js` (`handleInputKeyPress`, `shake`).

Kap behavior:

- **Two text fields** (W, H) plus a **preset dropdown** of 100/75/50/33/25/20/10% of the source width.
  The label is "W x H (NN%)", and 100% is labelled "Original". If the current width matches none of them, a "Custom"
  entry is shown at the top. The dropdown's closed label is the current width as a percentage of the source.
- **Typing** only updates the field text. Nothing is applied until **blur or Enter**. Arrow Up/Down step the text by
  ±1, or ±10 with Shift, clamped to [1, source]. Like typing, they don't update the other field until commit.
- **Commit** (`onBlur`):
  1. The text is evaluated with `string-math` (MIT), so `1920/2` or `720*1.5` work. The result is rounded with
     `Math.round`.
  2. `ratio = source.width / source.height`, from the **source** (the recording), which is always locked; there is no unlock.
  3. Width typed: `min = max(1, ceil(ratio))`, so the derived height is ≥ 1, and `max = source.width`
     (**never upscales**). Out of range: clamp and shake. Then
     `height = ratio > 1 ? ceil(width / ratio) : floor(width / ratio)`.
  4. Height typed: `min = max(1, ceil(1 / ratio))`, `max = source.height`. Then
     `width = ratio > 1 ? floor(height * ratio) : ceil(height * ratio)`.
  5. Non-numeric or empty input: shake and revert both fields to the last valid size.
- **Evenness** is not handled in the UI at all. It is applied later in the converter with
  `makeEven = 2*round(n/2)` for MP4, HEVC, AV1 and WebM only (not GIF/APNG). So the dialog and the
  "W x H at N FPS" export description can be 1 px off from the file. Rounding each dimension separately
  can also shift the aspect ratio by up to 1 px, and for an odd source dimension `makeEven` can round **up** past the
  source (e.g. 1919 → 1920), which is a 1 px upscale.

What we'd do (SPEC: Original, height presets, type W or H, even except GIF, no upscale by default):

- **[ADAPT]** commit-on-blur/Enter, math expressions in the field, ±1/±10 arrow stepping,
  clamp-and-shake feedback, reverting on invalid input, and deriving min from the ratio so the other side is ≥ 1 (≥ 2 for even formats).
- Presets: replace Kap's percentages with SPEC's **height** presets (2160/1440/1080/720/480, plus
  360/240 for GIF). Hide presets taller than the source unless "Allow upscale" is on. GIF additionally
  defaults to a maximum width of 960.
- Do the rounding **in the geometry module**, and show its result exactly:
  - Driving dimension typed (say H): clamp to [min, max]. For 4:2:0 formats, round it to even first, then
    derive the other as `round_to_even(H * ratio)`, rounding to the nearest even number, so no orientation-dependent ceil/floor
    is needed. For GIF/APNG/ProRes use `round(H * ratio)` without even rounding.
  - Never let the even rounding exceed the source when upscaling is off (round down to even at the max).
  - Hardware encoder limits (e.g. H.264 at about 4096 wide) come from the capability probe and clamp `max`.
- `ratio` is the **output canvas** aspect (after the aspect preset and crop: 16:9, 9:16, 1:1, ...), not
  the raw recording.

---

## 5. Cropper aspect-lock behavior (region selection)

Files: `renderer/containers/cropper.js`, `renderer/containers/action-bar.js`,
`renderer/utils/inputs.js` (`handleWidthInput`, `handleHeightInput`, `RATIOS`, `resizeTo`,
`buildAspectRatioMenu`), `renderer/pages/cropper.js` (keys), `renderer/components/cropper/handles.js`.
This applies to our capture-region picker and to the editor's crop/pan for aspect presets.

Kap behavior:

- **State:** `{x, y, width, height, ratio: [rw, rh], ratioLocked}`. The ratio lock lives in the action
  bar, and both are saved to settings, so the region and lock survive restarts. The default ratio is 4:3.
  Minimum size is **20x20**, and the region always stays inside the screen.
- **Ratio menu:** 16:9, 5:4, 5:3, 4:3, 3:2, 1:1, plus "Custom r:r", which is shown checked when the
  current ratio isn't one of the presets. Choosing a ratio keeps the width, sets `height = ceil(width*rh/rw)`,
  and clamps it to [20, screenHeight]. If clamped, it recomputes `width = ceil(height*rw/rh)`, then **turns the lock on**.
- **Ratio when unlocked:** after any size change, the ratio is re-derived with
  `nearest-normal-aspect-ratio` (MIT). This finds the closest `j:i` with both terms ≤ 16, so 649x360 → 9:5. Kap
  has a GCD fallback, but it is dead code because that library always returns a value. When you then lock,
  the locked ratio is this approximation, so the next locked drag can snap the box by a few pixels.
- **Swap button:** swaps W and H (height clamped to the screen) and reverses the ratio.
- **Modifier keys:** while **Shift** is held, the lock is temporarily forced on, and the previous lock state is restored on keyup.
  Shift typed inside the W/H fields calls `stopPropagation` so it doesn't toggle the lock. **Alt** = resize from
  center. **Esc** closes the cropper.
- **Handles:** when locked, the four edge handles are **hidden**, so only corners resize. This avoids a
  1-D drag that has to move the other axis.
- **Locked resize algorithm** (`resize`):
  1. First compute the unlocked candidate rectangle from the active handle and cursor (with center mode
     growing both sides). When unlocked, dragging past the opposite edge flips the handle. When locked, it
     doesn't flip on a single axis, and only flips both if both went negative.
  2. Decide which axis "leads" by comparing the change from the drag-start rectangle, scaled by the ratio:
     `(dW) * rh > (dH) * rw` means width leads.
  3. Derive the other axis with `ceil(lead * other/lead_ratio)`. Re-anchor it so the edge opposite the
     dragged handle stays fixed (top handle moves y; left handle moves x). In center mode it re-centers.
  4. If the derived side would go past the screen edge, shrink it to fit and **re-derive the leading side** from
     the clamped one (`ceil`), re-anchoring x/y. Center mode shrinks symmetrically (both sides by the overflow).
  5. Clamping to the 20 px minimum happens on mouse-up (`stopResizing`), not during the drag.
- **Typed W/H** (action bar): debounced **500 ms**, and blur/Enter flush immediately. Digits only. The value is
  clamped to [20, screen] with a shake if clamped. If locked, the other side = `ceil(v * ratio)`, and if that
  side gets clamped, both fields shake and the typed side is re-derived. `resizeTo` keeps x/y but pulls the box back
  inside the screen. When unlocked, the other side is left unchanged.
- **Move:** drag the box. Each axis updates only if the box would stay fully on screen, so the box stops at the edge
  instead of being clipped.
- **Window picking:** selecting an app window sets the bounds to the window, ignoring the lock, and re-derives
  the ratio.
- Fullscreen toggle: remember the rectangle, use the full screen, hide handles, and restore on exit.

What we'd do:

- **[ADAPT]** the interaction model: ratio presets + Custom, swap, temporary lock while Shift is held, Alt for
  center resize, corner-only handles when locked, leading-axis selection by ratio-scaled change from drag start,
  clamp-then-re-derive at screen edges, move that stops at the edges, debounced typed W/H with
  shake feedback, and persisting the region and lock.
- Changes:
  - Keep the locked ratio **exact**: store the user's chosen preset as integers. When locking from a free
    shape, either lock the exact current W:H (rational) or snap to the nearest "normal" ratio *and* adjust the
    box immediately so there is no surprise jump on the next drag.
  - Use consistent rounding (the geometry module rounds once, at the end, to even pixels in *capture*
    pixels, not points). Kap works in CSS/point coordinates, but we must convert to backing pixels for
    ScreenCaptureKit's `sourceRect`, and to even sizes for the encoder.
  - Enforce the minimum size during the drag as well, not only on mouse-up.
  - On Linux/Wayland (SPEC) this UI runs over a frozen frame, and the same geometry code applies.

---

## 6. How Kap ships GPL FFmpeg, and what notices it includes

- **Source of the binary:** the npm dependency `ffmpeg-static@^4.4.1` (locked at 4.4.1 in yarn.lock).
  Its `install.js` runs at `npm/yarn install` time and **downloads** a prebuilt static binary from the
  GitHub release `eugeneware/ffmpeg-static` tag `b4.4.1` (`<platform>-<arch>.gz`). It writes it to
  `node_modules/ffmpeg-static/ffmpeg` and marks it executable. It also tries to download
  `<platform>-<arch>.README` and `<platform>-<arch>.LICENSE` next to it as `ffmpeg.README` and
  `ffmpeg.LICENSE`. On a 404 it only **warns** and continues, so a build can end up without them. The package
  README says the macOS builds originally come from evermeet.cx (Intel) and osxexperts.net (Apple Silicon).
  `index.js` honours `FFMPEG_BIN` to override the path.
- **ffmpeg-static's own license:** `package.json` says `GPL-3.0-or-later`, but the package's
  `LICENSE` file contains a BSD-style text. That contradiction is a good example of why we must state our helper
  licenses ourselves rather than inherit them.
- **The binary is necessarily a GPL build:** Kap's recipes call `libx265` (and MP4 defaults to `libx264`),
  and both require `--enable-gpl`.
- **How it is run:** `main/utils/ffmpeg-path.ts` passes the package path through
  `electron-util.fixPathForAsarUnpack` (rewriting `app.asar` to `app.asar.unpacked`). Every use is a separate
  process started with `execa(ffmpegPath, args)`, so it is an "aggregate" separate program and never linked. Kap's
  `build` config has no explicit `asarUnpack`. It relies on electron-builder's automatic unpacking of
  native binaries (I didn't build Kap to confirm what ends up in `app.asar.unpacked`).
- **A second GPL helper:** `gifsicle@5.2.0`. The npm wrapper is MIT, but the gifsicle program is GPL-2.0. It downloads a
  prebuilt binary from `imagemin/gifsicle-bin` at install time, or compiles the bundled
  `gifsicle-1.92.tar.gz` if the downloaded binary fails `--version`.
- **Notices Kap ships:** none of its own. There is no THIRD_PARTY/NOTICE file in the repo, no mention of
  FFmpeg or gifsicle licensing in the README, and "About Kap" is Electron's default `app.showAboutPanel()`.
  The only FFmpeg notice that ships is whatever `ffmpeg.README` / `ffmpeg.LICENSE` ffmpeg-static managed
  to download into `node_modules` (and therefore into the unpacked app resources). There is no corresponding
  source or written offer from Kap itself. It relies on the upstream builders' pages.
- **Also note:** the binary is fetched over the network at *dependency install time*, and its version is whatever
  that release tag holds (not checksum-pinned in Kap's repo; yarn.lock only pins the npm tarball,
  not the downloaded binary).

What we do differently (already in SPEC, restated as requirements this research supports):

- Build the macOS helper from pinned source with a pinned script, and verify downloads by **checksum**
  (`third_party/`). The binary is a Tauri sidecar inside the signed `.app`, and it is signed with the same identity under the hardened
  runtime.
- Ship next to each helper: the license texts (GPL-2.0/3.0 as applicable, plus each component library's),
  `ffmpeg -version` / `-buildconf` output, the exact source tarballs or URLs + checksums, and our build
  script. Surface all of this in About → Licenses. Never `--enable-nonfree`.
- Don't ship gifsicle. Use gifski (AGPL, a separate helper, built from source) for best quality and FFmpeg
  palettegen for Fast.
- Don't parse `ffmpeg -i` stderr for metadata. Use our own probe (we know our recordings' metadata from
  project.json anyway).

---

## 7. Adaptation list (for docs/THIRD_PARTY.md)

Everything below comes from Kap (MIT, Copyright (c) Wulkano). The attribution entry has been appended to
`docs/THIRD_PARTY.md`.

| # | What | Kap source | Where it will land (planned) |
|---|---|---|---|
| K1 | FFmpeg argument recipes and their rationale: GIF palettegen/paletteuse two-pass + lanczos, HEVC `-tag:v hvc1`, VP9 `-deadline good`, AV1 libaom parallelism flags, APNG `-plays` vs GIF `-loop` semantics | main/converters/h264.ts | crates/export (recipe builders), docs/CODEC_MATRIX.md |
| K2 | Conditional argv builder pattern | main/converters/utils.ts (`conditionalArgs`) | crates/helpers or crates/export |
| K3 | Progress/ETA heuristic: warm-up before showing an ETA (media-time or wall-time threshold), ETA = remaining / speed, 1 s floor; cancel = kill child + treat cancellation as its own status | main/converters/utils.ts (`extractProgressFromStderr`), main/converters/process.ts, main/conversion.ts, main/export.ts | crates/helpers, ui export progress |
| K4 | Per-format fps memory capped at source fps; last-used-format ordering; GIF/APNG auto-mute with restore | main/remote-states/editor-options.ts, renderer/components/editor/options-container.tsx | crates/core (settings), ui export panel |
| K5 | Aspect-locked size inputs: commit on blur/Enter, math expressions, ±1/±10 arrows, clamp + shake, revert on invalid, ratio-derived minimum | renderer/components/editor/options/left.tsx, renderer/components/keyboard-number-input.js, renderer/utils/inputs.js | ui export panel, crates/render geometry |
| K6 | Cropper aspect-lock interaction model: ratio presets + Custom, swap, Shift temporary lock, Alt center-resize, corner-only handles when locked, leading-axis rule, clamp-then-re-derive, edge-stopping move, debounced typed W/H | renderer/containers/cropper.js, renderer/containers/action-bar.js, renderer/utils/inputs.js, renderer/pages/cropper.js, renderer/components/cropper/handles.js | ui region picker, crates/render geometry |
| K7 | Even-size helper idea (`makeEven`), applied in the geometry module rather than the converter | main/converters/utils.ts | crates/render geometry |

Kap's small npm dependencies whose behavior we described: `nearest-normal-aspect-ratio@1.2.1` (MIT),
`string-math@1.2.2` (MIT), `p-cancelable@2.1.0` (MIT). If we reimplement "nearest normal ratio" or
expression evaluation, we'll write our own (or use a permissive Rust/TS crate) and note it then.

## 8. Not done / open items

- Kap's recipes were not run with its own FFmpeg 4.4.1, only with Homebrew 9.0.2. The AV1 (libaom)
  recipe was not run at all, because that build has no libaom.
- No performance numbers here. The wall times in 1.2 are single smoke runs on an M3 Pro with all
  threads, on a 3 s clip, and must not be read as encoder benchmarks. Spike 1 owns real measurements
  (M3 Pro raw + constrained proxy, with the M1 8 GB caveat).
- Opus/FLAC/ALAC-in-MP4 playback in QuickTime was not tested. The capability probe and the DoD
  playback tests decide which ones appear.
- Which color primaries to tag (BT.709 vs Display P3) is a decision for DECISIONS.md, not this note.
