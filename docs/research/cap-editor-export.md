# Research: Cap's editor, project model, and export

M0 research note for Simple Capture. Source: `reference/Cap` at commit `880ec89`. Read on 2026-11-19.

Everything below is my own description of how Cap works. I copied no code from Cap. A few numeric constants, such as fade lengths and buffer sizes, are quoted as facts because the design depends on them. Paths are relative to `reference/Cap/`.

## Licenses

| Crate / app | Path | License |
|---|---|---|
| cap-editor | `crates/editor` | AGPL-3.0. Its Cargo.toml has no `license` field and the folder has no LICENSE file, so the repo-root `LICENSE` applies: only `cap-camera*` and `scap-*` are MIT. |
| cap-project | `crates/project` | AGPL-3.0 (same reasoning) |
| cap-export | `crates/export` | AGPL-3.0 (same reasoning) |
| cap-audio | `crates/audio` | AGPL-3.0 (same reasoning) |
| cap-rendering (read only for camera and text context) | `crates/rendering` | AGPL-3.0 |
| desktop-gpui app (read only for timeline edit and thumbnail behavior) | `apps/desktop-gpui` | AGPL-3.0 |
| cap-enc-gif (glanced at) | `crates/enc-gif` | AGPL-3.0. It links gifski in-process, and gifski is itself AGPL. |

Consequence: nothing in this area may be adapted. We can only reimplement the ideas. I wrote no `docs/THIRD_PARTY.md` entry because nothing was adapted.

---

## 1. Project and EDL model

Files: `crates/project/src/configuration.rs`, `crates/editor/src/segments.rs`, and `apps/desktop-gpui/src/editor_edits.rs` for the edit operations.

### Recording clips and timeline segments
- A recording is one or more **recording segments**. Each pause/resume starts a new one, with its own display, camera, mic, and system-audio files. The edit model calls them "recording clips" and refers to them by index.
- Each recording clip has a **clip configuration** with per-track sync offsets in seconds: camera, mic, and system audio. A flag records whether the offsets were computed automatically from start-time alignment and device calibration, or entered by the user.
- The EDL is `TimelineConfiguration.segments`, an ordered list of **timeline segments**. Each one has:
  - the recording-clip index;
  - source `start` and `end` in seconds, in that clip's own time;
  - `timescale` (the speed). The editor allows 0.25x to 8x. Values that are invalid or below 0.01 load as 1x, so a damaged project can't hang playback.
  - an optional name;
  - an audio mode for sped-up clips: mute (the default), keep pitch, or follow speed (chipmunk);
  - optional per-clip volume, a linear multiplier clamped to 0 to 2;
  - an optional "hide cursor" flag;
  - optional per-clip voice EQ.
- There is no explicit per-clip mute. Volume 0 serves that purpose for recorded audio, and sped-up clips default to muted audio.
- Order in the list is playback order, so reordering is just list order. A split produces two segments whose source ranges touch. A delete removes a segment.
- On load, Cap clamps any clip that runs more than 60 s past its source's end. This guards against damaged projects rendering billions of frames.

### Mapping timeline time to source time
- A segment's output duration is (end − start) / timescale.
- To map an output time *t*, walk the segments and keep a running total of output durations. The segment that contains *t* gives source time = segment.start + (t − segment output start) × timescale.
- It's a linear scan on every query, and the same mapping is called per frame by playback, prefetch, audio, thumbnails, and export.
- Total duration = the sum of segment output durations, minus the overlap of every transition, plus any "hold" windows (see below).

Cap has two extras that we don't need, but they show how the mapping generalizes:
- **Transitions** between adjacent clips (cross-fade, dip to black or white, push, wipe, zoom). The two clips overlap by the transition's duration. The duration is clamped to at least 50 ms and at most half the shorter clip. Inside the overlap, the mapping returns both the outgoing and incoming source times plus a 0 to 1 progress value.
- **Holds**: a full-screen text card pauses the recording clock. The mapping returns a frozen source frame and silent audio for that window, and later times shift back by the held amount.
- The mapping result is a small enum: a single source, a transition (two sources plus progress), or a hold. Each variant also carries the output time at which the current piece ends. Audio uses that end time to render in chunks that never cross a cut.

### Output-time tracks and ripple
- All other tracks live in **output (edited-timeline) time**: zoom, scene, mask, text, captions, keyboard, music/imported audio, style overrides, images, shapes, and 3D camera. Each is a list of `{start, end, …}` items, and most have a `track` (lane) index.
- The desktop-gpui editor applies a ripple rule to every output-time track when a clip is deleted or a time range is cut. Measured against the cut range [a, b), each item is handled like this:
  - before the cut: kept as is;
  - entirely inside the cut: removed;
  - after the cut: shifted earlier by the cut length;
  - spanning the whole cut: shortened by the cut length;
  - overlapping only the start of the cut: trimmed to end at a;
  - overlapping only the end of the cut: starts at the cut point, with what remains of it shifted earlier.
