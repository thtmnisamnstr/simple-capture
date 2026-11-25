//! The microphone level in the recorder panel, while the panel is open and nothing records.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use sc_platform::{AudioBuffer, AudioTrackKind, FrameSink, SourceEvent, VideoFrame};
use serde_json::json;
use tauri::{AppHandle, Emitter};

const EVERY: Duration = Duration::from_millis(66);

pub struct MeterSink {
    pub app: AppHandle,
    pub last: Mutex<(Instant, f32)>,
}

fn db(v: f32) -> f32 {
    if v <= 1e-6 { -120.0 } else { 20.0 * v.log10() }
}

impl FrameSink for MeterSink {
    fn on_screen(&self, _: VideoFrame) {}
    fn on_camera(&self, _: VideoFrame) {}
    fn on_event(&self, _: SourceEvent) {}

    fn on_audio(&self, _: AudioTrackKind, a: AudioBuffer) {
        let Some(s) = a.data.samples() else { return };
        let peak = s.iter().fold(0f32, |m, v| m.max(v.abs()));
        let mut g = self
            .last
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        g.1 = g.1.max(peak);
        if g.0.elapsed() < EVERY {
            return;
        }
        let level = g.1;
        *g = (Instant::now(), 0.0);
        drop(g);
        let _ = self.app.emit("mic-level", json!({ "peak_db": db(level) }));
    }
}
