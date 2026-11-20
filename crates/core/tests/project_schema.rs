use sc_core::journal::JournalEvent;
use sc_core::project::{Background, ProjectState, ProjectV1, SourceDescriptor};

const EXAMPLE: &str = include_str!("data/architecture_example.json");
const JOURNAL: &str = include_str!("data/journal_every_event.jsonl");

#[test]
fn architecture_example_deserializes_and_round_trips() {
    let p: ProjectV1 = serde_json::from_str(EXAMPLE).expect("example parses");
    assert_eq!(p.state, ProjectState::Finalized);
    assert_eq!(p.recording.screen.segments.len(), 3);
    assert_eq!(p.recording.tracks.len(), 6);
    assert!(matches!(
        p.recording.screen.segments[2].source,
        SourceDescriptor::Region { .. }
    ));
    assert!(matches!(
        p.output.canvas.background,
        Background::Linear { .. }
    ));
    let again: ProjectV1 =
        serde_json::from_str(&serde_json::to_string(&p).expect("serializes")).expect("reparses");
    assert_eq!(p, again);
}

#[test]
fn unknown_fields_survive_a_rewrite() {
    let mut v: serde_json::Value = serde_json::from_str(EXAMPLE).expect("example parses");
    v["future_field"] = serde_json::json!({"a": 1});
    v["recording"]["screen"]["segments"][0]["future_seg"] = serde_json::json!(true);
    let p: ProjectV1 = serde_json::from_value(v).expect("parses with unknown fields");
    let out = serde_json::to_value(&p).expect("serializes");
    assert_eq!(out["future_field"]["a"], 1);
    assert_eq!(
        out["recording"]["screen"]["segments"][0]["future_seg"],
        true
    );
}

#[test]
fn every_journal_event_deserializes() {
    let events: Vec<JournalEvent> = JOURNAL
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{l}: {e}")))
        .collect();
    assert_eq!(events.len(), 15);
    for ev in &events {
        let line = serde_json::to_string(ev).expect("serializes");
        let back: JournalEvent = serde_json::from_str(&line).expect("reparses");
        assert_eq!(&back, ev);
    }
}

#[test]
fn new_recording_serializes() {
    let p = ProjectV1::new_recording(
        "id".into(),
        "t".into(),
        "2026-11-19T10:00:00+00:00".into(),
        "0.1.0".into(),
        sc_core::ClockKind::MachHost,
    );
    let back: ProjectV1 =
        serde_json::from_str(&serde_json::to_string(&p).expect("serializes")).expect("reparses");
    assert_eq!(p, back);
}
