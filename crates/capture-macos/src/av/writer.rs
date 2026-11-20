use std::path::{Path, PathBuf};
use std::ptr::{self, NonNull};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_av_foundation::{
    AVAssetWriter, AVAssetWriterInput, AVAssetWriterInputPixelBufferAdaptor, AVAssetWriterStatus,
    AVFileTypeQuickTimeMovie, AVMediaTypeAudio, AVMediaTypeVideo, AVVideoAllowFrameReorderingKey,
    AVVideoAverageBitRateKey, AVVideoCodecKey, AVVideoCodecTypeH264, AVVideoCodecTypeHEVC,
    AVVideoColorPrimaries_ITU_R_709_2, AVVideoColorPrimariesKey, AVVideoColorPropertiesKey,
    AVVideoCompressionPropertiesKey, AVVideoExpectedSourceFrameRateKey, AVVideoHeightKey,
    AVVideoMaxKeyFrameIntervalDurationKey, AVVideoMaxKeyFrameIntervalKey,
    AVVideoProfileLevelH264HighAutoLevel, AVVideoProfileLevelKey,
    AVVideoTransferFunction_ITU_R_709_2, AVVideoTransferFunctionKey, AVVideoWidthKey,
    AVVideoYCbCrMatrix_ITU_R_709_2, AVVideoYCbCrMatrixKey,
};
use objc2_core_foundation::CFRetained;
use objc2_core_media::CMTime;
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRowOfPlane,
    CVPixelBufferGetHeightOfPlane, CVPixelBufferGetWidthOfPlane, CVPixelBufferLockBaseAddress,
    CVPixelBufferLockFlags, CVPixelBufferPool, CVPixelBufferUnlockBaseAddress,
    kCVPixelBufferHeightKey, kCVPixelBufferIOSurfacePropertiesKey,
    kCVPixelBufferPixelFormatTypeKey, kCVPixelBufferWidthKey,
};
use objc2_foundation::{NSDictionary, NSString, NSURL};
use sc_geometry::SizePx;
use sc_platform::{
    Append, AudioBuffer, AudioData, AudioWriterSpec, Error, FinishedTrack, NativeFrame, Nv12View,
    Result, TrackWriter, VideoCodec, VideoFrame, VideoWriterSpec,
};

use super::frames::{MacAudioSample, MacPixelFrame, PIX_420V};
use super::pcm::{PcmFormat, copy_frames, f32_bytes, make_sample};
use crate::util::{
    as_obj, cfstr_as_ns, dict, ns_error_text, ns_to_cmtime, nsstr, num_b, num_f, num_i,
};

const FINISH_TIMEOUT: Duration = Duration::from_secs(30);
const H264_MAX_SIDE: u32 = 4096;

