mod common;

use common::TempDir;
use sc_core::Error;
use sc_core::project::{Background, Fit, ProjectState, ProjectV1, copy_background_asset};

const EXAMPLE: &str = include_str!("data/architecture_example.json");

fn example() -> ProjectV1 {
    serde_json::from_str(EXAMPLE).expect("example parses")
}

#[test]
fn save_then_load_round_trips_semantically_and_keeps_unknown_fields() {
    let dir = TempDir::new("proj");
    let mut v: serde_json::Value = serde_json::from_str(EXAMPLE).expect("example parses");
    v["future_field"] = serde_json::json!({"nested": [1, 2, 3]});
    v["output"]["canvas"]["future_canvas"] = serde_json::json!("x");
    std::fs::write(
        dir.path().join("project.json"),
        serde_json::to_vec(&v).expect("ser"),
    )
    .expect("write");

    let p = ProjectV1::load(dir.path()).expect("load");
    p.save(dir.path()).expect("save");
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("project.json")).expect("read"))
            .expect("saved json parses");
    assert_eq!(
        common::numbers_as_f64(saved.clone()),
        common::numbers_as_f64(v.clone()),
        "saved file is semantically the input"
    );
    assert_eq!(saved["future_field"]["nested"][2], 3);
    assert_eq!(saved["output"]["canvas"]["future_canvas"], "x");
    assert_eq!(ProjectV1::load(dir.path()).expect("reload"), p);
}

#[test]
fn save_leaves_no_temp_files_and_overwrites() {
    let dir = TempDir::new("proj2");
    let mut p = example();
    p.save(dir.path()).expect("save 1");
    p.title = "second".into();
    p.state = ProjectState::Recovered;
    p.save(dir.path()).expect("save 2");
    let names: Vec<String> = std::fs::read_dir(dir.path())
        .expect("ls")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["project.json".to_string()]);
    let back = ProjectV1::load(dir.path()).expect("load");
    assert_eq!(back.title, "second");
    assert_eq!(back.state, ProjectState::Recovered);
}

#[test]
fn load_rejects_other_schema_versions_and_reports_path() {
    let dir = TempDir::new("proj3");
    let mut v: serde_json::Value = serde_json::from_str(EXAMPLE).expect("parse");
    v["schema_version"] = serde_json::json!(2);
    std::fs::write(dir.path().join("project.json"), v.to_string()).expect("write");
    match ProjectV1::load(dir.path()) {
        Err(Error::Invalid(m)) => assert!(m.contains("schema_version"), "{m}"),
        other => panic!("expected Invalid, got {other:?}"),
    }
    std::fs::write(dir.path().join("project.json"), "{not json").expect("write");
    match ProjectV1::load(dir.path()) {
        Err(Error::Json(m)) => assert!(m.contains("project.json"), "{m}"),
        other => panic!("expected Json, got {other:?}"),
    }
    let missing = TempDir::new("proj4");
    match ProjectV1::load(missing.path()) {
        Err(Error::Io(e)) => assert!(e.to_string().contains("project.json"), "{e}"),
        other => panic!("expected Io, got {other:?}"),
    }
}

#[test]
fn duration_and_segment_lookup() {
    let mut p = example();
    assert_eq!(p.duration_ns(), Some(60_100_000_000));
    assert_eq!(p.segment_at(-1).map(|s| s.id.as_str()), None);
    assert_eq!(p.segment_at(0).map(|s| s.id.as_str()), Some("s0"));
    assert_eq!(
        p.segment_at(21_479_999_999).map(|s| s.id.as_str()),
        Some("s0")
    );
    assert_eq!(
        p.segment_at(21_480_000_000).map(|s| s.id.as_str()),
        Some("s1")
    );
    assert_eq!(
        p.segment_at(60_099_999_999).map(|s| s.id.as_str()),
        Some("s2")
    );
    assert_eq!(p.segment_at(60_100_000_000).map(|s| s.id.as_str()), None);

    // A pause stays in recording time (the tracks hold it); the EDL cuts it out (ADR-048).
    let start = p.recording.start_host_ns;
    p.recording.pauses.push(sc_core::project::PauseRecord {
        start_host_ns: start + 10_000_000_000,
        end_host_ns: start + 15_000_000_000,
        extra: Default::default(),
    });
    assert_eq!(p.duration_ns(), Some(60_100_000_000));

    // Still recording: the last closed segment end; an open segment runs on.
    p.recording.stop_host_ns = None;
    p.recording.screen.segments[2].end_rec_ns = None;
    assert_eq!(p.duration_ns(), Some(47_020_000_000));
    assert_eq!(
        p.segment_at(999_000_000_000).map(|s| s.id.as_str()),
        Some("s2")
    );

    // A gap between segments has no segment.
    p.recording.screen.segments[1].end_rec_ns = Some(40_000_000_000);
    assert_eq!(p.segment_at(45_000_000_000).map(|s| s.id.as_str()), None);
}

#[test]
fn new_recording_has_no_duration_until_stopped() {
    let p = ProjectV1::new_recording(
        sc_core::ids::new_id(),
        "t".into(),
        "2026-11-19T10:00:00+00:00".into(),
        "0.1.0".into(),
        sc_core::ClockKind::MachHost,
    );
    assert_eq!(p.duration_ns(), None);
    assert!(p.segment_at(0).is_none());
    let dir = TempDir::new("proj5");
    p.save(dir.path()).expect("save");
    assert_eq!(ProjectV1::load(dir.path()).expect("load"), p);
}

const PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0,
    0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99, 0x3D, 0x1D, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

#[test]
fn background_assets_are_copied_by_hash() {
    let src = TempDir::new("bgsrc");
    let proj = TempDir::new("bgproj");
    let png = src.path().join("my background.PNG");
    std::fs::write(&png, PNG_1X1).expect("write png");
    let sha = sc_core::sha::sha256_bytes(PNG_1X1);

    let bg = copy_background_asset(&png, proj.path()).expect("copy png");
    let Background::Image {
        asset,
        sha256,
        fit,
        focus_norm,
    } = &bg
    else {
        panic!("expected image background, got {bg:?}");
    };
    assert_eq!(asset, &format!("assets/background-{}.png", &sha[..12]));
    assert_eq!(sha256, &sha);
    assert_eq!(*fit, Fit::Cover);
    assert_eq!((focus_norm.x, focus_norm.y), (0.5, 0.5));
    assert_eq!(
        std::fs::read(proj.path().join(asset)).expect("asset"),
        PNG_1X1
    );
    // Same image again: same asset, still one file.
    assert_eq!(copy_background_asset(&png, proj.path()).expect("again"), bg);
    assert_eq!(
        std::fs::read_dir(proj.path().join("assets"))
            .expect("ls")
            .count(),
        1
    );
    // The JSON shape matches ARCHITECTURE §6.
    let j = serde_json::to_value(&bg).expect("ser");
    assert_eq!(j["kind"], "image");
    assert_eq!(j["fit"], "cover");

    let jpg = src.path().join("photo.jpeg");
    std::fs::write(&jpg, [0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F']).expect("write jpg");
    match copy_background_asset(&jpg, proj.path()).expect("copy jpg") {
        Background::Image { asset, .. } => assert!(asset.ends_with(".jpg"), "{asset}"),
        other => panic!("{other:?}"),
    }

    let webp = src.path().join("x.png");
    std::fs::write(&webp, b"RIFF\0\0\0\0WEBPVP8 ").expect("write webp");
    assert!(matches!(
        copy_background_asset(&webp, proj.path()),
        Err(Error::Invalid(_))
    ));
}
