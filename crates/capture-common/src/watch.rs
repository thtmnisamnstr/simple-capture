//! Device hot-plug by polling: neither cpal nor the camera APIs used here push device changes.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use sc_platform::{DeviceKind, DeviceStream, FrameSink, Result, SourceEvent};

pub type Lister = Box<dyn Fn() -> Vec<String> + Send>;

pub const POLL: Duration = Duration::from_secs(2);

pub struct Watcher {
    stop: mpsc::Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Watcher {
    pub fn start(sink: Arc<dyn FrameSink>, mics: Lister, cameras: Lister) -> Result<Watcher> {
        let (stop, rx) = mpsc::channel::<()>();
        let thread = std::thread::Builder::new()
            .name("device-watch".into())
            .spawn(move || {
                let set = |l: &Lister| l().into_iter().collect::<BTreeSet<String>>();
                let mut last_mics = set(&mics);
                let mut last_cams = set(&cameras);
                while let Err(mpsc::RecvTimeoutError::Timeout) = rx.recv_timeout(POLL) {
                    let m = set(&mics);
                    if m != last_mics {
                        last_mics = m;
                        sink.on_event(SourceEvent::DevicesChanged {
                            kind: DeviceKind::Mic,
                        });
                    }
                    let c = set(&cameras);
                    if c != last_cams {
                        last_cams = c;
                        sink.on_event(SourceEvent::DevicesChanged {
                            kind: DeviceKind::Camera,
                        });
                    }
                }
            })?;
        Ok(Watcher {
            stop,
            thread: Some(thread),
        })
    }
}

impl DeviceStream for Watcher {
    fn format(&self) -> String {
        "device watcher (polling)".into()
    }

    fn stop(mut self: Box<Self>) -> Result<()> {
        let _ = self.stop.send(());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        Ok(())
    }
}
