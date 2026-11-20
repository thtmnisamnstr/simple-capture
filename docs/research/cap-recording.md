# Research: Cap's recording and encoding pipeline

Status: M0 research note. Read-only study of `reference/Cap` at commit `880ec89`
(cloned 2026-11-19). Nothing here was built or measured; every number quoted
below is a constant or a recorded result found in Cap's source or docs, not a
measurement on our machines.

**License position.** Cap's root `LICENSE` puts only the `cap-camera*` and
`scap-*` crate families under MIT; everything else is AGPL-3.0. None of the
crates studied for this note carry their own `license` field in `Cargo.toml`
or a per-file MIT header (checked with grep), so **all of them are AGPL-3.0**.
This note describes ideas in my own words and cites paths. It contains no code
from these crates, and nothing was adapted, so no `docs/THIRD_PARTY.md` entry
is needed for this area.

| Crate read | Path | License |
|---|---|---|
| cap-recording | `crates/recording` | AGPL-3.0 (root LICENSE; no crate override) |
| cap-enc-avfoundation | `crates/enc-avfoundation` | AGPL-3.0 |
| cap-enc-mediafoundation | `crates/enc-mediafoundation` | AGPL-3.0 |
| cap-enc-ffmpeg | `crates/enc-ffmpeg` | AGPL-3.0 |
| cap-enc-gif | `crates/enc-gif` | AGPL-3.0 (also links gifski, itself AGPL-3.0-or-later) |
| cap-timestamp | `crates/timestamp` | AGPL-3.0 |
| cap-muxer (binary) | `crates/cap-muxer` | AGPL-3.0 |
| cap-muxer-protocol | `crates/cap-muxer-protocol` | AGPL-3.0 |
| cap-media | `crates/media` | AGPL-3.0 |
| cap-cursor-capture | `crates/cursor-capture` | AGPL-3.0 |
| cap-mediafoundation-ffmpeg | `crates/mediafoundation-ffmpeg` | AGPL-3.0 |
| ffmpeg-hw-device | `crates/ffmpeg-hw-device` | AGPL-3.0 |
| (context only) scap-screencapturekit | `crates/scap-screencapturekit` | MIT (`licenses/LICENSE-MIT`) |

Scale warning: the crates above are about 160k lines of Rust. `output_pipeline/core.rs`
alone is about 13k lines and `recovery.rs` about 9k. Much of that is defensive
heuristics accreted from field bugs. We should take the lessons, not the shape.

---

## 1. Big picture

Cap has two recording modes:

- **Studio**: every source goes to its own file (screen, camera, mic, system
  audio, plus cursor/keyboard JSON). The editor composites later. This is the
  mode that matches Simple Capture.
- **Instant**: a single composed, low-bitrate file built for streaming upload.
  It is upload-oriented and out of scope for us, but some of its buffering
  choices (larger queues, longer audio finish timeouts) are informative.

Project layout for a studio recording (from `recovery.rs` and
`studio_recording.rs::create_segment_pipeline`):

- `<project>/content/segments/segment-<N>/` per recorded segment. A new segment
  directory is created on every resume after a pause (see section 6).
- Inside each segment, per track, either a finished file or a directory of
  fragments: `display.mp4` or `display/`, `camera.mp4` or `camera/`,
  `audio-input.ogg` or `audio-input.m4a`, `system_audio.ogg` or
  `system_audio.m4a`, `cursor.json`, plus `start-times.json` (per-track start
  offsets for the segment).
- Project metadata carries a status: in progress, needs remux, failed, or
  complete. Recovery on launch only looks at "in progress" and "needs remux".

The generic plumbing is `OutputPipeline` (`crates/recording/src/output_pipeline/core.rs`):
a builder takes one video source and/or audio sources, a shared timestamp
epoch, an optional start gate, and a `Muxer` implementation. Per-OS muxers
plug in behind the `Muxer` / `VideoMuxer` / `AudioMuxer` traits
(`core.rs` near the bottom). Per-OS selection happens in
`crates/recording/src/capture_pipeline.rs` (`make_studio_mode_pipeline`) and
`studio_recording.rs` (`create_segment_pipeline`).

A user setting called "crash recovery recording" (default **on** in
`apps/desktop-gpui/src/store.rs`) selects the *fragmented* variants below. An
"out-of-process muxer" setting (default **off**) moves muxing into a child
process.

---

## 2. Per-OS pipeline shape

### 2.1 macOS

**Screen source** (`crates/recording/src/sources/screen_capture/macos.rs`, built
on the MIT `scap-screencapturekit` crate):

- ScreenCaptureKit stream configured for bi-planar 4:2:0 **video range**
  (`420v`) output, sRGB colour space name, the target fps as the minimum frame
  interval, and a queue depth that scales with fps (5 surfaces per 30 fps,
  clamped between 3 and an overridable maximum of 8).
