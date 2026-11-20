use std::sync::mpsc;
use std::time::Duration;

use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;
use sc_platform::Result;

use crate::util::wait_on;

/// Only wraps objects whose framework documents thread safety (SCStream, SCContentFilter,
/// CMSampleBuffer) or that are touched again only on the main thread.
pub(crate) struct SendBox<T>(pub T);

// SAFETY: see the type's doc.
unsafe impl<T> Send for SendBox<T> {}
// SAFETY: see the type's doc.
unsafe impl<T> Sync for SendBox<T> {}

pub(crate) fn main_async(f: impl FnOnce(MainThreadMarker) + Send + 'static) {
    if let Some(mtm) = MainThreadMarker::new() {
        f(mtm);
        return;
    }
    DispatchQueue::main().exec_async(move || {
        if let Some(mtm) = MainThreadMarker::new() {
            f(mtm);
        }
    });
}

/// Times out unless the host is servicing the main queue.
pub(crate) fn main_sync<R: Send + 'static>(
    what: &str,
    timeout: Duration,
    f: impl FnOnce(MainThreadMarker) -> R + Send + 'static,
) -> Result<R> {
    if let Some(mtm) = MainThreadMarker::new() {
        return Ok(f(mtm));
    }
    let (tx, rx) = mpsc::channel();
    main_async(move |mtm| {
        let _ = tx.send(f(mtm));
    });
    wait_on(&rx, timeout, what)
}
