//! Plain `cargo test` must never reach a TCC-protected API (plan §2 rule 2).
#![cfg(target_os = "macos")]

use std::sync::Arc;

use sc_capture_macos::sck::{MacPicker, MacScreenSource, MacSystemAudio};
use sc_core::project::{Resolution, SystemAudioMode};
use sc_geometry::SizePx;
use sc_platform::{
    AudioBuffer, AudioTrackKind, FrameSink, ScreenSource, ScreenStreamConfig, SourceEvent,
    SourceSpec, StreamTag, VideoFrame, WindowId,
};

struct Null;

impl FrameSink for Null {
    fn on_screen(&self, _f: VideoFrame) {}
    fn on_camera(&self, _f: VideoFrame) {}
    fn on_audio(&self, _t: AudioTrackKind, _a: AudioBuffer) {}
    fn on_event(&self, _e: SourceEvent) {}
}

fn under_host() -> bool {
    ["SC_LIVE", "SC_DEV_HOST"]
        .iter()
        .any(|k| std::env::var(k).ok().as_deref() == Some("1"))
}

fn cfg() -> ScreenStreamConfig {
    ScreenStreamConfig {
        fps: 30,
        resolution: Resolution::Native,
        show_cursor: true,
        show_clicks: false,
        queue_depth: 6,
        exclude: Vec::new(),
    }
}

fn assert_tripwire<T>(what: &str, r: sc_platform::Result<T>) {
    match r {
        Ok(_) => panic!("{what}: reached a TCC API without the dev host"),
        Err(e) => assert!(e.to_string().contains("TCC tripwire"), "{what}: {e}"),
    }
}

#[test]
fn tcc_paths_refuse_without_dev_host() {
    if under_host() {
        eprintln!("SKIPPED: running under the dev host");
        return;
    }
    let src = MacScreenSource::new();
    let main = src
        .displays()
        .expect("displays() needs no TCC")
        .into_iter()
        .find(|d| d.is_main)
        .expect("a main display");
    let spec = SourceSpec::Display {
        display: main.id.clone(),
    };
    assert_tripwire("list", src.list(&[]));
    assert_tripwire(
        "start",
        src.start(StreamTag(1), &spec, &cfg(), Arc::new(Null)),
    );
    assert_tripwire("snapshot", src.snapshot(&spec, SizePx { w: 64, h: 64 }));
    assert_tripwire(
        "describe window",
        src.describe(
            &SourceSpec::Window {
                window: WindowId(1),
            },
            &cfg(),
        ),
    );
    assert_tripwire(
        "system audio",
        MacSystemAudio::new().start_mac(SystemAudioMode::All, &[], Arc::new(Null)),
    );
    assert_tripwire("picker", MacPicker::new().configuration_readback(&[]));
    assert_tripwire(
        "currentProcess",
        sc_capture_macos::sck::current_process_report(),
    );
}

#[test]
fn display_describe_needs_no_tcc() {
    let src = MacScreenSource::new();
    let ds = src.displays().expect("displays");
    assert!(!ds.is_empty());
    for d in &ds {
        assert!(d.size_px.w > 0 && d.scale >= 1.0, "{d:?}");
        let (desc, size) = src
            .describe(
                &SourceSpec::Display {
                    display: d.id.clone(),
                },
                &cfg(),
            )
            .expect("describe display");
        assert_eq!(size.w % 2, 0);
        assert_eq!(size.h % 2, 0);
        assert!(matches!(
            desc,
            sc_core::project::SourceDescriptor::Display { .. }
        ));
    }
    let main = ds.iter().find(|d| d.is_main).expect("main");
    let (_, size) = src
        .describe(
            &SourceSpec::Region {
                display: main.id.clone(),
                region_pt: sc_geometry::RectPt {
                    x: 10.0,
                    y: 10.0,
                    w: 617.0,
                    h: 283.5,
                },
            },
            &cfg(),
        )
        .expect("describe region");
    assert_eq!(size.w % 2, 0);
    assert_eq!(size.h % 2, 0);
}