pub(crate) fn key(k: Option<&'static NSString>, name: &str) -> Result<&'static NSString> {
    k.ok_or_else(|| Error::Other(format!("AVFoundation constant {name} is unavailable")))
}

fn os(code: i32, what: &str) -> Error {
    Error::Os {
        code: i64::from(code),
        message: what.to_string(),
    }
}

struct Common {
    path: PathBuf,
    writer: Retained<AVAssetWriter>,
    input: Retained<AVAssetWriterInput>,
    samples: u64,
    last_ns: Option<i64>,
    end_ns: Option<i64>,
}

impl Common {
    fn new(
        path: &Path,
        frag_s: f64,
        mt: &NSString,
        settings: &NSDictionary<NSString, AnyObject>,
    ) -> Result<Common> {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::Io(e)),
        }
        let s = path
            .to_str()
            .ok_or_else(|| Error::Other(format!("non-UTF-8 path {}", path.display())))?;
        let url = NSURL::fileURLWithPath(&NSString::from_str(s));
        // SAFETY: framework constant.
        let ft = key(
            unsafe { AVFileTypeQuickTimeMovie },
            "AVFileTypeQuickTimeMovie",
        )?;
        // SAFETY: valid URL and file type.
        let writer =
            unsafe { AVAssetWriter::initWithURL_fileType_error(AVAssetWriter::alloc(), &url, ft) }
                .map_err(|e| {
                    Error::Other(format!(
                        "AVAssetWriter {}: {}",
                        path.display(),
                        ns_error_text(&e)
                    ))
                })?;
        // SAFETY: configuring a writer that has not started.
        let input = unsafe {
            writer.setMovieFragmentInterval(ns_to_cmtime((frag_s.max(0.1) * 1e9) as i64));
            writer.setShouldOptimizeForNetworkUse(false);
            if !writer.canApplyOutputSettings_forMediaType(Some(settings), mt) {
                return Err(Error::Other(format!(
                    "AVAssetWriter rejects the output settings for {}",
                    path.display()
                )));
            }
            let input = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(
                mt,
                Some(settings),
            );
            input.setExpectsMediaDataInRealTime(true);
            if !writer.canAddInput(&input) {
                return Err(Error::Other(format!(
                    "cannot add an input for {}",
                    path.display()
                )));
            }
            writer.addInput(&input);
            input
        };
        Ok(Common {
            path: path.to_path_buf(),
            writer,
            input,
            samples: 0,
            last_ns: None,
            end_ns: None,
        })
    }

    fn start(&self) -> Result<()> {
        // SAFETY: inputs are added; the session starts at file time 0.
        unsafe {
            if !self.writer.startWriting() {
                return Err(Error::Other(format!(
                    "startWriting {}: {}",
                    self.path.display(),
                    self.error_text()
                )));
            }
            self.writer.startSessionAtSourceTime(ns_to_cmtime(0));
        }
        Ok(())
    }

    fn error_text(&self) -> String {
        // SAFETY: plain getter.
        unsafe { self.writer.error() }
            .map(|e| ns_error_text(&e))
            .unwrap_or_else(|| "no error reported".into())
    }

    fn check_writing(&self) -> Result<()> {
        // SAFETY: plain getter.
        let st = unsafe { self.writer.status() };
        if st == AVAssetWriterStatus::Writing {
            Ok(())
        } else {
            Err(Error::Other(format!(
                "writer for {} is not writing (status {}): {}",
                self.path.display(),
                st.0,
                self.error_text()
            )))
        }
    }

    fn ready(&self) -> bool {
        // SAFETY: plain getter.
        unsafe { self.input.isReadyForMoreMediaData() }
    }

    fn extend_end(&mut self, end: i64) {
        self.end_ns = Some(self.end_ns.map_or(end, |e| e.max(end)));
    }

    fn finish(self, end_file_ns: Option<i64>) -> Result<FinishedTrack> {
        let duration = end_file_ns.or(self.end_ns);
        if self.samples == 0 {
            // SAFETY: cancelling a writer is allowed in any state.
            unsafe { self.writer.cancelWriting() };
            return Ok(FinishedTrack {
                path: self.path,
                duration_ns: end_file_ns,
                samples: 0,
            });
        }
        self.check_writing()?;
        let (tx, rx) = mpsc::channel::<()>();
        let blk = RcBlock::new(move || {
            let _ = tx.send(());
        });
        // SAFETY: the writer is writing; the block only signals the channel.
        unsafe {
            self.input.markAsFinished();
            if let Some(end) = duration {
                self.writer.endSessionAtSourceTime(ns_to_cmtime(end));
            }
            self.writer.finishWritingWithCompletionHandler(&blk);
        }
        crate::util::wait_on(
            &rx,
            FINISH_TIMEOUT,
            &format!("finishWriting {}", self.path.display()),
        )?;
        // SAFETY: plain getter.
        let st = unsafe { self.writer.status() };
        if st != AVAssetWriterStatus::Completed {
            return Err(Error::Other(format!(
                "finishWriting {}: status {}: {}",
                self.path.display(),
                st.0,
                self.error_text()
            )));
        }
        Ok(FinishedTrack {
            path: self.path,
            duration_ns: duration,
            samples: self.samples,
        })
    }
}

pub(crate) struct VideoWriter {
    c: Common,
    adaptor: Retained<AVAssetWriterInputPixelBufferAdaptor>,
    size: SizePx,
    frame_ns: i64,
}

// SAFETY: AVAssetWriter and its input/adaptor are used from one thread at a time (`&mut self`).
unsafe impl Send for VideoWriter {}

