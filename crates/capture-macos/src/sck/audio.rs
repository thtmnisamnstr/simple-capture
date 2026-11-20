use std::sync::{Arc, Mutex};

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_core_graphics::CGMainDisplayID;
use objc2_core_media::{CMAudioFormatDescriptionGetStreamBasicDescription, CMSampleBuffer, CMTime};
use objc2_foundation::NSArray;
use objc2_screen_capture_kit::{SCContentFilter, SCStreamConfiguration};
use sc_core::project::SystemAudioMode;
use sc_platform::{
    AppRef, Error, FrameSink, Result, StreamTag, SystemAudioSource, SystemAudioStream, WindowId,
};

use super::content;
use super::stream::{StreamHandle, StreamShared, StreamStats, open};
use crate::tcc;

const AUDIO_STREAM_TAG: u64 = u64::MAX - 1;
const FLAG_NON_INTERLEAVED: u32 = 1 << 5;
const FLAG_FLOAT: u32 = 1;

#[derive(Debug, Default)]
pub struct MacSystemAudio {
    _priv: (),
}

impl MacSystemAudio {
    pub fn new() -> MacSystemAudio {
        MacSystemAudio::default()
    }

    pub fn start_mac(
        &self,
        mode: SystemAudioMode,
        exclude: &[WindowId],
        sink: Arc<dyn FrameSink>,
    ) -> Result<MacSystemAudioStream> {
        self.start_inner(mode, exclude, None, sink)
    }

    /// Starts already filtered to `app` (instead of `follow_app` on a running stream).
    pub fn start_following(
        &self,
        app: &AppRef,
        sink: Arc<dyn FrameSink>,
    ) -> Result<MacSystemAudioStream> {
        self.start_inner(SystemAudioMode::App, &[], Some(app.pid), sink)
    }

    fn start_inner(
        &self,
        mode: SystemAudioMode,
        exclude: &[WindowId],
        app: Option<i32>,
        sink: Arc<dyn FrameSink>,
    ) -> Result<MacSystemAudioStream> {
        if mode == SystemAudioMode::Off {
            return Err(Error::Other("system audio: mode is off".into()));
        }
        tcc::guard("SCStream start (system audio)")?;
        let display = CGMainDisplayID();
        let filter = match app {
            Some(pid) => app_audio_filter(display, pid)?,
            None => display_audio_filter(display, exclude)?,
        };
        let cfg = unsafe {
            let c = SCStreamConfiguration::new();
            c.setWidth(128);
            c.setHeight(72);
            c.setMinimumFrameInterval(CMTime::new(1, 1));
            c.setQueueDepth(3);
            c.setShowsCursor(false);
            c.setCapturesAudio(true);
            c.setExcludesCurrentProcessAudio(true);
            c.setSampleRate(48_000);
            c.setChannelCount(2);
            c
        };
        let shared = Arc::new(StreamShared::new(
            StreamTag(AUDIO_STREAM_TAG),
            sink,
            false,
            true,
            None,
        ));
        let handle = open(&filter, &cfg, shared.clone(), false, true)?;
        handle.start()?;
        Ok(MacSystemAudioStream {
            handle,
            shared,
            display,
            exclude: exclude.to_vec(),
            following: Mutex::new(app),
        })
    }
}

fn display_audio_filter(display: u32, exclude: &[WindowId]) -> Result<Retained<SCContentFilter>> {
    let content = content::shareable(false)?;
    let d = content::find_display(&content, display)
        .ok_or_else(|| Error::NotFound(format!("display {display} not shareable")))?;
    let wins = content::exclusion_windows(&content, exclude);
    Ok(unsafe {
        SCContentFilter::initWithDisplay_excludingWindows(
            SCContentFilter::alloc(),
            &d,
            &NSArray::from_retained_slice(&wins),
        )
    })
}

fn app_audio_filter(display: u32, pid: i32) -> Result<Retained<SCContentFilter>> {
    let content = content::shareable(false)?;
    let d = content::find_display(&content, display)
        .ok_or_else(|| Error::NotFound(format!("display {display} not shareable")))?;
    let app = content::find_app(&content, pid)
        .ok_or_else(|| Error::NotFound(format!("application pid {pid} not shareable")))?;
    Ok(unsafe {
        SCContentFilter::initWithDisplay_includingApplications_exceptingWindows(
            SCContentFilter::alloc(),
            &d,
            &NSArray::from_retained_slice(&[app]),
            &NSArray::new(),
        )
    })
}

