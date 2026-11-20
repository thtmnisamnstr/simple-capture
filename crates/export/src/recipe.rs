//! FFmpeg argument recipes, the quick-export plan and the per-frame renderer.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sc_core::camera::camera_at;
use sc_core::image::RgbaImage;
use sc_core::project::{Background, CameraKey, ProjectV1, TrackKind};
use sc_decode::{SegmentFrameSource, TrackFrameSource, cfr_frame_times};
use sc_geometry::{Ratio, RectPx, SizePx, camera_rects, contain_fit, even_round, preset_size};
use sc_platform::{Decoder, Error, Result};
use sc_render::{Backend, Compositor, FrameJob, OutputFormat, RenderedFrame, ScreenLayer};
use sc_text::BackgroundCache;

use crate::{ExportSize, QuickExportRequest};

/// Owner answer Q6: full-size Original exports wait for M5's resize presets.
pub const QUICK_LONG_SIDE_CAP: u32 = 3840;
pub const MIX_RATE: u32 = 48_000;
pub const MIX_CHANNELS: u16 = 2;

// FFmpeg 9 lets unspecified frame properties override -color_primaries/-color_trc, so the
// frames themselves are tagged (spike 1).
const TAG_FRAMES: &str =
    "setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv";

#[cfg(target_os = "macos")]
const AAC_ENCODER: &str = "aac_at";
#[cfg(not(target_os = "macos"))]
const AAC_ENCODER: &str = "aac";

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| (*x).to_string()).collect()
}

/// `<final>.wav.part` for `out` = `<final>.part`, a name the stale-part sweep removes.
pub fn mix_wav_path(out: &Path) -> PathBuf {
    let name = out
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = match name.strip_suffix(".part") {
        Some(stem) => format!("{stem}.wav.part"),
        None => format!("{name}.wav.part"),
    };
    out.with_file_name(name)
}