impl VideoWriter {
    pub(crate) fn create(path: &Path, spec: &VideoWriterSpec) -> Result<VideoWriter> {
        let (w, h) = (spec.size.w, spec.size.h);
        if w == 0 || h == 0 || spec.fps == 0 {
            return Err(Error::Other(format!(
                "video writer spec needs a size and fps: {spec:?}"
            )));
        }
        let hevc = match spec.codec {
            VideoCodec::Hevc => true,
            VideoCodec::H264 if w > H264_MAX_SIDE || h > H264_MAX_SIDE => {
                tracing::warn!("H.264 requested at {w}x{h} (> {H264_MAX_SIDE}); using HEVC");
                true
            }
            VideoCodec::H264 => false,
        };
        let fps = f64::from(spec.fps);
        let bitrate = (f64::from(w) * f64::from(h) * fps * spec.bits_per_pixel).max(100_000.0);
        let kfi_s = if spec.keyframe_interval_s > 0.0 {
            spec.keyframe_interval_s
        } else {
            1.0
        };
        let kfi_frames = num_i(((fps * kfi_s).round() as i64).max(1));
        let kfi_dur = num_f(kfi_s);
        let br = num_f(bitrate);
        let reorder = num_b(false);
        let efps = num_f(fps);
        let hevc_profile = nsstr("HEVC_Main_AutoLevel");
        let wv = num_i(i64::from(w));
        let hv = num_i(i64::from(h));
        // SAFETY: framework constants.
        let settings = unsafe {
            let profile: &NSString = if hevc {
                &hevc_profile
            } else {
                key(
                    AVVideoProfileLevelH264HighAutoLevel,
                    "AVVideoProfileLevelH264HighAutoLevel",
                )?
            };
            let codec = if hevc {
                key(AVVideoCodecTypeHEVC, "AVVideoCodecTypeHEVC")?
            } else {
                key(AVVideoCodecTypeH264, "AVVideoCodecTypeH264")?
            };
            let compression = dict(&[
                (
                    key(AVVideoAverageBitRateKey, "AverageBitRate")?,
                    as_obj(&*br),
                ),
                (
                    key(AVVideoMaxKeyFrameIntervalKey, "MaxKeyFrameInterval")?,
                    as_obj(&*kfi_frames),
                ),
                (
                    key(
                        AVVideoMaxKeyFrameIntervalDurationKey,
                        "MaxKeyFrameIntervalDuration",
                    )?,
                    as_obj(&*kfi_dur),
                ),
                (
                    key(AVVideoAllowFrameReorderingKey, "AllowFrameReordering")?,
                    as_obj(&*reorder),
                ),
                (
                    key(AVVideoExpectedSourceFrameRateKey, "ExpectedSourceFrameRate")?,
                    as_obj(&*efps),
                ),
                (
                    key(AVVideoProfileLevelKey, "ProfileLevel")?,
                    as_obj(profile),
                ),
            ]);
            let color = dict(&[
                (
                    key(AVVideoColorPrimariesKey, "ColorPrimaries")?,
                    as_obj(key(AVVideoColorPrimaries_ITU_R_709_2, "709 primaries")?),
                ),
                (
                    key(AVVideoTransferFunctionKey, "TransferFunction")?,
                    as_obj(key(AVVideoTransferFunction_ITU_R_709_2, "709 transfer")?),
                ),
                (
                    key(AVVideoYCbCrMatrixKey, "YCbCrMatrix")?,
                    as_obj(key(AVVideoYCbCrMatrix_ITU_R_709_2, "709 matrix")?),
                ),
            ]);
            dict(&[
                (key(AVVideoCodecKey, "Codec")?, as_obj(codec)),
                (key(AVVideoWidthKey, "Width")?, as_obj(&*wv)),
                (key(AVVideoHeightKey, "Height")?, as_obj(&*hv)),
                (
                    key(AVVideoCompressionPropertiesKey, "CompressionProperties")?,
                    as_obj(&*compression),
                ),
                (
                    key(AVVideoColorPropertiesKey, "ColorProperties")?,
                    as_obj(&*color),
                ),
            ])
        };
        // SAFETY: framework constant.
        let mt = key(unsafe { AVMediaTypeVideo }, "AVMediaTypeVideo")?;
        let c = Common::new(path, spec.fragment_interval_s, mt, &settings)?;
        let pf = num_i(i64::from(PIX_420V));
        let empty: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::new();
        // SAFETY: CoreVideo key constants; the adaptor is attached before startWriting.
        let adaptor = unsafe {
            let attrs = dict(&[
                (cfstr_as_ns(kCVPixelBufferPixelFormatTypeKey), as_obj(&*pf)),
                (cfstr_as_ns(kCVPixelBufferWidthKey), as_obj(&*wv)),
                (cfstr_as_ns(kCVPixelBufferHeightKey), as_obj(&*hv)),
                (
                    cfstr_as_ns(kCVPixelBufferIOSurfacePropertiesKey),
                    as_obj(&*empty),
                ),
            ]);
            AVAssetWriterInputPixelBufferAdaptor::assetWriterInputPixelBufferAdaptorWithAssetWriterInput_sourcePixelBufferAttributes(
                &c.input,
                Some(&attrs),
            )
        };
        c.start()?;
        Ok(VideoWriter {
            c,
            adaptor,
            size: spec.size,
            frame_ns: (1e9 / fps).round() as i64,
        })
    }

