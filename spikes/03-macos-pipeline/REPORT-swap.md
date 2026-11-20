# Spike 3b: switching the capture source during a recording (macOS)

Status: feasible. Written 2026-11-19 from `out/swap-r{1,2,3}/*/analysis.json`, `out/swap-r{1,2,3}/summary.json`, `out/swap-summary/summary.json` (per-switch aggregate) and `out/swap-picker/summary.json`. Recordings and `frames.json` are gitignored.

**Machine and runs**
- M3 Pro, macOS 26.7.2, one display: 7680x2160 px (3840x1080 pt, scale 2).
- Every capture run went through `tools/macos-dev-host/run.sh`, with the binary staged in a temp dir; all 16 `project.json` files record `permission_host = com.simplecapture.dev`.
- Load average at the start of each round: 12.15 (r1), 6.38 (r2), 4.91 (r3). The machine was busy, especially in round 1.

## Question
Can one recording switch between a display, a window and a region while it is running? How long is the gap at each switch, does the wrong content leak into the file, and does system audio keep flowing? Should we update the running SCStream or replace it?

## Method (RAN)
`sc-spike swap` records about 12.3 s with system audio and the mic, no camera. It switches source every 3 s:
1. display (display filter, 7680x2160)
2. a test window from a helper process (`desktopIndependentWindow`, 720x480)
3. a region of the display (display filter + `sourceRect` 560x320 pt, 1120x640 px)
4. back to display

Test setup:
- **Helper process 1** owns two borderless windows that change color every 10 ms, so SCK keeps sending frames: a green one (the window source) and an orange one (the region sits inside it). It also plays a 440 Hz tone.
- **Helper process 2** has no windows and plays a 660 Hz tone.

Three strategies (`--strategy`):
- `update`: `updateContentFilter` (only when the filter type changes), then `updateConfiguration`, each awaited in turn. `--order config-first` reverses the order.
- `restart`: `stopCapture` on the old stream, then create and `startCapture` a new one.
- `overlap`: create and start the new stream, wait for its first frame, then stop the old one.

System audio (`--audio`):
- `shared`: system audio comes from the video stream.
- `separate`: system audio comes from a persistent audio-only SCStream (display filter, 128x72 at 1 fps, screen output ignored).

Writing:
- Each source gets its own AVAssetWriter fragmented .mov (`screen-NNN.mov`; HEVC, 1 s fragments, bitrate = pixels x fps x 0.05).
- A segment opens on its first matching frame and closes when the next segment's first frame arrives. The last frame is held until then by appending a retimed copy at end minus 1/fps, and the session ends at `endSessionAtSourceTime(end)`.
- Mic (AVCaptureSession) and system audio are each one continuous file.

Frame routing:
- A frame belongs to a segment only if stream tag, pixel size, and SCK's `SCStreamFrameInfoContentRect` size plus `ContentScale` (must be 1) all match. `--route dims` checks pixel size only.
- Each frame also gets a 10-point color signature from its 420v pixels (G = green, O = orange, K = black, X = other). This classifies its real content as window, region, display or mixed, so leaks can be counted.

Validation: `swap_analyze.py` runs Homebrew ffprobe/ffmpeg on every segment (codec, size, color tags, duration, frame count, decode errors). For audio it uses buffer-PTS continuity plus Goertzel tone tracking (10 ms windows) on the decoded system_audio.mov.

Rounds: 3 rounds of `update:shared`, `update:separate`, `restart:separate`, `overlap:separate`. Round 1 adds controls: `restart:shared`, `update:shared:config-first`, `update:separate` with `--route dims`.

## Results (MEASURED)

### Switch gap: last old-source frame to first new-source frame, host clock
Frames arrive about every 34 ms (28-29 fps), so a gap up to about 34 ms means no frame was missed. Ranges cover 3 runs; controls are a single run.

| variant | display->window | window->region | region->display |
|---|---|---|---|
| overlap (separate audio) | 8-28 ms | 9-23 ms | 0-28 ms |
| restart (separate audio) | 77-89 ms | 74-99 ms | 76 ms |
| update (separate audio, size+rect routing) | 66-96 ms | 45-52 ms | 7-21 ms |
| update (shared audio) | 56-74 ms | 38-63 ms | 14-28 ms |
| restart (shared audio), 1 run | 100 ms | 113 ms | 111 ms |
| update, size-only routing, 1 run | 23 ms | 21 ms | 35 ms (3 wrong-content frames written into segments) |

