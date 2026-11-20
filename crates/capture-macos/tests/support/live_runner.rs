//! Runner for the `harness = false` live targets `sck_live` and `av_live` (plan §4.4).
//!
//! - Without `SC_LIVE=1` it prints `SKIPPED:` and a zero-test summary and passes, so plain
//!   `cargo test` never touches TCC.
//! - With `SC_LIVE=1` (set by `scripts/test-live.sh`, which runs the binary under the dev host)
//!   it runs `NSApplication` on the main thread with the Accessory activation policy (never
//!   activates, never takes focus) and each test body on one worker thread, in order.
//! - Accepts name filters (substring, or exact with `--exact`) and `--list`; ignores other
//!   libtest flags. Ends with a libtest-style `test result:` line that test-live.sh parses.
#![allow(dead_code)]

use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Instant;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

pub type TestFn = fn() -> Result<(), String>;

#[derive(Clone, Copy)]
pub struct LiveTest {
    pub name: &'static str,
    pub run: TestFn,
}

struct Outcome {
    name: &'static str,
    result: Result<(), String>,
    secs: f64,
}

fn panic_text(p: Box<dyn Any + Send>) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic".to_string()
    }
}

/// Entry point for a live target's `main`.
pub fn main(tests: &[LiveTest]) -> ! {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let list = args.iter().any(|a| a == "--list");
    let exact = args.iter().any(|a| a == "--exact");
    let filters: Vec<&str> = args
        .iter()
        .filter(|a| !a.starts_with("--"))
        .map(String::as_str)
        .collect();
    let selected: Vec<LiveTest> = tests
        .iter()
        .copied()
        .filter(|t| {
            filters.is_empty()
                || filters.iter().any(|f| {
                    if exact {
                        t.name == *f
                    } else {
                        t.name.contains(f)
                    }
                })
        })
        .collect();
    let filtered_out = tests.len() - selected.len();

    if list {
        for t in &selected {
            println!("{}: test", t.name);
        }
        println!();
        println!("{} tests, 0 benchmarks", selected.len());
        std::process::exit(0);
    }

    if std::env::var("SC_LIVE").ok().as_deref() != Some("1") {
        println!(
            "SKIPPED: {} live tests need SC_LIVE=1 (run them with `just test-live`)",
            selected.len()
        );
        println!();
        println!(
            "test result: ok. 0 passed; 0 failed; {} ignored; 0 measured; {filtered_out} filtered out; finished in 0.00s",
            selected.len()
        );
        std::process::exit(0);
    }

    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("live runner: not on the main thread");
        std::process::exit(101);
    };
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    println!();
    println!("running {} tests", selected.len());
    let started = Instant::now();
    let done = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<Vec<Outcome>>();
    let worker_done = done.clone();
    let worker = std::thread::spawn(move || {
        let mut outcomes = Vec::new();
        for t in selected {
            let t0 = Instant::now();
            let result =
                panic::catch_unwind(AssertUnwindSafe(t.run)).unwrap_or_else(|p| Err(panic_text(p)));
            let ok = result.is_ok();
            println!("test {} ... {}", t.name, if ok { "ok" } else { "FAILED" });
            outcomes.push(Outcome {
                name: t.name,
                result,
                secs: t0.elapsed().as_secs_f64(),
            });
        }
        let _ = tx.send(outcomes);
        worker_done.store(true, Ordering::SeqCst);
    });
    sc_capture_macos::mainloop::run_until(&done);
    let _ = worker.join();
    let outcomes = rx.recv().unwrap_or_default();

    let failed: Vec<&Outcome> = outcomes.iter().filter(|o| o.result.is_err()).collect();
    if !failed.is_empty() {
        println!();
        println!("failures:");
        for o in &failed {
            if let Err(e) = &o.result {
                println!("---- {} ({:.2}s) ----\n{e}", o.name, o.secs);
            }
        }
    }
    let passed = outcomes.len() - failed.len();
    println!();
    println!(
        "test result: {}. {passed} passed; {} failed; 0 ignored; 0 measured; {filtered_out} filtered out; finished in {:.2}s",
        if failed.is_empty() { "ok" } else { "FAILED" },
        failed.len(),
        started.elapsed().as_secs_f64()
    );
    std::process::exit(if failed.is_empty() { 0 } else { 101 });
}

/// Runs `f` on the main thread (the runner's main thread pumps the run loop) and returns its
/// value. The only way live bodies may create in-process windows.
pub fn on_main<R: Send>(f: impl FnOnce(MainThreadMarker) -> R + Send) -> R {
    if let Some(mtm) = MainThreadMarker::new() {
        return f(mtm);
    }
    let mut out = None;
    dispatch2::DispatchQueue::main().exec_sync(|| {
        let mtm = MainThreadMarker::new().expect("main queue runs on the main thread");
        out = Some(f(mtm));
    });
    out.expect("main-queue block ran")
}