    fn pool(&self) -> Result<Retained<CVPixelBufferPool>> {
        // SAFETY: plain getter; the pool exists once writing started.
        unsafe { self.adaptor.pixelBufferPool() }.ok_or_else(|| {
            Error::Other(format!(
                "no pixel buffer pool for {}: {}",
                self.c.path.display(),
                self.c.error_text()
            ))
        })
    }

    pub(crate) fn new_pixel_buffer(&self) -> Result<CFRetained<CVPixelBuffer>> {
        let pool = self.pool()?;
        let mut out: *mut CVPixelBuffer = ptr::null_mut();
        // SAFETY: valid pool; `out` receives a +1 buffer.
        let rc =
            unsafe { CVPixelBufferPool::create_pixel_buffer(None, &pool, NonNull::from(&mut out)) };
        let pb = NonNull::new(out)
            .filter(|_| rc == 0)
            .ok_or_else(|| os(rc, "CVPixelBufferPoolCreatePixelBuffer"))?;
        // SAFETY: created above with +1.
        Ok(unsafe { CFRetained::from_raw(pb) })
    }

    fn copy_frame(&self, frame: &dyn NativeFrame) -> Result<CFRetained<CVPixelBuffer>> {
        if frame.size() != self.size {
            return Err(Error::Other(format!(
                "frame {}x{} does not match the {}x{} writer for {}",
                frame.size().w,
                frame.size().h,
                self.size.w,
                self.size.h,
                self.c.path.display()
            )));
        }
        let pb = self.new_pixel_buffer()?;
        let mut res = Ok(());
        frame.with_nv12(&mut |v| res = write_nv12(&pb, &v))?;
        res.map(|()| pb)
    }

    pub(crate) fn append_pixel_buffer(&mut self, pb: &CVPixelBuffer, file_ns: i64) -> Result<()> {
        // SAFETY: writing; valid pixel buffer.
        let ok = unsafe {
            self.adaptor
                .appendPixelBuffer_withPresentationTime(pb, ns_to_cmtime(file_ns))
        };
        if !ok {
            return Err(Error::Other(format!(
                "append video at {file_ns} ns to {}: {}",
                self.c.path.display(),
                self.c.error_text()
            )));
        }
        self.c.samples += 1;
        self.c.last_ns = Some(file_ns);
        self.c.extend_end(file_ns + self.frame_ns);
        Ok(())
    }
}

/// Copies an NV12 view into a locked 420v pixel buffer, converting full range to video range.
pub(crate) fn write_nv12(pb: &CVPixelBuffer, v: &Nv12View<'_>) -> Result<()> {
    // SAFETY: valid pixel buffer; unlocked below.
    let rc = unsafe { CVPixelBufferLockBaseAddress(pb, CVPixelBufferLockFlags(0)) };
    if rc != 0 {
        return Err(os(rc, "CVPixelBufferLockBaseAddress"));
    }
    let res = (|| {
        for plane in 0..2usize {
            let dst = CVPixelBufferGetBaseAddressOfPlane(pb, plane) as *mut u8;
            if dst.is_null() {
                return Err(Error::Other("pool buffer has a null plane".into()));
            }
            let stride = CVPixelBufferGetBytesPerRowOfPlane(pb, plane);
            let rows = CVPixelBufferGetHeightOfPlane(pb, plane);
            let row_bytes =
                CVPixelBufferGetWidthOfPlane(pb, plane) * if plane == 0 { 1 } else { 2 };
            let (src, src_stride) = if plane == 0 {
                (v.y, v.y_stride)
            } else {
                (v.uv, v.uv_stride)
            };
            let n = row_bytes.min(src_stride).min(stride);
            for r in 0..rows {
                let Some(line) = src.get(r * src_stride..r * src_stride + n) else {
                    break;
                };
                // SAFETY: row `r` of the locked plane has `stride >= n` writable bytes.
                let out = unsafe { std::slice::from_raw_parts_mut(dst.add(r * stride), n) };
                if v.full_range {
                    for (o, &s) in out.iter_mut().zip(line) {
                        let s = u32::from(s);
                        *o = if plane == 0 {
                            (16 + s * 219 / 255) as u8
                        } else {
                            (128 + (s as i32 - 128) * 224 / 255) as u8
                        };
                    }
                } else {
                    out.copy_from_slice(line);
                }
            }
        }
        Ok(())
    })();
    // SAFETY: balances the lock above.
    unsafe { CVPixelBufferUnlockBaseAddress(pb, CVPixelBufferLockFlags(0)) };
    res
}

