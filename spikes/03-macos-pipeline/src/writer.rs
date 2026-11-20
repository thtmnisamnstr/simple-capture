use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::AnyThread;
use objc2_av_foundation::*;
use objc2_core_audio_types::AudioStreamBasicDescription;
use objc2_core_media::{CMAudioFormatDescriptionGetStreamBasicDescription, CMSampleBuffer, CMTime};
use objc2_foundation::{NSString, NSURL};

use crate::util::*;

#[derive(Clone, Debug)]
pub struct VideoCfg {
    pub width: usize,
    pub height: usize,
    pub fps: f64,
    pub bitrate: f64,
    pub hevc: bool,
    pub realtime: bool,
}

pub struct TrackWriter {
    pub kind: &'static str,
    pub path: PathBuf,
    writer: Retained<AVAssetWriter>,
    input: Retained<AVAssetWriterInput>,
    pub started: bool,
    pub first_pts: Option<f64>,
    pub last_pts: Option<f64>,
    pub appended: u64,
    pub not_ready_drops: u64,
    pub append_failures: u64,
    pub description: String,
}

unsafe impl Send for TrackWriter {}

fn new_writer(path: &Path, frag_secs: f64) -> Result<Retained<AVAssetWriter>> {
    let _ = std::fs::remove_file(path);
    let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str().unwrap()));
    let ft = unsafe { AVFileTypeQuickTimeMovie.unwrap() };
    let w = unsafe { AVAssetWriter::initWithURL_fileType_error(AVAssetWriter::alloc(), &url, ft) }
        .map_err(|e| anyhow!("AVAssetWriter: {}", e.localizedDescription()))?;
    unsafe {
        w.setMovieFragmentInterval(cmtime_secs(frag_secs));
        w.setShouldOptimizeForNetworkUse(false);
    }
    Ok(w)
}

impl TrackWriter {
    pub fn video(kind: &'static str, path: &Path, cfg: &VideoCfg, frag_secs: f64) -> Result<Self> {
        let writer = new_writer(path, frag_secs)?;
        unsafe {
            let codec = if cfg.hevc { AVVideoCodecTypeHEVC.unwrap() } else { AVVideoCodecTypeH264.unwrap() };
            let hevc_profile = NSString::from_str("HEVC_Main_AutoLevel");
            let profile: &NSString = if cfg.hevc { &hevc_profile } else { AVVideoProfileLevelH264HighAutoLevel.unwrap() };
            let kfi_frames = num_i(cfg.fps.round() as i64);
            let kfi_dur = num_f(1.0);
            let br = num_f(cfg.bitrate);
            let reorder = num_b(false);
            let efps = num_f(cfg.fps);
            let compression = dict(&[
                (AVVideoAverageBitRateKey.unwrap(), as_obj(&*br)),
                (AVVideoMaxKeyFrameIntervalKey.unwrap(), as_obj(&*kfi_frames)),
                (AVVideoMaxKeyFrameIntervalDurationKey.unwrap(), as_obj(&*kfi_dur)),
                (AVVideoAllowFrameReorderingKey.unwrap(), as_obj(&*reorder)),
                (AVVideoExpectedSourceFrameRateKey.unwrap(), as_obj(&*efps)),
                (AVVideoProfileLevelKey.unwrap(), as_obj(profile)),
            ]);
            let color = dict(&[
                (AVVideoColorPrimariesKey.unwrap(), as_obj(AVVideoColorPrimaries_ITU_R_709_2.unwrap())),
                (AVVideoTransferFunctionKey.unwrap(), as_obj(AVVideoTransferFunction_ITU_R_709_2.unwrap())),
                (AVVideoYCbCrMatrixKey.unwrap(), as_obj(AVVideoYCbCrMatrix_ITU_R_709_2.unwrap())),
            ]);
            let wv = num_i(cfg.width as i64);
            let hv = num_i(cfg.height as i64);
            let settings = dict(&[
                (AVVideoCodecKey.unwrap(), as_obj(codec)),
                (AVVideoWidthKey.unwrap(), as_obj(&*wv)),
                (AVVideoHeightKey.unwrap(), as_obj(&*hv)),
                (AVVideoCompressionPropertiesKey.unwrap(), as_obj(&*compression)),
                (AVVideoColorPropertiesKey.unwrap(), as_obj(&*color)),
            ]);
            let mt = AVMediaTypeVideo.unwrap();
            if !writer.canApplyOutputSettings_forMediaType(Some(&settings), mt) {
                return Err(anyhow!("writer rejects video settings {}x{}", cfg.width, cfg.height));
            }
            let input = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(mt, Some(&settings));
            input.setExpectsMediaDataInRealTime(cfg.realtime);
            if !writer.canAddInput(&input) {
                return Err(anyhow!("cannot add video input"));
            }
            writer.addInput(&input);
            let desc = format!(
                "{} {}x{} avg {:.1} Mbps, keyframe <=1 s / {} frames, no B-frames, BT.709 tags",
                if cfg.hevc { "HEVC" } else { "H.264" },
                cfg.width,
                cfg.height,
                cfg.bitrate / 1e6,
                cfg.fps.round()
            );
            Ok(Self::wrap(kind, path, writer, input, desc))
        }
    }

