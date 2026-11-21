//! `.sctrk`: a crash-tolerant track file for the synthetic platform.
//!
//! Layout: one header line `SCTRK 1 {json}\n` (`{"kind":"video","w":..,"h":..,"fps":..}` or
//! `{"kind":"audio","rate":..,"channels":..}`), then records `[u32 LE body_len][body]` where
//! body = `[u8 type][i64 LE file_ns][payload]`. Each record is written with one `write_all` and
//! flushed, so a crash leaves at most one torn record at the tail, which readers ignore.
//!
//! Types: 1 video frame (`u32 w, u32 h, u8 full_range, u32 len + RLE luma, u32 len + RLE
//! chroma`), 2 audio (`u32 frames, u16 channels, u32 rate, f32 LE samples`), 3 silence
//! (`u32 frames`), 4 end (`file_ns` = the duration passed to `finish`).

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use sc_geometry::SizePx;

use super::code::{FrameCode, coded_frame};
use crate::{
    Append, AudioBuffer, AudioData, AudioReader, AudioWriterSpec, CpuNv12Frame, DecodedFrame,
    Decoder, Error, FinishedTrack, FrameBuffer, NativeFrame, Nv12View, Result, TrackProbe,
    TrackWriter, VideoCodec, VideoFrame, VideoReader, VideoWriterSpec, WriterFactory,
};

const MAGIC: &str = "SCTRK 1 ";
const REC_VIDEO: u8 = 1;
const REC_AUDIO: u8 = 2;
const REC_SILENCE: u8 = 3;
const REC_END: u8 = 4;
const MAX_BODY: u32 = 1 << 30;
const MAX_HEADER: u64 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Header {
    Video { w: u32, h: u32, fps: u32 },
    Audio { rate: u32, channels: u16 },
}

fn io_err(path: &Path, what: &str, e: std::io::Error) -> Error {
    Error::Io(std::io::Error::new(
        e.kind(),
        format!("{what} {}: {e}", path.display()),
    ))
}

fn rle_encode(src: &[u8], out: &mut Vec<u8>) {
    fn literal(lit: &[u8], out: &mut Vec<u8>) {
        for chunk in lit.chunks(128) {
            out.push((chunk.len() - 1) as u8);
            out.extend_from_slice(chunk);
        }
    }
    let (mut i, mut lit_start) = (0, 0);
    while i < src.len() {
        let b = src[i];
        let mut run = 1;
        while i + run < src.len() && src[i + run] == b && run < 130 {
            run += 1;
        }
        if run >= 3 {
            literal(&src[lit_start..i], out);
            out.push(128 + (run - 3) as u8);
            out.push(b);
            i += run;
            lit_start = i;
        } else {
            i += run;
        }
    }
    literal(&src[lit_start..], out);
}

fn rle_decode(src: &[u8], expected: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(expected);
    let mut i = 0;
    while i < src.len() {
        let t = src[i];
        i += 1;
        if t < 128 {
            let n = usize::from(t) + 1;
            out.extend_from_slice(src.get(i..i + n)?);
            i += n;
        } else {
            let n = usize::from(t - 128) + 3;
            let b = *src.get(i)?;
            out.resize(out.len() + n, b);
            i += 1;
        }
        if out.len() > expected {
            return None;
        }
    }
    (out.len() == expected).then_some(out)
}

fn frame_ns(fps: u32) -> i64 {
    1_000_000_000 / i64::from(fps.max(1))
}

fn frames_ns(frames: u64, rate: u32) -> i64 {
    (frames as f64 * 1e9 / f64::from(rate.max(1))).round() as i64
}