- Inserting at a boundary shifts later items the other way.
- A comment in `configuration.rs` (near `hold_windows`) says clip **trims** do not ripple text. Ripple only happens for range cuts and clip deletes.
- Cut pieces shorter than one 60 fps frame (1/60 s) are dropped so playback never seeks into a sliver.

### Persistence
- `project-config.json` is saved atomically: write a temp file in the project folder, fsync it, then rename it over the original. Cap validates the project before writing.
- Per-recording metadata lives in a separate file (`meta.rs`, which I didn't read in depth).

---

## 2. Playback engine

Files: `crates/editor/src/playback.rs`, `audio_output.rs`, `audio.rs`, `preview_stretch.rs`, `crates/rendering/src/decoder/*`, and `crates/editor/PLAYBACK-FINDINGS.md`.

### Clock: video wall clock, with audio following it
**Cap's current design is not audio-driven.** Our spec says audio should drive the clock, so this is the most important finding in this section.

- The playback thread keeps a monotonic `Instant` anchor and an anchor frame. The deadline for frame N is anchor + (N − anchor frame) / fps / speed.
- It sleeps until each deadline with a "precision sleep": a coarse sleep followed by a short spin. On Windows it uses a high-resolution waitable timer and raises thread priority, because a normal-priority wake arrived up to 10 ms late.
- After each frame, the playback thread publishes the playhead in seconds on a watch channel.
- The audio callback compares that playhead with its own **audible** position. The audible position is the rendered sample position minus the measured output-device latency. If the two differ by more than **40 ms** at 1x, the audio re-seeks to the video playhead plus the device latency. At faster preview speeds the tolerance scales up.
- Audio is always rendered one device latency ahead of when it is heard. Latency is estimated per transport in `crates/audio/src/latency.rs`:
  - wired: about 30 ms fallback;
  - Bluetooth: at least 120 ms;
  - AirPlay: at least 1.8 s;
  - smoothing: increases are adopted fast, decreases slowly.
- The clock doesn't start until the audio callback has confirmed it is consuming the new source. That way video never runs ahead of audible audio at startup.
- One cpal output stream lives for the whole editor session and plays silence while paused. Pressing play installs a new source into the running callback instead of opening a device, which avoids slow Bluetooth wake-ups. The built-in output's stream is paused after 15 s idle; other devices keep running.

**Why this matters for us:** with video as the master clock, dropped or late video frames never cause audio glitches. Audio corrects itself in 40 ms steps. Slaving video to the audio clock is the alternative; it gives sample-accurate A/V, but you have to handle having no audio device or no audio tracks. That decision belongs in DECISIONS.md. Proposal: drive the clock from the cpal output position when an output device exists, and fall back to a wall clock otherwise. Keep Cap's "re-seat if drift > ~40 ms" as a safety net.

### Prefetch and decoding across cuts
- A separate async **prefetch producer** works in *timeline frame numbers*. For each future frame, it maps timeline time to (clip, source time) with the same EDL mapping and asks that recording clip's decoders for the frame.
  - Because it works in timeline frames, reading ahead across a cut is automatic. The frame after a cut simply maps to a different source position or clip, and its decode is already in flight before the playhead gets there.
  - During a transition it decodes both sources.
- How far ahead it reads:
  - **Runway:** at least 10 and at most 90 frames ahead of the playhead. The limit is recalculated from the largest frame seen so the queued frames fit in 128 MB.
  - **Boost:** when playback has to skip because decode starved, the budget rises to 192 MB, up to twice the frame count, for 6 s, then shrinks back.
  - **Parallelism:** up to 4 decode tasks in flight. Frames go to the playback thread over a bounded channel 4 deep.
  - **Small cache:** a 4-frame, 64 MB cache next to the playhead.
- **Seek generations:** every seek bumps a counter. Frames tagged with an older generation are thrown away. Short forward jumps keep in-flight decodes; backward seeks and long jumps restart the pipeline. There's a hard-won lesson in a code comment: skip paths must never send the prefetcher a frame number *behind* its read-ahead. The prefetcher treats that as a backward seek and drops all in-flight work, and the result is a cascade of more gaps and more skips.
- **Warm-up:** before the clock starts, the loop waits for 10 decoded frames (30 when starting mid-timeline), or 500 ms/800 ms after the first frame arrives, with a 5 s cap. Cap also renders the first frame before starting the clock.
- **Skip policy:**
  - If the loop wakes more than 1.5 frame durations past a deadline, it jumps ahead by the number of frames it is behind.
  - If it falls more than 2 frames behind the wall clock, it jumps ahead as well.
  - If the wanted frame hasn't arrived, it waits 8 ms (20 ms during the first 15 frames), then skips that frame.
  - Every skip is counted and reported through telemetry.
- **Decoders:** per recording clip, the macOS path uses AVAssetReader through a **multi-position decoder pool**. There are 5 decoders, or 7 to 8 for timelines over 10 or 30 minutes, each parked at a different position, and a decoder is repositioned when a request is more than 5 to 10 s from it. This keeps scrubbing and cut-hopping from paying a full seek each time. Decoded-frame caches are capped at 90 frames and 128 to 256 MB depending on the backend.
- Frame requests snap to the source's frame grid with a small allowance for floating-point error, so a request for frame *n* at *n/fps* never returns frame *n−1*.

### Preview speed and per-clip speed
- **Global preview speed** (the J/K/L-style transport speed) is changed in the audio callback with a WSOLA time-stretcher: 15 ms hops with Hann windows, a 12 ms search, 0.25x to 4x. At 1x it gets out of the way entirely, so 1x output is bit-exact. When speed changes, the difference in audio lead is caught up over 0.5 s rather than by seeking.
- **Per-clip speed** in the audio mix uses FFmpeg filters in-process. Keep-pitch chains `atempo` stages in the 0.5 to 2 range. Follow-speed resamples the clip, which shifts pitch. Each processor starts 50 ms (2,400 samples) early so the filter has history.
- Our spec uses signalsmith-stretch instead. It's permissive and needs no FFmpeg dependency in the app.

### Reported performance
These are Cap's own figures from `crates/editor/PLAYBACK-FINDINGS.md`, from their machines. **I did not measure them.**
- Decode: p95 1.4 ms at QHD and 4.3 ms at 4K.
- Decoder start-up: up to about 337 ms, because the pool opens several decoders.
- GPUI editor playback: 60 fps with no drops at about 17.5% CPU and 440 MB RSS for a 1248×702 preview.

The findings also record that sending RGBA to the preview was cheaper than NV12 at 1080p, and that half-resolution preview cut transport bandwidth from about 407 MB/s to about 173 MB/s.

---

## 3. Audio mixing path

Files: `crates/audio/src/{audio_data.rs,renderer.rs,latency.rs,progressive.rs}`, `crates/editor/src/{audio.rs,segments.rs,join_fade.rs,export_audio.rs}`.

- **Decode:** every source track (mic, system audio, imported music) is decoded with FFmpeg into **48 kHz interleaved f32**, resampled with a high-quality setting, and kept in memory. The cap is 8 hours of stereo. Decoding starts in the background when the editor opens, and playback start awaits it. A track that fails to decode becomes "no audio" with a warning instead of stopping playback. Export validates tracks strictly.
- **Mix bus:** always 48 kHz stereo f32.
  - Each track has a gain in dB (mic and system volumes are separate project settings), a stereo mode (stereo, left-only mono, right-only mono), and a sample offset from the clip's sync offsets.
  - A mono source is copied to both channels at about −3 dB (×0.707), so it is equal-power centered.
  - Tracks are summed, the per-clip linear volume is applied, and the result is clamped to [−1, 1]. Cap uses a hard clamp, not a limiter.
  - There is also an optional voice-enhancement and EQ stage and a noise-suppression model (rnnoise). Our spec doesn't need these.
- **Rendering in output time:**
  - The audio renderer keeps a cursor (clip, source sample, timescale).
  - For each requested block, it asks the EDL mapping which clip covers the output position and up to where, then renders only up to that boundary.
  - Contiguous split halves continue without a seek. Speed-changed clips go through the stretch processor.
  - Cut fades are applied next (§4), and timeline music is mixed in after that with linear fade-in/out ramps.
- **Playback buffering:**
  - If the whole timeline mix fits in **512 MB** (48 kHz × 2 ch × 4 B is about 384 KB/s, so roughly 22 minutes), a background thread renders it progressively. It starts at the playhead with a small pre-roll, then wraps around to cover the start, and the callback reads silence for any part not yet rendered.
  - Longer timelines use a streaming ring buffer.
  - Cached renders are freed after 120 s idle and before exports.
  - **This is heavy for an 8 GB M1**, and source tracks are also held fully decoded: 30 min of stereo f32 is about 690 MB per track. We should stream source audio in chunks and render the mix just ahead of the playhead.
- **Export audio:**
  - Audio is rendered per output video frame with a whole-sample budget: frame N owns the samples up to floor((N+1) × 48000 / fps). Audio length always matches video length exactly, and no rounding drift builds up.
  - It runs on its own thread next to video encoding, and video waits if audio falls more than 2 s behind. That keeps the muxer's interleaving deterministic.

---

## 4. Cut fades (`crates/editor/src/join_fade.rs`)

- **Where:** only at *hard cuts*, meaning output positions where playback jumps to a clip that doesn't continue the previous one's source. Two segments with the same recording clip whose source ranges touch (within 1 µs) are *not* faded, even if their speeds differ. Clips joined by a transition aren't faded either, because the transition already blends them. Join positions are computed in output samples, after accounting for transitions and holds.
- **Length:** **240 samples at 48 kHz, which is 5 ms on each side of the cut.** The comment's reasoning: long enough that the jump between unrelated waveforms can't click, and short enough not to dip speech audibly.
- **Curve:** a raised-cosine (half-Hann) ramp. Gain is 0 at the join and rises smoothly to 1 at 240 samples away, on both sides: fade-out before the cut, fade-in after it. The two sides don't overlap, so it's a brief **dip to silence**, not a crossfade.
- **Properties:**
  - It's applied after mixing to the stereo output buffer, so every track gets the same fade.
  - It's a pure function of absolute output-sample position, so the result doesn't depend on how the audio is chunked. Cap tests this.
  - The largest sample-to-sample gain step stays below 0.01.
- **Transitions** (which we don't need): dip transitions go through silence; all others use an equal-power crossfade.

For us: 5 ms per side with a raised-cosine curve is a sensible default. Our spec's "short automatic audio fades at every cut" matches it. Keep the rule that continuous splits aren't faded. Without it, splitting a clip and then changing one half's volume or speed would add an audible dip. If we decide to fade on speed changes, document that.

---

## 5. Thumbnails and waveforms

- **Project/library thumbnail** (`crates/editor/src/thumbnail.rs`):
  - It shows the *first frame of the edited timeline*, put through the full compositor (camera, cursor, styles) and fitted into a 640×640 box, rendered as if at 30 fps. It's saved as JPEG quality 75 to `screenshots/preview.jpg`.
  - The project configuration is serialized as a "signature", with fields that never affect frames (markers) left out, and saved next to the image. A thumbnail is re-rendered only when the signature changes.
  - Only one thumbnail renders at a time, with a 5 s timeout. The write uses temp files plus rename, and the signature is checked again just before the rename so a concurrent edit can't be overwritten.
- **Clip cards** (`apps/desktop-gpui/src/editor_clips.rs`): one frame per clip at the clip's source start. It's decoded with FFmpeg off the UI thread and scaled so its longest side is 192 px. Each decode has a budget of 4,096 packets and 2 s, at most 2 run at once, and results are cached in memory by (recording clip, start ms).
- **Timeline strip:** desktop-gpui's timeline draws **waveforms but no per-clip filmstrip**. I found a filmstrip only in Cap's web share page (`apps/web/.../timeline/useFilmstripFrames.ts`), which I didn't study. Our spec asks for a thumbnail strip, so we have to design one ourselves (see §9).
- **Waveforms:**
  - Peaks are the mean absolute value over **100 ms blocks**, converted to dB with a −60 dB floor (`crates/audio/src/progressive.rs`, `crates/editor/src/audio_levels.rs`). They're computed per source track in the background and can be cancelled.
  - The timeline maps each clip's source range through its speed, draws at about 2 samples per pixel (50 to 6,000 points per clip), and scales the drawn height by the clip's gain in dB. At −30 dB or below it draws nothing.

---

## 6. Export pipeline (`crates/export`)

### Formats in Cap
- MP4: H.264 + AAC.
- GIF: gifski, linked in-process.
- MOV: a ProRes-style intermediate, plus a "cursor-only" alpha export.

There's no WebM, HEVC, AV1, or Opus export. Settings are tagged by format, and each format has its own fps and output size.

### Standard MP4 export
1. Build an "exporter base": the project, recording metadata, decoders per recording clip, and render constants.
2. Total frames = ceil(timeline duration × fps). The output size comes from the requested base resolution and the project's aspect and padding.
3. A render task walks output frames 0..N. For each one it computes t = N / fps, maps t through the EDL, decodes from the right clip (or both clips in a transition), and renders on the GPU **straight to NV12**. On macOS it can hand VideoToolbox a zero-copy input.
4. Frames go over a **bounded channel, 4 deep**, to an encoder thread, so a slow encoder pushes back on rendering.
5. A separate audio thread renders and AAC-encodes each frame's audio budget (§3), and the muxer interleaves by timestamp.
6. Encoders:
   - The default is the hardware H.264 encoder.
   - On a hardware error, the export **restarts once on libx264**, except for estimate samples and cancelled exports.
   - With "optimize file size" on, libx264 runs in CRF mode.
7. Error priority: an audio-source failure is reported first, then other failures, then cancellation.
8. Frame-receive timeouts are 120 s for the first frame and 90 s per frame after that, giving up after 3 in a row.
9. Output safety:
   - Cap exports to a staging file. It claims the destination name with an exclusive create, then renames the export over that empty placeholder, so it never overwrites an existing file.
   - The exclusive-create step works around exFAT and some network volumes on macOS, which don't support exclusive rename or hard links.
   - "Turbo" stages its output; the standard path checks the destination is still free before falling back.
10. Progress is a per-frame callback that returns whether to continue, which is how cancel works.

### "Turbo" MP4 (`crates/export/src/mp4/turbo.rs`)
- The timeline is split into up to **4 contiguous chunks**, each at least 4 s long, with about 4 cores per chunk. Each chunk gets its own decoders, render loop, and libx264 encoder.
- Chunk 0 writes the output directly. The other chunks spool packets to temp files, which are appended in order.
- Each later chunk renders and throws away a few frames first, so state that carries from frame to frame matches at the seams.
- Cap's comment says that on Apple Silicon this is 2 to 3× faster than the single hardware encoder at matching quality.
- If turbo fails, the export falls back to the standard pipeline.
- **This is relevant to us:** it's the same idea as running several x264 helper processes over chunks and concatenating the streams. On a base M1 the gain over one well-threaded x264 is unknown; the helper spike should measure it.

### Quality presets
Four presets map to **bits per pixel**: Maximum 0.30, Social 0.15, Web 0.08, Potato 0.04. A custom bpp can override them.
- **Hardware bitrate** = width × height × bpp × effective fps, where effective fps = min(fps, 30) + 0.6 × max(fps − 30, 0). Frames above 30 fps get a tapered share of the bitrate.
- **libx264 CRF** per preset: 24, 28, 32, 36. Turbo adjusts CRF by frame rate, about 3 steps better at 60 fps and 2 steps worse at 15 fps. Cap's comment says these values matched or beat the hardware encoder's SSIM/PSNR at a smaller size against a lossless render.

### GIF
- gifski with only **quality** (1 to 100, default 90) and a **fast** flag. Loop is always infinite.
- Frames are rendered at the export fps and resolution and passed to gifski's collector in order.
- There is no motion-quality or lossy setting, no palette mode, no N-loop option, and no default width limit.

### Estimates (`crates/export/src/estimates.rs`, `preview.rs`, `EXPORT-FINDINGS.md`)
Cap has two estimators.

**A. Instant analytic estimate.** It's shown with a single rendered preview frame.
- Size ≈ (video bitrate + 192 kbps audio) × 0.5 × duration / 8. The video bitrate is the bpp formula above, and the 0.5 is an empirical H.264 efficiency factor.
- GIF size ≈ width × height × 0.035 bytes per frame × frame count. Cap notes this isn't calibrated against real recordings.
- Time = frames / a measured render+encode rate. Cap's figures from their own benchmark machines: about 290 fps at 1440p or below, about 175 fps at 4K.
- Cap reports this was within about 3% on average for MP4 size, with a worst case of −15% at low bpp and 720p. (Their numbers, not mine.)

**B. Sampled estimate.** This is the current, more accurate one. It actually encodes short windows of the real timeline with the real settings, records a timestamp for every frame and the size and keyframe flag of every packet, and extrapolates.
- **Sample plan by encoder:**
  - Hardware H.264: 4 windows of max(fps/2, 32) frames. One window starts at frame 0 to absorb the encoder's warm-up; the others are centred in equal slices of the timeline.
  - libx264 or turbo: 3 windows, each one GOP plus 30 warm-up frames long, because x264 stalls once per GOP on its keyframe decision.
  - GIF: one 60-frame window timed end to end, because gifski buffers about 30 frames and does the rest when it finishes.
  - If the timeline is shorter than the sample, it's simply encoded in full and the result is exact.
- **Time:**
  - Per window, the steady seconds per frame skip the first 8 frames (30 for x264) and the last 2. For GIF the time is amortized over the whole run.
  - Estimated time = a fixed cost + seconds per frame × total frames. The fixed cost is the start-up before the first sampled frame plus the flush after the last; the seeks between windows aren't counted.
  - The reported range is the spread across windows: mean ± standard deviation, widened to include every window's value.
- **Size:** a keyframe/delta model.
  - Key spacing comes from the configured GOP, or from the measured spacing if the encoder ignores the GOP (VideoToolbox uses FFmpeg's default 12-frame GOP). In that case the first GOP is skipped because rate control over-spends there.
  - Average keyframe size and average delta-frame size are fitted from the packets. The first packet after a jump between windows is excluded because it carries a whole new picture.
  - Size = key bytes × ceil(frames / spacing) + delta bytes × the remaining frames, plus non-video bytes (audio and container) per frame × frames.
  - The range comes from fitting each window separately.
- **Plumbing:** only one sampling runs at a time. It stops when a real export starts. Results are cached by a hash of the project (ignoring markers) plus the settings, and partial estimates stream to the UI as windows finish.

---

## 7. Feature checklist: Cap's Camera overlay model

Sources: `Camera` in `crates/project/src/configuration.rs`, used in `crates/rendering/src/lib.rs`.

- **hide**: on/off.
- **mirror**: on/off.
- **position**:
  - Preset anchor: x ∈ {left, center, right} × y ∈ {top, bottom}, default bottom-right. There's no vertical center.
  - **manual_position**: an optional normalized (0 to 1) *center* in output-frame space that overrides the preset. It's clamped so the camera stays fully inside the frame.
  - Preset anchors keep a padding margin from the edges.
- **size**: a percentage of the output's *shorter side* (default 30).
- **zoom_size**: the size used while a screen zoom is active (default 60).
- **scale_during_zoom**: a multiplier applied while zoomed (default 0.7). The camera rect follows the zoom spring.
- **shape**: Square, a centre crop of the camera to 1:1 (the default), or Source, the camera's native aspect.
  - There are no explicit 4:3, 16:9, 3:4, or 9:16 shapes.
- **rounding**: corner radius as a percentage (default 100). With Square that gives a circle.
- **rounding_type**: corner style, squircle (the default) or plain rounded.
- **shadow**: a 0 to 100 strength (default 62.5) plus optional advanced shadow settings for size, opacity, and blur.
- **background_blur**: off, light, heavy, or remove. Remove is macOS-only and drops the padding margin.
- **Not in Camera, but affecting it:**
  - **Colour grade**: the project has a separate colour-correction block for the camera (exposure, contrast, saturation, temperature, tint, fade, split tone, vignette, grain).
  - **Over time:**
    - *StyleSegments* (output time, lanes) can swap in a **whole Camera configuration** for a time range, with the higher lane, or later start, winning.
    - *SceneSegments* switch modes over a time range with fade in/out times (default 0.3 s): default, camera only, hide camera, split screen, and floating cards with per-pane zoom and pan.
    - There are **no per-property keyframes or interpolation for the camera bubble**. A style segment switches the configuration instantly.
  - **Per-clip camera sync offset**: in the clip configuration.
  - **Split and floating scene layouts**: per-pane `camera_zoom` and `camera_position`. That is a pan/zoom of the camera *inside* its pane, available only in those scene modes.
- **Not present at all:** border (there's a border config, but it belongs to the screen background, not the camera), reframe zoom/pan inside the bubble in the default layout, S/M/L presets as data (a UI concern), recorded-during-capture move/resize/hide keyframes, and multiple camera overlays.

## 8. Feature checklist: Cap's TextSegment

Sources: `TextSegment` in `crates/project/src/configuration.rs`; rendered by `crates/rendering/src/text.rs` and `layers/text.rs`.

- **Timing and identity:**
  - start and end in **output (edited-timeline) seconds**;
  - a **track** (lane) index;
  - **enabled**.
  - The project also has a `hidden_text_segments` list.
- **Content:** a plain string; multi-line works through newlines. There are no per-character or per-word style runs.
- **Box:**
  - **center**: normalized 0 to 1 (default 0.5, 0.5);
  - **size**: a normalized box width and height (default 0.35 × 0.2), clamped to 0.01 to 2. The text lays out inside this box.
- **Font:**
  - **font_family**: a string (default "sans-serif");
  - **font_size**: px relative to a 1080p frame height, clamped to 8 to 480 and scaled with the output height (default 48);
  - **font_weight**: numeric (default 700);
  - **italic**.
- **Fill:**
  - **color**: hex (default white);
  - **gradient_color**: an optional second colour for a **horizontal** two-stop gradient;
  - **opacity**: 0 to 1.
- **Stroke:** **stroke_width** (px relative to 1080p; 0 means off) and **stroke_color** (default black).
- **Effects:**
  - **shadow**: one 0 to 1 strength, with no offset, colour, or blur controls;
  - **glow**: a 0 to 1 soft halo in the text colour.
- **Background:**
  - **background_color**: optional hex;
  - **background_style**: Box (one rounded rectangle behind the whole block), Pill (half-height radius, wider padding), or Highlight (one marker rectangle per laid-out line, hugging each line's ink).
- **Layout:**
  - **align**: left, center (the default), or right;
  - **line_height**: a multiplier (default 1.2);
  - **letter_spacing**: px relative to 1080p;
  - **uppercase**: on/off.
- **Animation:**
  - **animation_in** and **animation_out**, each from: none, fade (the default), slide up/down/left/right, pop, zoom, bounce, wipe, words (word-by-word), letters, tracking (letter-spacing collapse), and typewriter.
  - **animation_in_duration** and **animation_out_duration**: default 0.15 s. If the two together are longer than the segment, both shrink in proportion.
  - **Easing is fixed in the renderer**: ease-out-cubic for most animations and ease-out-back for pop and bounce. Word and letter staggering uses a fixed window.
  - A legacy `fade_duration` field remains for older project versions.
- **Frame layout:**
  - **layout**: overlay (the default); full-screen, where the screen card shrinks away, text owns the frame, and the recording clock *pauses* (see holds in §1); split-left; or split-right.
  - **layout_transition**: the morph time, default 0.5 s.
- **Rendering stack:** glyphon (cosmic-text) on wgpu. **Not Skia.** Emoji depend on cosmic-text's fallback.

---

## 9. Gaps: where our spec asks for more than Cap does

### Playback and editing
1. **Audio-driven clock.** The spec says audio drives the playback clock; Cap drives it from video and corrects audio. We need a decision and a fallback for projects without audio or without an output device (§2).
2. **Thumbnail strip on the main track.** Cap's desktop editor has no filmstrip. We need a decoder or thumbnail service that fills progressively, built to meet "opens in under 2 s for a 10-minute recording." One option: decode only keyframes (we record one every ~1 s), at low resolution, into an on-disk cache keyed by (track, source time). (§5)
3. **Fit for an 8 GB M1.**
   - Cap decodes every audio track fully into memory as f32.
   - It pre-renders the whole mix (up to 512 MB).
   - It uses 128 to 192 MB of video prefetch plus 90-frame decoder caches per decoder, across a pool of 5 to 8 decoders.
   - We need stricter memory budgets and streamed audio.
4. **Explicit per-clip mute.** Cap uses volume 0. Our spec lists speed *and* mute per clip, plus per-track volume and mute for mic and system audio. Cap has per-track dB volume and a global mute, but no per-track mute flags.
5. **Speed options and pitch.** The spec offers 0.5×, 1×, 1.5×, 2×, 4× with pitch preserved. Cap's default for sped-up audio is *mute*, and it keeps pitch with FFmpeg's in-process `atempo`. We plan signalsmith-stretch, so no FFmpeg is needed in the app for this.
6. **Ripple on every edit, with undo restoring both.** The spec says ripple edits shift text after the edit point and remove text inside a deleted range. Cap does this for range cuts and clip deletes, but per its own comment **not for clip trims**. Undo/redo wasn't in scope for this read.
7. **Copy, paste, and duplicate that keep the camera layout.** Cap's camera layout is one global configuration plus time-range overrides in output time. Pasted clips don't carry a layout. Our spec stores the camera layout timeline in *recording time*, so pasted pieces carry it automatically. That's a real model difference (see item 8).
8. **Camera keyframes in recording time.** The spec wants moves, resizes, and hides during recording saved as timestamped keyframes, played back, and editable later. Cap has none: no recording-time camera layout, and no keyframes or interpolation for the bubble.
9. **Frame-exact cuts and J/K/L, arrow, and cut-jump navigation.** These are UI features; Cap's mapping supports them. Cap's frame-grid snapping with a floating-point allowance is worth reimplementing.

### Camera overlay
10. **Shapes.** The spec wants circle, square, rectangles at 4:3, 16:9, 3:4, and 9:16, and a rounded rectangle with a radius slider up to circle or pill. Cap has only Square and Source plus rounding.
11. **Reframing inside the shape.** The spec wants zoom from "fill the frame" plus pan, with a reset. Cap has this only in split and floating scenes, not for the bubble.
12. **Size presets, snapping, nudging, and a vertical-centre anchor.** Cap's presets have no vertical centre, and its size is a percentage of the shorter side. Our geometry module should define size and position in output-normalized terms that also work for the live bubble ("what you see is what you get").
13. **Optional border on the camera.** Cap has none (only shadow).
14. **Multiple overlays in the data model.** Cap has a single `camera` field.

### Text
15. **Rotation.** Cap has none. The spec wants a rotate handle with snapping at 0°, ±15°, and 90°.
16. **Layer order** between text items. Cap has lanes and an overlay order, but no explicit z-order per item.
17. **Gradient fill.** Cap's is horizontal only, with two stops. Decide the angle and number of stops for ours.
18. **Shadow.** Cap has a single strength value. The spec's "drop shadow" probably needs offset, blur, and colour.
19. **Background box.** The spec wants solid or translucent, rounded, TikTok-style *merged* line backgrounds, and an inverted style. Cap's Highlight is per-line but not merged, and it has no background opacity or corner-radius field.
20. **Neon style.** Cap's glow exists. Neon as a preset is a combination of glow, stroke, and colour; that is a preset question, not a missing field.
21. **Animations.**
    - The spec wants **adjustable easing**. Cap's easing is fixed.
    - The spec wants **emphasis loops** (pulse, wiggle, float). Cap has none.
    - The spec's out-animations (fade, pop, slide) are covered.
    - The spec's in-animations: fade, pop, slide ×4, typewriter, and word-by-word are covered. Cap has bounce too.
    - "Identical at every export fps" means animations must be a pure function of time. Cap's are.
22. **Style presets, recent styles, and duplicate/copy/paste for text.** These are app-level features; there's no model support in Cap beyond the segment itself.
23. **Fonts.** The spec wants 10 to 15 bundled OFL fonts plus system fonts, and **Skia** rendering for emoji, strokes, shadows, and line backgrounds. Cap uses cosmic-text through glyphon. Our layout must match exactly between preview and export, including colour emoji on every OS.
24. **Eyedropper and typing directly on the preview.** These are UI concerns, outside the model.

### Export
25. **Formats and codecs.**
    - Cap exports only H.264/AAC MP4, gifski GIF, and MOV.
    - The spec adds HEVC (tagged hvc1), AV1, Opus, FLAC, and ALAC in MP4; VP9, AV1, VP8, Opus, and Vorbis in WebM; and an optional ProRes MOV.
    - All of ours go through **helper processes**, never in-process like Cap's FFmpeg and gifski.
26. **Best quality versus Fast.** Cap's default is hardware, with libx264 as a fallback or turbo. The spec defaults to x264 "Best quality", with hardware as "Fast". Cap's own finding supports our default: libx264 turbo is 2 to 3× faster at matching quality on Apple Silicon.
27. **GIF controls.** The spec wants quality, motion quality, lossy, loop (forever, once, or N), a 960 px default maximum width, and a palette "Fast" mode with colours and dithering. Cap has only quality and fast.
28. **Quality presets.** Small, Balanced, High, plus Custom CRF or bitrate. Cap's four bpp/CRF presets are a reasonable starting calibration, but the values have to be re-measured for x265, SVT-AV1, and libvpx.
29. **FPS remembered per format, capped at the recording's fps; GIF fps capped at 50 with hundredths-of-a-second timing.** Cap has neither (it adapts nothing from Kap).
30. **Aspect-locked resize with height presets, no upscaling, and even dimensions.** Cap uses a free resolution base and fits the output to it.
31. **Estimates for every codec, including helper-process codecs.**
    - Cap's *sampled* approach carries over directly: encode 3 to 4 short windows through the real helper, then extrapolate time and size with a keyframe/delta model.
    - We also need a cheap analytic estimate for before sampling finishes, calibrated per codec on an M1-class machine.
    - Our numbers can't be borrowed from Cap's figures.
32. **Named presets, filename templates, and Reveal / Copy / Open.** These are app-level features; Cap has no export-preset model in these crates.

---

## 10. Recommendations to carry into ARCHITECTURE.md and DECISIONS.md

These are proposals and need the user's OK.

- **Mapping:** keep the EDL as an ordered list of `{track set / recording ref, source_in, source_out, speed, mute, volume}`. Make the timeline-to-source mapping one pure function that returns the current piece and where it ends. Use it for playback, prefetch, audio chunking, thumbnails, and export, with a binary search over cached cumulative output starts rather than Cap's linear scan.
- **Cut fades:** 5 ms per side, raised cosine, at non-contiguous joins only. Compute them from absolute output-sample positions so the result doesn't depend on chunking. Apply them after mixing.
- **Export audio:** use whole-sample per-frame budgets so A/V length is exact.
- **Prefetch:** read ahead in timeline-frame space with seek generations, a byte-bounded runway, and a small fixed pool of in-flight decodes. Remember that skip paths must never look like backward seeks to the prefetcher.
- **Clock:** audio master when an output device exists, wall clock otherwise. Wait for the audio callback's acknowledgement before starting the clock. Compensate for device latency, and keep a ~40 ms re-seat safety net.
- **One output stream per editor session:** keep it open and paused rather than reopening it per play press, for Bluetooth.
- **Estimates:** sampled windows through the real helper, with the keyframe/delta size model and a mean ± deviation range; analytic placeholder first.
- **Safe writes:** atomic project save (temp file, fsync, rename), and non-overwriting export publishing with an exclusive-create placeholder plus rename.
- **Memory budget for the M1 8 GB target:** stream source audio rather than fully decoding it; cap the video prefetch around 64 to 128 MB; use at most 2 to 3 pooled decoders per track. **Validate these in the M0 or M3 spikes.**

## 11. What I didn't verify
- I didn't run any of Cap's code or benchmarks. Every performance or accuracy number above is quoted from Cap's own `PLAYBACK-FINDINGS.md` and `EXPORT-FINDINGS.md`, which come from their machines, not this M3 Pro or an M1.
- I didn't read `editor_instance.rs`, `preparing_*`, `export_audio.rs`, or `tracks.rs` line by line. The editor's "preparing" mode (editing while a recording finalizes) and the details of streaming export audio are summarized at a high level only.
- I didn't trace Cap's undo/redo implementation.
- I inferred the clip-trim ripple behavior from a code comment in `configuration.rs`. I didn't check it against the UI.