- The SCK callback converts the sample buffer's presentation time (host time)
  to mach absolute ticks and tags the frame with that. Frames that arrive
  without an image buffer (SCK's idle/blank status frames) are dropped on the
  spot, so the stream is naturally variable frame rate.
- Size changes mid-stream (window resized, display mode change) are handled by
  rescaling to the configured size rather than failing the writer.
- The frame goes into a bounded channel toward the pipeline (default capacity
  300; overridable by env var) with a short stall budget (50 ms) before it
  counts as a drop and raises a health event.
- A supervisor restarts the SCK stream after stalls/errors, with a cap on
  restart attempts.

**Pipeline task** (`output_pipeline/core.rs`): per source, an async mux loop
re-stamps each frame onto the recording timeline (section 5), applies pause
offsets and the start gate, and calls the muxer's `send_video_frame`. The mux
loop never blocks on the encoder.

**Muxer, non-fragmented** (`output_pipeline/macos.rs`, `AVFoundationMp4Muxer`;
encoder in `crates/enc-avfoundation/src/mp4.rs`):

- The capture-side call is non-blocking: it `try_send`s into a bounded
  `sync_channel` (60 frames for studio, 240 for instant) and counts a drop when
  the channel is full. This was a deliberate fix (`crates/recording/FINDINGS.md`
  "Fix #1"): an earlier blocking retry loop of up to 3 s caused 19% drops; the
  non-blocking design brought it to about 2-3% on their M4 Max test rig.
- A dedicated thread named for the encoder (with raised QoS) owns the
  `AVAssetWriter`. It polls `isReadyForMoreMediaData` in 0.5 ms steps up to a
  bounded number of retries, then drops the frame and counts it as "encoder
  busy". Audio has its own thread and channel into the same writer.
- **Buffer ownership (important):** before queueing, each screen frame is
  copied into a pixel buffer pool owned by Cap (`FrameCopier`). The comment
  explains why: `AVAssetWriter` can hold appended buffers for a second or more
  under load, SCK has only a handful of surfaces, and while the app holds them
  all SCK stops delivering, freezing the recording. Copying returns the capture
  surface immediately. This is exactly our Pitfall 13.
- The writer thread polls free disk space every 2 s and stops the recording
  cleanly (so the moov can still be written) when space falls under a hard
  threshold. Warn and stop thresholds live in `core.rs::DiskSpaceMonitor`.
  A 500 MB minimum is required to start.
- Output is **H.264 in plain (non-fragmented) MP4**. There is no
  `movieFragmentInterval`. A crash or kill therefore loses the whole file
  (Cap's own comment calls a lost moov "unrecoverable").

**Muxer, fragmented** (`output_pipeline/macos_fragmented_m4s.rs`, encoder in
`crates/enc-ffmpeg/src/mux/segmented_stream.rs`):

- Same non-blocking `try_send` into a bounded channel (60), one encoder thread.
- Encoding goes through **in-process FFmpeg**: `h264_videotoolbox` first, with a
  zero-copy path that wraps SCK's IOSurface-backed `CVPixelBuffer` as an
  `AV_PIX_FMT_VIDEOTOOLBOX` hardware frame (`enc-ffmpeg/src/video/videotoolbox_hw.rs`),
  else a CPU copy into a pooled `AVFrame` (`output_pipeline/macos_frame_convert.rs`),
  with `libx264` as fallback.
- Muxing uses FFmpeg's DASH muxer: one `init.mp4` plus numbered
  `segment_NNN.m4s` media segments (2 s each, timed fragments), a DASH `.mpd`,
  and Cap's own JSON manifest (section 3).
- Optional write-behind thread (`enc-ffmpeg/src/mux/write_behind.rs`): the
  encoder hands packets to a thread that owns the `AVFormatContext`, so a slow
  disk blocks only that thread. Its queue is bounded by a byte budget (32 MB to
  512 MB, scaled from RAM); when full, the producer blocks.
- **Quirk:** on macOS, studio mode turns fragmentation **off whenever the
  camera is active** (`studio_recording.rs`, `segment_fragmented = fragmented && !camera_active`).
  No rationale is recorded next to it. My inference is a CPU/encoder-load
  trade-off, but it means the common "screen + camera" case falls back to the
  non-crash-safe AVAssetWriter MP4 path.

**Out-of-process muxer** (opt-in; `output_pipeline/oop_muxer.rs`,
`oop_fragmented_m4s.rs`, `crates/cap-muxer`, `crates/cap-muxer-protocol`):

- Encoding stays in the app process. Only encoded packets cross to a child
  `cap-muxer` binary over its **stdin**, in a small length-prefixed binary
  framing (magic number, protocol version, frame kinds for init-video,
  init-audio, start, packet, finish, abort; packet flags for keyframe; CRC32
  per frame; 16 MiB payload cap). The child's stdout is discarded and its
  stderr is captured for diagnostics.
- In the child, a reader thread fills a byte-bounded queue; the main thread
  runs FFmpeg's DASH muxer with `skip_trailer` and negative CTS offsets. If
  stdin hits EOF without a Finish frame (parent died), the child still closes
  the open fragment cleanly. Distinct exit codes report protocol, FFmpeg, init,
  abort, bad-state and disk-full failures.
- The parent wraps the child in a respawning supervisor: on a crash it starts
  a new child writing into a new "respawn" directory, with a cooldown (500 ms),
  a stability window (5 s) that resets the fast-failure counter, and a cap on
  consecutive fast failures. Disk-full is sticky and never respawned.
- If the `cap-muxer` binary is missing, it falls back to the in-process muxer
  rather than failing the recording.

**Camera**: separate pipeline per segment, either `AVFoundationCameraMuxer`
(AVAssetWriter, same thread design) or `MacOSFragmentedM4SCameraMuxer`
(FFmpeg, CPU copy from a frame pool, no zero-copy).

**Mic and system audio**: separate pipelines. Non-fragmented: Ogg/Opus
(`enc-ffmpeg/src/mux/ogg.rs`). Fragmented: fragmented MP4 AAC
(`enc-ffmpeg/src/mux/fragmented_audio.rs`), preferring AudioToolbox's AAC on
macOS in average-bitrate mode so silence costs almost nothing (their comment:
constant bitrate pads silence to the full rate).

**Cursor/keyboard**: a separate actor (`crates/recording/src/cursor.rs`) polls
the global cursor position about every 16 ms, records move and click events
with times relative to the start gate's arm point, captures cursor images
(deduplicated, with hotspots) when the shape changes, and flushes JSON
incrementally every 5 s so a crash keeps most of it. Coordinates are
normalized to the capture crop bounds; `crates/cursor-capture/src/position.rs`
keeps logical points on macOS and physical pixels on Windows/Linux, wrapped in
opaque types so they cannot be mixed. Cursor rendering in the editor is
optional (screen capture can also bake the cursor in).

### 2.2 Windows

(`output_pipeline/win.rs`, `win_segmented.rs`, `win_fragmented_m4s.rs`,
`oop_fragmented_m4s_win.rs`, `frame_pacing.rs`; `crates/enc-mediafoundation`;
`crates/mediafoundation-ffmpeg`.)

- Windows.Graphics.Capture delivers D3D11 textures. A **cadence gate**
  (`sources/screen_capture/cadence.rs`) estimates the source's real rate with
  an exponential moving average and only starts decimating when the source is
  clearly faster than nominal (about 14% fast to enter, within about 5% to
  leave), because WGC delivers at monitor refresh (up to 165 Hz) and
  `MinUpdateInterval` is not honoured on older builds.
- A **frame pacer** (`frame_pacing.rs`) repeats the last frame when the source
  stalls, stamped one interval after the previous frame, but only once a real
  frame for that slot would be two intervals late. This avoids an earlier bug
  where late-but-real frames collided with synthetic repeats and produced 11 µs
  frames.
- Non-fragmented path: Media Foundation's hardware H.264 MFT, driven
  asynchronously with a DXGI device manager; a D3D11 video processor converts
  BGRA to NV12 on the GPU (zero copy). Encoded `IMFSample`s are then muxed by
  FFmpeg's libavformat (`mediafoundation-ffmpeg`), optionally as fragmented MP4
  (`frag_keyframe+empty_moov+default_base_moof`, 2 s fragments). If MF is
  unavailable, fails validation, is quarantined after a crash, or the primary
  GPU is AMD, it falls back to FFmpeg encoders.
- Fragmented path: FFmpeg encoders (NVENC/QSV/AMF/`h264_mf`/libx264) plus the
  same DASH segment writer as macOS.

### 2.3 Linux

Capture through the portal/PipeWire (with dmabuf import) or X11 SHM, feeding
FFmpeg encoders (`h264_nvenc`, then `h264_vaapi`, then `libx264`), muxed as the
DASH segment stream (2 s) or a plain MP4. Linux uses a cadence-preserving
variant of the timestamp remap and `Instant` (CLOCK_MONOTONIC) timestamps.

---

## 3. File formats and crash safety

| Track | Crash-safe mode (default) | Non-fragmented mode |
|---|---|---|
| Screen (macOS, no camera) | DASH: `init.mp4` + 2 s `segment_NNN.m4s` + `manifest.json` | AVAssetWriter H.264 MP4 (not crash-safe) |
| Screen (macOS, camera on) | *falls back to non-fragmented* | AVAssetWriter H.264 MP4 |
| Camera (macOS) | FFmpeg DASH segments | AVAssetWriter MP4 |
| Screen (Windows) | FFmpeg DASH segments | MF H.264 + libavformat MP4 (optionally fMP4) |
| Mic / system audio | fragmented MP4 AAC (`.m4a`) | Ogg/Opus |

How crash safety is achieved in the fragmented mode:

1. **Small independent units.** Each 2 s media segment is self-contained
   (`moof`+`mdat` pairs) and decodable with the init segment. A crash loses at
   most the segment being written.
2. **Write to `.tmp`, publish by rename.** A segment is written as
   `segment_NNN.m4s.tmp` and renamed when complete; the file is fsynced
   (`enc-ffmpeg/src/file_sync.rs`).
3. **Manifest written atomically.** The JSON manifest (version, type, init
   segment name, codec info, per-segment path/index/duration/size/
   `is_complete`, total duration, `is_complete` for the whole stream) is written
   to a temp file, fsynced, renamed over the old one, and then the **directory**
   is fsynced (`segmented_stream.rs::atomic_write_json`). The manifest is
   refreshed every few packets.
4. **Packet-level ordering.** The encoder holds back one packet so it can stamp
   real durations; the init segment therefore appears one frame after the first
   queue call.
5. **Optional process isolation** (OOP muxer) so a crash in FFmpeg muxing or a
   vendor driver inside the muxer can't take the file down with the app. Note
   that the *encoder* still runs in-process even then.
6. **Ogg for non-fragmented audio**, which is a page-based streaming format and
   survives truncation far better than MP4.

After a normal stop, a "needs remux" status is set and the fragments are
concatenated into a progressive MP4 for the editor (the editor's AVAssetReader
cannot open a fragment directory; see the FINDINGS note dated 2026-02-15).

## 4. How `recovery.rs` repairs recordings

(`crates/recording/src/recovery.rs`, `recovery/*.rs`,
`crates/enc-ffmpeg/src/remux.rs`, `enc-ffmpeg/src/mux/fragmented_mp4.rs`,
`crates/recording/src/track_heal.rs`.)

**Discovery.** On launch, scan the recordings directory for projects whose
status is "in progress" or "needs remux". For each `segment-N` (sorted
numerically), collect per-track fragments, the init segment, audio fragments
and cursor JSON; skip segments without display media; estimate duration.

**Which fragments are trusted:**

- Manifest-listed segments marked complete, that exist, and whose size equals
  the size recorded in the manifest. Manifest versions newer than supported are
  logged but still read.
- Segments not in the manifest are accepted only if a structural check passes:
  walk the MP4 box headers from the start and require the walk to end exactly
  at end-of-file with the last two boxes being `moof` then `mdat`, and a
  minimum size. This catches torn tails cheaply without decoding.
- Pending `.m4s.tmp` files are "rescued" if they hold a complete prefix of
  fragments (copy the valid prefix to the final name, with a marker file for
  truncated ones). A torn fragment is only accepted as the end of the track if
  no later media exists anywhere (later fragments, later respawn directories,
  later recording segments); otherwise it is treated as corruption in the
  middle.
- Respawned-muxer directories are joined in order. When files from different
  muxer processes are joined, each keeps its own start time so a gap stays a
  gap (the last frame is held across it) instead of being closed up.

**Rebuild.** For each segment: concatenate `init.mp4` + segments byte-wise into
a temporary fragmented file, fsync, then **stream-copy remux** to a progressive
MP4 (no re-encode). A "validated" variant decodes packets as it goes. Then
validate that the output actually decodes, compare its span against the
expected display duration persisted at record time (a sync invariant), and do
the same for camera and audio. Single-fragment tracks are simply renamed.

**Publication safety.** Recovery takes a per-project lock, snapshots and
hashes inputs (parallel hashing), works in a private workspace, writes a
publication journal, and publishes with rename-without-replace
(`renamex_np(RENAME_EXCL)` on macOS, `renameat2(RENAME_NOREPLACE)` on Linux;
`recovery/prefix.rs`). Originals are kept until the new files validate, and an
interrupted recovery can be reconciled or rolled back on the next launch.
Symlinks and AppleDouble files are rejected. Tracks that fail can be left out
rather than failing the whole recovery (a "salvage" purpose).

**Load-time healing** (`track_heal.rs`): detects tracks stamped at the wrong
rate (a camera free-running at 60 fps recorded as 30, or WGC at 165 Hz
recorded as 60, producing slow-motion, stretched tracks) by comparing the
packet-timestamp ladder against the real span, and losslessly rescales
timestamps. This exists because earlier recorder bugs shipped; it is a good
reminder that bad timestamps in raw files are expensive forever.

---

## 5. Clock and timestamp model

**Clock sources** (`crates/timestamp`):

- A `Timestamp` is an enum over `Instant`, `SystemTime`, Windows QPC
  (`PerformanceCounter`), and macOS mach absolute time. A `Timestamps` value
  captures all of them at the same moment and serves as the recording **epoch**.
  Any source timestamp can be expressed as a signed offset from the epoch in
  its own clock domain, so mixing clocks is avoided.
- macOS: SCK sample-buffer PTS (host time) is converted to mach ticks;
  CoreAudio buffers come in via cpal's host-time extension; ticks are converted
  to nanoseconds with the mach timebase (not 1:1 on Apple Silicon).
- Windows: QPC; cpal WASAPI timestamps arrive in 100 ns units and are scaled by
  the QPC frequency.
- Linux: `Instant` (CLOCK_MONOTONIC) only; audio is stamped at arrival.

**Master clock** (`timestamp/src/master_clock.rs`): a shared object holding the
epoch, a nominal audio rate (48 kHz) and a count of committed audio samples.
Audio timestamps come from **sample counts**, not per-buffer host times: the
first buffer anchors the track, after which each buffer's time is the running
sample total divided by the source's *real* sample rate (a comment notes that
counting a 44.1 kHz mic against the 48 kHz clock made audio play at the wrong
speed). Cumulative conversion avoids per-buffer rounding drift.

**Video re-stamping** (`master_clock.rs::SourceClockState`,
`core.rs::VideoDriftTracker`, `CaptureClockDrift`):

- Each video source has a state that snaps small jitter onto the expected
  cadence ladder, but caps how far the ladder may lead the source clock so a
  source faster than nominal is not stretched into slow motion.
- Optionally it detects real skipped frames (lag that persists) and moves
  frames to their true time instead of snapping them back.
- For sources stamped on the host clock, the pipeline also measures the
  minimum lag between capture time and hand-off time per one-second window and
  slowly slews out real drift (engages above 25 ms, capped at 2% rate), while
  ignoring jitter.
- Backward jumps over 1 s and forward jumps over 2 s (beyond 0.3 s of wall-clock
  tolerance) count as anomalies; 60 consecutive anomalies fail the pipeline.
- AVAssetWriter specifics (`enc-avfoundation/src/mp4.rs`): timestamps are
  truncated to whole microseconds *before* the monotonicity check, because two
  frames in the same microsecond pass a nanosecond check but collide in the
  writer's 1 MHz timescale, and AVAssetWriter reports that asynchronously a few
  frames later as a fatal error (-11800/-16364). Ties are bumped by exactly one
  tick, never more. The writer also refuses to let video run more than 2 s
  ahead of written audio (it reports "not ready" instead), to keep interleaving
  sane.

**Cross-track alignment** (`studio_recording.rs::SegmentStartTimes`): each
track's first timestamp is turned into a start offset (seconds since epoch) and
saved per segment. Camera start is snapped to mic start, and display to camera
(or mic), when they are within a small threshold: currently one 1024-sample
audio buffer at 48 kHz, about 21 ms (FINDINGS "Fix #2" describes an earlier
30 ms version). This made their sync test read "0 ms" but it hides genuine
offsets inside that window rather than measuring them.

**System audio** is anchored at the pipeline epoch, not at its first packet,
because loopback capture is intermittent (WASAPI only delivers while sound
plays). Gaps in any audio track longer than a threshold (70 ms wired, 160 ms
wireless) are filled with generated silence, in chunks of at most 1 s; small
overlaps are trimmed; the tail is padded with silence up to the stop point.

**Start gate** (`output_pipeline/start_gate.rs`): sources, encoders and muxers
are built during the countdown so their warm-up overlaps it. Every frame
captured before the gate is "armed" is discarded, and the first admitted frame
becomes timeline zero. If the gate is never armed, it opens after a timeout
rather than recording nothing. A static screen that produces no fresh frame
after arming gets its last pre-arm frame released after 50 ms.

**Measured quality, per Cap's own logs** (`crates/recording/BENCHMARKS.md`,
`FINDINGS.md`, Apple M4 Max, 5120x1440 display): roughly 28.8 fps / 2-2.7%
drops for the AVAssetWriter path and 29.5 fps / 0.7-1.3% drops for the
fragmented path at 30 fps; mic offset 1-137 ms depending on mode and run;
macOS system audio consistently 85-230 ms late, left as a "known issue". These
are their numbers on a much faster machine than our M1 target; they are not
ours.

