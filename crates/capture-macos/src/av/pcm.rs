use std::ptr::{self, NonNull};

use objc2_core_audio_types::{
    AudioBuffer as CaBuffer, AudioBufferList, AudioStreamBasicDescription,
};
use objc2_core_foundation::CFRetained;
use objc2_core_media::{
    CMAudioFormatDescriptionCreate, CMAudioSampleBufferCreateWithPacketDescriptions,
    CMFormatDescription, CMSampleBuffer, CMTime,
};
use sc_platform::{Error, Result};

use super::frames::{LPCM, asbd_of_desc};

pub(crate) const FLAG_FLOAT: u32 = 1;
#[cfg(test)]
pub(crate) const FLAG_SIGNED_INT: u32 = 4;
pub(crate) const FLAG_PACKED: u32 = 8;
pub(crate) const FLAG_NON_INTERLEAVED: u32 = 1 << 5;
const MAX_BUFFERS: usize = 16;

#[repr(C)]
struct AblN {
    n: u32,
    bufs: [CaBuffer; MAX_BUFFERS],
}

fn os(code: i32, what: &str) -> Error {
    Error::Os {
        code: i64::from(code),
        message: what.to_string(),
    }
}

/// An LPCM format: its description plus the ASBD it holds.
#[derive(Clone)]
pub(crate) struct PcmFormat {
    pub desc: CFRetained<CMFormatDescription>,
    pub asbd: AudioStreamBasicDescription,
}

// SAFETY: format descriptions are immutable and atomically reference counted.
unsafe impl Send for PcmFormat {}

impl PcmFormat {
    pub fn from_desc(desc: CFRetained<CMFormatDescription>) -> Result<PcmFormat> {
        let asbd = asbd_of_desc(&desc)
            .ok_or_else(|| Error::Other("audio buffer has no stream description".into()))?;
        if asbd.mFormatID != LPCM || asbd.mBytesPerFrame == 0 || asbd.mChannelsPerFrame == 0 {
            return Err(Error::Other(format!(
                "audio buffer is not packed LPCM (format {}, {} bytes/frame)",
                crate::util::fourcc(asbd.mFormatID),
                asbd.mBytesPerFrame
            )));
        }
        Ok(PcmFormat { desc, asbd })
    }

    pub fn f32_interleaved(rate: u32, channels: u16) -> Result<PcmFormat> {
        let ch = u32::from(channels.max(1));
        Self::from_asbd(AudioStreamBasicDescription {
            mSampleRate: f64::from(rate),
            mFormatID: LPCM,
            mFormatFlags: FLAG_FLOAT | FLAG_PACKED,
            mBytesPerPacket: 4 * ch,
            mFramesPerPacket: 1,
            mBytesPerFrame: 4 * ch,
            mChannelsPerFrame: ch,
            mBitsPerChannel: 32,
            mReserved: 0,
        })
    }

    pub fn from_asbd(asbd: AudioStreamBasicDescription) -> Result<PcmFormat> {
        let mut out: *const CMFormatDescription = ptr::null();
        // SAFETY: valid ASBD; no layout, cookie or extensions; `out` receives a +1 reference.
        let rc = unsafe {
            CMAudioFormatDescriptionCreate(
                None,
                NonNull::from(&asbd),
                0,
                ptr::null(),
                0,
                ptr::null(),
                None,
                NonNull::from(&mut out),
            )
        };
        let desc = NonNull::new(out.cast_mut())
            .filter(|_| rc == 0)
            .ok_or_else(|| os(rc, "CMAudioFormatDescriptionCreate"))?;
        // SAFETY: created above with +1.
        let desc = unsafe { CFRetained::from_raw(desc) };
        Ok(PcmFormat { desc, asbd })
    }

    pub fn rate(&self) -> f64 {
        self.asbd.mSampleRate
    }

    pub fn non_interleaved(&self) -> bool {
        self.asbd.mFormatFlags & FLAG_NON_INTERLEAVED != 0
    }

    pub fn buffers(&self) -> usize {
        if self.non_interleaved() {
            self.asbd.mChannelsPerFrame as usize
        } else {
            1
        }
    }

    /// Bytes per frame in each buffer.
    pub fn buffer_stride(&self) -> usize {
        self.asbd.mBytesPerFrame as usize
    }