**Time from request to first new frame:**
- overlap: 55-88, 39-45 and 53-65 ms
- restart: 68-90, 47-74 and 66-68 ms
- update-separate: 88-128, 66-78 and 42-54 ms
- update-shared: 168-201, 155-180 and 34-43 ms

**API step times:**
- `updateContentFilter`, display->window: 71-103 ms with separate audio, 149-175 ms when the stream also captures audio.
- `updateContentFilter`, window->region: 38-45 ms with separate audio, 137-154 ms with shared audio.
- `updateConfiguration`: 10-43 ms.
- restart: `stopCapture` 16-33 ms, `startCapture` 39-61 ms (up to 216 ms with shared audio).
- overlap: new `startCapture` 34-89 ms, wait for first new frame 7-23 ms, old `stopCapture` 16-28 ms.

**Old content after the request:**
- 0-4 old-source frames arrived after the request; they were captured before the switch took effect.
- No old-content frames arrived after the API calls completed.
- restart had none at all.

### Hazards of updating a running stream
- **Config-first fails.** In the r1 config-first control (window->region), `updateConfiguration` with a `sourceRect` failed with -3812 "Failed due to an invalid parameter" against the current window filter. The following `updateContentFilter` then failed with -3812 too. The stream stayed on the window filter, and the region and final display segments never got a frame (87 transition frames dropped). An earlier trial (folder deleted after its code changed) that issued both updates at once without awaiting also got -3812.
- **Filter-first has a transition.** Between the filter update and the config update, SCK delivers new content at the old size. SCK's per-frame attachments identify these frames:
  - display->window: `7680x2160, contentRect [0,0,360,240], contentScale 1`, the window drawn small at the top-left of a black display-size frame.
  - window->region: `720x480, contentRect [0,69.375,360,101.25], contentScale 0.09375`, the whole display squeezed into the window's frame size.
- Routing on pixel size alone let 1-2 of these per switch into the old segment (3 wrong frames in the size-only control). Routing on size plus contentRect and contentScale removed them in 9 of 9 runs. The cost is 1-3 dropped frames per filter switch, which is why update has the larger gaps above.

### System audio across switches
- **Audio carried by the switched stream (shared):** every filter change interrupts SCK audio: 60.3/94.7, 86.6/83.2 and 96.1/110.1 ms for update-shared r1/r2/r3. restart-shared lost 167.5, 122.5 and 171.0 ms. A config-only switch (region->display) caused no gap.
- **Early gap:** 2 of 5 shared runs (both in round 1) also had a 17-20 ms gap about 1 s after start that was not near a switch. Cause not investigated.
- **AVAssetWriter closes timestamp gaps.**
  - `ffprobe` on r2 update-shared system_audio.mov shows 551 contiguous AAC packets, last at 11.689 s, while the buffers span 11.870 s of host time.
  - File durations against host span: update-shared 11.70 vs 11.87-11.91 s; restart-shared 11.40 vs 11.88 s.
  - So all audio after a gap plays early by the total of the gaps before it, up to 0.48 s here. **The writer has to insert silence for any audio PTS gap.**
- **Separate audio-only stream:** 0 gaps over 1 ms in 10 of 10 runs. File duration (12.34 s; one 12.32 s) equals the host span. Neither tone has a dropout.
- **Audio does not follow the window filter.** The 660 Hz tone from a process with no windows stays in the shared stream's audio throughout the window-filter segment; its only dropouts are at the switch moments, like the 440 Hz tone's.
- **Mic:** 0 gaps over 1 ms in 16 of 16 runs; it is independent of SCK.

### Segment files
- 58 segments from 15 runs; the config-first control never started its last 2 segments.
- All hevc/hvc1 with bt709/bt709/bt709 tags, and each size equals the source size (7680x2160, 720x480, 1120x640).
- 0 decode errors from `ffmpeg -f null` and `ffprobe -count_frames`.
- ffmpeg's null muxer printed 58 "non monotonically increasing dts" warnings: an artifact of the 1/600 track timebase with irregular frame times, not a decode error.
- ffprobe duration is within 0.0315 s of the intended duration (end_host - start_host).
- In 4 segments the last frame's pts is at or after the session end, so it is trimmed: container frame count = decoded + 1. That is intended.
- Delivery: 26.4-29.0 fps per segment; late/missing 0-8 frames, mostly in the first display segment; encoder not-ready drops 0.