## 6. Pause handling

Two different strategies coexist:

- **Studio mode: pause = stop.** Pausing stops and finalizes the whole segment
  pipeline (all tracks, capture included). Resume builds a fresh pipeline into
  `segment-<N+1>`. Each segment records its own start/stop times and start
  offsets. Empty segments are dropped. Device changes (mic, camera) are only
  allowed while paused, and apply to the next segment. Upside: every pause
  leaves fully finalized files, and a new segment is a natural place to switch
  devices. Downside: resume pays the full start-up cost again (new SCK stream,
  new writers), and the project grows a directory per pause.
- **Within one pipeline (instant mode, fragmented muxers):** a shared pause
  flag; while paused, frames are discarded; on the first frame after resume the
  paused duration is added to a running offset that is subtracted from every
  later timestamp, so the file has no gap. The AVAssetWriter encoder applies
  the same idea internally and has subtle handling for the one frame it holds
  back for duration stamping (a held frame's offset must be shifted by the gap,
  otherwise the pause leaks back into every later timestamp; there is a
  regression test for exactly this). At finish, the end time is clamped to the
  pause start if the recording is stopped while paused.

## 7. Screen frames that only arrive on change

- SCK idle frames (no image buffer) are dropped at the callback, so the raw
  screen track is genuinely variable frame rate; static periods are simply
  long gaps.