    pub fn aac_for(kind: &'static str, path: &Path, sb: &CMSampleBuffer, frag_secs: f64) -> Result<Self> {
        let asbd = asbd_of(sb).ok_or_else(|| anyhow!("no ASBD on audio buffer"))?;
        let channels = asbd.mChannelsPerFrame.clamp(1, 2);
        let rate = asbd.mSampleRate;
        let writer = new_writer(path, frag_secs)?;
        unsafe {
            let fmt = num_i(u32::from_be_bytes(*b"aac ") as i64);
            let sr = num_f(rate);
            let ch = num_i(channels as i64);
            let br = num_i(if channels == 1 { 96_000 } else { 160_000 });
            let (k1, k2, k3, k4) = (nsstr("AVFormatIDKey"), nsstr("AVSampleRateKey"), nsstr("AVNumberOfChannelsKey"), nsstr("AVEncoderBitRateKey"));
            let settings = dict(&[(&k1, as_obj(&*fmt)), (&k2, as_obj(&*sr)), (&k3, as_obj(&*ch)), (&k4, as_obj(&*br))]);
            let mt = AVMediaTypeAudio.unwrap();
            let input = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(mt, Some(&settings));
            input.setExpectsMediaDataInRealTime(true);
            if !writer.canAddInput(&input) {
                return Err(anyhow!("cannot add audio input"));
            }
            writer.addInput(&input);
            let desc = format!(
                "AAC {} ch @ {} Hz, {} kbps (source: {} ch, fmt {}, {} bits, flags {:#x})",
                channels,
                rate,
                if channels == 1 { 96 } else { 160 },
                asbd.mChannelsPerFrame,
                fourcc(asbd.mFormatID),
                asbd.mBitsPerChannel,
                asbd.mFormatFlags
            );
            Ok(Self::wrap(kind, path, writer, input, desc))
        }
    }

    fn wrap(kind: &'static str, path: &Path, writer: Retained<AVAssetWriter>, input: Retained<AVAssetWriterInput>, description: String) -> Self {
        Self {
            kind,
            path: path.to_path_buf(),
            writer,
            input,
            started: false,
            first_pts: None,
            last_pts: None,
            appended: 0,
            not_ready_drops: 0,
            append_failures: 0,
            description,
        }
    }

    pub fn raw(&self) -> (&AVAssetWriter, &AVAssetWriterInput) {
        (&self.writer, &self.input)
    }

    pub fn mark_started(&mut self, first: f64) {
        self.started = true;
        self.first_pts = Some(first);
    }

    pub fn ready(&self) -> bool {
        unsafe { self.input.isReadyForMoreMediaData() }
    }

    pub fn append(&mut self, sb: &CMSampleBuffer) -> bool {
        let pts = unsafe { sb.presentation_time_stamp() };
        unsafe {
            if !self.started {
                if !self.writer.startWriting() {
                    self.append_failures += 1;
                    eprintln!("[{}] startWriting failed: {:?}", self.kind, self.writer.error().map(|e| e.localizedDescription()));
                    return false;
                }
                self.writer.startSessionAtSourceTime(pts);
                self.started = true;
                self.first_pts = Some(secs(pts));
                let side = self.path.with_extension("start.json");
                let v = serde_json::json!({"kind": self.kind, "first_pts_host_s": secs(pts), "encoder": self.description});
                let _ = write_json(&side, &v);
            }
            if self.writer.status() != AVAssetWriterStatus::Writing {
                self.append_failures += 1;
                return false;
            }
            if !self.input.isReadyForMoreMediaData() {
                self.not_ready_drops += 1;
                return false;
            }
            if self.input.appendSampleBuffer(sb) {
                self.appended += 1;
                self.last_pts = Some(secs(pts));
                true
            } else {
                self.append_failures += 1;
                if self.append_failures < 3 {
                    eprintln!("[{}] append failed: {:?}", self.kind, self.writer.error().map(|e| e.localizedDescription()));
                }
                false
            }
        }
    }

    pub fn append_wait(&mut self, sb: &CMSampleBuffer) -> bool {
        while self.started && !self.ready() {
            std::thread::sleep(Duration::from_micros(200));
        }
        self.append(sb)
    }

    pub fn finish(&mut self, end: Option<CMTime>) -> Result<()> {
        if !self.started {
            unsafe { self.writer.cancelWriting() };
            return Err(anyhow!("[{}] never started", self.kind));
        }
        unsafe {
            self.input.markAsFinished();
            if let Some(t) = end {
                self.writer.endSessionAtSourceTime(t);
            }
            let (tx, rx) = mpsc::channel::<()>();
            let blk = RcBlock::new(move || {
                let _ = tx.send(());
            });
            self.writer.finishWritingWithCompletionHandler(&blk);
            if rx.recv_timeout(Duration::from_secs(30)).is_err() {
                return Err(anyhow!("[{}] finishWriting timed out", self.kind));
            }
            if self.writer.status() != AVAssetWriterStatus::Completed {
                return Err(anyhow!(
                    "[{}] finish status {:?}: {:?}",
                    self.kind,
                    self.writer.status(),
                    self.writer.error().map(|e| e.localizedDescription())
                ));
            }
        }
        Ok(())
    }

    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({
            "kind": self.kind,
            "file": self.path.file_name().unwrap().to_string_lossy(),
            "encoder": self.description,
            "first_pts_host_s": self.first_pts,
            "last_pts_host_s": self.last_pts,
            "appended": self.appended,
            "encoder_not_ready_drops": self.not_ready_drops,
            "append_failures": self.append_failures,
            "bytes": std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0),
        })
    }
}

pub fn asbd_of(sb: &CMSampleBuffer) -> Option<AudioStreamBasicDescription> {
    unsafe {
        let fd = sb.format_description()?;
        let p = CMAudioFormatDescriptionGetStreamBasicDescription(&fd);
        if p.is_null() {
            None
        } else {
            Some(*p)
        }
    }
}