fn encode_video(v: &Nv12View<'_>, out: &mut Vec<u8>) -> Result<()> {
    let (w, h) = (v.size.w as usize, v.size.h as usize);
    let uv_w = CpuNv12Frame::uv_stride_for(v.size);
    let uv_rows = CpuNv12Frame::uv_rows_for(v.size);
    let y_ok = h == 0 || (v.y_stride >= w && v.y.len() >= (h - 1) * v.y_stride + w);
    let uv_ok =
        uv_rows == 0 || (v.uv_stride >= uv_w && v.uv.len() >= (uv_rows - 1) * v.uv_stride + uv_w);
    if !y_ok || !uv_ok {
        return Err(Error::Other(format!(
            "sctrk: NV12 view {w}x{h} has short planes or strides"
        )));
    }
    out.extend_from_slice(&v.size.w.to_le_bytes());
    out.extend_from_slice(&v.size.h.to_le_bytes());
    out.push(u8::from(v.full_range));
    for (plane, stride, row_len, rows) in
        [(v.y, v.y_stride, w, h), (v.uv, v.uv_stride, uv_w, uv_rows)]
    {
        let mut tight = Vec::with_capacity(row_len * rows);
        for r in 0..rows {
            tight.extend_from_slice(&plane[r * stride..r * stride + row_len]);
        }
        let len_at = out.len();
        out.extend_from_slice(&[0; 4]);
        rle_encode(&tight, out);
        let len = (out.len() - len_at - 4) as u32;
        out[len_at..len_at + 4].copy_from_slice(&len.to_le_bytes());
    }
    Ok(())
}

struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (a, b) = (self.0.get(..n)?, self.0.get(n..)?);
        self.0 = b;
        Some(a)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}

fn decode_video(payload: &[u8]) -> Option<CpuNv12Frame> {
    let mut c = Cursor(payload);
    let size = SizePx {
        w: c.u32()?,
        h: c.u32()?,
    };
    let full_range = c.u8()? != 0;
    let y_len = c.u32()? as usize;
    let y = rle_decode(c.take(y_len)?, size.w as usize * size.h as usize)?;
    let uv_len = c.u32()? as usize;
    let uv = rle_decode(
        c.take(uv_len)?,
        CpuNv12Frame::uv_stride_for(size) * CpuNv12Frame::uv_rows_for(size),
    )?;
    Some(CpuNv12Frame {
        size,
        y,
        uv,
        full_range,
    })
}

/// Writes `.sctrk` tracks.
#[derive(Debug, Clone, Copy, Default)]
pub struct SctrkWriterFactory;

struct SctrkWriter {
    path: PathBuf,
    file: File,
    header: Header,
    samples: u64,
    last_ns: Option<i64>,
    end_ns: Option<i64>,
}

impl SctrkWriter {
    fn create(path: &Path, header: Header) -> Result<SctrkWriter> {
        let mut file = File::create(path).map_err(|e| io_err(path, "creating", e))?;
        let json = serde_json::to_string(&header)
            .map_err(|e| Error::Other(format!("sctrk header: {e}")))?;
        file.write_all(format!("{MAGIC}{json}\n").as_bytes())
            .and_then(|()| file.flush())
            .map_err(|e| io_err(path, "writing", e))?;
        Ok(SctrkWriter {
            path: path.to_path_buf(),
            file,
            header,
            samples: 0,
            last_ns: None,
            end_ns: None,
        })
    }

    fn record(
        &mut self,
        ty: u8,
        file_ns: i64,
        fill: impl FnOnce(&mut Vec<u8>) -> Result<()>,
    ) -> Result<()> {
        let mut buf = Vec::with_capacity(64);
        buf.extend_from_slice(&[0; 4]);
        buf.push(ty);
        buf.extend_from_slice(&file_ns.to_le_bytes());
        fill(&mut buf)?;
        let len = (buf.len() - 4) as u32;
        buf[..4].copy_from_slice(&len.to_le_bytes());
        self.file
            .write_all(&buf)
            .and_then(|()| self.file.flush())
            .map_err(|e| io_err(&self.path, "writing", e))
    }

    fn audio_format(&self) -> Result<(u32, u16)> {
        match self.header {
            Header::Audio { rate, channels } => Ok((rate, channels)),
            Header::Video { .. } => Err(Error::Other(format!(
                "sctrk: {} is a video track",
                self.path.display()
            ))),
        }
    }