impl TrackWriter for VideoWriter {
    fn append_video(&mut self, f: &VideoFrame, file_ns: i64) -> Result<Append> {
        self.c.check_writing()?;
        if let Some(last) = self.c.last_ns
            && file_ns <= last
        {
            return Err(Error::Other(format!(
                "video file_ns {file_ns} not after {last} in {}",
                self.c.path.display()
            )));
        }
        if !self.c.ready() {
            return Ok(Append::NotReady);
        }
        let frame = &*f.buffer.0;
        if let Some(m) = frame.as_any().downcast_ref::<MacPixelFrame>() {
            self.append_pixel_buffer(m.pixel_buffer(), file_ns)?;
        } else {
            let pb = self.copy_frame(frame)?;
            self.append_pixel_buffer(&pb, file_ns)?;
        }
        Ok(Append::Written)
    }

    fn append_audio(&mut self, _a: &AudioBuffer, _file_ns: i64, _skip: u32) -> Result<Append> {
        Err(Error::Other(format!(
            "{} is a video track",
            self.c.path.display()
        )))
    }

    fn append_silence(&mut self, _file_ns: i64, _frames: u32) -> Result<Append> {
        Err(Error::Other(format!(
            "{} is a video track",
            self.c.path.display()
        )))
    }

    fn is_ready(&self) -> bool {
        self.c.ready()
    }

    fn finish(self: Box<Self>, end_file_ns: Option<i64>) -> Result<FinishedTrack> {
        let me = *self;
        drop(me.adaptor);
        me.c.finish(end_file_ns)
    }
}

pub(crate) struct AudioWriter {
    c: Common,
    spec: AudioWriterSpec,
    fmt: Option<PcmFormat>,
}

// SAFETY: see VideoWriter.
unsafe impl Send for AudioWriter {}

impl AudioWriter {
    pub(crate) fn create(path: &Path, spec: &AudioWriterSpec) -> Result<AudioWriter> {
        if spec.sample_rate == 0 || spec.channels == 0 {
            return Err(Error::Other(format!(
                "audio writer spec needs a rate and channels: {spec:?}"
            )));
        }
        let fmt = num_i(i64::from(u32::from_be_bytes(*b"aac ")));
        let sr = num_f(f64::from(spec.sample_rate));
        let ch = num_i(i64::from(spec.channels));
        let br = num_i(i64::from(spec.bitrate.max(32_000)));
        let (k1, k2, k3, k4) = (
            nsstr("AVFormatIDKey"),
            nsstr("AVSampleRateKey"),
            nsstr("AVNumberOfChannelsKey"),
            nsstr("AVEncoderBitRateKey"),
        );
        let settings = dict(&[
            (&k1, as_obj(&*fmt)),
            (&k2, as_obj(&*sr)),
            (&k3, as_obj(&*ch)),
            (&k4, as_obj(&*br)),
        ]);
        // SAFETY: framework constant.
        let mt = key(unsafe { AVMediaTypeAudio }, "AVMediaTypeAudio")?;
        let c = Common::new(path, spec.fragment_interval_s, mt, &settings)?;
        c.start()?;
        Ok(AudioWriter {
            c,
            spec: spec.clone(),
            fmt: None,
        })
    }

