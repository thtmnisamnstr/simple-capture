//! Reads helper-written tracks back through the FFmpeg helper: video as NV12 frames (frame `k`
//! at `k / fps`, see `writer`), audio as interleaved f32 at any rate.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use sc_geometry::SizePx;
use sc_platform::{
    AudioReader, DecodedFrame, Decoder, Error, FrameBuffer, Result, TrackProbe, VideoReader,
};

use crate::nv12::unpack_nv12;

const NS: f64 = 1e9;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StreamInfo {
    pub size: Option<SizePx>,
    pub fps: Option<f64>,
    pub has_audio: bool,
}

fn command(ffmpeg: &Path) -> Command {
    let mut cmd = Command::new(ffmpeg);
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

fn parse_size(line: &str) -> Option<SizePx> {
    line.split([',', ' '])
        .filter_map(|t| {
            let (w, h) = t.trim().split_once('x')?;
            Some(SizePx {
                w: w.parse().ok()?,
                h: h.parse().ok()?,
            })
        })
        .find(|s| s.w > 0 && s.h > 0)
}

fn parse_fps(line: &str) -> Option<f64> {
    let words: Vec<&str> = line.split([',', ' ']).filter(|w| !w.is_empty()).collect();
    words
        .windows(2)
        .find(|p| p[1] == "fps")
        .and_then(|p| p[0].parse().ok())
}

pub fn parse_info(stderr: &str) -> StreamInfo {
    let mut info = StreamInfo {
        size: None,
        fps: None,
        has_audio: false,
    };
    for line in stderr.lines() {
        let line = line.trim();
        if !line.starts_with("Stream #") {
            continue;
        }
        if let Some(rest) = line.split_once("Video:").map(|(_, r)| r) {
            if info.size.is_none() {
                info.size = parse_size(rest);
                info.fps = parse_fps(rest);
            }
        } else if line.contains("Audio:") {
            info.has_audio = true;
        }
    }
    info
}

/// `time=HH:MM:SS.cc` of the last stats line, and `frame=` from it.
pub fn parse_stats(stderr: &str) -> (Option<i64>, Option<u64>) {
    let mut time = None;
    let mut frames = None;
    for seg in stderr.split(['\r', '\n']) {
        if let Some(t) = seg
            .split("time=")
            .nth(1)
            .and_then(|r| r.split_whitespace().next())
            && let Some(ns) = hms_ns(t)
        {
            time = Some(ns);
        }
        if let Some(f) = seg
            .split("frame=")
            .nth(1)
            .and_then(|r| r.split_whitespace().next())
            .and_then(|f| f.parse().ok())
        {
            frames = Some(f);
        }
    }
    (time, frames)
}

fn hms_ns(t: &str) -> Option<i64> {
    let mut parts = t.split(':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let s: f64 = parts.next()?.parse().ok()?;
    Some(((h * 3600.0 + m * 60.0 + s) * NS).round() as i64)
}

type InfoCache = Mutex<HashMap<PathBuf, (SystemTime, u64, StreamInfo)>>;

fn info_cache() -> &'static InfoCache {
    static C: OnceLock<InfoCache> = OnceLock::new();
    C.get_or_init(Default::default)
}

#[derive(Debug, Clone)]
pub struct HelperDecoder {
    ffmpeg: PathBuf,
}

impl HelperDecoder {
    pub fn new(ffmpeg: PathBuf) -> HelperDecoder {
        HelperDecoder { ffmpeg }
    }

    pub fn locate() -> Result<HelperDecoder> {
        Ok(HelperDecoder::new(sc_helpers::locate(
            sc_helpers::HelperKind::Ffmpeg,
        )?))
    }

    pub fn info(&self, path: &Path) -> Result<StreamInfo> {
        let meta = std::fs::metadata(path)?;
        let stamp = (
            meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            meta.len(),
        );
        if let Some((m, l, i)) = info_cache().lock().ok().and_then(|c| c.get(path).copied())
            && (m, l) == stamp
        {
            return Ok(i);
        }
        let out = command(&self.ffmpeg)
            .args(["-hide_banner", "-i"])
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()?;
        let info = parse_info(&String::from_utf8_lossy(&out.stderr));
        if let Ok(mut c) = info_cache().lock() {
            c.insert(path.to_path_buf(), (stamp.0, stamp.1, info));
        }
        Ok(info)
    }
}

struct Pipe {
    child: Child,
    out: ChildStdout,
}

impl Pipe {
    fn spawn(mut cmd: Command) -> Result<Pipe> {
        let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
        let out = child
            .stdout
            .take()
            .ok_or_else(|| Error::Other("decoder helper has no stdout".into()))?;
        Ok(Pipe { child, out })
    }

    /// `Ok(false)` at a clean end of stream.
    fn fill(&mut self, buf: &mut [u8]) -> Result<bool> {
        let mut got = 0;
        while got < buf.len() {
            match self.out.read(&mut buf[got..]) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(got == buf.len())
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct HelperVideoReader {
    pipe: Pipe,
    size: SizePx,
    fps: f64,
    next_index: u64,
}

impl VideoReader for HelperVideoReader {
    fn size(&self) -> SizePx {
        self.size
    }

    fn next_frame(&mut self) -> Result<Option<DecodedFrame>> {
        let mut buf = vec![0u8; self.size.w as usize * self.size.h as usize * 3 / 2];
        if !self.pipe.fill(&mut buf)? {
            return Ok(None);
        }
        let k = self.next_index;
        self.next_index += 1;
        Ok(Some(DecodedFrame {
            file_ns: (k as f64 * NS / self.fps).round() as i64,
            frame: FrameBuffer(Arc::new(unpack_nv12(buf, self.size))),
        }))
    }
}

pub struct HelperAudioReader {
    pipe: Pipe,
    channels: usize,
    first_ns: i64,
    bytes: Vec<u8>,
}

impl AudioReader for HelperAudioReader {
    fn first_sample_file_ns(&self) -> i64 {
        self.first_ns
    }

    fn read(&mut self, out: &mut [f32]) -> Result<usize> {
        let want = out.len() / self.channels * self.channels;
        self.bytes.resize(want * 4, 0);
        let mut got = 0;
        while got < self.bytes.len() {
            match self.pipe.out.read(&mut self.bytes[got..]) {
                Ok(0) => break,
                Ok(n) => {
                    got += n;
                    if got % (4 * self.channels) == 0 {
                        break;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e.into()),
            }
        }
        let n = got / (4 * self.channels) * self.channels;
        for (o, b) in out.iter_mut().zip(self.bytes[..n * 4].chunks_exact(4)) {
            *o = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        }
        Ok(n)
    }
}

/// Reads RIFF chunks up to the start of `data`; a piped WAV has no usable sizes, so `data` runs
/// to the end of the stream.
fn skip_wav_header(pipe: &mut Pipe) -> Result<()> {
    let mut riff = [0u8; 12];
    if !pipe.fill(&mut riff)? {
        return Ok(());
    }
    if &riff[0..4] != b"RIFF" || &riff[8..12] != b"WAVE" {
        return Err(Error::Other(
            "decoder helper did not send a WAV stream".into(),
        ));
    }
    loop {
        let mut head = [0u8; 8];
        if !pipe.fill(&mut head)? {
            return Ok(());
        }
        if &head[0..4] == b"data" {
            return Ok(());
        }
        let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
        let mut skip = vec![0u8; len + (len & 1)];
        if !pipe.fill(&mut skip)? {
            return Ok(());
        }
    }
}

impl Decoder for HelperDecoder {
    fn open_video(&self, path: &Path) -> Result<Box<dyn VideoReader>> {
        self.open_video_at(path, 0)
    }

    fn open_video_at(&self, path: &Path, from_file_ns: i64) -> Result<Box<dyn VideoReader>> {
        let info = self.info(path)?;
        let size = info
            .size
            .ok_or_else(|| Error::Other(format!("{}: no video stream", path.display())))?;
        let fps = info.fps.filter(|f| *f > 0.0).unwrap_or(30.0);
        let k = (from_file_ns.max(0) as f64 * fps / NS).floor() as u64;
        let mut cmd = command(&self.ffmpeg);
        cmd.args(["-hide_banner", "-loglevel", "error", "-hwaccel", "auto"]);
        if k > 0 {
            cmd.args(["-ss", &format!("{:.6}", (k as f64 - 0.5) / fps)]);
        }
        cmd.arg("-i").arg(path).args([
            "-map",
            "0:v:0",
            "-fps_mode",
            "passthrough",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "nv12",
            "pipe:1",
        ]);
        Ok(Box::new(HelperVideoReader {
            pipe: Pipe::spawn(cmd)?,
            size,
            fps,
            next_index: k,
        }))
    }

    fn open_audio(&self, path: &Path, rate: u32, channels: u16) -> Result<Box<dyn AudioReader>> {
        self.open_audio_at(path, rate, channels, 0)
    }

    fn open_audio_at(
        &self,
        path: &Path,
        rate: u32,
        channels: u16,
        from_file_ns: i64,
    ) -> Result<Box<dyn AudioReader>> {
        let mut cmd = command(&self.ffmpeg);
        cmd.args(["-hide_banner", "-loglevel", "error"]);
        let from = from_file_ns.max(0);
        if from > 0 {
            cmd.args(["-ss", &format!("{:.6}", from as f64 / NS)]);
        }
        cmd.arg("-i").arg(path).args([
            "-map",
            "0:a:0",
            "-c:a",
            "pcm_f32le",
            "-f",
            "wav",
            "-ar",
            &rate.to_string(),
            "-ac",
            &channels.max(1).to_string(),
            "pipe:1",
        ]);
        let mut pipe = Pipe::spawn(cmd)?;
        skip_wav_header(&mut pipe)?;
        Ok(Box::new(HelperAudioReader {
            pipe,
            channels: usize::from(channels.max(1)),
            first_ns: from,
            bytes: Vec::new(),
        }))
    }

    fn probe(&self, path: &Path) -> Result<TrackProbe> {
        let out = command(&self.ffmpeg)
            .args(["-hide_banner", "-stats", "-i"])
            .arg(path)
            .args(["-map", "0", "-c", "copy", "-f", "null", "-"])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()?;
        let text = String::from_utf8_lossy(&out.stderr);
        let info = parse_info(&text);
        let (time, frames) = parse_stats(&text);
        let readable = out.status.success() && time.is_some_and(|t| t > 0);
        let duration_ns = match (info.size, info.fps, frames) {
            (Some(_), Some(fps), Some(n)) if fps > 0.0 => Some((n as f64 * NS / fps) as i64),
            _ => time,
        };
        Ok(TrackProbe {
            readable,
            duration_ns: if readable { duration_ns } else { None },
            samples: if info.size.is_some() {
                frames.unwrap_or(0)
            } else {
                0
            },
            size: info.size,
            error: if readable {
                None
            } else {
                Some(
                    text.lines()
                        .rev()
                        .find(|l| !l.trim().is_empty())
                        .unwrap_or("not readable")
                        .to_string(),
                )
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_lines_give_size_fps_and_audio() {
        let err = "Input #0, matroska,webm, from 'x.mkv':\n  Duration: N/A, start: 0.000000\n  Stream #0:0: Video: h264 (High), yuv420p(tv, bt709, progressive), 1920x1080 [SAR 1:1 DAR 16:9], 30 fps, 30 tbr, 1k tbn\n  Stream #0:1: Audio: aac (LC), 48000 Hz, stereo, fltp\n";
        let i = parse_info(err);
        assert_eq!(i.size, Some(SizePx { w: 1920, h: 1080 }));
        assert_eq!(i.fps, Some(30.0));
        assert!(i.has_audio);
    }

    #[test]
    fn stats_give_the_last_time_and_frame() {
        let err = "frame=   10 fps=0.0 q=-1.0 size=N/A time=00:00:00.33 bitrate=N/A\rframe=  150 fps=0.0 q=-1.0 Lsize=N/A time=00:00:04.96 bitrate=N/A speed= 900x\n";
        assert_eq!(parse_stats(err), (Some(4_960_000_000), Some(150)));
    }
}
