use std::path::Path;
use std::sync::Arc;

use sc_core::project::ProjectV1;
use sc_platform::{AudioReader, Decoder, Error, Result};

pub struct TrackAudio {
    track_id: String,
    reader: Option<Box<dyn AudioReader>>,
    channels: usize,
    pad_frames: u64,
    skip_frames: u64,
}

fn read_err(track_id: &str, e: Error) -> Error {
    Error::Other(format!("sc_decode: reading audio track {track_id}: {e}"))
}

impl TrackAudio {
    pub fn open(
        project: &ProjectV1,
        dir: &Path,
        track_id: &str,
        decoder: Arc<dyn Decoder>,
        rate: u32,
        channels: u16,
    ) -> Result<TrackAudio> {
        if rate == 0 || channels == 0 {
            return Err(Error::Other(format!(
                "sc_decode: track {track_id}: audio needs rate and channels > 0 ({rate} Hz, {channels} ch)"
            )));
        }
        let track = project
            .recording
            .tracks
            .iter()
            .find(|t| t.id == track_id)
            .ok_or_else(|| {
                Error::NotFound(format!(
                    "sc_decode: audio track {track_id} is not in project.json"
                ))
            })?;
        let path = dir.join(&track.file);
        let reader = decoder.open_audio(&path, rate, channels).map_err(|e| {
            Error::Other(format!(
                "sc_decode: opening audio track {track_id} ({}): {e}",
                path.display()
            ))
        })?;
        let offset = i128::from(track.first_sample_offset_ns);
        let offset_frames =
            (2 * offset * i128::from(rate) + 1_000_000_000).div_euclid(2_000_000_000);
        let (pad_frames, skip_frames) = if offset_frames >= 0 {
            (u64::try_from(offset_frames).unwrap_or(u64::MAX), 0)
        } else {
            (0, u64::try_from(-offset_frames).unwrap_or(u64::MAX))
        };
        Ok(TrackAudio {
            track_id: track_id.to_string(),
            reader: Some(reader),
            channels: usize::from(channels),
            pad_frames,
            skip_frames,
        })
    }

    /// Returns frames, not samples: always `out.len() / channels`, silence-filled past the end.
    pub fn read(&mut self, out: &mut [f32]) -> Result<usize> {
        let ch = self.channels;
        let frames = out.len() / ch;
        let n = frames * ch;
        let pad = usize::try_from(self.pad_frames)
            .unwrap_or(usize::MAX)
            .min(frames);
        out[..pad * ch].fill(0.0);
        self.pad_frames -= pad as u64;
        let mut i = pad * ch;
        if i < n {
            self.skip_leading()?;
        }
        while i < n {
            let Some(r) = self.reader.as_mut() else { break };
            let got = r
                .read(&mut out[i..n])
                .map_err(|e| read_err(&self.track_id, e))?;
            if got == 0 {
                self.reader = None;
                break;
            }
            if got % ch != 0 || got > n - i {
                return Err(read_err(
                    &self.track_id,
                    Error::Other(format!(
                        "reader returned {got} samples for {} requested at {ch} ch",
                        n - i
                    )),
                ));
            }
            i += got;
        }
        out[i..n].fill(0.0);
        Ok(frames)
    }

    fn skip_leading(&mut self) -> Result<()> {
        const CHUNK: usize = 4096;
        let ch = self.channels;
        let mut scratch = Vec::new();
        while self.skip_frames > 0 {
            let Some(r) = self.reader.as_mut() else {
                self.skip_frames = 0;
                break;
            };
            let want = usize::try_from(self.skip_frames)
                .unwrap_or(usize::MAX)
                .min(CHUNK)
                * ch;
            scratch.resize(want, 0.0);
            let got = r
                .read(&mut scratch)
                .map_err(|e| read_err(&self.track_id, e))?;
            if got == 0 {
                self.reader = None;
                self.skip_frames = 0;
                break;
            }
            self.skip_frames = self.skip_frames.saturating_sub((got / ch) as u64);
        }
        Ok(())
    }
}
