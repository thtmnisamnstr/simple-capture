//! Timeline thumbnails and the audio waveform for the open project, made in the background and
//! kept in the project's `thumbs/` folder. Both are indexed by recording time, so they stay
//! valid through every edit. Thumbnails are made coarse-to-fine so the strip fills evenly.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use sc_core::image::RgbaImage;
use sc_core::project::{ProjectV1, TrackKind};
use sc_decode::{SegmentFrameSource, TrackAudio, TrackFrameSource};
use sc_geometry::{Ratio, SizePx};
use sc_platform::FrameBuffer;
use sc_render::{Backend, Compositor, FrameJob, OutputFormat, ScreenLayer};
use serde_json::json;

use crate::core::Core;

pub const THUMB_H: u32 = 96;
pub const PEAKS_PER_S: u32 = 100;
const PEAK_RATE: u32 = 8_000;
const MAX_PEAK_BINS: u32 = 24 * 60 * 60 * PEAKS_PER_S;
const MAX_THUMBS: i64 = 400;

pub fn thumbs_dir(dir: &Path) -> PathBuf {
    dir.join("thumbs")
}

/// `kind` is `v1` for every audio track, `mic` or `system` for one kind.
pub fn peaks_path(dir: &Path, kind: &str) -> PathBuf {
    match kind {
        "mic" | "system" => thumbs_dir(dir).join(format!("peaks-{kind}-v1.bin")),
        _ => thumbs_dir(dir).join("peaks-v1.bin"),
    }
}

pub fn lane_thumb_path(dir: &Path, lane: Lane, step_ms: i64, index: i64) -> PathBuf {
    thumbs_dir(dir).join(format!("{}{step_ms}-{index}.png", lane.prefix()))
}

pub fn thumb_step_ms(rec_len_ns: i64) -> i64 {
    let ms = rec_len_ns / 1_000_000;
    let raw = ((ms + MAX_THUMBS - 1) / MAX_THUMBS).max(1000);
    (raw + 499) / 500 * 500
}

/// 0, ½, ¼, ¾, ⅛, … of `n`: each pass halves the gaps.
fn coarse_to_fine(n: i64) -> Vec<i64> {
    let mut order = Vec::with_capacity(n as usize);
    let mut seen = vec![false; n as usize];
    let mut step = n.max(1);
    while step >= 1 {
        let mut i = 0;
        while i < n {
            if !seen[i as usize] {
                seen[i as usize] = true;
                order.push(i);
            }
            i += step;
        }
        if step == 1 {
            break;
        }
        step = (step + 1) / 2;
    }
    order
}

fn current(core: &Weak<Core>, session: u64) -> Option<Arc<Core>> {
    let c = core.upgrade()?;
    if c.exiting() {
        return None;
    }
    let ok = c
        .editor
        .session
        .lock()
        .ok()?
        .as_ref()
        .is_some_and(|s| s.id == session);
    ok.then_some(c)
}

pub fn spawn(core: &Arc<Core>, session: u64, dir: PathBuf, project: Arc<ProjectV1>) {
    let weak = Arc::downgrade(core);
    let r = std::thread::Builder::new()
        .name("sc-editor-media".into())
        .spawn(move || {
            if let Err(e) = std::fs::create_dir_all(thumbs_dir(&dir)) {
                tracing::warn!("thumbs folder: {e}");
                return;
            }
            peaks(&weak, session, &dir, &project);
            thumbs(&weak, session, &dir, &project);
        });
    if let Err(e) = r {
        tracing::warn!("editor media thread: {e}");
    }
}

fn peaks(core: &Weak<Core>, session: u64, dir: &Path, project: &ProjectV1) {
    if !peaks_path(dir, "system").exists() {
        let Some(c) = current(core, session) else {
            return;
        };
        if let Err(e) = write_peaks(&c, dir, project) {
            tracing::warn!("peaks: {e}");
            return;
        }
    }
    if let Some(c) = current(core, session) {
        c.emit(
            "editor-media",
            json!({ "session": session, "peaks": true, "peaks_per_s": PEAKS_PER_S }),
        );
    }
}