- The AVAssetWriter encoder holds each frame until the next one arrives so it
  can stamp a real duration (capped at one nominal frame interval).
- **Tail extension at stop:** if the last frame is older than about one frame
  before the stop point, the pipeline duplicates the last frame and sends it
  stamped just before the stop time (`core.rs::static_video_tail_timestamp`),
  and `endSession(atSourceTime:)` is set to at least the stop time (or audio
  end, whichever is later). So the screen track always spans the recording.
- The *start* of a static screen is covered by the start gate's 50 ms release
  of the last pre-arm frame.
- Reported duration uses the first/last timestamp span actually sent to the
  muxer, never frame count divided by fps (which under-reports VFR tracks).
- Windows does the opposite problem too: too *many* frames (high-refresh
  monitors) via the cadence gate, plus last-frame repeats during stalls.

## 8. Encoder selection and fallbacks

**macOS AVAssetWriter path** (`enc-avfoundation/src/mp4.rs`): always H.264 via
VideoToolbox, starting from Apple's 4K H.264 settings preset and overriding
size (forced even), average bitrate, keyframe interval, frame reordering, and
colour tags. Bitrate scales linearly with pixel count and fps (the default
profile works out to about 10 Mbit/s at 1080p30; a high-quality profile doubles
it; a compatibility profile halves it, with floors and caps). Frame reordering
(B-frames) is enabled only in the high-quality profile.

