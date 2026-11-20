use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use sc_core::project::{ProjectV1, ScreenSegment, SourceDescriptor, TrackKind};
use sc_decode::TrackAudio;
use sc_platform::Decoder;

use super::stats::{SyncRow, luma_onset, tone_onset};

const WINDOW_NS: i64 = 700_000_000;
const RATE: u32 = 48_000;
const MIN_LUMA_STEP: f64 = 40.0;
const MIN_TONE_RATIO: f64 = 8.0;

#[derive(Debug, Clone, Copy)]
pub struct FlashGeom {
    pub rect_pt: [f64; 4],
    pub display_pt: [f64; 2],
}

/// The flash panel's inner 60 % in the segment's frame pixels (edges skip shadows and scaling).
pub fn flash_rect_px(seg: &ScreenSegment, g: &FlashGeom) -> (u32, u32, u32, u32) {
    let (fw, fh) = (seg.frame_px.w, seg.frame_px.h);
    let (x, y, w, h) = match &seg.source {
        SourceDescriptor::Display { frame_pt, .. } => {
            let wpt = frame_pt.map_or(g.display_pt[0], |f| f.w);
            let k = f64::from(fw) / wpt.max(1.0);
            (
                g.rect_pt[0] * k,
                g.rect_pt[1] * k,
                g.rect_pt[2] * k,
                g.rect_pt[3] * k,
            )
        }
        SourceDescriptor::Window { .. } => {
            let c = seg.content_px;
            (
                f64::from(c.x),
                f64::from(c.y),
                f64::from(c.w),
                f64::from(c.h),
            )
        }
        _ => (0.0, 0.0, f64::from(fw), f64::from(fh)),
    };
    let (x, y, w, h) = (x + w * 0.2, y + h * 0.2, w * 0.6, h * 0.6);
    let x0 = x.clamp(0.0, f64::from(fw)) as u32;
    let y0 = y.clamp(0.0, f64::from(fh)) as u32;
    let x1 = (x + w).clamp(0.0, f64::from(fw)) as u32;
    let y1 = (y + h).clamp(0.0, f64::from(fh)) as u32;
    (x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
}

fn segment_at(p: &ProjectV1, t: i64) -> Option<&ScreenSegment> {
    p.recording
        .screen
        .segments
        .iter()
        .filter(|s| s.start_rec_ns <= t)
        .max_by_key(|s| s.start_rec_ns)
}

fn video_lumas(
    p: &ProjectV1,
    dir: &Path,
    dec: &dyn Decoder,
    g: &FlashGeom,
    lo: i64,
    hi: i64,
) -> Result<Vec<(i64, f64)>> {
    let seg = segment_at(p, hi).ok_or_else(|| anyhow!("no screen segment at {hi} ns"))?;
    let track = p
        .recording
        .tracks
        .iter()
        .find(|t| t.id == seg.track_id)
        .ok_or_else(|| anyhow!("track {} missing", seg.track_id))?;
    let path = dir.join(&track.file);
    let mut r = dec
        .open_video(&path)
        .map_err(|e| anyhow!("open {}: {e}", path.display()))?;
    let (rx, ry, rw, rh) = flash_rect_px(seg, g);
    let mut out = Vec::new();
    while let Some(f) = r
        .next_frame()
        .map_err(|e| anyhow!("decode {}: {e}", path.display()))?
    {
        let t = seg.start_rec_ns + f.file_ns;
        if t < lo {
            continue;
        }
        if t > hi {
            break;
        }
        let mut sum = 0u64;
        let mut n = 0u64;
        f.frame
            .0
            .with_nv12(&mut |v| {
                let x1 = (rx + rw).min(v.size.w) as usize;
                let y1 = (ry + rh).min(v.size.h) as usize;
                for row in (ry as usize..y1).step_by(2) {
                    let line = &v.y[row * v.y_stride..row * v.y_stride + x1];
                    for px in line[rx as usize..].iter().step_by(2) {
                        sum += u64::from(*px);
                        n += 1;
                    }
                }
            })
            .map_err(|e| anyhow!("nv12: {e}"))?;
        if n > 0 {
            out.push((t, sum as f64 / n as f64));
        }
    }
    Ok(out)
}

fn audio_windows(
    p: &ProjectV1,
    dir: &Path,
    dec: Arc<dyn Decoder>,
    track_id: &str,
    wins: &[(i64, i64)],
) -> Result<Vec<Vec<f32>>> {
    let mut a = TrackAudio::open(p, dir, track_id, dec, RATE, 1)
        .map_err(|e| anyhow!("open audio {track_id}: {e}"))?;
    let to_frame = |ns: i64| (i128::from(ns) * i128::from(RATE) / 1_000_000_000) as i64;
    let end = wins.iter().map(|w| to_frame(w.1)).max().unwrap_or(0);
    let mut out: Vec<Vec<f32>> = vec![Vec::new(); wins.len()];
    let mut buf = vec![0f32; RATE as usize];
    let mut pos = 0i64;
    while pos < end {
        let n = a
            .read(&mut buf)
            .map_err(|e| anyhow!("read audio {track_id}: {e}"))? as i64;
        if n == 0 {
            break;
        }
        for (i, w) in wins.iter().enumerate() {
            let (s, e) = (to_frame(w.0).max(pos), to_frame(w.1).min(pos + n));
            if s < e {
                out[i].extend_from_slice(&buf[(s - pos) as usize..(e - pos) as usize]);
            }
        }
        pos += n;
    }
    Ok(out)
}

/// `flashes` are (label, issue time in recording ns).
pub fn measure_sync(
    dir: &Path,
    dec: Arc<dyn Decoder>,
    g: &FlashGeom,
    flashes: &[(String, i64)],
) -> Result<Vec<SyncRow>> {
    let p = ProjectV1::load(dir)
        .map_err(|e| anyhow!("{e}"))
        .context("project.json")?;
    let wins: Vec<(i64, i64)> = flashes
        .iter()
        .map(|f| ((f.1 - WINDOW_NS).max(0), f.1 + WINDOW_NS))
        .collect();
    let mut rows: Vec<SyncRow> = flashes
        .iter()
        .map(|(l, t)| SyncRow {
            label: l.clone(),
            at_s: *t as f64 / 1e9,
            ..SyncRow::default()
        })
        .collect();
    let mut video = Vec::new();
    for (row, w) in rows.iter_mut().zip(&wins) {
        let v = match video_lumas(&p, dir, &*dec, g, w.0, w.1) {
            Ok(l) => luma_onset(&l, MIN_LUMA_STEP),
            Err(e) => {
                row.note = Some(format!("screen: {e:#}"));
                None
            }
        };
        if v.is_none() && row.note.is_none() {
            row.note = Some("flash not seen on screen".into());
        }
        row.video_latency_ms = v.map(|o| (o.t_ns as f64 - row.at_s * 1e9) / 1e6);
        row.video_step = v.map(|o| o.strength);
        video.push(v);
    }
    for t in &p.recording.tracks {
        let kind = match t.kind {
            TrackKind::Mic => "mic",
            TrackKind::SystemAudio => "system_audio",
            _ => continue,
        };
        let audio = match audio_windows(&p, dir, dec.clone(), &t.id, &wins) {
            Ok(a) => a,
            Err(e) => {
                for row in &mut rows {
                    row.offsets_ms.push((kind.into(), None));
                    let n = row.note.get_or_insert_with(String::new);
                    n.push_str(&format!(" {kind}: {e:#}"));
                }
                continue;
            }
        };
        for (i, row) in rows.iter_mut().enumerate() {
            let on = tone_onset(&audio[i], RATE, wins[i].0, 1000.0, 480, 96, MIN_TONE_RATIO);
            let off = on
                .zip(video[i])
                .map(|(a, v)| (a.t_ns - v.t_ns) as f64 / 1e6);
            if on.is_none() {
                let n = row.note.get_or_insert_with(String::new);
                n.push_str(&format!(" {kind}: beep not heard."));
            }
            row.offsets_ms.push((kind.into(), off));
        }
    }
    for row in &mut rows {
        if let Some(n) = &mut row.note {
            *n = n.trim().to_string();
        }
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::project::{StartReason, Via};
    use sc_geometry::{RectPt, RectPx, SizePx};

    fn seg(source: SourceDescriptor, w: u32, h: u32, content: RectPx) -> ScreenSegment {
        ScreenSegment {
            id: "s0".into(),
            track_id: "screen-s0".into(),
            start_rec_ns: 0,
            end_rec_ns: None,
            start_reason: StartReason::RecordStart,
            end_reason: None,
            via: Via::OwnChooser,
            source,
            frame_px: SizePx { w, h },
            content_px: content,
            extra: Default::default(),
        }
    }

    #[test]
    fn rect_on_a_retina_display() {
        let g = FlashGeom {
            rect_pt: [1000.0, 500.0, 200.0, 100.0],
            display_pt: [1512.0, 982.0],
        };
        let s = seg(
            SourceDescriptor::Display {
                display_id: "1".into(),
                display_name: None,
                scale: 2.0,
                frame_pt: Some(RectPt {
                    x: 0.0,
                    y: 0.0,
                    w: 1512.0,
                    h: 982.0,
                }),
            },
            3024,
            1964,
            RectPx {
                x: 0,
                y: 0,
                w: 3024,
                h: 1964,
            },
        );
        assert_eq!(flash_rect_px(&s, &g), (2080, 1040, 240, 120));
    }

    #[test]
    fn rect_in_a_window_segment_is_the_content() {
        let g = FlashGeom {
            rect_pt: [0.0; 4],
            display_pt: [1.0, 1.0],
        };
        let s = seg(
            SourceDescriptor::Window {
                window_id: 7,
                app_bundle_id: None,
                app_name: None,
                window_title: None,
                scale: 2.0,
            },
            480,
            320,
            RectPx {
                x: 0,
                y: 0,
                w: 480,
                h: 320,
            },
        );
        assert_eq!(flash_rect_px(&s, &g), (96, 64, 288, 192));
    }
}
