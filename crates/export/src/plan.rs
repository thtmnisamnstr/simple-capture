//! Turning settings into an export: the output size (aspect locked, even except GIF, never
//! upscaled unless asked), the frame rate, the file name and a size and time estimate.

use std::path::{Path, PathBuf};

use sc_core::folder::{LocalDateTime, sanitize_title};
use sc_core::project::ProjectV1;
use sc_decode::cfr_frame_times;
use sc_geometry::{Ratio, SizeClass, SizePx, preset_size};
use sc_platform::{Error, Result};

use crate::settings::{
    AudioCodec, Encoder, ExportSettings, Format, GifMode, Quality, SizeChoice, VideoCodec,
};

#[derive(Debug, Clone, PartialEq)]
pub struct ExportRequest {
    pub project_dir: PathBuf,
    pub settings: ExportSettings,
    /// Final path; otherwise named from the settings' folder and template.
    pub out_path: Option<PathBuf>,
    /// Overrides the size the settings ask for (the quick export's capped reference size).
    pub canvas: Option<SizePx>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// Set when the settings asked for a size limit (they then carry its bitrate).
    pub target: Option<TargetBitrate>,
    pub settings: ExportSettings,
    pub canvas: SizePx,
    pub fps: u32,
    pub duration_ns: i64,
    pub frames: u64,
    pub out_path: PathBuf,
    pub audio: bool,
}

fn segments(project: &ProjectV1) -> Vec<SizePx> {
    project
        .recording
        .screen
        .segments
        .iter()
        .map(|s| SizePx {
            w: s.content_px.w,
            h: s.content_px.h,
        })
        .collect()
}

/// The reference size and the exact frame shape the size choices lock to.
pub fn reference(project: &ProjectV1) -> (SizePx, Ratio) {
    let c = &project.output.canvas;
    let reference = sc_geometry::reference_canvas_size(c.aspect, c.ratio, &segments(project));
    let ratio = match c.aspect.ratio() {
        Some(r) => r,
        None if project.recording.screen.segments.len() > 1 => Ratio::new(c.ratio.num, c.ratio.den),
        None => Ratio::of(reference),
    };
    (reference, ratio)
}

fn class(c: u32) -> Option<SizeClass> {
    Some(match c {
        2160 => SizeClass::S2160,
        1440 => SizeClass::S1440,
        1080 => SizeClass::S1080,
        720 => SizeClass::S720,
        480 => SizeClass::S480,
        360 => SizeClass::S360,
        240 => SizeClass::S240,
        _ => return None,
    })
}

fn round_side(v: f64, gif: bool) -> u32 {
    if gif {
        (v.round() as u32).max(1)
    } else {
        ((2.0 * (v / 2.0).round()) as u32).max(2)
    }
}

/// Even on both sides for every format: frames reach the encoders, gifski included, as I420.
pub fn output_size(project: &ProjectV1, st: &ExportSettings) -> SizePx {
    let gif = st.format == Format::Gif;
    let (reference, ratio) = reference(project);
    let r = ratio.as_f64();
    let landscape = r >= 1.0;
    let from_w = |w: f64| SizePx {
        w: round_side(w, gif),
        h: round_side(w / r, gif),
    };
    let from_h = |h: f64| SizePx {
        w: round_side(h * r, gif),
        h: round_side(h, gif),
    };
    let want = match st.size {
        SizeChoice::Original => reference,
        SizeChoice::Class(c) => class(c).map_or(reference, |c| preset_size(c, ratio, gif)),
        SizeChoice::Width(w) => from_w(f64::from(w)),
        SizeChoice::Height(h) => from_h(f64::from(h)),
        SizeChoice::Long(l) if landscape => from_w(f64::from(l)),
        SizeChoice::Long(l) => from_h(f64::from(l)),
    };
    let size = if !st.upscale && want.w.max(want.h) > reference.w.max(reference.h) {
        reference
    } else {
        want
    };
    SizePx {
        w: (size.w & !1).max(2),
        h: (size.h & !1).max(2),
    }
}

/// `{title}`, `{date}`, `{time}`, `{size}`, `{fps}`, `{format}` filled in and made safe.
pub fn file_stem(
    template: &str,
    title: &str,
    now: LocalDateTime,
    size: SizePx,
    fps: u32,
    format: &str,
) -> String {
    let t = if template.trim().is_empty() {
        "{title}"
    } else {
        template
    };
    let filled = t
        .replace("{title}", title)
        .replace(
            "{date}",
            &format!("{:04}-{:02}-{:02}", now.y, now.mo, now.d),
        )
        .replace(
            "{time}",
            &format!("{:02}.{:02}.{:02}", now.h, now.mi, now.s),
        )
        .replace("{size}", &format!("{}x{}", size.w, size.h))
        .replace("{fps}", &format!("{fps}fps"))
        .replace("{format}", format);
    let clean = sanitize_title(&filled);
    if clean.is_empty() {
        "Export".into()
    } else {
        clean
    }
}