**FFmpeg path** (`enc-ffmpeg/src/video/h264.rs`):

- Priority lists per OS: macOS VideoToolbox then libx264. Windows depends on
  detected GPU vendor (for example NVIDIA: NVENC, MF, QSV, AMF, then libx264;
  a GPU without hardware encode goes straight to libx264). Linux: NVENC (if
  `/dev/nvidiactl`), VAAPI (if render nodes exist), then libx264.
- **Throughput pre-check:** a hard-coded table of assumed maximum hardware
  encode rates per encoder at 1080p and 4K (interpolated by pixel count) forces
  libx264 if the target fps exceeds 90% of the estimate. For VideoToolbox the
  4K entry is about 55 fps. These are constants, not measurements. On our
  target this heuristic would push a 5K/60 capture to libx264 ultrafast, which
  is the opposite of what an M1 needs.
- Per-encoder options differ between recording (low latency: VideoToolbox
  real-time + speed priority; NVENC low-latency tune; x264 ultrafast/veryfast
  with zero-latency tune) and export (quality presets, look-ahead, no
  software fallback inside VideoToolbox).
- **Crash quarantine** (`video/hardware_crash.rs`): the host is told just
  before any hardware encoder's driver code runs and when it is released. If
  the app dies while one is live, the next launch disables that encoder for the
  rest of the process, so the priority list falls through instead of crashing
  again.
