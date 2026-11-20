mod common;

use common::TempDir;
use sc_core::journal::{JournalEvent, JournalWriter, StopReason, parse_journal, read_journal};

const JOURNAL: &str = include_str!("data/journal_every_event.jsonl");

const EV_NAMES: [&str; 15] = [
    "start",
    "segment_prepare",
    "segment_open",
    "switch_begin",
    "segment_close",
    "switch_failed",
    "source_lost",
    "source_resized",
    "audio_gap",
    "pause",
    "resume",
    "device",
    "track_finished",
    "health",
    "stop",
];

fn all_events() -> Vec<JournalEvent> {
    JOURNAL
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{l}: {e}")))
        .collect()
}

#[test]
fn every_variant_round_trips_with_exact_ev_names() {
    for (line, name) in JOURNAL.lines().zip(EV_NAMES) {
        let ev: JournalEvent = serde_json::from_str(line).expect("parses");
        let out = serde_json::to_value(&ev).expect("ser");
        assert_eq!(out["ev"], name);
        let input: serde_json::Value = serde_json::from_str(line).expect("json");
        assert_eq!(
            common::numbers_as_f64(out),
            common::numbers_as_f64(input),
            "{name} serializes back to the same JSON"
        );
    }
    assert_eq!(JOURNAL.lines().count(), EV_NAMES.len());
}

#[test]
fn writer_appends_and_reader_reads_back() {
    let dir = TempDir::new("jrnl");
    let events = all_events();
    {
        let mut w = JournalWriter::create(dir.path()).expect("create");
        for ev in &events[..7] {
            w.append(ev).expect("append");
        }
    }
    // Reopening appends, never truncates.
    let mut w = JournalWriter::create(dir.path()).expect("reopen");
    for ev in &events[7..] {
        w.append(ev).expect("append");
    }
    let r = read_journal(dir.path()).expect("read");
    assert_eq!(r.events, events);
    assert!(!r.torn_tail);
    assert_eq!(r.unknown_lines, 0);
}

#[test]
fn missing_journal_reads_empty() {
    let dir = TempDir::new("jrnl-none");
    let r = read_journal(dir.path()).expect("read");
    assert!(r.events.is_empty() && !r.torn_tail && r.unknown_lines == 0);
}

#[test]
fn torn_tail_is_tolerated_and_flagged() {
    let events = all_events();
    let mut text = String::new();
    for l in JOURNAL.lines().take(3) {
        text.push_str(l);
        text.push('\n');
    }
    let last = JOURNAL.lines().nth(3).expect("line 4");
    for cut in [1, 10, last.len() - 1] {
        let r = parse_journal(format!("{text}{}", &last[..cut]).as_bytes());
        assert_eq!(r.events, events[..3], "cut at {cut}");
        assert!(r.torn_tail, "cut at {cut}");
        assert_eq!(r.unknown_lines, 0);
    }
    // A complete last event without its newline is kept, not torn.
    let r = parse_journal(format!("{text}{last}").as_bytes());
    assert_eq!(r.events, events[..4]);
    assert!(!r.torn_tail);
    // NUL fill after a crash counts as torn.
    let mut bytes = text.clone().into_bytes();
    bytes.extend_from_slice(&[0; 64]);
    let r = parse_journal(&bytes);
    assert_eq!(r.events, events[..3]);
    assert!(r.torn_tail);
}

#[test]
fn unknown_events_and_fields_are_counted_or_ignored() {
    let text = concat!(
        r#"{"ev":"pause","host_ns":5,"future":"field"}"#,
        "\n",
        r#"{"ev":"zoom_keyframe","t":1}"#,
        "\n",
        "garbage\n",
        "\n",
        r#"{"ev":"stop","stop_host_ns":9,"reason":"signal"}"#,
        "\r\n",
    );
    let r = parse_journal(text.as_bytes());
    assert_eq!(
        r.events,
        vec![
            JournalEvent::Pause { host_ns: 5 },
            JournalEvent::Stop {
                stop_host_ns: 9,
                reason: StopReason::Signal
            }
        ]
    );
    assert_eq!(r.unknown_lines, 2);
    assert!(!r.torn_tail);
}

#[test]
fn reopening_after_a_torn_line_starts_a_new_line() {
    let dir = TempDir::new("jrnl-torn");
    std::fs::write(
        dir.path().join("journal.jsonl"),
        "{\"ev\":\"pause\",\"host_ns\":1}\n{\"ev\":\"res",
    )
    .expect("write");
    let mut w = JournalWriter::create(dir.path()).expect("reopen");
    w.append(&JournalEvent::Resume { host_ns: 2 })
        .expect("append");
    let r = read_journal(dir.path()).expect("read");
    assert_eq!(
        r.events,
        vec![
            JournalEvent::Pause { host_ns: 1 },
            JournalEvent::Resume { host_ns: 2 }
        ]
    );
    assert_eq!(r.unknown_lines, 1, "the torn line is now a middle line");
    assert!(!r.torn_tail);
}
