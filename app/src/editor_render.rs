//! The editor preview's renderer: the compositor's output for the playhead, drawn into the
//! preview's part of the editor window (a native layer on macOS, ADR-007). While playing, the
//! playback clock picks the frame; otherwise the playhead does, and a frame is drawn only when
//! something it shows changed.

use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use sc_geometry::SizePx;
use sc_player::video::TimelineVideo;
use sc_render::{Backend, Compositor};
use serde_json::json;

use crate::core::Core;
use crate::sink::Sink;

const CLEAR: [u8; 3] = [0, 0, 0];
const PLAYHEAD_EVERY: Duration = Duration::from_millis(33);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

#[derive(PartialEq, Clone, Copy)]
struct Shown {
    session: u64,
    revision: u64,
    at_ns: i64,
    canvas: SizePx,
    drawable: SizePx,
    at: (u32, u32),
}

struct Snap {
    id: u64,
    dir: std::path::PathBuf,
    project: Arc<sc_core::project::ProjectV1>,
    revision: u64,
    doc: Option<sc_core::edit::Doc>,
    playhead: i64,
    playing: bool,
    rect: Option<crate::editor::PreviewRect>,
    fps: u32,
    duration: i64,
}

fn snapshot(core: &Core, seen: Option<(u64, u64)>) -> Option<Snap> {
    let g = lock(&core.editor.session);
    let s = g.as_ref()?;
    let fresh = seen != Some((s.id, s.history.revision()));
    Some(Snap {
        id: s.id,
        dir: s.dir.clone(),
        project: s.project.clone(),
        revision: s.history.revision(),
        doc: fresh.then(|| s.history.doc().clone()),
        playhead: s.playhead,
        playing: s.playing,
        rect: s.rect,
        fps: s.fps,
        duration: s.duration(),
    })
}

fn wait(core: &Core, d: Duration) {
    let g = lock(&core.editor.session);
    let _ = core.editor.wake.wait_timeout(g, d);
}