/// Y4M on stdin (+ the WAV at [`mix_wav_path`]`(out)` when `audio`) → MP4 at `out`. Size and fps
/// come from the Y4M header, so the recipe never scales or retimes.
pub fn x264_quick(size: SizePx, fps: u32, audio: bool, out: &Path) -> Vec<String> {
    tracing::debug!(w = size.w, h = size.h, fps, audio, "x264_quick recipe");
    let mut a = s(&[
        "-hide_banner",
        "-loglevel",
        "warning",
        "-nostats",
        "-progress",
        "pipe:2",
        "-stats_period",
        "0.5",
        "-f",
        "yuv4mpegpipe",
        "-i",
        "pipe:0",
    ]);
    if audio {
        a.extend(s(&["-f", "wav", "-i"]));
        a.push(mix_wav_path(out).to_string_lossy().into_owned());
    }
    a.extend(s(&["-map", "0:v:0"]));
    if audio {
        a.extend(s(&["-map", "1:a:0"]));
    }
    a.extend(s(&[
        "-vf",
        TAG_FRAMES,
        "-c:v",
        "libx264",
        "-preset",
        "medium",
        "-profile:v",
        "high",
        "-pix_fmt",
        "yuv420p",
        "-crf",
        "22",
        "-color_primaries",
        "bt709",
        "-color_trc",
        "bt709",
        "-colorspace",
        "bt709",
        "-color_range",
        "tv",
    ]));
    if audio {
        a.extend(s(&["-c:a", AAC_ENCODER, "-b:a", "160k"]));
    } else {
        a.push("-an".into());
    }
    a.extend(s(&["-movflags", "+faststart", "-f", "mp4", "-y"]));
    a.push(out.to_string_lossy().into_owned());
    a
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickPlan {
    pub canvas: SizePx,
    pub fps: u32,
    pub duration_ns: i64,
    pub frames: u64,
    /// Final path; the job writes `<out_path>.part` and renames it.
    pub out_path: PathBuf,
    pub audio_tracks: Vec<String>,
}

fn class_ratio(project: &ProjectV1, reference: SizePx) -> Ratio {
    match project.output.canvas.aspect.ratio() {
        Some(r) => r,
        None if project.recording.screen.segments.len() > 1 => {
            let r = project.output.canvas.ratio;
            Ratio::new(r.num, r.den)
        }
        None => Ratio::of(reference),
    }
}

fn cap_long_side(size: SizePx, cap: u32) -> SizePx {
    let long = size.w.max(size.h);
    if long <= cap {
        return size;
    }
    let k = f64::from(cap) / f64::from(long);
    let scale = |v: u32| {
        if v == long {
            cap
        } else {
            even_round(f64::from(v) * k).min(cap)
        }
    };
    SizePx {
        w: scale(size.w),
        h: scale(size.h),
    }
}

/// The reference canvas, or a size-class preset that is never larger than it; the long side is
/// capped at [`QUICK_LONG_SIDE_CAP`].
pub fn quick_canvas(project: &ProjectV1, size: ExportSize) -> SizePx {
    let segs: Vec<SizePx> = project
        .recording
        .screen
        .segments
        .iter()
        .map(|s| SizePx {
            w: s.content_px.w,
            h: s.content_px.h,
        })
        .collect();
    let canvas = &project.output.canvas;
    let reference = sc_geometry::reference_canvas_size(canvas.aspect, canvas.ratio, &segs);
    let reference = cap_long_side(reference, QUICK_LONG_SIDE_CAP);
    let out = match size {
        ExportSize::Reference => reference,
        ExportSize::Class(c) => {
            let preset = preset_size(c, class_ratio(project, reference), false);
            if preset.w.min(preset.h) >= reference.w.min(reference.h) {
                reference
            } else {
                preset
            }
        }
    };
    SizePx {
        w: (out.w & !1).max(2),
        h: (out.h & !1).max(2),
    }
}

/// `<project parent>/<folder name without .sc>.mp4`, then `" 2"`, `" 3"`, … while that file or
/// its `.part` exists.
pub fn default_out_path(project_dir: &Path) -> Result<PathBuf> {
    let (Some(parent), Some(name)) = (project_dir.parent(), project_dir.file_name()) else {
        return Err(Error::Other(format!(
            "sc_export: project folder {} has no parent or name",
            project_dir.display()
        )));
    };
    let name = name.to_string_lossy();
    let base = name
        .strip_suffix(sc_core::folder::PROJECT_SUFFIX)
        .filter(|b| !b.is_empty())
        .unwrap_or(&name);
    for n in 1u32..10_000 {
        let p = parent.join(if n == 1 {
            format!("{base}.mp4")
        } else {
            format!("{base} {n}.mp4")
        });
        let part = sc_helpers::part::part_path_for(&p);
        if p.symlink_metadata().is_err() && part.symlink_metadata().is_err() {
            return Ok(p);
        }
    }
    Err(Error::Other(format!(
        "sc_export: no free output name for {base}.mp4 in {}",
        parent.display()
    )))
}

pub fn quick_plan(
    project: &ProjectV1,
    project_dir: &Path,
    req: &QuickExportRequest,
) -> Result<QuickPlan> {
    let duration_ns = project.duration_ns().filter(|d| *d > 0).ok_or_else(|| {
        Error::Other(format!(
            "sc_export: {} has no recorded duration (still recording, or empty)",
            project_dir.display()
        ))
    })?;
    let fps = req
        .fps
        .unwrap_or(project.recording.screen.fps_requested)
        .clamp(1, 240);
    let frames = cfr_frame_times(duration_ns, fps).count() as u64;
    if frames == 0 {
        return Err(Error::Other(format!(
            "sc_export: {} is too short to export at {fps} fps",
            project_dir.display()
        )));
    }
    let out_path = match &req.out_path {
        Some(p) => p.clone(),
        None => default_out_path(project_dir)?,
    };
    Ok(QuickPlan {
        canvas: quick_canvas(project, req.size),
        fps,
        duration_ns,
        frames,
        out_path,
        audio_tracks: audio_track_ids(project),
    })
}

pub(crate) fn audio_track_ids(project: &ProjectV1) -> Vec<String> {
    project
        .recording
        .tracks
        .iter()
        .filter(|t| matches!(t.kind, TrackKind::Mic | TrackKind::SystemAudio))
        .map(|t| t.id.clone())
        .collect()
}

/// Background, then the active (or held) screen segment contain-fit on the canvas.
pub struct FrameRenderer {
    source: SegmentFrameSource,
    compositor: Compositor,
    camera: Option<(TrackFrameSource, Vec<CameraKey>)>,
    backgrounds: BackgroundCache,
    background: Background,
    project_dir: PathBuf,
    canvas: SizePx,
    format: OutputFormat,
}

impl std::fmt::Debug for FrameRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameRenderer")
            .field("canvas", &self.canvas)
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

impl FrameRenderer {
    pub fn new(
        project: &ProjectV1,
        project_dir: &Path,
        decoder: Arc<dyn Decoder>,
        canvas: SizePx,
        backend: Backend,
        format: OutputFormat,
    ) -> Result<FrameRenderer> {
        let camera = match project.camera_layout.overlays.first() {
            Some(o) if !o.keyframes.is_empty() => {
                TrackFrameSource::open(project, project_dir, decoder.clone(), &o.track_id)?
                    .map(|src| (src, o.keyframes.clone()))
            }
            _ => None,
        };
        Ok(FrameRenderer {
            camera,
            source: SegmentFrameSource::open(project, project_dir, decoder)?,
            compositor: Compositor::new(backend)?,
            backgrounds: BackgroundCache::new(1),
            background: project.output.canvas.background.clone(),
            project_dir: project_dir.to_path_buf(),
            canvas,
            format,
        })
    }

    pub fn compositor(&self) -> &Compositor {
        &self.compositor
    }

    pub fn background(&mut self) -> Result<Arc<RgbaImage>> {
        self.backgrounds
            .get(&self.background, self.canvas, &self.project_dir)
            .map_err(|e| Error::Other(format!("sc_export: background: {e}")))
    }

    /// Also returns where the screen frame landed (`None`: background only).
    pub fn render(&mut self, t_rec_ns: i64) -> Result<(RenderedFrame, Option<RectPx>)> {
        let bg = self.background()?;
        let shown = self.source.frame_at(t_rec_ns)?;
        let layer = shown
            .as_ref()
            .filter(|f| f.content_px.w > 0 && f.content_px.h > 0)
            .map(|f| ScreenLayer {
                frame: &f.frame,
                src: f.content_px,
                dst: contain_fit(
                    self.canvas,
                    SizePx {
                        w: f.content_px.w,
                        h: f.content_px.h,
                    },
                ),
            });
        let dst = layer.as_ref().map(|l| l.dst);
        let mut cam_frame = None;
        let mut placement = None;
        if let Some((src, keys)) = self.camera.as_mut()
            && let Some(p) = camera_at(keys, t_rec_ns)
        {
            cam_frame = src.frame_at(t_rec_ns)?;
            placement = Some(p);
        }
        let camera = match (&cam_frame, &placement) {
            (Some(f), Some(p)) => {
                let rects = camera_rects(self.canvas, f.0.size(), p.shape.aspect(), &p.pose);
                Some(sc_render::camera_layer(p, f, rects.dst))
            }
            _ => None,
        };
        let job = FrameJob {
            camera,
            canvas: self.canvas,
            background: &bg,
            screen: layer,
        };
        Ok((self.compositor.render(&job, self.format)?, dst))
    }
}