    pub fn same_as(&self, other: &PcmFormat) -> bool {
        let (a, b) = (&self.asbd, &other.asbd);
        a.mSampleRate == b.mSampleRate
            && a.mFormatFlags == b.mFormatFlags
            && a.mBytesPerFrame == b.mBytesPerFrame
            && a.mChannelsPerFrame == b.mChannelsPerFrame
            && a.mBitsPerChannel == b.mBitsPerChannel
    }
}

fn abl(fmt: &PcmFormat, bufs: &mut [Vec<u8>]) -> Result<AblN> {
    if bufs.len() > MAX_BUFFERS || bufs.len() != fmt.buffers() {
        return Err(Error::Other(format!(
            "audio: {} buffers for a {}-buffer format",
            bufs.len(),
            fmt.buffers()
        )));
    }
    let empty = CaBuffer {
        mNumberChannels: 0,
        mDataByteSize: 0,
        mData: ptr::null_mut(),
    };
    let mut list = AblN {
        n: bufs.len() as u32,
        bufs: [empty; MAX_BUFFERS],
    };
    let per_buffer_ch = if fmt.non_interleaved() {
        1
    } else {
        fmt.asbd.mChannelsPerFrame
    };
    for (slot, b) in list.bufs.iter_mut().zip(bufs.iter_mut()) {
        *slot = CaBuffer {
            mNumberChannels: per_buffer_ch,
            mDataByteSize: u32::try_from(b.len())
                .map_err(|_| Error::Other("audio buffer over 4 GiB".into()))?,
            mData: b.as_mut_ptr().cast(),
        };
    }
    Ok(list)
}

/// Copies `frames` frames starting at `offset` out of an LPCM sample buffer, in its own format.
pub(crate) fn copy_frames(
    sb: &CMSampleBuffer,
    fmt: &PcmFormat,
    offset: u32,
    frames: u32,
) -> Result<Vec<Vec<u8>>> {
    let mut bufs = vec![vec![0u8; frames as usize * fmt.buffer_stride()]; fmt.buffers()];
    let mut list = abl(fmt, &mut bufs)?;
    // SAFETY: `list` describes `bufs`, sized for `frames` frames, and outlives the call.
    let rc = unsafe {
        sb.copy_pcm_data_into_audio_buffer_list(
            i32::try_from(offset).unwrap_or(i32::MAX),
            i32::try_from(frames).unwrap_or(i32::MAX),
            NonNull::from(&mut list).cast::<AudioBufferList>(),
        )
    };
    if rc != 0 {
        return Err(os(rc, "CMSampleBufferCopyPCMDataIntoAudioBufferList"));
    }
    Ok(bufs)
}

/// A ready LPCM sample buffer holding `bufs` (one per channel when non-interleaved).
pub(crate) fn make_sample(
    fmt: &PcmFormat,
    mut bufs: Vec<Vec<u8>>,
    frames: u32,
    pts: CMTime,
) -> Result<CFRetained<CMSampleBuffer>> {
    let mut out: *mut CMSampleBuffer = ptr::null_mut();
    // SAFETY: no data buffer yet (filled below), no packet descriptions for LPCM.
    let rc = unsafe {
        CMAudioSampleBufferCreateWithPacketDescriptions(
            None,
            None,
            false,
            None,
            ptr::null_mut(),
            &fmt.desc,
            frames as isize,
            pts,
            ptr::null(),
            NonNull::from(&mut out),
        )
    };
    let sb = NonNull::new(out)
        .filter(|_| rc == 0)
        .ok_or_else(|| os(rc, "CMAudioSampleBufferCreateWithPacketDescriptions"))?;
    // SAFETY: created above with +1.
    let sb = unsafe { CFRetained::from_raw(sb) };
    let mut list = abl(fmt, &mut bufs)?;
    // SAFETY: `list` describes `bufs`; the call copies the data into a new block buffer.
    let rc = unsafe {
        sb.set_data_buffer_from_audio_buffer_list(
            None,
            None,
            0,
            NonNull::from(&mut list).cast::<AudioBufferList>(),
        )
    };
    if rc != 0 {
        return Err(os(rc, "CMSampleBufferSetDataBufferFromAudioBufferList"));
    }
    Ok(sb)
}

pub(crate) fn f32_bytes(samples: &[f32]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}