impl SystemAudioSource for MacSystemAudio {
    fn start(
        &self,
        mode: SystemAudioMode,
        exclude: &[WindowId],
        sink: Arc<dyn FrameSink>,
    ) -> Result<Box<dyn SystemAudioStream>> {
        Ok(Box::new(self.start_mac(mode, exclude, sink)?))
    }
}

pub struct MacSystemAudioStream {
    handle: StreamHandle,
    shared: Arc<StreamShared>,
    display: u32,
    exclude: Vec<WindowId>,
    following: Mutex<Option<i32>>,
}

impl MacSystemAudioStream {
    pub fn stats(&self) -> StreamStats {
        self.shared.stats()
    }
}

impl SystemAudioStream for MacSystemAudioStream {
    fn follow_app(&self, app: Option<&AppRef>) -> Result<()> {
        tcc::guard("SCStream updateContentFilter (system audio)")?;
        let filter = match app {
            Some(a) => app_audio_filter(self.display, a.pid)?,
            None => display_audio_filter(self.display, &self.exclude)?,
        };
        self.handle.update_filter(&filter)?;
        if let Ok(mut f) = self.following.lock() {
            *f = app.map(|a| a.pid);
        }
        Ok(())
    }

    fn stop(self: Box<Self>) -> Result<()> {
        self.handle.stop()
    }
}

pub(crate) fn asbd_rate_channels(sb: &CMSampleBuffer) -> Option<(u32, u16)> {
    let fd = unsafe { sb.format_description() }?;
    let asbd = unsafe { CMAudioFormatDescriptionGetStreamBasicDescription(&fd) };
    let a = unsafe { asbd.as_ref() }?;
    Some((a.mSampleRate.round() as u32, a.mChannelsPerFrame as u16))
}

/// Float32 PCM of an SCK audio buffer as interleaved samples, with (rate, channels).
pub fn pcm_f32_interleaved(sb: &CMSampleBuffer) -> Result<(Vec<f32>, u32, u16)> {
    let fd = unsafe { sb.format_description() }
        .ok_or_else(|| Error::Other("audio: no format description".into()))?;
    let asbd = unsafe { CMAudioFormatDescriptionGetStreamBasicDescription(&fd) };
    let a = unsafe { asbd.as_ref() }.ok_or_else(|| Error::Other("audio: no ASBD".into()))?;
    if a.mFormatFlags & FLAG_FLOAT == 0 || a.mBitsPerChannel != 32 {
        return Err(Error::Other(format!(
            "audio: not Float32 (flags {:#x}, bits {})",
            a.mFormatFlags, a.mBitsPerChannel
        )));
    }
    let ch = a.mChannelsPerFrame.max(1) as usize;
    let frames = unsafe { sb.num_samples() }.max(0) as usize;
    let bb = unsafe { sb.data_buffer() }.ok_or_else(|| Error::Other("audio: no data".into()))?;
    let len = unsafe { bb.data_length() };
    if len < frames * ch * 4 {
        return Err(Error::Other(format!(
            "audio: {len} bytes for {frames} frames × {ch} channels"
        )));
    }
    let mut raw = vec![0f32; frames * ch];
    let rc = unsafe {
        bb.copy_data_bytes(
            0,
            frames * ch * 4,
            std::ptr::NonNull::new_unchecked(raw.as_mut_ptr().cast()),
        )
    };
    if rc != 0 {
        return Err(Error::Os {
            code: i64::from(rc),
            message: "CMBlockBufferCopyDataBytes".into(),
        });
    }
    let out = if a.mFormatFlags & FLAG_NON_INTERLEAVED != 0 && ch > 1 {
        let mut il = vec![0f32; frames * ch];
        for c in 0..ch {
            for i in 0..frames {
                il[i * ch + c] = raw[c * frames + i];
            }
        }
        il
    } else {
        raw
    };
    Ok((out, a.mSampleRate.round() as u32, ch as u16))
}