/// One byte per 10 ms of recording time: the loudest sample over the tracks, on a square-root
/// scale so quiet speech still shows. Written for the microphone, the screen's sound (system and device audio) and both.
fn write_peaks(core: &Core, dir: &Path, project: &ProjectV1) -> Result<(), String> {
    let of = |kinds: &[TrackKind]| -> Result<Vec<f32>, String> {
        let ids: Vec<String> = project
            .recording
            .tracks
            .iter()
            .filter(|t| kinds.contains(&t.kind))
            .map(|t| t.id.clone())
            .collect();
        window_peaks(core, dir, project, &ids)
    };
    let (mic, system) = (
        of(&[TrackKind::Mic])?,
        of(&[TrackKind::SystemAudio, TrackKind::DeviceAudio])?,
    );
    let all: Vec<f32> = mic.iter().zip(&system).map(|(a, b)| a.max(*b)).collect();
    let bytes = |v: &[f32]| -> Vec<u8> {
        v.iter()
            .map(|p| (p.clamp(0.0, 1.0).sqrt() * 255.0).round() as u8)
            .collect()
    };
    for (kind, v) in [("mic", &mic), ("v1", &all), ("system", &system)] {
        sc_core::fsio::atomic_write(&peaks_path(dir, kind), &bytes(v))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The loudest sample of `tracks` in each 10 ms of recording time, in dBFS.
pub fn loudness_db(
    core: &Core,
    dir: &Path,
    project: &ProjectV1,
    tracks: &[String],
) -> Result<Vec<f32>, String> {
    Ok(window_peaks(core, dir, project, tracks)?
        .into_iter()
        .map(|p| 20.0 * p.max(1e-6).log10())
        .collect())
}

fn window_peaks(
    core: &Core,
    dir: &Path,
    project: &ProjectV1,
    tracks: &[String],
) -> Result<Vec<f32>, String> {
    let len = project.duration_ns().unwrap_or(0).max(0);
    let bins = i128::from(len) * i128::from(PEAKS_PER_S) / 1_000_000_000;
    if bins > i128::from(MAX_PEAK_BINS) {
        return Err(format!(
            "The recording claims to last more than {} hours, which can't be right, so its sound can't be read.",
            len / 3_600_000_000_000
        ));
    }
    let bins = bins as usize;
    let per_bin = (PEAK_RATE / PEAKS_PER_S) as usize;
    let mut peaks = vec![0f32; bins];
    for id in tracks {
        let mut a = TrackAudio::open(project, dir, id, core.platform.decoder(), PEAK_RATE, 1)
            .map_err(|e| e.to_string())?;
        let mut buf = vec![0f32; per_bin * 100];
        let mut bin = 0;
        while bin < bins {
            a.read(&mut buf).map_err(|e| e.to_string())?;
            for chunk in buf.chunks(per_bin) {
                if bin >= bins {
                    break;
                }
                let m = chunk.iter().fold(0f32, |m, s| m.max(s.abs()));
                peaks[bin] = peaks[bin].max(m);
                bin += 1;
            }
        }
    }
    Ok(peaks)
}

fn ratio_size(ratio: Ratio) -> SizePx {
    SizePx {
        w: ((f64::from(THUMB_H) * ratio.as_f64()).round() as u32).max(2) & !1,
        h: THUMB_H,
    }
}

fn thumbs(core: &Weak<Core>, session: u64, dir: &Path, project: &ProjectV1) {
    let len = project.duration_ns().unwrap_or(0).max(0);
    let ratio = project
        .recording
        .screen
        .segments
        .first()
        .filter(|s| s.content_px.w > 0 && s.content_px.h > 0)
        .map(|s| {
            Ratio::of(SizePx {
                w: s.content_px.w,
                h: s.content_px.h,
            })
        })
        .unwrap_or(Ratio::new(16, 9));
    let Some(c) = current(core, session) else {
        return;
    };
    let decoder = c.platform.decoder();
    drop(c);
    let mut screen: Option<SegmentFrameSource> = None;
    let opened = render_thumbs(
        core,
        session,
        dir,
        len,
        Lane::Screen,
        ratio_size(ratio),
        |t| {
            if screen.is_none() {
                screen = Some(
                    SegmentFrameSource::open(project, dir, decoder.clone())
                        .map_err(|e| e.to_string())?,
                );
            }
            let Some(src) = screen.as_mut() else {
                return Ok(None);
            };
            Ok(src
                .frame_at(t)
                .map_err(|e| e.to_string())?
                .filter(|f| f.content_px.w > 0 && f.content_px.h > 0)
                .map(|f| (f.frame, f.content_px)))
        },
    );
    if !opened {
        return;
    }
    let Some(overlay) = project.camera_layout.overlays.first() else {
        return;
    };
    let Some(track) = project
        .recording
        .tracks
        .iter()
        .find(|t| t.id == overlay.track_id)
    else {
        return;
    };
    let ratio = track
        .video
        .as_ref()
        .filter(|v| v.size_px.w > 0 && v.size_px.h > 0)
        .map_or(Ratio::new(16, 9), |v| Ratio::of(v.size_px));
    let mut camera: Option<TrackFrameSource> = None;
    render_thumbs(
        core,
        session,
        dir,
        len,
        Lane::Camera,
        ratio_size(ratio),
        |t| {
            if camera.is_none() {
                camera = TrackFrameSource::open(project, dir, decoder.clone(), &overlay.track_id)
                    .map_err(|e| e.to_string())?;
            }
            let Some(src) = camera.as_mut() else {
                return Ok(None);
            };
            Ok(src.frame_at(t).map_err(|e| e.to_string())?.map(|f| {
                let size = f.0.size();
                let rect = sc_geometry::RectPx {
                    x: 0,
                    y: 0,
                    w: size.w,
                    h: size.h,
                };
                (f, rect)
            }))
        },
    );
}

#[derive(Clone, Copy)]
pub enum Lane {
    Screen,
    Camera,
}

impl Lane {
    fn prefix(self) -> &'static str {
        match self {
            Lane::Screen => "t",
            Lane::Camera => "c",
        }
    }

    fn event_key(self) -> &'static str {
        match self {
            Lane::Screen => "thumbs",
            Lane::Camera => "camera_thumbs",
        }
    }
}