    fn extend_end(&mut self, end: i64) {
        self.end_ns = Some(self.end_ns.map_or(end, |e| e.max(end)));
    }
}

impl TrackWriter for SctrkWriter {
    fn append_video(&mut self, f: &VideoFrame, file_ns: i64) -> Result<Append> {
        let Header::Video { w, h, fps } = self.header else {
            return Err(Error::Other(format!(
                "sctrk: {} is an audio track",
                self.path.display()
            )));
        };
        let size = f.buffer.0.size();
        if size != (SizePx { w, h }) {
            return Err(Error::Other(format!(
                "sctrk: frame {}x{} does not match the {w}x{h} writer for {}",
                size.w,
                size.h,
                self.path.display()
            )));
        }
        if let Some(last) = self.last_ns
            && file_ns <= last
        {
            return Err(Error::Other(format!(
                "sctrk: video file_ns {file_ns} not after {last} in {}",
                self.path.display()
            )));
        }
        let buffer = f.buffer.clone();
        self.record(REC_VIDEO, file_ns, |out| {
            let mut res = Ok(());
            buffer.0.with_nv12(&mut |v| res = encode_video(&v, out))?;
            res
        })?;
        self.samples += 1;
        self.last_ns = Some(file_ns);
        self.extend_end(file_ns + frame_ns(fps));
        Ok(Append::Written)
    }

    /// `file_ns` is the file time of the buffer's first sample; the record starts `skip_frames`
    /// later.
    fn append_audio(&mut self, a: &AudioBuffer, file_ns: i64, skip_frames: u32) -> Result<Append> {
        let (rate, channels) = self.audio_format()?;
        if a.sample_rate != rate || a.channels != channels {
            return Err(Error::Other(format!(
                "sctrk: audio {} Hz {} ch does not match the {rate} Hz {channels} ch writer for {}",
                a.sample_rate,
                a.channels,
                self.path.display()
            )));
        }
        let AudioData::F32Interleaved(data) = &a.data else {
            return Err(Error::Unsupported("sctrk stores F32Interleaved audio only"));
        };
        let ch = usize::from(channels);
        if data.len() < a.frames as usize * ch {
            return Err(Error::Other(format!(
                "sctrk: audio buffer has {} samples, {} frames x {ch} ch needed",
                data.len(),
                a.frames
            )));
        }
        if skip_frames >= a.frames {
            return Ok(Append::Written);
        }
        let n = a.frames - skip_frames;
        let at = file_ns + frames_ns(u64::from(skip_frames), rate);
        let samples = &data[skip_frames as usize * ch..a.frames as usize * ch];
        self.record(REC_AUDIO, at, |out| {
            out.extend_from_slice(&n.to_le_bytes());
            out.extend_from_slice(&channels.to_le_bytes());
            out.extend_from_slice(&rate.to_le_bytes());
            out.reserve(samples.len() * 4);
            for s in samples {
                out.extend_from_slice(&s.to_le_bytes());
            }
            Ok(())
        })?;
        self.samples += u64::from(n);
        self.extend_end(at + frames_ns(u64::from(n), rate));
        Ok(Append::Written)
    }

    fn append_silence(&mut self, file_ns: i64, frames: u32) -> Result<Append> {
        let (rate, _) = self.audio_format()?;
        self.record(REC_SILENCE, file_ns, |out| {
            out.extend_from_slice(&frames.to_le_bytes());
            Ok(())
        })?;
        self.samples += u64::from(frames);
        self.extend_end(file_ns + frames_ns(u64::from(frames), rate));
        Ok(Append::Written)
    }

    fn is_ready(&self) -> bool {
        true
    }

    fn finish(mut self: Box<Self>, end_file_ns: Option<i64>) -> Result<FinishedTrack> {
        if let Some(end) = end_file_ns {
            self.record(REC_END, end, |_| Ok(()))?;
        }
        Ok(FinishedTrack {
            path: self.path.clone(),
            duration_ns: end_file_ns.or(self.end_ns),
            samples: self.samples,
        })
    }
}

