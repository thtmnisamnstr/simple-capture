//! The quick export's canvas and file name, and the per-frame renderer.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sc_core::image::RgbaImage;
use sc_core::project::ProjectV1;
use sc_geometry::{Ratio, RectPx, SizePx, even_round, preset_size};
use sc_platform::{Decoder, Error, Result};
use sc_player::video::TimelineVideo;
use sc_render::{Backend, Compositor, OutputFormat, RenderedFrame};

use crate::ExportSize;

/// Owner answer Q6: full-size Original exports wait for M5's resize presets.
pub const QUICK_LONG_SIDE_CAP: u32 = 3840;
pub const MIX_RATE: u32 = 48_000;
pub const MIX_CHANNELS: u16 = 2;

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

pub(crate) use sc_player::audio::audio_track_ids;

/// One output frame of the edited timeline: background, screen segment, camera.
pub struct FrameRenderer {
    video: TimelineVideo,
    compositor: Compositor,
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
        Ok(FrameRenderer {
            video: TimelineVideo::open(
                project,
                project_dir,
                decoder,
                sc_player::timeline_doc(project),
            )?,
            compositor: Compositor::new(backend)?,
            canvas,
            format,
        })
    }

    pub fn compositor(&self) -> &Compositor {
        &self.compositor
    }

    pub fn background(&mut self) -> Result<Arc<RgbaImage>> {
        self.video.background(self.canvas)
    }

    /// The frame at timeline time `t_tl_ns`, and where the screen landed (`None`: background
    /// only).
    pub fn render(&mut self, t_tl_ns: i64) -> Result<(RenderedFrame, Option<RectPx>)> {
        let bg = self.background()?;
        let c = self.video.compose(t_tl_ns, self.canvas)?;
        let dst = c.screen_dst(self.canvas);
        let layers = c.text_layers();
        let job = c.job(self.canvas, &bg, Some(Arc::as_ptr(&bg) as u64), &layers);
        Ok((self.compositor.render(&job, self.format)?, dst))
    }
}
