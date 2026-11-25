//! The pointer log behind our own click highlights (where the system cannot draw them):
//! `cursor.jsonl`, one JSON line per change of position or buttons, at most every 16 ms, with the
//! recording time, the position in global points and the buttons down.

use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use super::shared::Shared;

pub const FILE_NAME: &str = "cursor.jsonl";
const EVERY: Duration = Duration::from_millis(16);

pub(crate) struct PointerLog {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl PointerLog {
    pub(crate) fn spawn(sh: Arc<Shared>) -> Option<PointerLog> {
        sh.platform.desktop().pointer().ok()?;
        let file = std::fs::File::create(sh.dir.join(FILE_NAME)).ok()?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let join = std::thread::Builder::new()
            .name("sc-pointer".into())
            .spawn(move || {
                let mut out = std::io::BufWriter::new(file);
                let mut last = None;
                let mut lines = 0u32;
                while !flag.load(Ordering::Acquire) {
                    if let Ok(p) = sh.platform.desktop().pointer() {
                        let now = (p.at.x.round(), p.at.y.round(), p.buttons);
                        if last != Some(now) {
                            last = Some(now);
                            let t = sh.now() - sh.start_host();
                            let _ = writeln!(
                                out,
                                "{{\"t\":{t},\"x\":{:.1},\"y\":{:.1},\"b\":{}}}",
                                p.at.x, p.at.y, p.buttons
                            );
                            lines += 1;
                            if lines.is_multiple_of(30) {
                                let _ = out.flush();
                            }
                        }
                    }
                    std::thread::sleep(EVERY);
                }
                let _ = out.flush();
            })
            .ok()?;
        Some(PointerLog {
            stop,
            join: Some(join),
        })
    }

    pub(crate) fn stop(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}