impl WriterFactory for SctrkWriterFactory {
    fn extension(&self) -> &'static str {
        "sctrk"
    }

    fn video(&self, path: &Path, spec: &VideoWriterSpec) -> Result<Box<dyn TrackWriter>> {
        Ok(Box::new(SctrkWriter::create(
            path,
            Header::Video {
                w: spec.size.w,
                h: spec.size.h,
                fps: spec.fps,
            },
        )?))
    }

    fn audio(&self, path: &Path, spec: &AudioWriterSpec) -> Result<Box<dyn TrackWriter>> {
        if spec.sample_rate == 0 || spec.channels == 0 {
            return Err(Error::Other(format!(
                "sctrk: audio spec needs a rate and channels ({spec:?})"
            )));
        }
        Ok(Box::new(SctrkWriter::create(
            path,
            Header::Audio {
                rate: spec.sample_rate,
                channels: spec.channels,
            },
        )?))
    }

    /// Frames per second the RLE encoder sustains for a coded frame of `size`, into memory.
    fn encode_benchmark(
        &self,
        size: SizePx,
        _fps: u32,
        frames: u32,
        _codec: VideoCodec,
    ) -> Result<f64> {
        let mut out = Vec::new();
        let started = Instant::now();
        for i in 0..frames.max(1) {
            let f = coded_frame(
                size,
                FrameCode {
                    source_hash: 1,
                    index: i,
                    pts_ms: i,
                },
                false,
            );
            out.clear();
            f.with_nv12(&mut |v| {
                let _ = encode_video(&v, &mut out);
            })?;
        }
        Ok(f64::from(frames.max(1)) / started.elapsed().as_secs_f64().max(1e-9))
    }
}

struct RawRecord {
    ty: u8,
    file_ns: i64,
    payload: Vec<u8>,
}

struct TrackFile {
    path: PathBuf,
    r: BufReader<File>,
    header: Header,
}

impl TrackFile {
    fn open(path: &Path) -> Result<std::result::Result<TrackFile, String>> {
        let file = File::open(path).map_err(|e| io_err(path, "opening", e))?;
        let mut r = BufReader::new(file);
        let mut line = Vec::new();
        (&mut r)
            .take(MAX_HEADER)
            .read_until(b'\n', &mut line)
            .map_err(|e| io_err(path, "reading", e))?;
        let header = std::str::from_utf8(&line)
            .ok()
            .and_then(|l| l.strip_suffix('\n'))
            .and_then(|l| l.strip_prefix(MAGIC))
            .and_then(|json| serde_json::from_str::<Header>(json).ok());
        Ok(match header {
            Some(header) => Ok(TrackFile {
                path: path.to_path_buf(),
                r,
                header,
            }),
            None => Err(format!("{}: no sctrk header", path.display())),
        })
    }

    fn open_readable(path: &Path) -> Result<TrackFile> {
        Self::open(path)?.map_err(Error::Other)
    }

    /// `None` at the end, including a torn or implausible tail record.
    fn next(&mut self) -> Option<RawRecord> {
        let mut len = [0u8; 4];
        self.r.read_exact(&mut len).ok()?;
        let len = u32::from_le_bytes(len);
        if !(9..=MAX_BODY).contains(&len) {
            return None;
        }
        let mut body = vec![0u8; len as usize];
        self.r.read_exact(&mut body).ok()?;
        let ty = body[0];
        let mut ns = [0u8; 8];
        ns.copy_from_slice(&body[1..9]);
        body.drain(..9);
        Some(RawRecord {
            ty,
            file_ns: i64::from_le_bytes(ns),
            payload: body,
        })
    }
}

/// Reads `.sctrk` tracks.
#[derive(Debug, Clone, Copy, Default)]
pub struct SctrkDecoder;

struct SctrkVideoReader {
    file: TrackFile,
    size: SizePx,
}

impl VideoReader for SctrkVideoReader {
    fn size(&self) -> SizePx {
        self.size
    }