/// Renders the missing thumbnails of one lane, announcing them in batches; `frame` gives the
/// lane's frame at a recording time and the part of it to show. False when the editor moved on.
fn render_thumbs(
    core: &Weak<Core>,
    session: u64,
    dir: &Path,
    len: i64,
    lane: Lane,
    size: SizePx,
    mut frame: impl FnMut(i64) -> Result<Option<(FrameBuffer, sc_geometry::RectPx)>, String>,
) -> bool {
    let step_ms = thumb_step_ms(len);
    let count = ((len / 1_000_000 + step_ms - 1) / step_ms).max(1);
    let announce = |ready: &[i64]| {
        if let Some(c) = current(core, session) {
            c.emit(
                "editor-media",
                json!({
                    "session": session,
                    lane.event_key(): { "step_ms": step_ms, "count": count, "w": size.w, "h": size.h, "ready": ready },
                }),
            );
        }
    };
    let order = coarse_to_fine(count);
    let (have, todo): (Vec<i64>, Vec<i64>) = order
        .iter()
        .partition(|i| lane_thumb_path(dir, lane, step_ms, **i).exists());
    announce(&have);
    if todo.is_empty() {
        return true;
    }
    let mut compositor = match Compositor::new(Backend::Auto) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("thumbs: {e}");
            return false;
        }
    };
    let black = RgbaImage {
        size,
        stride: size.w as usize * 4,
        data: [0, 0, 0, 255].repeat((size.w * size.h) as usize),
    };
    let mut batch = Vec::new();
    for i in todo {
        if current(core, session).is_none() {
            return false;
        }
        let t = (i * step_ms * 1_000_000).min((len - 1).max(0));
        let shown = match frame(t) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("{} thumb {i}: {e}", lane.event_key());
                return true;
            }
        };
        let screen = shown.as_ref().map(|(f, src)| ScreenLayer {
            frame: f,
            src: *src,
            dst: sc_geometry::contain_fit(size, SizePx { w: src.w, h: src.h }),
            redact: &[],
        });
        let job = FrameJob {
            text: &[],
            canvas: size,
            background: &black,
            background_key: Some(1),
            screen,
            camera: None,
        };
        let png = compositor
            .render(&job, OutputFormat::Rgba8)
            .map_err(|e| e.to_string())
            .and_then(|r| {
                sc_text::encode_png(&RgbaImage {
                    size: r.size,
                    stride: r.size.w as usize * 4,
                    data: r.data,
                })
                .map_err(|e| e.to_string())
            });
        match png.and_then(|b| {
            sc_core::fsio::atomic_write(&lane_thumb_path(dir, lane, step_ms, i), &b)
                .map_err(|e| e.to_string())
        }) {
            Ok(()) => batch.push(i),
            Err(e) => tracing::warn!("thumb {i}: {e}"),
        }
        if batch.len() >= 8 {
            announce(&batch);
            batch.clear();
        }
    }
    announce(&batch);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_and_order() {
        assert_eq!(thumb_step_ms(10_000_000_000), 1000);
        assert_eq!(thumb_step_ms(600_000_000_000), 1500);
        assert_eq!(thumb_step_ms(1_800_000_000_000), 4500);
        let o = coarse_to_fine(9);
        assert_eq!(o.len(), 9);
        assert_eq!(&o[..3], &[0, 5, 3]);
        let mut sorted = o.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..9).collect::<Vec<_>>());
        assert_eq!(coarse_to_fine(1), vec![0]);
    }

    #[test]
    fn each_lane_keeps_its_own_thumbnails() {
        let d = Path::new("/p");
        assert_eq!(
            lane_thumb_path(d, Lane::Screen, 1000, 3),
            Path::new("/p/thumbs/t1000-3.png")
        );
        assert_eq!(
            lane_thumb_path(d, Lane::Camera, 1000, 3),
            Path::new("/p/thumbs/c1000-3.png")
        );
    }
}
