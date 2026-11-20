mod common;

use std::collections::HashSet;

use common::TempDir;
use sc_core::folder::LocalDateTime;
use sc_core::sessionlog::{Level, SessionLog, format_line};
use sc_core::time::{HostTime, PauseSpan, RecClock, RecTime, ToRec};

const S: i64 = 1_000_000_000;

#[test]
fn to_rec_table_with_two_pauses() {
    // Start at 100 s; pauses [110, 115) and [130, 132).
    let c = RecClock {
        start: HostTime(100 * S),
        pauses: vec![
            PauseSpan {
                start: HostTime(110 * S),
                end: Some(HostTime(115 * S)),
            },
            PauseSpan {
                start: HostTime(130 * S),
                end: Some(HostTime(132 * S)),
            },
        ],
    };
    let table: &[(i64, ToRec, i64)] = &[
        (0, ToRec::BeforeStart, 0),
        (100 * S - 1, ToRec::BeforeStart, 0),
        (100 * S, ToRec::At(RecTime(0)), 0),
        (105 * S, ToRec::At(RecTime(5 * S)), 0),
        (110 * S - 1, ToRec::At(RecTime(10 * S - 1)), 0),
        (110 * S, ToRec::Paused, 0),
        (112 * S, ToRec::Paused, 2 * S),
        (115 * S - 1, ToRec::Paused, 5 * S - 1),
        (115 * S, ToRec::At(RecTime(10 * S)), 5 * S),
        (120 * S, ToRec::At(RecTime(15 * S)), 5 * S),
        (131 * S, ToRec::Paused, 6 * S),
        (132 * S, ToRec::At(RecTime(25 * S)), 7 * S),
        (200 * S, ToRec::At(RecTime(93 * S)), 7 * S),
    ];
    for &(h, want, total) in table {
        assert_eq!(c.to_rec(HostTime(h)), want, "to_rec({h})");
        if h >= 100 * S {
            assert_eq!(
                c.pause_total_before(HostTime(h)),
                total,
                "pause_total_before({h})"
            );
        }
    }
}

#[test]
fn open_pause_holds_until_resumed() {
    let c = RecClock {
        start: HostTime(0),
        pauses: vec![PauseSpan {
            start: HostTime(10),
            end: None,
        }],
    };
    assert_eq!(c.to_rec(HostTime(9)), ToRec::At(RecTime(9)));
    assert_eq!(c.to_rec(HostTime(10)), ToRec::Paused);
    assert_eq!(c.to_rec(HostTime(1_000)), ToRec::Paused);
    assert_eq!(c.pause_total_before(HostTime(1_000)), 990);
}

#[test]
fn sha256_vectors() {
    assert_eq!(
        sc_core::sha::sha256_bytes(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sc_core::sha::sha256_bytes(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let million_a = vec![b'a'; 1_000_000];
    let want = "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0";
    assert_eq!(sc_core::sha::sha256_bytes(&million_a), want);
    let dir = TempDir::new("sha");
    let p = dir.path().join("a.bin");
    std::fs::write(&p, &million_a).expect("write");
    assert_eq!(sc_core::sha::sha256_file(&p).expect("sha file"), want);
    assert!(sc_core::sha::sha256_file(&dir.path().join("missing")).is_err());
}

#[test]
fn ids_are_uuid_v4_and_unique() {
    let mut seen = HashSet::new();
    for _ in 0..10_000 {
        let id = sc_core::ids::new_id();
        let b = id.as_bytes();
        assert_eq!(id.len(), 36, "{id}");
        for i in [8, 13, 18, 23] {
            assert_eq!(b[i], b'-', "{id}");
        }
        assert_eq!(b[14], b'4', "version nibble: {id}");
        assert!(matches!(b[19], b'8' | b'9' | b'a' | b'b'), "variant: {id}");
        assert!(
            id.chars()
                .all(|c| c == '-' || c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "{id}"
        );
        assert!(seen.insert(id));
    }
}

#[test]
fn session_log_lines() {
    let t = LocalDateTime {
        y: 2026,
        mo: 11,
        d: 19,
        h: 10,
        mi: 0,
        s: 1,
        offset_min: 0,
    };
    assert_eq!(
        format_line(&t, 42, Level::Warn, "two\nlines"),
        "2026-11-19T10:00:01+00:00 42 WARN two\\nlines\n"
    );

    let dir = TempDir::new("log");
    {
        let mut log = SessionLog::open(dir.path()).expect("open");
        log.line(Level::Info, "first").expect("line");
        log.line(Level::Error, "second").expect("line");
    }
    let mut log = SessionLog::open(dir.path()).expect("reopen appends");
    log.line(Level::Warn, "third").expect("line");
    let text = std::fs::read_to_string(dir.path().join("session.log")).expect("read");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3);
    let mut last_host = i64::MIN;
    for (line, (level, msg)) in
        lines
            .iter()
            .zip([("INFO", "first"), ("ERROR", "second"), ("WARN", "third")])
    {
        let parts: Vec<&str> = line.splitn(4, ' ').collect();
        assert_eq!(parts.len(), 4, "{line}");
        assert_eq!(parts[0].len(), 25, "rfc3339 with offset: {line}");
        assert_eq!(&parts[0][10..11], "T");
        let host: i64 = parts[1].parse().expect("host ns");
        assert!(host > 0 && host >= last_host, "{line}");
        last_host = host;
        assert_eq!(parts[2], level);
        assert_eq!(parts[3], msg);
    }
}