    fn next_frame(&mut self) -> Result<Option<DecodedFrame>> {
        while let Some(rec) = self.file.next() {
            if rec.ty != REC_VIDEO {
                continue;
            }
            let frame = decode_video(&rec.payload).ok_or_else(|| {
                Error::Other(format!(
                    "sctrk: corrupt frame at file_ns {} in {}",
                    rec.file_ns,
                    self.file.path.display()
                ))
            })?;
            return Ok(Some(DecodedFrame {
                file_ns: rec.file_ns,
                frame: FrameBuffer(std::sync::Arc::new(frame)),
            }));
        }
        Ok(None)
    }
}

struct Primed {
    inner: Box<dyn VideoReader>,
    first: Option<DecodedFrame>,
    then: Option<DecodedFrame>,
}

impl VideoReader for Primed {
    fn size(&self) -> SizePx {
        self.inner.size()
    }

    fn next_frame(&mut self) -> Result<Option<DecodedFrame>> {
        if let Some(f) = self.first.take() {
            return Ok(Some(f));
        }
        if let Some(f) = self.then.take() {
            return Ok(Some(f));
        }
        self.inner.next_frame()
    }
}

struct SctrkAudioReader {
    first_ns: i64,
    samples: Vec<f32>,
    pos: usize,
    channels: usize,
}

impl AudioReader for SctrkAudioReader {
    fn first_sample_file_ns(&self) -> i64 {
        self.first_ns
    }

