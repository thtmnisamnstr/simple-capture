//! SIGINT/SIGTERM: a dedicated thread waits for them and runs the app's exit path.

use std::sync::{Arc, OnceLock};

#[cfg(unix)]
use sc_core::journal::StopReason;

use crate::core::Core;

static CORE: OnceLock<Arc<Core>> = OnceLock::new();

pub fn attach(core: Arc<Core>) {
    let _ = CORE.set(core);
}

/// Call first in `main`, before any thread exists, so every thread inherits the blocked mask and
/// only the waiter sees the signals.
#[cfg(unix)]
pub fn install() {
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGINT);
        libc::sigaddset(&mut set, libc::SIGTERM);
        if libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut()) != 0 {
            tracing::warn!("signals: pthread_sigmask failed; default signal handling stays");
            return;
        }
        let set = set;
        let spawned = std::thread::Builder::new()
            .name("signals".into())
            .spawn(move || {
                let mut seen = 0;
                loop {
                    let mut sig: libc::c_int = 0;
                    if libc::sigwait(&set, &mut sig) != 0 {
                        continue;
                    }
                    seen += 1;
                    tracing::info!(sig, "signal received");
                    match (seen, CORE.get()) {
                        (1, Some(core)) => {
                            let core = core.clone();
                            std::thread::spawn(move || core.shutdown(StopReason::Signal, 0));
                        }
                        _ => std::process::exit(128 + sig),
                    }
                }
            });
        if let Err(e) = spawned {
            tracing::warn!("signals: thread: {e}");
        }
    }
}

#[cfg(not(unix))]
pub fn install() {}
