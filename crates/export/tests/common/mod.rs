#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use sc_core::image::RgbaImage;
use sc_core::project::{
    Background, EndReason, GradientStop, ProjectState, ProjectV1, Rgba, ScreenSegment,
    SourceDescriptor, StartReason, Track, TrackHealth, TrackKind, Via,
};
use sc_core::time::{ClockKind, HostTime};
use sc_export::{ExportObserver, ExportProgress, ExportState};
use sc_geometry::{Aspect, PointF, Ratio, RectPx, SizePx};
use sc_platform::synthetic::{FrameCode, SctrkWriterFactory, coded_frame};
use sc_platform::{
    AudioBuffer, AudioData, AudioWriterSpec, CpuNv12Frame, FrameBuffer, StreamTag, VideoCodec,
    VideoFrame, VideoWriterSpec, WriterFactory,
};

pub const MS: i64 = 1_000_000;

pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(name: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!(
            "sc-export-{name}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("temp dir");
        TempDir(d)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(&self.0)
            .expect("read temp dir")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn helpers_available() -> bool {
    let found = sc_helpers::locate(sc_helpers::HelperKind::Ffmpeg).is_ok()
        && sc_helpers::locate(sc_helpers::HelperKind::Ffprobe).is_ok();
    if !found && std::env::var("SC_REQUIRE_HELPERS").as_deref() == Ok("1") {
        panic!(
            "SC_REQUIRE_HELPERS=1 but ffmpeg/ffprobe were not found (build them with `just helpers`)"
        );
    }
    if !found {
        eprintln!("SKIPPED: helpers not built");
    }
    found
}

pub fn require_gpu() -> bool {
    std::env::var("SC_REQUIRE_GPU").as_deref() == Ok("1")
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

pub struct Seg {
    pub start_ms: i64,
    pub end_ms: i64,
    pub size: SizePx,
    pub content: Option<RectPx>,
    pub fps: u32,
    pub frame: Option<CpuNv12Frame>,
}

impl Seg {
    pub fn new(start_ms: i64, end_ms: i64, w: u32, h: u32) -> Seg {
        Seg {
            start_ms,
            end_ms,
            size: SizePx { w, h },
            content: None,
            fps: 30,
            frame: None,
        }
    }

    pub fn content_px(&self) -> RectPx {
        self.content.unwrap_or(RectPx {
            x: 0,
            y: 0,
            w: self.size.w,
            h: self.size.h,
        })
    }
}

pub struct Proj {
    pub dir: PathBuf,
    pub project: ProjectV1,
}

pub fn new_project(root: &Path, name: &str, duration_ms: i64) -> Proj {
    let dir = root.join(format!("{name}.sc"));
    std::fs::create_dir_all(dir.join("tracks")).expect("project dir");
    let mut p = ProjectV1::new_recording(
        "test".into(),
        name.into(),
        "2026-11-19T00:00:00Z".into(),
        "0.0.0".into(),
        ClockKind::MachHost,
    );
    p.state = ProjectState::Finalized;
    p.recording.start_host_ns = 1_000 * MS;
    p.recording.stop_host_ns = Some(1_000 * MS + duration_ms * MS);
    Proj { dir, project: p }
}

impl Proj {
    pub fn aspect(&mut self, a: Aspect) -> &mut Self {
        self.project.output.canvas.aspect = a;
        if let Some(r) = a.ratio() {
            self.project.output.canvas.ratio = r;
        }
        self
    }

    pub fn background(&mut self, b: Background) -> &mut Self {
        self.project.output.canvas.background = b;
        self
    }

    pub fn segment(&mut self, s: Seg) -> &mut Self {
        let n = self.project.recording.screen.segments.len() as u32;
        let file = format!("tracks/screen-{n:03}.sctrk");
        let spec = VideoWriterSpec {
            size: s.size,
            fps: s.fps,
            codec: VideoCodec::Hevc,
            keyframe_interval_s: 1.0,
            fragment_interval_s: 1.0,
            bits_per_pixel: 0.1,
        };
        let mut w = SctrkWriterFactory
            .video(&self.dir.join(&file), &spec)
            .expect("video writer");
        let step = 1000 / i64::from(s.fps);
        let mut i = 0u32;
        let mut ms = 0i64;
        while ms < s.end_ms - s.start_ms {
            let f = match &s.frame {
                Some(f) => f.clone(),
                None => coded_frame(
                    s.size,
                    FrameCode {
                        source_hash: n + 1,
                        index: i,
                        pts_ms: ms as u32,
                    },
                    false,
                ),
            };
            let vf = VideoFrame {
                stream: StreamTag(u64::from(n)),
                pts: HostTime(0),
                size: s.size,
                content_rect_pt: None,
                content_scale: None,
                buffer: FrameBuffer(Arc::new(f)),
            };
            w.append_video(&vf, ms * MS).expect("append video");
            i += 1;
            ms += step;
        }
        w.finish(Some((s.end_ms - s.start_ms) * MS))
            .expect("finish video");
        let track_id = format!("screen-{n}");
        let seg_id = format!("s{n}");
        self.project.recording.tracks.push(Track {
            id: track_id.clone(),
            kind: TrackKind::Screen,
            segment_id: Some(seg_id.clone()),
            file,
            container: "sctrk".into(),
            codec: "sctrk".into(),
            first_sample_offset_ns: s.start_ms * MS,
            duration_ns: Some((s.end_ms - s.start_ms) * MS),
            video: None,
            audio: None,
            device: None,
            health: health(),
            fragment_interval_s: 1.0,
            extra: Default::default(),
        });
        self.project.recording.screen.segments.push(ScreenSegment {
            id: seg_id,
            track_id,
            start_rec_ns: s.start_ms * MS,
            end_rec_ns: Some(s.end_ms * MS),
            start_reason: if n == 0 {
                StartReason::RecordStart
            } else {
                StartReason::UserSwitch
            },
            end_reason: Some(EndReason::UserSwitch),
            via: Via::OwnChooser,
            source: SourceDescriptor::Display {
                display_id: "1".into(),
                display_name: None,
                scale: 1.0,
                frame_pt: None,
            },
            frame_px: s.size,
            content_px: s.content_px(),
            extra: Default::default(),
        });
        self
    }

    pub fn audio(
        &mut self,
        id: &str,
        kind: TrackKind,
        rate: u32,
        channels: u16,
        offset_ms: i64,
        samples: &[f32],
    ) -> &mut Self {
        let file = format!("tracks/{id}.sctrk");
        let mut w = SctrkWriterFactory
            .audio(
                &self.dir.join(&file),
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
            0,
            0,
        )
        .expect("append audio");
        w.finish(None).expect("finish audio");
        self.project.recording.tracks.push(Track {
            id: id.into(),
            kind,
            segment_id: None,
            file,
            container: "sctrk".into(),
            codec: "sctrk".into(),
            first_sample_offset_ns: offset_ms * MS,
            duration_ns: None,
            video: None,
            audio: None,
            device: None,
            health: health(),
            fragment_interval_s: 1.0,
            extra: Default::default(),
        });
        self
    }

    pub fn save(&self) -> PathBuf {
        self.project.save(&self.dir).expect("save project");
        self.dir.clone()
    }
}

pub fn stop(at: f64, r: f64, g: f64, b: f64) -> GradientStop {
    GradientStop {
        at,
        color: Rgba([r, g, b, 1.0]),
        extra: Default::default(),
    }
}

pub fn solid() -> Background {
    Background::Solid {
        color: Rgba([0.2, 0.4, 0.6, 1.0]),
    }
}

pub fn linear() -> Background {
    Background::Linear {
        stops: vec![stop(0.0, 1.0, 0.2, 0.1), stop(1.0, 0.1, 0.2, 1.0)],
        angle_deg: 135.0,
    }
}

pub fn radial() -> Background {
    Background::Radial {
        stops: vec![stop(0.0, 1.0, 1.0, 0.8), stop(1.0, 0.1, 0.0, 0.3)],
        center_norm: PointF { x: 0.5, y: 0.5 },
        radius_norm: 0.7,
    }
}

/// A PNG asset with `paint(x, y) -> [r, g, b]` copied into the project.
pub fn image_background(
    project_dir: &Path,
    size: SizePx,
    paint: impl Fn(u32, u32) -> [u8; 3],
) -> Background {
    let mut data = Vec::with_capacity((size.w * size.h * 4) as usize);
    for y in 0..size.h {
        for x in 0..size.w {
            let [r, g, b] = paint(x, y);
            data.extend_from_slice(&[r, g, b, 255]);
        }
    }
    let img = RgbaImage {
        size,
        stride: size.w as usize * 4,
        data,
    };
    let png = sc_text::encode_png(&img).expect("encode png");
    let src = project_dir.join("bg-src.png");
    std::fs::write(&src, png).expect("write png");
    sc_core::project::copy_background_asset(&src, project_dir).expect("copy asset")
}

pub fn image() -> impl Fn(&Path) -> Background {
    |dir| {
        image_background(dir, SizePx { w: 200, h: 120 }, |x, y| {
            [(x % 256) as u8, (y * 2 % 256) as u8, 128]
        })
    }
}

pub fn ratio(w: u32, h: u32) -> Ratio {
    Ratio::new(w, h)
}

#[derive(Default)]
pub struct Obs {
    pub states: Mutex<Vec<ExportState>>,
    pub progress: Mutex<Vec<ExportProgress>>,
}

impl ExportObserver for Obs {
    fn on_state(&self, s: &ExportState) {
        self.states.lock().expect("states").push(s.clone());
    }
    fn on_progress(&self, p: &ExportProgress) {
        self.progress.lock().expect("progress").push(p.clone());
    }
}

impl Obs {
    pub fn last_state(&self) -> Option<ExportState> {
        self.states.lock().expect("states").last().cloned()
    }
}

/// BT.709 limited-range Y'CbCr of an sRGB colour, as code values.
pub fn rgb_to_yuv709(rgb: [u8; 3]) -> (u8, u8, u8) {
    let [r, g, b] = rgb.map(|c| f64::from(c) / 255.0);
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let cb = (b - y) / 1.8556;
    let cr = (r - y) / 1.5748;
    let q = |v: f64| v.round().clamp(0.0, 255.0) as u8;
    (
        q(16.0 + 219.0 * y),
        q(128.0 + 224.0 * cb),
        q(128.0 + 224.0 * cr),
    )
}
