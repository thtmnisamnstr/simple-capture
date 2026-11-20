use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use sc_core::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WavFormat {
    F32,
    S16,
}

impl WavFormat {
    fn bytes_per_sample(self) -> u16 {
        match self {
            WavFormat::F32 => 4,
            WavFormat::S16 => 2,
        }
    }
}

#[derive(Debug)]
pub struct WavWriter {
    out: BufWriter<File>,
    path: PathBuf,
    channels: u16,
    fmt: WavFormat,
    frames: u64,
    data_bytes: u64,
    scratch: Vec<u8>,
}

const RIFF_SIZE_OFFSET: u64 = 4;

fn header_len(fmt: WavFormat) -> u64 {
    match fmt {
        WavFormat::S16 => 44,
        WavFormat::F32 => 58,
    }
}

fn io_err(e: std::io::Error, what: &str, path: &Path) -> Error {
    Error::Io(std::io::Error::new(
        e.kind(),
        format!("{what} {}: {e}", path.display()),
    ))
}

impl WavWriter {
    pub fn create(path: &Path, rate: u32, channels: u16, fmt: WavFormat) -> Result<WavWriter> {
        if channels == 0 || rate == 0 {
            return Err(Error::Invalid(format!(
                "wav {}: rate {rate} and channels {channels} must be non-zero",
                path.display()
            )));
        }
        let file = File::create(path).map_err(|e| io_err(e, "create wav", path))?;
        let mut w = WavWriter {
            out: BufWriter::with_capacity(256 * 1024, file),
            path: path.to_path_buf(),
            channels,
            fmt,
            frames: 0,
            data_bytes: 0,
            scratch: Vec::new(),
        };
        let header = w.header_bytes(rate);
        w.out
            .write_all(&header)
            .map_err(|e| io_err(e, "write wav header", path))?;
        Ok(w)
    }

    fn header_bytes(&self, rate: u32) -> Vec<u8> {
        let bps = self.fmt.bytes_per_sample();
        let block_align = self.channels * bps;
        let byte_rate = rate * block_align as u32;
        let mut h = Vec::with_capacity(header_len(self.fmt) as usize);
        h.extend_from_slice(b"RIFF");
        h.extend_from_slice(&0u32.to_le_bytes());
        h.extend_from_slice(b"WAVEfmt ");
        match self.fmt {
            WavFormat::S16 => {
                h.extend_from_slice(&16u32.to_le_bytes());
                h.extend_from_slice(&1u16.to_le_bytes());
            }
            WavFormat::F32 => {
                h.extend_from_slice(&18u32.to_le_bytes());
                h.extend_from_slice(&3u16.to_le_bytes());
            }
        }
        h.extend_from_slice(&self.channels.to_le_bytes());
        h.extend_from_slice(&rate.to_le_bytes());
        h.extend_from_slice(&byte_rate.to_le_bytes());
        h.extend_from_slice(&block_align.to_le_bytes());
        h.extend_from_slice(&(bps * 8).to_le_bytes());
        if self.fmt == WavFormat::F32 {
            h.extend_from_slice(&0u16.to_le_bytes());
            h.extend_from_slice(b"fact");
            h.extend_from_slice(&4u32.to_le_bytes());
            h.extend_from_slice(&0u32.to_le_bytes());
        }
        h.extend_from_slice(b"data");
        h.extend_from_slice(&0u32.to_le_bytes());
        h
    }

    /// Interleaved samples; the length must be a whole number of frames. S16 clamps to ±1.0.
    pub fn write_f32(&mut self, interleaved: &[f32]) -> Result<()> {
        if !interleaved.len().is_multiple_of(self.channels as usize) {
            return Err(Error::Invalid(format!(
                "wav {}: {} samples is not a whole number of {}-channel frames",
                self.path.display(),
                interleaved.len(),
                self.channels
            )));
        }
        let bytes = interleaved.len() as u64 * self.fmt.bytes_per_sample() as u64;
        if header_len(self.fmt) - 8 + self.data_bytes + bytes > u32::MAX as u64 {
            return Err(Error::Invalid(format!(
                "wav {}: data would exceed the 4 GiB RIFF limit",
                self.path.display()
            )));
        }
        self.scratch.clear();
        match self.fmt {
            WavFormat::F32 => {
                for s in interleaved {
                    self.scratch.extend_from_slice(&s.to_le_bytes());
                }
            }
            WavFormat::S16 => {
                for s in interleaved {
                    let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
                    self.scratch.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
        self.out
            .write_all(&self.scratch)
            .map_err(|e| io_err(e, "write wav samples", &self.path))?;
        self.data_bytes += bytes;
        self.frames += (interleaved.len() / self.channels as usize) as u64;
        Ok(())
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Patches the header; returns frames written.
    pub fn finish(mut self) -> Result<u64> {
        let path = self.path.clone();
        let hl = header_len(self.fmt);
        let riff = (hl - 8 + self.data_bytes) as u32;
        let data = self.data_bytes as u32;
        let patch = |w: &mut BufWriter<File>, at: u64, v: u32| -> std::io::Result<()> {
            w.seek(SeekFrom::Start(at))?;
            w.write_all(&v.to_le_bytes())
        };
        (|| -> std::io::Result<()> {
            self.out.flush()?;
            patch(&mut self.out, RIFF_SIZE_OFFSET, riff)?;
            if self.fmt == WavFormat::F32 {
                patch(&mut self.out, hl - 12, self.frames as u32)?;
            }
            patch(&mut self.out, hl - 4, data)?;
            self.out.flush()?;
            self.out.get_ref().sync_all()
        })()
        .map_err(|e| io_err(e, "finish wav", &path))?;
        Ok(self.frames)
    }
}
