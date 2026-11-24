//! Timeline thumbnails and the audio waveform for the open project, made in the background and
//! kept in the project's `thumbs/` folder. Both are indexed by recording time, so they stay
//! valid through every edit. Thumbnails are made coarse-to-fine so the strip fills evenly.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use sc_core::image::RgbaImage;
use sc_core::project::{ProjectV1, TrackKind};
use sc_decode::{SegmentFrameSource, TrackAudio};
use sc_geometry::{Ratio, SizePx};
use sc_render::{Backend, Compositor, FrameJob, OutputFormat, ScreenLayer};
use serde_json::json;

use crate::core::Core;

pub const THUMB_H: u32 = 96;
pub const PEAKS_PER_S: u32 = 100;
const PEAK_RATE: u32 = 8_000;
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

pub fn thumb_path(dir: &Path, step_ms: i64, index: i64) -> PathBuf {
    thumbs_dir(dir).join(format!("t{step_ms}-{index}.png"))
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
    let bins = (i128::from(len) * i128::from(PEAKS_PER_S) / 1_000_000_000) as usize;
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

fn thumbs(core: &Weak<Core>, session: u64, dir: &Path, project: &ProjectV1) {
    let len = project.duration_ns().unwrap_or(0).max(0);
    let step_ms = thumb_step_ms(len);
    let count = ((len / 1_000_000 + step_ms - 1) / step_ms).max(1);
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
    let size = SizePx {
        w: ((f64::from(THUMB_H) * ratio.as_f64()).round() as u32).max(2) & !1,
        h: THUMB_H,
    };
    let announce = |ready: &[i64]| {
        if let Some(c) = current(core, session) {
            c.emit(
                "editor-media",
                json!({
                    "session": session,
                    "thumbs": { "step_ms": step_ms, "count": count, "w": size.w, "h": size.h, "ready": ready },
                }),
            );
        }
    };
    let order = coarse_to_fine(count);
    let (have, todo): (Vec<i64>, Vec<i64>) = order
        .iter()
        .partition(|i| thumb_path(dir, step_ms, **i).exists());
    announce(&have);
    if todo.is_empty() {
        return;
    }
    let Some(c) = current(core, session) else {
        return;
    };
    let mut source = match SegmentFrameSource::open(project, dir, c.platform.decoder()) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("thumbs: {e}");
            return;
        }
    };
    drop(c);
    let mut compositor = match Compositor::new(Backend::Auto) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("thumbs: {e}");
            return;
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
            return;
        }
        let t = (i * step_ms * 1_000_000).min((len - 1).max(0));
        let frame = match source.frame_at(t) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("thumb {i}: {e}");
                continue;
            }
        };
        let screen = frame
            .as_ref()
            .filter(|f| f.content_px.w > 0 && f.content_px.h > 0)
            .map(|f| ScreenLayer {
                frame: &f.frame,
                src: f.content_px,
                dst: sc_geometry::contain_fit(
                    size,
                    SizePx {
                        w: f.content_px.w,
                        h: f.content_px.h,
                    },
                ),
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
            sc_core::fsio::atomic_write(&thumb_path(dir, step_ms, i), &b).map_err(|e| e.to_string())
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
}
