use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_av_foundation::{
    AVAssetReader, AVAssetReaderStatus, AVAssetReaderTrackOutput, AVAssetTrack, AVMediaTypeAudio,
    AVMediaTypeVideo, AVURLAsset,
};
use objc2_core_media::{CMSampleBuffer, CMTime, CMTimeRange, kCMTimePositiveInfinity};
use objc2_core_video::{kCVPixelBufferIOSurfacePropertiesKey, kCVPixelBufferPixelFormatTypeKey};
use objc2_foundation::{NSArray, NSDictionary, NSString, NSURL};
use sc_geometry::SizePx;
use sc_platform::{AudioReader, DecodedFrame, Error, FrameBuffer, Result, TrackProbe, VideoReader};

use super::frames::{MacPixelFrame, PIX_420V, asbd_of};
use super::pcm::{PcmFormat, copy_frames};
use super::writer::key;
use crate::util::{
    as_obj, cfstr_as_ns, cmtime_to_ns, dict, ns_error_text, ns_to_cmtime, nsstr, num_b, num_f,
    num_i,
};

struct Track {
    reader: Retained<AVAssetReader>,
    output: Retained<AVAssetReaderTrackOutput>,
    track: Retained<AVAssetTrack>,
}

// SAFETY: the reader and its output are used from one thread at a time (`&mut self`).
unsafe impl Send for Track {}

fn reader_error(r: &AVAssetReader) -> String {
    // SAFETY: plain getter.
    unsafe { r.error() }
        .map(|e| ns_error_text(&e))
        .unwrap_or_else(|| "no error reported".into())
}

fn first_track(
    path: &Path,
    video: bool,
) -> Result<(Retained<AVURLAsset>, Option<Retained<AVAssetTrack>>)> {
    if !path.exists() {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{} does not exist", path.display()),
        )));
    }
    let s = path
        .to_str()
        .ok_or_else(|| Error::Other(format!("non-UTF-8 path {}", path.display())))?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(s));
    // SAFETY: valid file URL; framework constants.
    unsafe {
        let asset = AVURLAsset::URLAssetWithURL_options(&url, None);
        let mt = if video {
            key(AVMediaTypeVideo, "AVMediaTypeVideo")?
        } else {
            key(AVMediaTypeAudio, "AVMediaTypeAudio")?
        };
        #[allow(deprecated)]
        let tracks: Retained<NSArray<AVAssetTrack>> = asset.tracksWithMediaType(mt);
        let track = tracks.firstObject();
        Ok((asset, track))
    }
}

fn open_track(
    path: &Path,
    video: bool,
    settings: Option<&NSDictionary<NSString, AnyObject>>,
    from_ns: Option<i64>,
) -> Result<Track> {
    let (asset, track) = first_track(path, video)?;
    let track = track.ok_or_else(|| {
        Error::Other(format!(
            "no {} track in {}",
            if video { "video" } else { "audio" },
            path.display()
        ))
    })?;
    // SAFETY: valid asset and track; the output is added before startReading.
    unsafe {
        let reader =
            AVAssetReader::initWithAsset_error(AVAssetReader::alloc(), &asset).map_err(|e| {
                Error::Other(format!(
                    "AVAssetReader {}: {}",
                    path.display(),
                    ns_error_text(&e)
                ))
            })?;
        let output = AVAssetReaderTrackOutput::assetReaderTrackOutputWithTrack_outputSettings(
            &track, settings,
        );
        output.setAlwaysCopiesSampleData(false);
        if !reader.canAddOutput(&output) {
            return Err(Error::Other(format!(
                "cannot read {} with these settings",
                path.display()
            )));
        }
        reader.addOutput(&output);
        if let Some(from) = from_ns.filter(|f| *f > 0) {
            reader.setTimeRange(CMTimeRange {
                start: ns_to_cmtime(from),
                duration: kCMTimePositiveInfinity,
            });
        }
        if !reader.startReading() {
            return Err(Error::Other(format!(
                "startReading {}: {}",
                path.display(),
                reader_error(&reader)
            )));
        }
        Ok(Track {
            reader,
            output,
            track,
        })
    }
}

impl Track {
    fn next(&self) -> Option<Retained<CMSampleBuffer>> {
        // SAFETY: reading has started.
        unsafe { self.output.copyNextSampleBuffer() }
    }

    fn failed(&self) -> Option<String> {
        // SAFETY: plain getter.
        (unsafe { self.reader.status() } == AVAssetReaderStatus::Failed)
            .then(|| reader_error(&self.reader))
    }
}

impl Drop for Track {
    fn drop(&mut self) {
        // SAFETY: cancelling is allowed in any state.
        unsafe { self.reader.cancelReading() };
    }
}