- **Pre-flight self-test:** on Windows (and NVIDIA/VAAPI on Linux) each
  hardware candidate encodes a short neutral-gray clip and decodes it back in
  software; if the gray does not survive (some drivers emit all-zero YUV, i.e.
  green video, with no error), the candidate is rejected. Results are cached by
  everything that shapes the session (encoder, size, pixel format, fps,
  bitrate inputs, options). Intel's MF encoder can crash on certain sizes, so
  that self-test runs in a **child copy of the app** (`isolated_self_test.rs`)
  with a 20 s timeout. VideoToolbox is skipped: they measured session creation
  at about 1.6 s and never saw the failure there.
- **Mid-stream fallback:** if a hardware encoder fails after recording has
  started, it is swapped for libx264 at the same size and timeline. Frames the
  dead encoder still held are lost; libx264 restarts on an IDR with in-band
  parameter sets so the stream stays decodable.
- Odd dimensions are rounded to even before opening.
- HEVC encoders exist (`enc-ffmpeg/src/video/hevc.rs`, `enc-mediafoundation/src/video/hevc.rs`)
  but **recording uses H.264 only**.

**Windows MF path:** hardware MFT with async unlock and D3D manager; validated
before use; AMD GPUs are routed to the FFmpeg path; quarantined like the FFmpeg
hardware encoders (it shares the `h264_mf` name because it loads the same
vendor transforms).

**Decode side** (`crates/ffmpeg-hw-device`): wraps FFmpeg hardware device
contexts (VideoToolbox, D3D11, CUDA, VAAPI) with a `get_format` callback that
falls back to software decoding when the hardware format fails to initialize
(busy or reset media engine) instead of failing every packet.

## 9. Keyframe intervals

| Path | Interval |
|---|---|
| AVAssetWriter H.264 (screen, camera) | floor(0.75 x fps) frames: 22 at 30 fps, 45 at 60 fps |
| FFmpeg encoders (all) | 2 s (`g`; for x264 also `keyint_min`) |
| DASH segments / fMP4 fragments | 2 s |
| Windows MF MFT | **not set** (no GOP/`CODECAPI` attribute found), so vendor default |

The editor seeks from the previous keyframe, so 2 s GOPs make scrubbing
noticeably worse than the 0.75 s ones. Our spec's "about one second" sits
between.

## 10. Colour handling

- SCK output is requested as video-range 4:2:0 with the sRGB colour space
  name. Cap does not set SCK's YCbCr matrix explicitly, so it relies on the
  default.
- AVAssetWriter output is tagged BT.709 for primaries, transfer function and
  matrix.
- FFmpeg encoders tag BT.709 + limited (MPEG) range and set primaries and
  transfer explicitly. RGB-to-YUV conversion through swscale is configured for
  BT.709 limited range, with a comment noting swscale's BT.601 default
  (`enc-ffmpeg/src/video/colour.rs`, `rgb_to_yuv.rs`).