/// A free `<dir>/<stem>.<ext>`, adding " 2", " 3" … while that file or its `.part` exists.
pub fn free_path(dir: &Path, stem: &str, ext: &str) -> Result<PathBuf> {
    for n in 1u32..10_000 {
        let name = if n == 1 {
            format!("{stem}.{ext}")
        } else {
            format!("{stem} {n}.{ext}")
        };
        let p = dir.join(name);
        let part = sc_helpers::part::part_path_for(&p);
        if p.symlink_metadata().is_err() && part.symlink_metadata().is_err() {
            return Ok(p);
        }
    }
    Err(Error::Other(format!(
        "sc_export: no free name for {stem}.{ext} in {}",
        dir.display()
    )))
}

fn project_stem(project_dir: &Path) -> String {
    let name = project_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.strip_suffix(sc_core::folder::PROJECT_SUFFIX)
        .filter(|b| !b.is_empty())
        .unwrap_or(&name)
        .to_string()
}

/// What a size limit becomes for one edit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TargetBitrate {
    pub bytes: u64,
    pub video_kbps: u32,
    pub audio_kbps: u32,
}

/// 6% is left for the container and the encoder's rate control, which overshoots a little.
pub fn target_bitrate(mb: f64, duration_ns: i64, audio: bool) -> TargetBitrate {
    let bytes = (mb.max(0.1) * 1e6) as u64;
    let secs = (duration_ns as f64 / 1e9).max(0.1);
    let total_kbps = bytes as f64 * 8.0 * 0.94 / secs / 1000.0;
    let audio_kbps: u32 = if !audio {
        0
    } else if total_kbps < 600.0 {
        48
    } else if total_kbps < 2000.0 {
        96
    } else {
        128
    };
    let video_kbps = (total_kbps - f64::from(audio_kbps)).max(60.0) as u32;
    TargetBitrate {
        bytes,
        video_kbps,
        audio_kbps,
    }
}

/// Bits per pixel below which screen text turns to mush (H.264; HEVC and AV1 need less).
const MIN_BPP: f64 = 0.025;

/// Steps the frame down through 1080, 720, 480 and 360 lines until the bitrate gives each pixel
/// enough bits, keeping the aspect; never grows it.
fn fit_to_bitrate(
    project: &ProjectV1,
    st: &ExportSettings,
    canvas: SizePx,
    video_kbps: u32,
    fps: u32,
) -> SizePx {
    let codec_gain = match st.video {
        VideoCodec::Hevc | VideoCodec::Vp9 => 0.7,
        VideoCodec::Av1 => 0.55,
        _ => 1.0,
    };
    let bpp = |c: SizePx| {
        f64::from(video_kbps) * 1000.0 / (f64::from(c.w) * f64::from(c.h) * f64::from(fps.max(1)))
    };
    let mut best = canvas;
    for class in [1080u32, 720, 480, 360] {
        if bpp(best) >= MIN_BPP * codec_gain {
            break;
        }
        let smaller = output_size(
            project,
            &ExportSettings {
                size: SizeChoice::Class(class),
                ..st.clone()
            },
        );
        if smaller.w * smaller.h < best.w * best.h {
            best = smaller;
        }
    }
    best
}

pub fn plan(project: &ProjectV1, req: &ExportRequest, now: LocalDateTime) -> Result<Plan> {
    let st = req.settings.clone().normalized();
    let duration_ns = Some(sc_player::timeline_doc(project).duration())
        .filter(|d| *d > 0)
        .ok_or_else(|| {
            Error::Other(format!(
                "sc_export: {} has nothing to export",
                req.project_dir.display()
            ))
        })?;
    let fps = st.effective_fps(f64::from(project.recording.screen.fps_requested.max(1)));
    let frames = cfr_frame_times(duration_ns, fps).count() as u64;
    if frames == 0 {
        return Err(Error::Other(
            "sc_export: the edit is too short to export".into(),
        ));
    }
    let mut st = st;
    let mut canvas = req.canvas.unwrap_or_else(|| output_size(project, &st));
    let has_audio = st.audio != AudioCodec::None
        && st.format != Format::Gif
        && !crate::recipe::audio_track_ids(project).is_empty();
    let target = match st.quality {
        Quality::TargetSize { mb } if st.format != Format::Gif => {
            let t = target_bitrate(mb, duration_ns, has_audio);
            st.audio_kbps = has_audio.then_some(t.audio_kbps);
            st.quality = Quality::Custom {
                crf: None,
                kbps: Some(t.video_kbps),
            };
            if req.canvas.is_none() {
                canvas = fit_to_bitrate(project, &st, canvas, t.video_kbps, fps);
            }
            Some(t)
        }
        _ => None,
    };
    let out_path = match &req.out_path {
        Some(p) => p.clone(),
        None => {
            let dir = match &st.out_dir {
                Some(d) => d.clone(),
                None => req
                    .project_dir
                    .parent()
                    .map(Path::to_path_buf)
                    .ok_or_else(|| Error::Other("sc_export: the project has no folder".into()))?,
            };
            let title = project_stem(&req.project_dir);
            let stem = file_stem(&st.name, &title, now, canvas, fps, st.extension());
            free_path(&dir, &stem, st.extension())?
        }
    };
    let audio = has_audio;
    Ok(Plan {
        target,
        settings: st,
        canvas,
        fps,
        duration_ns,
        frames,
        out_path,
        audio,
    })
}

