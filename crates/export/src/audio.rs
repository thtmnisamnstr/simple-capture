//! Mixes every audio track to one WAV for the export.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sc_core::project::ProjectV1;
use sc_decode::TrackAudio;
use sc_helpers::wav::{WavFormat, WavWriter};
use sc_platform::{Decoder, Error, Result};

use crate::recipe::{MIX_CHANNELS, MIX_RATE, audio_track_ids};

const BLOCK_FRAMES: usize = 4800;

/// `floor(frames·48000/fps)`: frame N ends at sample `floor((N+1)·48000/fps)`.
pub fn mix_samples(fps: u32, frames: u64) -> u64 {
    if fps == 0 {
        return 0;
    }
    let n = u128::from(frames) * u128::from(MIX_RATE) / u128::from(fps);
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// Mic and system audio at 0 dB, 48 kHz stereo f32, clamped to ±1. `false` = the project has no
/// audio tracks and no WAV was written.
pub fn mix_to_wav(
    project: &ProjectV1,
    dir: &Path,
    decoder: Arc<dyn Decoder>,
    fps: u32,
    frames: u64,
    wav: &Path,
) -> Result<bool> {
    mix_to_wav_cancelable(
        project,
        dir,
        decoder,
        fps,
        frames,
        wav,
        &AtomicBool::new(false),
    )
}

pub(crate) fn mix_to_wav_cancelable(
    project: &ProjectV1,
    dir: &Path,
    decoder: Arc<dyn Decoder>,
    fps: u32,
    frames: u64,
    wav: &Path,
    cancel: &AtomicBool,
) -> Result<bool> {
    let ids = audio_track_ids(project);
    if ids.is_empty() {
        return Ok(false);
    }
    let mut tracks = ids
        .iter()
        .map(|id| TrackAudio::open(project, dir, id, decoder.clone(), MIX_RATE, MIX_CHANNELS))
        .collect::<Result<Vec<_>>>()?;
    let mut w = WavWriter::create(wav, MIX_RATE, MIX_CHANNELS, WavFormat::F32)
        .map_err(|e| Error::Other(format!("sc_export: mix wav: {e}")))?;
    let ch = usize::from(MIX_CHANNELS);
    let mut mix = vec![0f32; BLOCK_FRAMES * ch];
    let mut buf = vec![0f32; BLOCK_FRAMES * ch];
    let mut left = mix_samples(fps, frames);
    while left > 0 {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Other("sc_export: audio mix canceled".into()));
        }
        let n = usize::try_from(left)
            .unwrap_or(usize::MAX)
            .min(BLOCK_FRAMES)
            * ch;
        mix[..n].fill(0.0);
        for t in &mut tracks {
            t.read(&mut buf[..n])?;
            for (m, s) in mix[..n].iter_mut().zip(&buf[..n]) {
                *m += *s;
            }
        }
        for m in &mut mix[..n] {
            *m = if m.is_finite() {
                m.clamp(-1.0, 1.0)
            } else {
                0.0
            };
        }
        w.write_f32(&mix[..n])
            .map_err(|e| Error::Other(format!("sc_export: mix wav: {e}")))?;
        left -= (n / ch) as u64;
    }
    w.finish()
        .map_err(|e| Error::Other(format!("sc_export: mix wav: {e}")))?;
    Ok(true)
}