- Windows MF: the D3D11 video processor's input/output colour spaces are set
  by OR-ing enum values into the low bits of the
  `D3D11_VIDEO_PROCESSOR_COLOR_SPACE` bitfield
  (`enc-mediafoundation/src/video/video_processor.rs`). By the documented
  field layout (Usage, RGB_Range, YCbCr_Matrix, xvYCC, Nominal_Range), this
  appears to leave the YCbCr matrix bit at BT.601 and does not reach the
  nominal-range field; I also found no BT.709 tagging on the MF output media
  type. I have not verified this on Windows. Treat it as a warning to test our
  own Windows path against a colour test pattern.
- No HDR/P3/10-bit handling in recording; everything is 8-bit SDR.

## 11. gifski integration (`crates/enc-gif`)

- Cap links the `gifski` crate (1.32) directly as a library, which is fine for
  an AGPL app and **not** an option for us (spec: gifski only as a sidecar).
- A collector/writer pair: frames are pushed as RGBA (with stride handling) on
  the caller's thread, and a separate thread runs the gifski writer to the
  output file. Presentation time is frame index divided by fps (constant
  rate). Settings exposed: quality (1-100, default 90), a "fast" flag, fixed
  infinite repeat, output width/height. Motion quality, lossy quality, loop
  count and progress reporting are not exposed.
- For our sidecar plan, note from the gifski CLI source (`reference/gifski/src/bin`,
  AGPL; describing behaviour only): it accepts Y4M on stdin (`-` as input),
  has `--quality`, `--motion-quality`, `--lossy-quality`, `--fps`, `--fast` /
  `--extra`, `--width`, `--repeat`, and a hidden `--y4m-color-override`. When
  the Y4M header has no colour info, it **assumes BT.601 for frames 720x480 or
  smaller and BT.709 above**, and limited range. Our GIF presets go down to
  240 px, so we must always pass the matrix explicitly (or emit the Y4M colour
  tags) or small GIFs will shift colour.

## 12. `cap-media` and `mediafoundation-ffmpeg`

- `cap-media` is now a thin shell (an FFmpeg init plus an error type), a
  remnant of an older "pipeline stage" framework. Nothing to learn there.
- `mediafoundation-ffmpeg` bridges MF `IMFSample` H.264 output to libavformat:
  extracts sample bytes, detects keyframes, converts MF's 100 ns timestamps to
  the stream time base, and can set fragmented-MP4 flags (keyframe-driven
  fragments, empty moov, default-base-moof) with a fragment duration.

---

## 13. What we should adopt, avoid, or do differently

### Adopt (ideas, re-implemented in our own code)

1. **Copy capture surfaces out before the writer sees them.** Never hand SCK's
   IOSurface to AVAssetWriter if the writer can lag; blit into our own
   `CVPixelBufferPool` (GPU copy via VTPixelTransferSession or Metal) and
   return SCK's buffer at once. Size the pool and count pool exhaustion as a
   drop. This is Pitfall 13 made concrete.
2. **Non-blocking capture callbacks.** Callback does one bounded `try_send`,
   counts drops per track, and emits a health event above a drop-rate
   threshold. One dedicated writer thread per `AVAssetWriter` with raised QoS.
3. **Start gate.** Build capture and writers during the countdown; discard
   pre-arm samples; the arm instant (host time) is timeline zero. Release the
   last pre-arm screen frame if the screen is static.
4. **Static tail and duration.** Duplicate the last screen frame at stop and
   end the writer session at the stop time; compute durations from timestamp
   spans, never frame counts.
5. **Writer-timescale monotonicity.** Quantize PTS to the writer's timescale
   before checking monotonicity; correct ties by one tick only.
6. **Audio timestamps from sample counts** anchored to the first buffer's host
   time, at the device's real sample rate; fill true gaps with silence; anchor
   system audio at the epoch, not at its first packet.
7. **Disk-space guard inside the writer loop** (stop while the writer is still
   alive so files finalize), plus a minimum free space to start.
8. **Atomic manifests**: temp file, fsync, rename, fsync the directory.
9. **Cheap structural validation of fragments** by walking box headers
   (complete `moof`/`mdat` at EOF), plus size-vs-manifest checks, before any
   decode-based validation.
10. **Careful recovery publication**: a per-project lock, work on copies,
    publish with rename-no-replace, keep originals until validated, make an
    interrupted recovery resumable.
11. **Hardware encoder safety net for the helper/capability probe**: real test
    encodes with a decode-back check (the gray round trip), cached per
    configuration; a "live encoder" marker so a crash quarantines that encoder
    on next launch; run risky probes in a child process. For us the export
    encoders already run in the FFmpeg sidecar, so a crash there is naturally
    isolated; the quarantine idea still applies to in-process Windows MF and
    Linux VAAPI recording encoders.
12. **AudioToolbox AAC in average-bitrate mode** so silent stretches are cheap.
13. **Cursor clicks as a separate timestamped data stream** (normalized to the
    crop, logical vs physical kept distinct by type) to drive our optional
    click highlight, flushed incrementally.
14. **Health/diagnostics counters** per track (frames, drops, encoder-busy,
    channel depth) logged at finish; these feed our debug overlay and the
    `--self-test` report.

### Avoid

