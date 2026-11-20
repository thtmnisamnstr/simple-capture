#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use sc_core::project::{
    EndReason, ProjectV1, ScreenSegment, SourceDescriptor, StartReason, Track, TrackHealth,
    TrackKind, Via,
};
use sc_core::time::{ClockKind, HostTime};
use sc_geometry::{RectPx, SizePx};
use sc_platform::synthetic::{FrameCode, SctrkWriterFactory, coded_frame, read_code};
use sc_platform::{
    AudioBuffer, AudioData, AudioWriterSpec, FrameBuffer, StreamTag, VideoCodec, VideoFrame,
    VideoWriterSpec, WriterFactory,
};

pub const MS: i64 = 1_000_000;

pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(name: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!(
            "sc-decode-{name}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("tracks")).expect("temp dir");
        TempDir(d)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn project() -> ProjectV1 {
    ProjectV1::new_recording(
        "test".into(),
        "decode".into(),
        "2026-11-19T00:00:00Z".into(),
        "0.0.0".into(),
        ClockKind::MachHost,
    )
}

fn health() -> TrackHealth {
    TrackHealth {
        frames: 0,
        dropped: 0,
        writer_not_ready: 0,
        gaps: Vec::new(),
        extra: Default::default(),
    }
}

#[derive(Clone)]
pub struct SegSpec {
    pub n: u32,
    pub start_ms: i64,
    pub end_ms: Option<i64>,
    pub size: SizePx,
    pub frames_ms: Vec<i64>,
    pub listed: bool,
}

impl SegSpec {
    pub fn new(n: u32, start_ms: i64, end_ms: i64, frames_ms: Vec<i64>) -> SegSpec {
        SegSpec {
            n,
            start_ms,
            end_ms: Some(end_ms),
            size: SizePx { w: 320, h: 180 },
            frames_ms,
            listed: true,
        }
    }

    pub fn id(&self) -> String {
        format!("s{}", self.n)
    }

    pub fn content(&self) -> RectPx {
        RectPx {
            x: self.n as i32,
            y: 0,
            w: self.size.w - self.n,
            h: self.size.h,
        }
    }
}

pub fn every(step_ms: i64, from_ms: i64, to_ms: i64) -> Vec<i64> {
    (from_ms..to_ms).step_by(step_ms as usize).collect()
}

pub fn write_segments(dir: &Path, p: &mut ProjectV1, specs: &[SegSpec]) {
    for s in specs {
        let file = format!("tracks/screen-{:03}.sctrk", s.n);
        let spec = VideoWriterSpec {
            size: s.size,
            fps: 30,
            codec: VideoCodec::Hevc,
            keyframe_interval_s: 1.0,
            fragment_interval_s: 1.0,
            bits_per_pixel: 0.1,
        };
        let mut w = SctrkWriterFactory
            .video(&dir.join(&file), &spec)
            .expect("video writer");
        for (i, &ms) in s.frames_ms.iter().enumerate() {
            let f = coded_frame(
                s.size,
                FrameCode {
                    source_hash: s.n + 1,
                    index: i as u32,
                    pts_ms: ms as u32,
                },
                false,
            );
            let vf = VideoFrame {
                stream: StreamTag(u64::from(s.n)),
                pts: HostTime(0),
                size: s.size,
                content_rect_pt: None,
                content_scale: None,
                buffer: FrameBuffer(Arc::new(f)),
            };
            w.append_video(&vf, ms * MS).expect("append video");
        }
        w.finish(s.end_ms.map(|e| (e - s.start_ms) * MS))
            .expect("finish video");
        let track_id = format!("screen-{}", s.n);
        p.recording.tracks.push(Track {
            id: track_id.clone(),
            kind: TrackKind::Screen,
            segment_id: Some(s.id()),
            file,
            container: "sctrk".into(),
            codec: "sctrk".into(),
            first_sample_offset_ns: s.start_ms * MS,
            duration_ns: s.end_ms.map(|e| (e - s.start_ms) * MS),
            video: None,
            audio: None,
            device: None,
            health: health(),
            fragment_interval_s: 1.0,
            extra: Default::default(),
        });
        if s.listed {
            p.recording.screen.segments.push(ScreenSegment {
                id: s.id(),
                track_id,
                start_rec_ns: s.start_ms * MS,
                end_rec_ns: s.end_ms.map(|e| e * MS),
                start_reason: if s.start_ms == 0 {
                    StartReason::RecordStart
                } else {
                    StartReason::UserSwitch
                },
                end_reason: s.end_ms.map(|_| EndReason::UserSwitch),
                via: Via::OwnChooser,
                source: SourceDescriptor::Display {
                    display_id: "1".into(),
                    display_name: None,
                    scale: 1.0,
                    frame_pt: None,
                },
                frame_px: s.size,
                content_px: s.content(),
                extra: Default::default(),
            });
        }
    }
}

pub fn identify(f: &FrameBuffer) -> (u32, i64) {
    let mut code = None;
    f.0.with_nv12(&mut |v| code = read_code(&v)).expect("nv12");
    let c = code.expect("frame code");
    (c.source_hash - 1, i64::from(c.pts_ms))
}

pub fn write_audio(
    dir: &Path,
    p: &mut ProjectV1,
    id: &str,
    rate: u32,
    channels: u16,
    offset_ns: i64,
    samples: &[f32],
) {
    let file = format!("tracks/{id}.sctrk");
    let mut w = SctrkWriterFactory
        .audio(
            &dir.join(&file),
            &AudioWriterSpec {
                sample_rate: rate,
                channels,
                bitrate: 128_000,
                fragment_interval_s: 1.0,
            },
        )
        .expect("audio writer");
    let frames = samples.len() / usize::from(channels);
    w.append_audio(
        &AudioBuffer {
            pts: HostTime(0),
            sample_rate: rate,
            channels,
            frames: frames as u32,
            data: AudioData::F32Interleaved(samples.to_vec().into()),
        },
        offset_ns.max(0),
        0,
    )
    .expect("append audio");
    w.finish(None).expect("finish audio");
    p.recording.tracks.push(Track {
        id: id.into(),
        kind: TrackKind::Mic,
        segment_id: None,
        file,
        container: "sctrk".into(),
        codec: "sctrk".into(),
        first_sample_offset_ns: offset_ns,
        duration_ns: None,
        video: None,
        audio: None,
        device: None,
        health: health(),
        fragment_interval_s: 1.0,
        extra: Default::default(),
    });
}
