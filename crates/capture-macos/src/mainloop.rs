//! Run-loop pumping, ONLY for the CLI's main thread and the live-test runner (plan §4.3).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop};

/// Runs the current thread's run loop once, for at most `d`.
pub fn pump(d: Duration) {
    // SAFETY: running the current thread's run loop in the default mode; NSDefaultRunLoopMode is
    // a framework constant.
    unsafe {
        let date = NSDate::dateWithTimeIntervalSinceNow(d.as_secs_f64());
        NSRunLoop::currentRunLoop().runMode_beforeDate(NSDefaultRunLoopMode, &date);
    }
}

/// Pumps until `rx` yields or `timeout` elapses.
pub fn pump_recv<T>(rx: &mpsc::Receiver<T>, timeout: Duration) -> Option<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(v) = rx.try_recv() {
            return Some(v);
        }
        if Instant::now() >= deadline {
            return None;
        }
        pump(Duration::from_millis(5));
    }
}

/// Pumps until `done` is set.
pub fn run_until(done: &AtomicBool) {
    while !done.load(Ordering::SeqCst) {
        pump(Duration::from_millis(10));
    }
}
