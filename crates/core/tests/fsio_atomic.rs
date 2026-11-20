mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use common::TempDir;
use sc_core::fsio::{atomic_write, fsync_dir};

const SIZE: usize = 256 * 1024;

fn content(b: u8) -> Vec<u8> {
    vec![b; SIZE]
}

fn assert_old_or_new(bytes: &[u8]) {
    assert_eq!(bytes.len(), SIZE, "file is whole");
    let first = bytes[0];
    assert!(first == b'A' || first == b'B', "unexpected byte {first}");
    assert!(bytes.iter().all(|&b| b == first), "file mixes old and new");
}

#[test]
fn atomic_write_replaces_and_leaves_no_temp() {
    let dir = TempDir::new("fsio");
    let p = dir.path().join("f.json");
    atomic_write(&p, b"old").expect("write old");
    atomic_write(&p, b"new").expect("write new");
    assert_eq!(std::fs::read(&p).expect("read"), b"new");
    assert_eq!(std::fs::read_dir(dir.path()).expect("ls").count(), 1);
    fsync_dir(dir.path()).expect("fsync dir");
    assert!(atomic_write(&dir.path().join("missing/f.json"), b"x").is_err());
    assert_eq!(std::fs::read_dir(dir.path()).expect("ls").count(), 1);
}

#[test]
fn concurrent_reader_sees_old_or_new() {
    let dir = TempDir::new("fsio-race");
    let p = dir.path().join("f.bin");
    atomic_write(&p, &content(b'A')).expect("seed");
    let stop = Arc::new(AtomicBool::new(false));
    let reader = {
        let (p, stop) = (p.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut reads = 0u32;
            while !stop.load(Ordering::Relaxed) {
                assert_old_or_new(&std::fs::read(&p).expect("read"));
                reads += 1;
            }
            reads
        })
    };
    for i in 0..100u32 {
        atomic_write(&p, &content(if i.is_multiple_of(2) { b'B' } else { b'A' })).expect("write");
    }
    stop.store(true, Ordering::Relaxed);
    let reads = reader.join().expect("reader");
    assert!(reads > 0);
}

/// Helper body: rewrites the file in a loop until killed.
#[test]
fn helper_atomic_writer_loop() {
    let Ok(path) = std::env::var("SC_CORE_ATOMIC_HELPER") else {
        return;
    };
    let p = std::path::PathBuf::from(path);
    println!("\nWRITING");
    let mut i = 0u64;
    loop {
        atomic_write(&p, &content(if i.is_multiple_of(2) { b'B' } else { b'A' })).expect("write");
        i += 1;
    }
}

#[cfg(unix)]
#[test]
fn sigkill_mid_write_leaves_old_or_new() {
    let dir = TempDir::new("fsio-kill");
    let p = dir.path().join("f.bin");
    for round in 0..5u64 {
        atomic_write(&p, &content(b'A')).expect("seed");
        let mut child = common::spawn_helper(
            "helper_atomic_writer_loop",
            &[("SC_CORE_ATOMIC_HELPER", p.to_str().expect("utf8 path"))],
        );
        common::wait_for_line(&mut child, "WRITING");
        std::thread::sleep(std::time::Duration::from_millis(20 + round * 13));
        child.kill().expect("SIGKILL helper");
        child.wait().expect("reap helper");
        assert_old_or_new(&std::fs::read(&p).expect("read after kill"));
    }
}