pub(crate) struct MacVideoReader {
    t: Track,
    size: SizePx,
}

impl MacVideoReader {
    pub(crate) fn open(path: &Path, from_ns: Option<i64>) -> Result<MacVideoReader> {
        let pf = num_i(i64::from(PIX_420V));
        let empty: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::new();
        // SAFETY: CoreVideo key constants.
        let settings = unsafe {
            dict(&[
                (cfstr_as_ns(kCVPixelBufferPixelFormatTypeKey), as_obj(&*pf)),
                (
                    cfstr_as_ns(kCVPixelBufferIOSurfacePropertiesKey),
                    as_obj(&*empty),
                ),
            ])
        };
        let t = open_track(path, true, Some(&settings), from_ns)?;
        // SAFETY: plain getter.
        let sz = unsafe { t.track.naturalSize() };
        Ok(MacVideoReader {
            t,
            size: SizePx {
                w: sz.width.round().max(0.0) as u32,
                h: sz.height.round().max(0.0) as u32,
            },
        })
    }
}

impl VideoReader for MacVideoReader {
    fn size(&self) -> SizePx {
        self.size
    }

    fn next_frame(&mut self) -> Result<Option<DecodedFrame>> {
        while let Some(sb) = self.t.next() {
            let Some(frame) = MacPixelFrame::from_sample_buffer(&sb) else {
                continue;
            };
            // SAFETY: valid sample buffer.
            let pts = unsafe { sb.presentation_time_stamp() };
            let Some(file_ns) = cmtime_to_ns(pts) else {
                continue;
            };
            return Ok(Some(DecodedFrame {
                file_ns,
                frame: FrameBuffer(Arc::new(frame)),
            }));
        }
        match self.t.failed() {
            Some(e) => Err(Error::Other(format!("decoding video: {e}"))),
            None => Ok(None),
        }
    }
}

pub(crate) struct MacAudioReader {
    t: Track,
    rate: u32,
    channels: usize,
    first_ns: i64,
    have_frames: i64,
    pending: VecDeque<f32>,
    done: bool,
}

impl MacAudioReader {
    pub(crate) fn open(
        path: &Path,
        rate: u32,
        channels: u16,
        from_ns: Option<i64>,
    ) -> Result<MacAudioReader> {
        if rate == 0 || channels == 0 {
            return Err(Error::Other(
                "open_audio needs rate and channels > 0".into(),
            ));
        }
        let fmt = num_i(i64::from(u32::from_be_bytes(*b"lpcm")));
        let yes = num_b(true);
        let no = num_b(false);
        let bits = num_i(32);
        let sr = num_f(f64::from(rate));
        let ch = num_i(i64::from(channels));
        let keys = [
            nsstr("AVFormatIDKey"),
            nsstr("AVLinearPCMIsFloatKey"),
            nsstr("AVLinearPCMBitDepthKey"),
            nsstr("AVLinearPCMIsNonInterleaved"),
            nsstr("AVLinearPCMIsBigEndianKey"),
            nsstr("AVSampleRateKey"),
            nsstr("AVNumberOfChannelsKey"),
        ];
        let settings = dict(&[
            (&keys[0], as_obj(&*fmt)),
            (&keys[1], as_obj(&*yes)),
            (&keys[2], as_obj(&*bits)),
            (&keys[3], as_obj(&*no)),
            (&keys[4], as_obj(&*no)),
            (&keys[5], as_obj(&*sr)),
            (&keys[6], as_obj(&*ch)),
        ]);
        let t = open_track(path, false, Some(&settings), from_ns)?;
        let mut r = MacAudioReader {
            t,
            rate,
            channels: usize::from(channels),
            first_ns: 0,
            have_frames: 0,
            pending: VecDeque::new(),
            done: false,
        };
        r.fill(true)?;
        Ok(r)
    }

