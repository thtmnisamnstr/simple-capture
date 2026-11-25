//! Live frames for the preview window (ADR-007 webview frames, spike 2 mode B): the
//! renderer publishes the newest frame per stream, and `sc://live/<name>?after=<id>` waits
//! for one newer than `id`. Body: a 16-byte header (id, width, height, format as little-endian
//! u32) followed by the tightly packed pixels: premultiplied RGBA (format 0) or I420 planes, BT.709
//! limited range ([`I420`]). `NATIVE` (macOS) carries no pixels: a native layer under the page
//! draws the frame, and the header gives its size.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub const HEADER: usize = 16;
pub const I420: u32 = 1;
#[cfg(target_os = "macos")]
pub const NATIVE: u32 = 2;

use crate::core::lock;

#[derive(Clone)]
pub struct LiveFrame {
    pub id: u32,
    pub body: Arc<Vec<u8>>,
}

/// Ids keep counting up across `clear`, so a page still waiting after the last id it saw gets
/// the next frame even when the stream was cleared in between.
#[derive(Default)]
pub struct LiveHub {
    frames: Mutex<(HashMap<String, LiveFrame>, u32)>,
    cv: Condvar,
}

impl LiveHub {
    pub fn publish(&self, name: &str, w: u32, h: u32, format: u32, pixels: &[u8]) {
        let mut guard = lock(&self.frames);
        let (frames, last_id) = &mut *guard;
        *last_id = last_id.wrapping_add(1).max(1);
        let id = *last_id;
        let mut body = Vec::with_capacity(HEADER + pixels.len());
        for v in [id, w, h, format] {
            body.extend_from_slice(&v.to_le_bytes());
        }
        body.extend_from_slice(pixels);
        frames.insert(
            name.to_string(),
            LiveFrame {
                id,
                body: Arc::new(body),
            },
        );
        self.cv.notify_all();
    }

    pub fn clear(&self, name: &str) {
        lock(&self.frames).0.remove(name);
    }

    pub fn next(&self, name: &str, after: u32, timeout: Duration) -> Option<LiveFrame> {
        let end = Instant::now() + timeout;
        let mut guard = lock(&self.frames);
        loop {
            if let Some(f) = guard.0.get(name)
                && f.id != after
            {
                return Some(f.clone());
            }
            let left = end.checked_duration_since(Instant::now())?;
            guard = self
                .cv
                .wait_timeout(guard, left)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }
}

pub fn parse(host: Option<&str>, path: &str, query: Option<&str>) -> Option<(String, u32)> {
    let rest = match host {
        Some("live") => path.trim_start_matches('/'),
        Some("localhost") | None => path.trim_start_matches('/').strip_prefix("live/")?,
        Some(_) => return None,
    };
    let name = rest.trim_end_matches('/');
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    let after = query
        .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("after=")))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    Some((name.to_string(), after))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RGBA: u32 = 0;

    #[test]
    fn next_returns_newer_frames_and_times_out_otherwise() {
        let hub = LiveHub::default();
        assert!(hub.next("stage", 0, Duration::from_millis(5)).is_none());
        hub.publish("stage", 2, 1, RGBA, &[1, 2, 3, 4, 5, 6, 7, 8]);
        let f = hub
            .next("stage", 0, Duration::from_millis(5))
            .expect("frame");
        assert_eq!(f.id, 1);
        assert_eq!(
            &f.body[..16],
            &[1, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(f.body.len(), HEADER + 8);
        assert!(hub.next("stage", 1, Duration::from_millis(5)).is_none());
        let hub = Arc::new(hub);
        let h = hub.clone();
        let t = std::thread::spawn(move || h.next("stage", 1, Duration::from_secs(2)));
        std::thread::sleep(Duration::from_millis(20));
        hub.publish("stage", 2, 2, I420, &[0; 6]);
        assert_eq!(t.join().expect("join").expect("woken").id, 2);
    }

    #[test]
    fn a_page_waiting_across_a_clear_gets_the_next_frame() {
        let hub = LiveHub::default();
        hub.publish("stage", 2, 2, RGBA, &[]);
        let seen = hub
            .next("stage", 0, Duration::from_millis(5))
            .expect("first")
            .id;
        hub.clear("stage");
        hub.publish("stage", 4, 2, RGBA, &[]);
        let f = hub
            .next("stage", seen, Duration::from_millis(5))
            .expect("not mistaken for the frame already seen");
        assert_eq!(&f.body[4..8], &4u32.to_le_bytes());
    }

    #[test]
    fn parses_live_uris() {
        assert_eq!(
            parse(Some("live"), "/stage", Some("after=7")),
            Some(("stage".into(), 7))
        );
        assert_eq!(
            parse(Some("localhost"), "/live/stage", None),
            Some(("stage".into(), 0))
        );
        assert_eq!(parse(Some("live"), "/../x", None), None);
        assert_eq!(parse(Some("still"), "/a.png", None), None);
    }
}