1. **A non-crash-safe default path.** Cap's AVAssetWriter MP4 has no fragments,
   and on macOS it becomes the path whenever the camera is on. Our spec says
   "never lose a recording"; every raw track must be crash-safe in every mode.
2. **In-process FFmpeg on macOS just to get crash safety.** Cap routes its
   crash-safe macOS path through FFmpeg's DASH muxer (and libx264 fallbacks).
   We can get fragmentation natively (see "Do differently" 1) and keep FFmpeg
   out of the macOS app process entirely.
3. **Dropping audio when a queue is full.** Cap's AVAssetWriter muxer drops
   audio frames on a full channel. Audio is tiny; size its buffer generously
   and never drop it silently (if it must be dropped, record the gap so it can
   be filled with silence of the right length).
4. **Snapping track start times together** to make a sync metric read zero.
   Store measured per-track offsets on the shared host clock in project.json
   and prove sync with the flash/beep/clap test.
5. **Hard-coded hardware throughput tables** that force software encoding.
   Measure headroom on the actual machine (capability probe) instead; on an M1
   the software fallback is the worst choice for 5K capture.
6. **Accreting heuristics.** Cap's drift slewing, cadence ladders and anomaly
   counters exist largely to clean up timestamps from mixed clocks and older
   bugs, and `track_heal.rs` exists because bad timestamps shipped. On macOS
   all our sources (SCK, AVCapture, CoreAudio) deliver host-time PTS on the
   same mach clock; trust them, convert once, and add a correction only when a
   test shows a need.
7. **Long or unset GOPs**: 2 s (FFmpeg) or unset (MF) makes editor seeking
   slow. Set the interval explicitly on every encoder.
8. **Untagged or ambiguous colour on any path.** Set SCK's colour matrix
   explicitly, tag BT.709 on every writer, and check the Windows path with a
   test pattern.
9. **Linking gifski** (AGPL) or any GPL/AGPL encoder into the app.

### Do differently

1. **macOS raw tracks: AVAssetWriter with fragments.** Two candidates for the
   M1 pipeline spike (spike 3):
   - set `movieFragmentInterval` (about 1-2 s) on an `AVAssetWriter` writing
     `.mov`/`.mp4`, so a killed recording leaves a file whose fragments up to
     the last flush are readable, and recovery can be "re-mux to a regular
     movie" (via AVFoundation, no FFmpeg); or
   - use AVAssetWriter's segment output mode (Apple HLS fMP4 profile with a
     delegate receiving init and media segments) and write numbered segments
     plus our own atomic manifest, which mirrors Cap's DASH layout but stays
     native and zero-copy.
   Both keep VideoToolbox HEVC hardware encode with IOSurface input. The spike
   should kill the process mid-recording and confirm what AVAssetReader can
   open afterwards.
2. **HEVC for the screen track** (spec Pitfall 4), with `hvc1` tagging and a
   measured headroom check on M1 for native-Retina sizes; H.264 only as a
   compatibility fallback. Cap records H.264 everywhere.
3. **Keyframe about every 1 s** on every raw track (spec), set explicitly
   (`AVVideoMaxKeyFrameIntervalKey` plus the duration variant) so VFR gaps do
   not stretch the GOP in time.
4. **Pause without tearing down capture.** Keep the SCK/AVCapture sessions
   running, drop samples while paused, shift timestamps by the accumulated
   pause offset in our writer wrapper (handling the held-back frame as Cap
   learned the hard way), and record pause intervals (host time) in
   project.json. This gives instant resume and avoids a directory per pause.
   Device changes during a recording can still start a new segment file
   when required. Decide this in spike 3.
5. **One clock, one conversion.** Convert every sample to nanoseconds on the
   monotonic host clock (mach host time, QPC, CLOCK_MONOTONIC) at the source
   boundary; store the epoch and per-track first-sample offsets in
   project.json; no snapping.
6. **Always pass explicit colour metadata to helpers** (Y4M colour tags or
   `--y4m-color-override bt709` for gifski; `-colorspace/-color_primaries/
   -color_trc/-color_range` for FFmpeg), because helper defaults differ by
   resolution.
7. **Isolation where it matters for us**: export encoders and gifski are
   already separate processes (sidecars). For recording, the riskiest
   in-process code is vendor encoder drivers on Windows/Linux; adopt the crash
   quarantine and child-process probe there rather than a separate muxer
   process.

### Open questions for spikes

- Does AVAssetWriter with `movieFragmentInterval` (or segment output) on macOS
  14-26 produce files that AVAssetReader opens after SIGKILL, and how much is
  lost? (spike 3)
- VideoToolbox HEVC headroom on a base M1 at native Retina (5K-class) 30/60 fps
  with camera running in parallel. Only M3 Pro hardware is available locally,
  so M1 figures will need a constrained proxy and an explicit caveat.
- Does SCK's frame delivery stay healthy with our pool-copy approach under the
  writer back-pressure Cap observed?
- macOS system audio latency: Cap reports 85-230 ms late system audio from SCK.
  Measure it ourselves and decide whether to compensate with a measured offset.

No code was copied, translated or closely paraphrased from any of the AGPL
crates above; all descriptions are my own summaries with file references.
