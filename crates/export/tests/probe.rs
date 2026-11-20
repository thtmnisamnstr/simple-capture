//! Probe v1 against the synthetic platform's writers (HEVC benchmark) and, on macOS with the
//! helpers built, the real VideoToolbox H.264 size limit.

mod common;

use common::*;
use sc_core::project::Resolution;
use sc_export::probe::{IN_PROGRESS_FILE, load_or_run, run};
use sc_geometry::{RectPt, SizePx};
use sc_platform::synthetic::{Scenario, SyntheticPlatform};
use sc_platform::{DisplayId, DisplayInfo};
use std::sync::{Mutex, MutexGuard, PoisonError};

static HARDWARE: Mutex<()> = Mutex::new(());

/// VideoToolbox has few hardware sessions; concurrent probes fail opens that pass alone.
fn one_at_a_time() -> MutexGuard<'static, ()> {
    HARDWARE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn displays() -> Vec<DisplayInfo> {
    vec![
        DisplayInfo {
            id: DisplayId("synthetic:retina".into()),
            name: "Retina".into(),
            frame_pt: RectPt {
                x: 0.0,
                y: 0.0,
                w: 960.0,
                h: 540.0,
            },
            scale: 2.0,
            size_px: SizePx { w: 1920, h: 1080 },
            is_main: true,
        },
        DisplayInfo {
            id: DisplayId("synthetic:plain".into()),
            name: "Plain".into(),
            frame_pt: RectPt {
                x: 960.0,
                y: 0.0,
                w: 1280.0,
                h: 720.0,
            },
            scale: 1.0,
            size_px: SizePx { w: 1280, h: 720 },
            is_main: false,
        },
    ]
}

#[test]
fn run_measures_every_display() {
    let _hw = one_at_a_time();
    let p = SyntheticPlatform::new(Scenario::new());
    let probe = run(&*p, &displays()).expect("probe");
    assert_eq!(probe.schema, 1);
    assert!(!probe.os_version.is_empty());
    assert_eq!(probe.displays.len(), 2);
    let retina = &probe.displays[0];
    assert_eq!(retina.native_px, SizePx { w: 1920, h: 1080 });
    assert_eq!(retina.one_x_px, SizePx { w: 960, h: 540 });
    assert!(retina.hevc_fps_native.is_some_and(|f| f > 0.0));
    assert!(retina.hevc_fps_1x.is_some_and(|f| f > 0.0));
    let plain = &probe.displays[1];
    assert_eq!(plain.hevc_fps_native, plain.hevc_fps_1x);
    for d in displays() {
        for res in [Resolution::Native, Resolution::OneX] {
            assert!(probe.allows_60fps(&d.id, res).is_some());
        }
    }
    assert_eq!(
        probe.allows_60fps(&DisplayId("nope".into()), Resolution::Native),
        None
    );
    let json = serde_json::to_string(&probe).expect("json");
    let back: sc_export::probe::ProbeV1 = serde_json::from_str(&json).expect("parse");
    assert_eq!(back, probe);
    if cfg!(target_os = "macos") && helpers_available() {
        let h = probe.helper.as_ref().expect("helper info");
        assert_eq!(h.sha256.len(), 64);
        assert!(h.network_disabled);
        assert_eq!(
            probe.vt_h264_opens,
            vec![
                (SizePx { w: 3840, h: 2160 }, true),
                (SizePx { w: 4096, h: 2160 }, true),
                (SizePx { w: 4098, h: 2160 }, false),
            ]
        );
    }
}

#[test]
fn allows_60fps_needs_headroom() {
    let _hw = one_at_a_time();
    let p = SyntheticPlatform::new(Scenario::new());
    let mut probe = run(&*p, &displays()[1..]).expect("probe");
    let id = DisplayId("synthetic:plain".into());
    probe.displays[0].hevc_fps_native = Some(78.0);
    probe.displays[0].hevc_fps_1x = Some(77.9);
    assert_eq!(probe.allows_60fps(&id, Resolution::Native), Some(true));
    assert_eq!(probe.allows_60fps(&id, Resolution::OneX), Some(false));
    probe.displays[0].hevc_fps_1x = None;
    assert_eq!(probe.allows_60fps(&id, Resolution::OneX), None);
}

#[test]
fn cache_and_quarantine() {
    let _hw = one_at_a_time();
    let t = TempDir::new("probe-cache");
    let p = SyntheticPlatform::new(Scenario::new());
    let first = load_or_run(&*p, t.path(), &displays()).expect("probe");
    assert!(t.path().join("probe-v1.json").exists());
    assert!(!t.path().join(IN_PROGRESS_FILE).exists());
    let again = load_or_run(&*p, t.path(), &displays()).expect("cached");
    assert_eq!(again, first, "same helper, OS and displays: cached");

    let fewer = load_or_run(&*p, t.path(), &displays()[..1]).expect("re-probe");
    assert_eq!(fewer.displays.len(), 1);

    let key = "hevc:synthetic:retina:1920x1080";
    std::fs::write(t.path().join(IN_PROGRESS_FILE), key).expect("marker");
    let q = load_or_run(&*p, t.path(), &displays()).expect("after crash");
    assert_eq!(q.quarantined, vec![key.to_string()]);
    assert_eq!(q.displays[0].hevc_fps_native, None);
    assert!(q.displays[0].hevc_fps_1x.is_some());
    assert!(!t.path().join(IN_PROGRESS_FILE).exists());
    let kept = load_or_run(&*p, t.path(), &displays()).expect("cached");
    assert_eq!(kept.quarantined, vec![key.to_string()]);
}
