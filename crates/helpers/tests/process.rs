//! HelperProcess against a fake helper: this test binary re-executes itself with the
//! `fake_helper_entry` filter and a `mode=` argument.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use sc_helpers::{HelperProcess, SpawnSpec};

const FRAME: usize = 256 * 1024;

#[test]
fn fake_helper_entry() {
    let Some(mode) = std::env::args().find_map(|a| a.strip_prefix("mode=").map(str::to_owned))
    else {
        return;
    };
    fake_main(&mode);
}

fn ready(err: &mut impl Write) {
    let _ = err.write_all(b"progress=continue\n");
    let _ = err.flush();
}

fn fake_main(mode: &str) -> ! {
    let mut err = std::io::stderr();
    let mut stdin = std::io::stdin().lock();
    match mode {
        "eof" => {
            let (mut n, mut buf) = (0u64, vec![0u8; 65536]);
            while let Ok(k) = stdin.read(&mut buf) {
                if k == 0 {
                    break;
                }
                n += k as u64;
            }
            let _ = writeln!(err, "bytes={n}");
            std::process::exit(0)
        }
        "slow" => {
            let mut buf = vec![0u8; FRAME];
            let mut frame = 0u64;
            while stdin.read_exact(&mut buf).is_ok() {
                frame += 1;
                let _ = write!(
                    err,
                    "frame={frame}\nout_time_us={}\nspeed=1.5x\nprogress=continue\n",
                    frame * 33_333
                );
                std::thread::sleep(Duration::from_millis(40));
            }
            let _ = writeln!(err, "frame={frame}\nprogress=end");
            std::process::exit(0)
        }
        "hang" | "term" => {
            #[cfg(unix)]
            if mode == "hang" {
                // SAFETY: setting a signal disposition to SIG_IGN has no preconditions.
                unsafe {
                    libc::signal(libc::SIGTERM, libc::SIG_IGN);
                }
            }
            ready(&mut err);
            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        "fail" => {
            let _ = writeln!(err, "fake: fatal error");
            std::process::exit(1)
        }
        other => {
            let _ = writeln!(err, "unknown fake mode {other}");
            std::process::exit(2)
        }
    }
}

fn spec(mode: &str, queue_frames: usize) -> SpawnSpec {
    SpawnSpec {
        program: std::env::current_exe().expect("current_exe"),
        args: vec![
            "--exact".into(),
            "--nocapture".into(),
            "--test-threads=1".into(),
            "fake_helper_entry".into(),
            format!("mode={mode}"),
        ],
        frame_bytes: FRAME,
        queue_frames,
    }
}

fn wait_ready(h: &HelperProcess) {
    let t = Instant::now();
    while h.progress().is_none() {
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "fake helper never got ready"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(unix)]
fn assert_gone(pid: u32) {
    let pid = pid as libc::pid_t;
    // SAFETY: signal 0 only checks for existence.
    let (r, g) = unsafe { (libc::kill(pid, 0), libc::kill(-pid, 0)) };
    assert_eq!(r, -1, "helper {pid} still exists");
    assert_eq!(g, -1, "process group {pid} still exists");
}

#[cfg(not(unix))]
fn assert_gone(_pid: u32) {}

#[test]
fn eof_makes_the_helper_exit_and_counts_frames() {
    let h = HelperProcess::spawn(spec("eof", 0)).expect("spawn");
    let pid = h.pid();
    h.write_bytes(b"HEADER0123").expect("header");
    for _ in 0..5 {
        h.write_frame(&[7u8; 1000]).expect("frame");
    }
    let t = Instant::now();
    let r = h.finish().expect("finish");
    assert!(r.success(), "{r:?}");
    assert_eq!(r.frames_written, 5);
    assert!(
        r.stderr_tail.iter().any(|l| l == "bytes=5010"),
        "{:?}",
        r.stderr_tail
    );
    assert!(t.elapsed() < Duration::from_secs(5));
    assert_gone(pid);
}

#[test]
fn slow_reader_applies_backpressure_and_reports_progress() {
    let n = 20u64;
    let h = HelperProcess::spawn(spec("slow", 3)).expect("spawn");
    let pid = h.pid();
    let frame = vec![1u8; FRAME];
    let t0 = Instant::now();
    let mut first_three = Duration::ZERO;
    let mut saw_progress = None;
    for i in 0..n {
        h.write_frame(&frame).expect("frame");
        if i == 2 {
            first_three = t0.elapsed();
        }
        if let Some(p) = h.progress().filter(|p| p.frame > 0) {
            saw_progress = Some(p);
        }
    }
    let producer = t0.elapsed();
    let r = h.finish().expect("finish");
    let total = t0.elapsed();
    assert!(r.success(), "{r:?}");
    assert_eq!(r.frames_written, n);
    assert!(r.stderr_tail.is_empty(), "{:?}", r.stderr_tail);
    assert!(
        first_three < Duration::from_millis(200),
        "first 3 writes took {first_three:?}"
    );
    // 40 ms per frame; at most queue (3) + in-flight (1) + pipe and reader (~2) frames of slack.
    assert!(
        producer >= Duration::from_millis(40 * (n - 6)),
        "producer was not held back: {producer:?}"
    );
    assert!(total >= Duration::from_millis(40 * n), "{total:?}");
    let p = saw_progress.expect("progress seen while writing");
    assert_eq!(p.speed, Some(1.5));
    assert_eq!(p.out_time_ns, p.frame as i64 * 33_333_000);
    assert!(!p.done);
    assert_gone(pid);
}

#[cfg(unix)]
#[test]
fn cancel_escalates_to_kill_after_grace_when_sigterm_is_ignored() {
    let h = HelperProcess::spawn(spec("hang", 3)).expect("spawn");
    let pid = h.pid();
    wait_ready(&h);
    let r = h.cancel();
    assert!(r.escalated_to_kill, "{r:?}");
    assert!(
        (1200..=1800).contains(&r.waited_ms),
        "waited {} ms, want 1500 ± 300",
        r.waited_ms
    );
    assert_gone(pid);
}

#[test]
fn cancel_with_sigterm_is_quick() {
    let h = HelperProcess::spawn(spec("term", 3)).expect("spawn");
    let pid = h.pid();
    wait_ready(&h);
    let r = h.cancel();
    assert!(!r.escalated_to_kill, "{r:?}");
    assert!(r.waited_ms < 500, "{r:?}");
    assert_gone(pid);
}

#[test]
fn interrupter_unblocks_a_producer_stuck_on_a_hung_helper() {
    let h = HelperProcess::spawn(spec("hang", 2)).expect("spawn");
    let pid = h.pid();
    wait_ready(&h);
    let stop = h.interrupter();
    let frame = vec![0u8; FRAME];
    let (written, err) = std::thread::scope(|s| {
        let producer = s.spawn(|| {
            let mut n = 0;
            loop {
                match h.write_frame(&frame) {
                    Ok(()) => n += 1,
                    Err(e) => return (n, e.to_string()),
                }
            }
        });
        std::thread::sleep(Duration::from_millis(300));
        stop.interrupt();
        producer.join().expect("producer")
    });
    assert!(
        written <= 4,
        "queue was not bounded: {written} frames accepted"
    );
    assert!(err.contains("interrupted"), "{err}");
    let r = h.cancel();
    assert!(r.escalated_to_kill || cfg!(not(unix)));
    assert_gone(pid);
}

#[test]
fn failing_helper_surfaces_exit_code_and_stderr() {
    let h = HelperProcess::spawn(spec("fail", 3)).expect("spawn");
    let pid = h.pid();
    let frame = vec![0u8; FRAME];
    let t = Instant::now();
    let err = loop {
        match h.write_frame(&frame) {
            Ok(()) => assert!(t.elapsed() < Duration::from_secs(10), "writes never failed"),
            Err(e) => break e.to_string(),
        }
    };
    assert!(err.contains("not accepting frames"), "{err}");
    let r = h.finish().expect("finish");
    assert_eq!(r.code, Some(1));
    assert!(!r.success());
    assert!(
        r.stderr_tail
            .iter()
            .any(|l| l.contains("fake: fatal error")),
        "{:?}",
        r.stderr_tail
    );
    assert_gone(pid);
}

#[test]
fn dropping_without_finish_kills_the_helper() {
    let h = HelperProcess::spawn(spec("hang", 3)).expect("spawn");
    let pid = h.pid();
    wait_ready(&h);
    drop(h);
    assert_gone(pid);
}

#[test]
fn spawn_of_a_missing_program_names_it() {
    let mut s = spec("eof", 3);
    s.program = PathBuf::from("/nonexistent/sc-helper-missing");
    let e = HelperProcess::spawn(s).expect_err("must fail");
    assert!(
        e.to_string().contains("/nonexistent/sc-helper-missing"),
        "{e}"
    );
}