/// Rough file size (bytes, low and high) and encode time (seconds) for a plan, from the M0
/// measurements (CODEC_MATRIX §2); real screen content varies a lot, hence the range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Estimate {
    pub bytes_low: u64,
    pub bytes_high: u64,
    pub seconds: f64,
}

pub fn estimate(p: &Plan) -> Estimate {
    let st = &p.settings;
    let px = f64::from(p.canvas.w) * f64::from(p.canvas.h);
    let frames = p.frames as f64;
    let t = match st.quality {
        Quality::Small => 0.6,
        Quality::Balanced | Quality::Custom { .. } | Quality::TargetSize { .. } => 1.0,
        Quality::High => 1.8,
    };
    let fast = st.encoder == Encoder::Fast;
    let bpp = match st.video {
        VideoCodec::H264 => 0.035 * t * if fast { 1.6 } else { 1.0 },
        VideoCodec::Hevc => 0.024 * t * if fast { 1.6 } else { 1.0 },
        VideoCodec::Av1 => 0.02 * t,
        VideoCodec::Vp9 => 0.024 * t,
        VideoCodec::Vp8 => 0.045 * t,
        VideoCodec::Prores => [1.6, 2.4, 3.5][match st.quality {
            Quality::Small => 0,
            Quality::High => 2,
            _ => 1,
        }],
        VideoCodec::Gif => match st.gif.mode {
            GifMode::Gifski => 0.6 + f64::from(st.gif.quality) / 100.0 * 0.8,
            GifMode::Palette => 2.0 + f64::from(st.gif.colors) / 256.0 * 1.8,
        },
    };
    let audio_kbps = if p.audio {
        match st.audio {
            AudioCodec::Aac | AudioCodec::Opus | AudioCodec::Vorbis => 160.0 * t.min(1.6),
            AudioCodec::Flac | AudioCodec::Alac => 700.0,
            AudioCodec::Pcm => 1536.0,
            AudioCodec::None => 0.0,
        }
    } else {
        0.0
    };
    let secs = p.duration_ns as f64 / 1e9;
    let bytes = px * frames * bpp / 8.0 + audio_kbps * 1000.0 / 8.0 * secs;
    let fps_1080 = match (st.video, fast) {
        (VideoCodec::H264, false) => 190.0,
        (VideoCodec::H264, true) => 200.0,
        (VideoCodec::Hevc, false) => 115.0,
        (VideoCodec::Hevc, true) => 170.0,
        (VideoCodec::Av1, _) => 150.0,
        (VideoCodec::Vp9, _) => 60.0,
        (VideoCodec::Vp8, _) => 110.0,
        (VideoCodec::Prores, _) => 120.0,
        (VideoCodec::Gif, _) => match st.gif.mode {
            GifMode::Gifski => 40.0 * 518_400.0 / 2_073_600.0,
            GifMode::Palette => 7.0 * 518_400.0 / 2_073_600.0,
        },
    };
    let speed = fps_1080 * 2_073_600.0 / px.max(1.0);
    let (low, high) = match (st.quality, &p.target) {
        (_, Some(t)) => (t.bytes as f64 * 0.8, t.bytes as f64),
        (Quality::Custom { kbps: Some(k), .. }, None) => {
            let b = (f64::from(k) + audio_kbps) * 1000.0 / 8.0 * secs;
            (b * 0.9, b * 1.05)
        }
        _ => (bytes * 0.5, bytes * 1.8),
    };
    Estimate {
        bytes_low: low as u64,
        bytes_high: high as u64,
        seconds: frames / speed.max(0.1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::project::{EndReason, Extra, ScreenSegment, SourceDescriptor, StartReason, Via};
    use sc_core::time::ClockKind;
    use sc_geometry::{Aspect, RectPx};

    fn project(w: u32, h: u32, aspect: Aspect) -> ProjectV1 {
        let mut p = ProjectV1::new_recording(
            "id".into(),
            "Demo".into(),
            "now".into(),
            "0".into(),
            ClockKind::MachHost,
        );
        p.output.canvas.aspect = aspect;
        p.recording.screen.segments.push(ScreenSegment {
            id: "s".into(),
            track_id: "screen".into(),
            start_rec_ns: 0,
            end_rec_ns: Some(10_000_000_000),
            start_reason: StartReason::RecordStart,
            end_reason: Some(EndReason::RecordStop),
            source: SourceDescriptor::Display {
                display_id: "1".into(),
                display_name: None,
                frame_pt: None,
                scale: 2.0,
            },
            via: Via::OwnChooser,
            frame_px: SizePx { w, h },
            content_px: RectPx { x: 0, y: 0, w, h },
            extra: Extra::new(),
        });
        p
    }

    fn st(format: Format, size: SizeChoice) -> ExportSettings {
        ExportSettings {
            size,
            ..ExportSettings::for_format(format)
        }
    }

    #[test]
    fn size_limits_become_bitrates_with_room_for_the_container() {
        let t = target_bitrate(18.0, 60_000_000_000, true);
        assert_eq!(t.bytes, 18_000_000);
        assert_eq!(t.audio_kbps, 128);
        let total = f64::from(t.video_kbps + t.audio_kbps) * 1000.0 / 8.0 * 60.0;
        assert!(total <= 18e6 * 0.95 && total >= 18e6 * 0.9, "{total}");
        let tiny = target_bitrate(1.0, 300_000_000_000, true);
        assert_eq!(tiny.audio_kbps, 48);
        assert_eq!(tiny.video_kbps, 60, "floor");
        assert_eq!(target_bitrate(5.0, 10_000_000_000, false).audio_kbps, 0);
    }

    #[test]
    fn a_tight_size_limit_steps_the_frame_down() {
        let p = project(3024, 1964, Aspect::R16x9);
        let st = ExportSettings {
            size: SizeChoice::Original,
            ..ExportSettings::for_format(Format::Mp4)
        };
        let full = output_size(&p, &st);
        assert_eq!(
            fit_to_bitrate(&p, &st, full, 20_000, 30),
            full,
            "plenty of bits"
        );
        assert_eq!(fit_to_bitrate(&p, &st, full, 900, 30).h, 720);
        let small = fit_to_bitrate(&p, &st, full, 500, 30);
        assert_eq!(small, SizePx { w: 854, h: 480 });
        let tiny = fit_to_bitrate(&p, &st, full, 100, 30);
        assert_eq!(tiny.h, 360, "never below 360 lines");
    }

    #[test]
    fn sizes_lock_the_aspect_round_and_never_upscale() {
        let p = project(3024, 1964, Aspect::R16x9);
        let s = |c| output_size(&p, &st(Format::Mp4, c));
        assert_eq!(s(SizeChoice::Class(1080)), SizePx { w: 1920, h: 1080 });
        assert_eq!(s(SizeChoice::Width(1000)), SizePx { w: 1000, h: 562 });
        assert_eq!(s(SizeChoice::Height(701)), SizePx { w: 1246, h: 702 });
        let reference = s(SizeChoice::Original);
        assert_eq!(s(SizeChoice::Class(2160)), reference, "no upscale");
        let up = output_size(
            &p,
            &ExportSettings {
                upscale: true,
                ..st(Format::Mp4, SizeChoice::Class(2160))
            },
        );
        assert_eq!(up, SizePx { w: 3840, h: 2160 });
        let gif = output_size(&p, &st(Format::Gif, SizeChoice::Class(240)));
        assert_eq!(
            gif,
            SizePx { w: 426, h: 240 },
            "GIF is even too (I420 frames)"
        );
        let tall = project(1080, 1920, Aspect::Original);
        assert_eq!(
            output_size(&tall, &st(Format::Gif, SizeChoice::Long(960))),
            SizePx { w: 540, h: 960 }
        );
    }

    #[test]
    fn names_fill_the_template_and_avoid_collisions() {
        let now = LocalDateTime {
            y: 2026,
            mo: 11,
            d: 21,
            h: 9,
            mi: 5,
            s: 7,
            offset_min: 0,
        };
        let stem = file_stem(
            "{title} {date} {size} {fps}",
            "Demo: one/two",
            now,
            SizePx { w: 1280, h: 720 },
            30,
            "mp4",
        );
        assert_eq!(stem, "Demo- one-two 2026-11-21 1280x720 30fps");
        let dir = std::env::temp_dir().join(format!("sc-plan-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let a = free_path(&dir, "x", "gif").expect("free");
        std::fs::write(&a, b"").expect("touch");
        assert_eq!(
            free_path(&dir, "x", "gif").expect("next"),
            dir.join("x 2.gif")
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