pub fn render_loop(core: Weak<Core>) {
    let mut compositor: Option<Compositor> = None;
    let mut video: Option<(u64, TimelineVideo)> = None;
    let mut sink: Option<Sink> = None;
    let mut seen: Option<(u64, u64)> = None;
    let mut shown: Option<Shown> = None;
    let mut told: Option<serde_json::Value> = None;
    let mut last_emit = Instant::now();
    loop {
        let Some(core) = core.upgrade() else { return };
        if core.exiting() {
            return;
        }
        core.editor_autosave_tick();
        let Some(snap) = snapshot(&core, seen) else {
            video = None;
            seen = None;
            shown = None;
            told = None;
            wait(&core, Duration::from_millis(500));
            continue;
        };
        if let Some(doc) = snap.doc.clone() {
            match video.as_mut() {
                Some((id, v)) if *id == snap.id => v.set_doc(doc),
                _ => {
                    video = match TimelineVideo::open(
                        &snap.project,
                        &snap.dir,
                        core.platform.decoder(),
                        doc,
                    ) {
                        Ok(v) => Some((snap.id, v)),
                        Err(e) => {
                            tracing::warn!("editor preview: {e}");
                            None
                        }
                    };
                }
            }
            seen = Some((snap.id, snap.revision));
        }
        let Some((_, v)) = video.as_mut() else {
            wait(&core, Duration::from_millis(250));
            continue;
        };
        let mut tl = snap.playhead;
        if snap.playing {
            let now = lock(&core.editor.playback)
                .as_ref()
                .map_or(snap.playhead, |p| p.now_tl());
            if now >= snap.duration {
                let _ = core.editor_pause();
                tl = snap.duration;
            } else {
                tl = now;
                if let Some(s) = lock(&core.editor.session).as_mut()
                    && s.id == snap.id
                    && s.playing
                {
                    s.playhead = now;
                }
                if last_emit.elapsed() >= PLAYHEAD_EVERY {
                    last_emit = Instant::now();
                    core.emit_playhead(now, true);
                }
            }
        }
        let (Some(page), Some(rect)) = (core.editor_page(), snap.rect) else {
            wait(&core, Duration::from_millis(100));
            continue;
        };
        let compositor = match compositor.as_mut() {
            Some(c) => c,
            None => match Compositor::new(Backend::Auto) {
                Ok(c) => compositor.insert(c),
                Err(e) => {
                    tracing::warn!("editor renderer: {e}");
                    return;
                }
            },
        };
        let sink = sink.get_or_insert_with(|| {
            Sink::open(compositor, "editor", || {
                #[cfg(target_os = "macos")]
                return core.editor_layer();
                #[cfg(not(target_os = "macos"))]
                Err("no native layer".into())
            })
        });
        let scale = sink.scale(page);
        let drawable = sink.drawable(page);
        let px = |p: f64| (p * scale).round().max(0.0) as u32;
        let (rx, ry, rw, rh) = (px(rect.x), px(rect.y), px(rect.w).max(2), px(rect.h).max(2));
        let canvas = crate::stage::fit(v.canvas_ratio(), SizePx { w: rw, h: rh });
        let at = (
            rx + rw.saturating_sub(canvas.w) / 2,
            ry + rh.saturating_sub(canvas.h) / 2,
        );
        let fps = i64::from(snap.fps.max(1));
        let at_ns = if snap.playing {
            tl * fps / 1_000_000_000
        } else {
            tl
        };
        let now_showing = Shown {
            session: snap.id,
            revision: snap.revision,
            at_ns,
            canvas,
            drawable,
            at,
        };
        if shown == Some(now_showing) {
            if snap.playing {
                let next = ((at_ns + 1) * 1_000_000_000 / fps - tl).clamp(1_000_000, 16_000_000);
                drop(core);
                std::thread::sleep(Duration::from_nanos(next as u64));
            } else {
                wait(&core, Duration::from_millis(250));
            }
            continue;
        }
        let composed = match v.compose(tl) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("editor preview frame: {e}");
                wait(&core, Duration::from_millis(250));
                continue;
            }
        };
        let bg = match v.background(canvas) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("editor preview background: {e}");
                wait(&core, Duration::from_millis(250));
                continue;
            }
        };
        let job = composed.job(canvas, &bg, Some(Arc::as_ptr(&bg) as u64));
        match sink.show(&core, compositor, &job, drawable, Some(at), CLEAR) {
            Ok(true) => {
                shown = Some(now_showing);
                core.editor
                    .frames
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if let Some(s) = lock(&core.editor.session).as_mut()
                    && s.id == snap.id
                    && s.first_frame_ms.is_none()
                {
                    let ms = s.opened_at.elapsed().as_millis() as u64;
                    s.first_frame_ms = Some(ms);
                    tracing::info!(ms, "editor: first frame after opening");
                }
                let s = 1.0 / scale;
                let cam = composed
                    .camera_dst(canvas)
                    .map(|r| json!({ "x": r.x * s, "y": r.y * s, "w": r.w * s, "h": r.h * s }));
                let frame = json!({
                    "native": sink.native(),
                    "canvas_px": { "w": canvas.w, "h": canvas.h },
                    "rect": {
                        "x": f64::from(at.0) * s, "y": f64::from(at.1) * s,
                        "w": f64::from(canvas.w) * s, "h": f64::from(canvas.h) * s,
                    },
                    "camera": cam,
                    "rec_ns": composed.rec_ns,
                });
                if told.as_ref() != Some(&frame) {
                    core.emit("editor-frame", frame.clone());
                    told = Some(frame);
                }
            }
            Ok(false) => wait(&core, Duration::from_millis(100)),
            Err(e) => {
                tracing::warn!("editor preview render: {e}");
                wait(&core, Duration::from_millis(250));
            }
        }
    }
}

pub fn spawn(core: &Arc<Core>) {
    let weak = Arc::downgrade(core);
    if let Err(e) = std::thread::Builder::new()
        .name("sc-editor".into())
        .spawn(move || render_loop(weak))
    {
        tracing::warn!("editor renderer thread: {e}");
    }
}