### CPU and memory (% of one core, whole run)
- Our process: 13.4-21.9 (includes the spike's pixel sampling and attachment parsing; not separated).
- replayd 3.9-5.0, VTEncoderXPCService 2.7-4.4.
- Peak memory: 47-50 MB.
- Separate audio does not clearly cost more: our process was 16.4-21.8 with it and 13.4-21.0 with shared audio.

### SCContentSharingPicker (RAN `sc-spike picker`, never presented)
- The class exists, and `present`, `presentPickerUsingContentStyle:`, `presentPickerForStream:`, `presentPickerForStream:usingContentStyle:`, `setConfiguration:forStream:`, `addObserver:`, `setActive:` and `setMaximumStreamCount:` all respond.
- Default configuration before: all 5 modes (raw 31), `allowsChangingSelectedContent` true, no exclusions.
- I set and read back: SingleWindow, SingleApplication and SingleDisplay (raw 21), `allowsChangingSelectedContent` true, excluded bundle ID `com.simplecapture.dev`.
- `maximumStreamCount` was 1 before, and setting it to 1 read back 1.
- `setConfiguration:forStream:` on a stream that was never started raised no exception.
- `setActive` true then false read back as true then false.
- An observer was added and removed; 0 callbacks.
- No `present*` method was called. Region selection is not a picker mode, so regions still need our own overlay plus `sourceRect`.

## Recommendation
1. **Swap by overlap (make-before-break).** Build the new SCStream from the new filter and config, start it, and close the old segment on the new stream's first frame. Then stop the old stream and route frames by stream identity.
   - Measured gap: 0-28 ms, never more than one frame interval.
   - No wrong-content frames.
   - It avoids the update ordering hazard (-3812) and the attachment-based routing.
   - If the new stream fails to start, keep the old one running and report the error.
   - restart is the simple fallback: a consistent 74-99 ms gap (about 2 frames).
2. **Keep system audio on a persistent audio-only SCStream** that never switches. Shared audio loses 60-171 ms at every filter change.
3. **Insert silence for any audio PTS gap** before AVAssetWriter, because its AAC input closes gaps and shifts all later audio.
4. **Moving or resizing a region on the same display:** a single `updateConfiguration` on the running stream is fine (7-28 ms, no audio impact).
   - Never send a `sourceRect` config while the stream has a window filter.
   - If you do update filters in place: filter first, await it, then config, and route by size + `SCStreamFrameInfoContentRect` + `ContentScale`.
5. **SCContentSharingPicker can drive swaps on macOS:** set `allowsChangingSelectedContent`, observe `didUpdateWithFilter:forStream:`, and feed the filter into step 1. This is not verified with a human picking yet, and Windows/Linux equivalents are untested.

## Proposed segment format (project.json)
```json
"screen_segments": [
  {
    "index": 0,
    "file": "screen-000.mov",
    "source": {"kind": "display", "display_id": 1, "excluded": "..."},
    "pixels": [7680, 2160],
    "content_rect_pts": [0, 0, 3840, 1080],
    "point_pixel_scale": 2,
    "requested_host_s": 0.0,
    "start_host_s": 0.0,
    "end_host_s": 0.0,
    "last_real_frame_host_s": 0.0,
    "frames_written": 0,
    "writer": {"encoder": "HEVC ...", "held_last_frame_s": 0.0}
  }
]
```
- `source.kind` is one of:
  - `display`: `display_id`, exclusions
  - `window`: `window_id`, owner pid/bundle, title at capture
  - `region`: `display_id`, `source_rect_pts`
- `start_host_s` is the first frame's PTS and is file time 0.
- `end_host_s` equals the next segment's `start_host_s`, or the stop time for the last segment. The file holds its last frame until then, so the segments tile the timeline with no holes.
- `last_real_frame_host_s` records the real last capture.
- Mic and system audio stay single continuous files on the same host clock.
- Each segment has its own size, so the compositor fits each one into the output canvas separately. This fits the new aspect-ratio and letterbox requirement.
- The product should write a segment's entry when it opens and patch `end_host_s` when it closes, for crash recovery. The spike writes them only at the end; not tested.

## Not tested
- Windows and Linux.
- A human using the picker.
- Multi-display, display hot-plug, a window closing or minimizing mid-segment.
- 60 fps, switches more often than every 3 s, static content (no frames).
- Whether the menu-bar recording indicator flickers on restart/overlap.
- Peak memory and encoder load while two 7680x2160 streams overlap.
- Cost of the audio-only stream on its own.
- Cause of the 17-20 ms early audio gap.

## Files
- `src/swap.rs`: subcommands `swap`, `swaphelper`, `picker`
- `src/main.rs`: wiring
- `src/avcap.rs`: per-buffer mic timing
- `src/capture.rs`: `frame_info` made pub
- `swap_bench.sh`
- `swap_analyze.py`
- Outputs: `out/swap-r{1,2,3}/`, `out/swap-summary/summary.json`, `out/swap-picker/summary.json`

No new dependencies.

---

## Independent verification (2026-11-19)

An adversarial verifier re-read the sources and re-ran the builds and measurements. Verdict: **partially confirmed**. Where this section and the text above disagree, this section wins.

### Discrepancies found

- Run count is wrong. The investigator says 'all 16 project.json files' and 'Mic: 0 gaps in 16 of 16 runs', but there are 15 swap runs and 15 project.json files (find out/swap-r*/ -name project.json gives 15: 7 in r1, 4 in r2, 4 in r3). The picker run writes no project.json and records no permission_host.
- Some ranges don't match the saved data. Per-segment fps runs 26.63-29.0, not 26.4-29.0 (minimum is swap-r1/update-shared/screen-000). Late/missing frames peak at 7, not 8. Our process's CPU peaks at 21.8% of one core (swap-r3/update-separate), not 21.9%.
- 'Routing on size + contentRect + contentScale removed them in 9 of 9 runs' has no source. Every variant except the dims control used dims+content_rect+content_scale routing (project.json frame_routing). That is 14 runs, all with 0 wrong-content frames in segments; 7 of those are update-strategy runs, the config-first control included.
- '1-3 transition frames were dropped per switch' for update-separate is overbroad. Config-only region->display switches dropped 0 frames. Filter switches dropped 2-3 (display->window) and 2 (window->region).
- Overlap also drops frames, and the report never says so. 5 of 9 overlap switches (and 3 of 3 in my re-run) discarded 1 old-source frame from the old stream that arrived after the new stream's first frame. The frame sequence interleaves, e.g. display, window, display, window. No wrong content reached any segment.
- The claim that updateContentFilter is slower because the stream carries audio is confounded. The r1 dims control used separate audio and still took 150.0 ms for display->window. The investigator's '71-103 ms with separate audio' range leaves that run out. In my re-run, update-separate took 88 ms and update-shared 199 ms, so shared audio was slower in that paired run, but the 'separate audio = 38-103 ms' range does not hold.
- The ranges come from 3 runs and shift between runs. My re-run fell outside several of them: restart display->window gap 98.1 ms (claimed 77-89), restart region->display 90.3 ms (claimed 76 in all 3 runs), update-shared display->window 85.0 ms (claimed 56-74), update-shared updateContentFilter 198.9 ms (claimed 149-175), overlap stopCapture(old) 32.6 ms (claimed 16-28), our process CPU 20.3-23.2%.
- The early 16-20 ms system-audio gap was not limited to round 1 or high load. My re-run (load average 4.59) had a 16.3 ms gap about 1.96 s before the first switch in update-shared. That makes 3 of 6 shared-audio runs.
- Picker provenance is weak. out/swap-picker/picker.log holds only a shell error ('(eval):1: permission denied: ../../../../../../'), not a run log. The picker writes no permission_host. The excludedBundleIDs value comes from __CFBundleIdentifier with a hard-coded fallback of 'com.simplecapture.dev' (src/swap.rs picker()), so it does not prove the run went through the dev host. The code also sets excludedWindowIDs [1], which the report does not mention. 'setConfiguration:forStream: worked' only means no exception was raised; nothing was read back. My own run.sh re-run produced byte-identical JSON content, so the results themselves stand.
- The '0.17-0.48 s shorter' file-duration range leaves out the config-first control (0.08 s shorter: 11.82 s against an 11.898 s span). My update-shared re-run came out 0.21 s shorter.

### Corrected summary

Hot-swapping the source during a recording is feasible on macOS (only one machine tested, under load). The spike's `swap` subcommand records about 12.3 s with system audio and mic. It switches every 3 s: display (7680x2160) -> helper-process window (720x480) -> sourceRect region (1120x640) -> display. Each source is written to its own fragmented HEVC .mov segment. All 15 swap runs (3 rounds of update-shared, update-separate, restart-separate and overlap-separate, plus 3 round-1 controls) went through tools/macos-dev-host/run.sh (permission_host = com.simplecapture.dev, rc=0). A verifier re-run of 5 variants through run.sh reproduced the qualitative results.

**Last-old to first-new frame gap.** Frames arrive about every 34 ms. Original 3 runs, then the verifier run:
- **overlap** (make-before-break): 0-28 ms in the original runs; 14-22 ms in the verifier run. No wrong content in segments. 0-1 trailing old-stream frame is dropped per switch.
- **restart:** 74-99 ms in the original runs; 79-98 ms in the verifier run. No wrong content.
- **update in place, separate audio:** 66-96 ms (display->window), 45-52 ms (window->region), 7-21 ms (region->display, a config-only change); verifier 80 / 39 / 21. Filter switches drop 1-3 transition frames.
- **update in place, shared audio:** 56-85 ms / 38-63 ms / 14-28 ms across all 4 runs.
- **updateContentFilter time:** 38-150 ms with separate audio (150 ms in one control) and 137-199 ms with shared audio. The audio effect is plausible but confounded.

**Hazards of update in place:**
- Sending the config with a sourceRect while a window filter is active fails with -3812, and so does the filter update that follows. The stream stays on the window filter (reproduced).
- With filter-first order, SCK delivers 1-3 transitional frames of new content at the old size. They can be identified with the SCStreamFrameInfo contentRect and contentScale attachments. Routing on pixel size alone let 3 of them into segments. Routing on size + contentRect + contentScale gave 0 wrong-content frames in all 14 runs that used it.

**System audio:**
- When the switched stream also carries system audio, each filter change or restart loses 60-171 ms. A config-only change loses nothing.
- An unexplained 16-20 ms gap about 1 s after start appeared in 3 of 6 shared-audio runs, at both high and moderate load.
- AVAssetWriter's AAC input closes PTS gaps. Files come out 0.08-0.48 s shorter than the host span (551 contiguous packets confirmed with ffprobe), so the writer must insert silence.
- A persistent audio-only SCStream had 0 gaps over 1 ms in all 10 original runs and all 3 in the re-run. File duration equals the host span.
- A tone from a process with no windows stays audible under a window filter.
- Mic: 0 gaps in all 15 original and 5 re-run runs.

**Segments:** 58 segments in the original runs and 18 in the re-run. All are hevc/hvc1 with BT.709 tags, sizes match their sources, and there are 0 decode errors. Durations are within 0.032 s of intended. 4 trimmed last frames are intended.

**Delivery and cost:** 26.6-29.0 fps per segment, 0-7 late/missing frames. Our process used 13-23% of one core; replayd 3.9-5.2% and VTEncoderXPCService 2.7-4.5% across both sets of runs. Peak RSS was 47-50 MB.

**SCContentSharingPicker** (configured but never presented; the re-run via run.sh gave identical output):
- The class exists and all present*, setConfiguration:forStream:, addObserver:, setActive: and setMaximumStreamCount: selectors respond.
- A default configuration of SingleWindow, SingleApplication and SingleDisplay (raw 21) with allowsChangingSelectedContent and the excluded bundle ID com.simplecapture.dev reads back correctly. The code also excludes window ID 1.
- maximumStreamCount was 1 before and after setting it to 1.
- setConfiguration:forStream: on a stream that was never started raised no exception; nothing was read back.
- setActive true then false read back true then false.
- 0 observer callbacks.

**Recommendation** (supported by the data):
- Swap by overlap: start the new stream, close the old segment on the new stream's first frame, then stop the old stream, and route frames by stream identity. Fall back to restart if the new stream fails to start.
- Keep system audio on a persistent audio-only SCStream, and insert silence for any audio PTS gap.
- Use updateConfiguration only for config-only changes, such as region changes on the same display. Never send a sourceRect config while a window filter is active.

**Segment format:** one screen-NNN.mov per contiguous source in project.json `screen_segments`. Each entry has the source descriptor, pixels, content rect, requested/start/end/last-real-frame host times, and frames written. start_host_s is file time 0, end_host_s is the next segment's start, and the last frame is held until then. Mic and system audio stay single continuous files on the host clock.

**Not tested:** Windows/Linux, a human using the picker, multi-display, windows closing mid-segment, 60 fps, static content, and indicator flicker.