    fn append_pcm(
        &mut self,
        fmt: PcmFormat,
        bufs: Vec<Vec<u8>>,
        frames: u32,
        file_ns: i64,
        skip: u32,
    ) -> Result<Append> {
        if frames == 0 {
            return Ok(Append::Written);
        }
        let rate = fmt.rate();
        let scale = rate.round() as i32;
        if scale <= 0 {
            return Err(Error::Other(format!("audio rate {rate} is not usable")));
        }
        if let Some(prev) = &self.fmt
            && !prev.same_as(&fmt)
        {
            tracing::warn!(
                "audio format changed mid-track in {} ({} Hz -> {} Hz)",
                self.c.path.display(),
                prev.rate(),
                rate
            );
        }
        let start = (file_ns as f64 * rate / 1e9).round() as i64 + i64::from(skip);
        // SAFETY: CMTimeMake has no preconditions.
        let pts = unsafe { CMTime::new(start, scale) };
        let sb = make_sample(&fmt, bufs, frames, pts)?;
        // SAFETY: writing; a ready LPCM sample buffer.
        let ok = unsafe { self.c.input.appendSampleBuffer(&sb) };
        if !ok {
            return Err(Error::Other(format!(
                "append audio at {file_ns} ns to {}: {}",
                self.c.path.display(),
                self.c.error_text()
            )));
        }
        self.c.samples += u64::from(frames);
        let start_ns = (start as f64 * 1e9 / rate).round() as i64;
        self.c.last_ns = Some(start_ns);
        self.c
            .extend_end(start_ns + (f64::from(frames) * 1e9 / rate).round() as i64);
        self.fmt = Some(fmt);
        Ok(Append::Written)
    }
}

impl TrackWriter for AudioWriter {
    fn append_video(&mut self, _f: &VideoFrame, _file_ns: i64) -> Result<Append> {
        Err(Error::Other(format!(
            "{} is an audio track",
            self.c.path.display()
        )))
    }

    /// `file_ns` is the file time of the buffer's first sample; writing starts `skip_frames`
    /// later.
    fn append_audio(&mut self, a: &AudioBuffer, file_ns: i64, skip_frames: u32) -> Result<Append> {
        self.c.check_writing()?;
        if !self.c.ready() {
            return Ok(Append::NotReady);
        }
        match &a.data {
            AudioData::Native(n) => {
                let s = n
                    .as_any()
                    .downcast_ref::<MacAudioSample>()
                    .ok_or(Error::Unsupported(
                        "MacWriterFactory: native audio must be a MacAudioSample",
                    ))?;
                let sb = s.sample_buffer();
                // SAFETY: valid sample buffer.
                let (desc, n) = unsafe { (sb.format_description(), sb.num_samples()) };
                let desc = desc.ok_or_else(|| Error::Other("audio buffer has no format".into()))?;
                let fmt = PcmFormat::from_desc(desc)?;
                let total = u32::try_from(n).unwrap_or(0);
                if skip_frames >= total {
                    return Ok(Append::Written);
                }
                let frames = total - skip_frames;
                let bufs = copy_frames(sb, &fmt, skip_frames, frames)?;
                self.append_pcm(fmt, bufs, frames, file_ns, skip_frames)
            }
            AudioData::F32Interleaved(data) => {
                let ch = usize::from(a.channels.max(1));
                if data.len() < a.frames as usize * ch {
                    return Err(Error::Other(format!(
                        "audio buffer has {} samples, {} frames x {ch} ch needed",
                        data.len(),
                        a.frames
                    )));
                }
                if skip_frames >= a.frames {
                    return Ok(Append::Written);
                }
                let fmt = match &self.fmt {
                    Some(f)
                        if f.rate() == f64::from(a.sample_rate)
                            && f.asbd.mChannelsPerFrame == u32::from(a.channels)
                            && !f.non_interleaved()
                            && f.asbd.mBitsPerChannel == 32 =>
                    {
                        f.clone()
                    }
                    _ => PcmFormat::f32_interleaved(a.sample_rate, a.channels)?,
                };
                let frames = a.frames - skip_frames;
                let bytes = f32_bytes(&data[skip_frames as usize * ch..a.frames as usize * ch]);
                self.append_pcm(fmt, vec![bytes], frames, file_ns, skip_frames)
            }
        }
    }

    fn append_silence(&mut self, file_ns: i64, frames: u32) -> Result<Append> {
        self.c.check_writing()?;
        if !self.c.ready() {
            return Ok(Append::NotReady);
        }
        let fmt = match &self.fmt {
            Some(f) => f.clone(),
            None => PcmFormat::f32_interleaved(self.spec.sample_rate, self.spec.channels)?,
        };
        let bufs = vec![vec![0u8; frames as usize * fmt.buffer_stride()]; fmt.buffers()];
        self.append_pcm(fmt, bufs, frames, file_ns, 0)
    }