    /// Decodes one more buffer into `pending`, placing it at its timestamp.
    fn fill(&mut self, first: bool) -> Result<()> {
        let Some(sb) = self.t.next() else {
            self.done = true;
            return match self.t.failed() {
                Some(e) => Err(Error::Other(format!("decoding audio: {e}"))),
                None => Ok(()),
            };
        };
        // SAFETY: valid sample buffer.
        let (pts, n, desc) = unsafe {
            (
                sb.presentation_time_stamp(),
                sb.num_samples(),
                sb.format_description(),
            )
        };
        let frames = u32::try_from(n).unwrap_or(0);
        if frames == 0 {
            return Ok(());
        }
        let desc = desc.ok_or_else(|| Error::Other("decoded audio has no format".into()))?;
        let fmt = PcmFormat::from_desc(desc)?;
        if asbd_of(&sb).map(|a| a.mChannelsPerFrame as usize) != Some(self.channels) {
            return Err(Error::Other(
                "decoded audio has an unexpected channel count".into(),
            ));
        }
        let pts_ns = cmtime_to_ns(pts).unwrap_or(0);
        if first {
            self.first_ns = pts_ns;
        }
        let at = ((pts_ns - self.first_ns) as f64 * f64::from(self.rate) / 1e9).round() as i64;
        if at > self.have_frames + 1 {
            let holes = (at - self.have_frames) as usize * self.channels;
            self.pending.extend(std::iter::repeat_n(0.0, holes));
            self.have_frames = at;
        }
        let bytes = copy_frames(&sb, &fmt, 0, frames)?;
        let data = bytes.into_iter().next().unwrap_or_default();
        self.pending.extend(
            data.as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
        );
        self.have_frames += i64::from(frames);
        Ok(())
    }
}

impl AudioReader for MacAudioReader {
    fn first_sample_file_ns(&self) -> i64 {
        self.first_ns
    }

    fn read(&mut self, out: &mut [f32]) -> Result<usize> {
        let want = out.len() / self.channels * self.channels;
        while self.pending.len() < want && !self.done {
            self.fill(false)?;
        }
        let n = want.min(self.pending.len() / self.channels * self.channels);
        for (o, s) in out.iter_mut().zip(self.pending.drain(..n)) {
            *o = s;
        }
        Ok(n)
    }
}

fn end_of(sb: &CMSampleBuffer, fallback_ns: i64) -> Option<(i64, i64)> {
    // SAFETY: valid sample buffer.
    let (pts, dur): (CMTime, CMTime) = unsafe { (sb.presentation_time_stamp(), sb.duration()) };
    let p = cmtime_to_ns(pts)?;
    let d = cmtime_to_ns(dur).filter(|d| *d > 0).unwrap_or(fallback_ns);
    Some((p, d))
}

/// Reads every packet without decoding; a fragmented file cut off by a crash stays readable up
/// to its last complete fragment. Duration comes from the track's edit-aware time range when it
/// has one, because raw AAC packets include encoder priming.
pub(crate) fn probe(path: &Path) -> Result<TrackProbe> {
    let not_readable = |e: String| TrackProbe {
        readable: false,
        duration_ns: None,
        samples: 0,
        size: None,
        error: Some(e),
    };
    let video = match first_track(path, true)? {
        (_, Some(_)) => true,
        (_, None) => match first_track(path, false)? {
            (_, Some(_)) => false,
            (_, None) => {
                return Ok(not_readable(format!(
                    "{}: no audio or video track",
                    path.display()
                )));
            }
        },
    };
    let t = match open_track(path, video, None, None) {
        Ok(t) => t,
        Err(e) => return Ok(not_readable(e.to_string())),
    };
    // SAFETY: plain getters.
    let (size, fps) = unsafe { (t.track.naturalSize(), t.track.nominalFrameRate()) };
    let frame_ns = if fps > 0.0 {
        (1e9 / f64::from(fps)) as i64
    } else {
        0
    };
    let (mut samples, mut end, mut rate) = (0u64, None::<i64>, 48_000.0);
    while let Some(sb) = t.next() {
        let Some((p, d)) = end_of(&sb, frame_ns) else {
            continue;
        };
        if video {
            // SAFETY: valid sample buffer.
            samples += u64::try_from(unsafe { sb.num_samples() }).unwrap_or(0);
        } else {
            rate = asbd_of(&sb).map_or(rate, |a| a.mSampleRate);
            samples += (d as f64 * rate / 1e9).round() as u64;
        }
        end = Some(end.map_or(p + d, |e: i64| e.max(p + d)));
    }
    let error = t.failed();
    // SAFETY: plain getter.
    let tr = unsafe { t.track.timeRange() };
    if let (Some(s), Some(d)) = (cmtime_to_ns(tr.start), cmtime_to_ns(tr.duration))
        && d > 0
    {
        end = Some(s + d);
        if !video {
            samples = (d as f64 * rate / 1e9).round() as u64;
        }
    }
    if samples == 0 {
        return Ok(not_readable(
            error.unwrap_or_else(|| format!("{}: no samples", path.display())),
        ));
    }
    Ok(TrackProbe {
        readable: true,
        duration_ns: end,
        samples,
        size: video.then(|| SizePx {
            w: size.width.round().max(0.0) as u32,
            h: size.height.round().max(0.0) as u32,
        }),
        error,
    })
}