    /// Returns the number of `f32` values written (whole frames); 0 at the end.
    fn read(&mut self, out: &mut [f32]) -> Result<usize> {
        let n = (out.len() / self.channels * self.channels).min(self.samples.len() - self.pos);
        out[..n].copy_from_slice(&self.samples[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

fn remix(src: &[f32], from: usize, to: usize) -> Vec<f32> {
    if from == to {
        return src.to_vec();
    }
    let frames = src.len() / from;
    let mut out = Vec::with_capacity(frames * to);
    for f in src.chunks_exact(from) {
        if to == 1 {
            out.push(f.iter().sum::<f32>() / from as f32);
        } else {
            out.extend((0..to).map(|c| f[c % from]));
        }
    }
    out
}

fn resample(src: &[f32], channels: usize, from: u32, to: u32) -> Vec<f32> {
    if from == to {
        return src.to_vec();
    }
    let frames = src.len() / channels;
    if frames == 0 {
        return Vec::new();
    }
    let out_frames = (frames as f64 * f64::from(to) / f64::from(from)).round() as usize;
    let mut out = Vec::with_capacity(out_frames * channels);
    for j in 0..out_frames {
        let x = j as f64 * f64::from(from) / f64::from(to);
        let i0 = (x.floor() as usize).min(frames - 1);
        let i1 = (i0 + 1).min(frames - 1);
        let t = (x - i0 as f64) as f32;
        for c in 0..channels {
            let (a, b) = (src[i0 * channels + c], src[i1 * channels + c]);
            out.push(a + (b - a) * t);
        }
    }
    out
}

impl Decoder for SctrkDecoder {
    fn open_video(&self, path: &Path) -> Result<Box<dyn VideoReader>> {
        let file = TrackFile::open_readable(path)?;
        let Header::Video { w, h, .. } = file.header else {
            return Err(Error::Other(format!(
                "sctrk: {} is not a video track",
                path.display()
            )));
        };
        Ok(Box::new(SctrkVideoReader {
            file,
            size: SizePx { w, h },
        }))
    }

    /// Like AVAssetReader with a time range: the first frame is the one shown at `from_file_ns`.
    fn open_video_at(&self, path: &Path, from_file_ns: i64) -> Result<Box<dyn VideoReader>> {
        let mut r = self.open_video(path)?;
        let mut shown = None;
        while let Some(f) = r.next_frame()? {
            if f.file_ns > from_file_ns {
                let (first, then) = match shown {
                    Some(s) => (s, Some(f)),
                    None => (f, None),
                };
                return Ok(Box::new(Primed {
                    inner: r,
                    first: Some(first),
                    then,
                }));
            }
            shown = Some(f);
        }
        Ok(Box::new(Primed {
            inner: r,
            first: shown,
            then: None,
        }))
    }

    /// Places each record at its `file_ns` (zeros fill holes longer than one sample), then
    /// converts channels (duplicate or average) and rate (linear).
    fn open_audio(&self, path: &Path, rate: u32, channels: u16) -> Result<Box<dyn AudioReader>> {
        let mut file = TrackFile::open_readable(path)?;
        let Header::Audio {
            rate: src_rate,
            channels: src_ch,
        } = file.header
        else {
            return Err(Error::Other(format!(
                "sctrk: {} is not an audio track",
                path.display()
            )));
        };
        if rate == 0 || channels == 0 {
            return Err(Error::Other(
                "sctrk: open_audio needs rate and channels > 0".into(),
            ));
        }
        let ch = usize::from(src_ch);
        let mut buf: Vec<f32> = Vec::new();
        let mut first_ns = None;
        while let Some(rec) = file.next() {
            let mut c = Cursor(&rec.payload);
            let (frames, data) = match rec.ty {
                REC_AUDIO => {
                    let Some(frames) = c.u32() else { break };
                    let (Some(_), Some(_)) = (c.u16(), c.u32()) else {
                        break;
                    };
                    let Some(bytes) = c.take(frames as usize * ch * 4) else {
                        break;
                    };
                    (frames, Some(bytes))
                }
                REC_SILENCE => {
                    let Some(frames) = c.u32() else { break };
                    (frames, None)
                }
                _ => continue,
            };
            let first = *first_ns.get_or_insert(rec.file_ns);
            let at = ((rec.file_ns - first) as f64 * f64::from(src_rate) / 1e9).round() as i64;
            let have = (buf.len() / ch) as i64;
            if at > have + 1 {
                buf.resize(at as usize * ch, 0.0);
            }
            match data {
                Some(bytes) => buf.extend(
                    bytes
                        .chunks_exact(4)
                        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
                ),
                None => buf.resize(buf.len() + frames as usize * ch, 0.0),
            }
        }
        let to_ch = usize::from(channels);
        let samples = resample(&remix(&buf, ch, to_ch), to_ch, src_rate, rate);
        Ok(Box::new(SctrkAudioReader {
            first_ns: first_ns.unwrap_or(0),
            samples,
            pos: 0,
            channels: to_ch,
        }))
    }

    /// A torn tail is normal after a crash: the probe counts the complete records and stays
    /// readable. Duration: the `finish` end when present, else the end of the last sample.
    fn probe(&self, path: &Path) -> Result<TrackProbe> {
        let mut file = match TrackFile::open(path)? {
            Ok(f) => f,
            Err(e) => {
                return Ok(TrackProbe {
                    readable: false,
                    duration_ns: None,
                    samples: 0,
                    size: None,
                    error: Some(e),
                });
            }
        };
        let (mut samples, mut end, mut finished) = (0u64, None::<i64>, None);
        while let Some(rec) = file.next() {
            let rec_end = match (rec.ty, file.header) {
                (REC_VIDEO, Header::Video { fps, .. }) => {
                    samples += 1;
                    rec.file_ns + frame_ns(fps)
                }
                (REC_AUDIO | REC_SILENCE, Header::Audio { rate, .. }) => {
                    let Some(frames) = Cursor(&rec.payload).u32() else {
                        break;
                    };
                    samples += u64::from(frames);
                    rec.file_ns + frames_ns(u64::from(frames), rate)
                }
                (REC_END, _) => {
                    finished = Some(rec.file_ns);
                    continue;
                }
                _ => continue,
            };
            end = Some(end.map_or(rec_end, |e: i64| e.max(rec_end)));
        }
        let size = match file.header {
            Header::Video { w, h, .. } => Some(SizePx { w, h }),
            Header::Audio { .. } => None,
        };
        Ok(TrackProbe {
            readable: true,
            duration_ns: finished.or(end),
            samples,
            size,
            error: None,
        })
    }
}