    fn is_ready(&self) -> bool {
        self.c.ready()
    }

    fn finish(self: Box<Self>, end_file_ns: Option<i64>) -> Result<FinishedTrack> {
        self.c.finish(end_file_ns)
    }
}

fn texel(x: usize, y: usize) -> u8 {
    let gx = x / 2;
    let gy = y / 2;
    let line = (gy / 9).is_multiple_of(2);
    let mut hsh =
        (gx as u32).wrapping_mul(0x9E37_79B1) ^ ((gy / 9) as u32).wrapping_mul(0x85EB_CA77);
    hsh ^= hsh >> 15;
    let glyph = ((gx / 6) as u32).wrapping_mul(2_654_435_761) >> 28;
    if line && glyph > 3 && (hsh >> 7) & 3 != 0 && (gy % 9) < 7 {
        40
    } else {
        235
    }
}

/// Spike 3 `encbench` "scroll" content: a static text-like page, a scrolling region and a box.
fn scroll_frame(size: SizePx, k: usize) -> sc_platform::CpuNv12Frame {
    let (w, h) = (size.w as usize, size.h as usize);
    let mut f = sc_platform::CpuNv12Frame::filled(size, 235, 128, 128, false);
    let (bx0, bx1, by0, by1) = (w / 10, w * 6 / 10, h / 5, h * 4 / 5);
    let (mx, my, ms) = ((k * 40) % (w / 2).max(1) + w / 2, h / 3, h / 6);
    for row in 0..h {
        for x in 0..w {
            f.y[row * w + x] = if x >= mx && x < mx + ms && row >= my && row < my + ms {
                120
            } else if x >= bx0 && x < bx1 && row >= by0 && row < by1 {
                texel(x, row + k * 12)
            } else {
                texel(x, row)
            };
        }
    }
    let uv_stride = f.uv_stride();
    for row in 0..f.uv_rows() {
        for x in 0..uv_stride / 2 {
            if 2 * x >= mx && 2 * x < mx + ms && 2 * row >= my && 2 * row < my + ms {
                f.uv[row * uv_stride + 2 * x] = 200;
                f.uv[row * uv_stride + 2 * x + 1] = 60;
            }
        }
    }
    f
}

/// Frames per second the hardware encoder sustains for `size` at `fps` (spike 3 `encbench`:
/// 16 pre-filled IOSurface buffers cycled, appended as fast as the input accepts them).
pub(crate) fn encode_benchmark(
    size: SizePx,
    fps: u32,
    frames: u32,
    codec: VideoCodec,
) -> Result<f64> {
    let path = std::env::temp_dir().join(format!(
        "sc-encbench-{}-{}x{}-{}.mov",
        std::process::id(),
        size.w,
        size.h,
        crate::util::mach_now()
    ));
    let spec = VideoWriterSpec {
        size,
        fps: fps.max(1),
        codec,
        keyframe_interval_s: 1.0,
        fragment_interval_s: 1.0,
        bits_per_pixel: 0.05,
    };
    let res = (|| {
        let mut w = VideoWriter::create(&path, &spec)?;
        let mut bufs = Vec::with_capacity(16);
        for k in 0..16 {
            let src = scroll_frame(size, k);
            let pb = w.new_pixel_buffer()?;
            let mut r = Ok(());
            src.with_nv12(&mut |v| r = write_nv12(&pb, &v))?;
            r?;
            bufs.push(pb);
        }
        let frame_ns = 1e9 / f64::from(spec.fps);
        let started = Instant::now();
        let deadline = started + Duration::from_secs(120);
        for i in 0..frames.max(1) {
            while !w.c.ready() {
                w.c.check_writing()?;
                if Instant::now() > deadline {
                    return Err(Error::Timeout("encode_benchmark: encoder stalled".into()));
                }
                std::thread::sleep(Duration::from_micros(100));
            }
            let pb = &bufs[i as usize % bufs.len()];
            w.append_pixel_buffer(pb, (f64::from(i) * frame_ns).round() as i64)?;
        }
        drop(bufs);
        let fin = Box::new(w).finish(None)?;
        let secs = started.elapsed().as_secs_f64().max(1e-9);
        Ok(fin.samples as f64 / secs)
    })();
    let _ = std::fs::remove_file(&path);
    res
}
