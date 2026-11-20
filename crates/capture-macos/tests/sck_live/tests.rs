use std::sync::Arc;
use std::time::{Duration, Instant};

use sc_capture_macos::sck::{
    MacOverlays, MacPicker, MacScreenSource, MacSystemAudio, current_process_report,
};
use sc_core::project::SystemAudioMode;
use sc_geometry::{RectPt, SizePx};
use sc_platform::{
    AppRef, DisplayId, FrameSink, OverlayRole, OverlayWindows, PermissionKind, PermissionStatus,
    ScreenSource, ScreenStream, SourceEvent, SourceSpec, StreamTag, SystemAudioStream, WindowId,
};

use crate::common::*;
use crate::live_runner::{LiveTest, on_main};

pub const ALL: &[LiveTest] = &[
    LiveTest {
        name: "sck_status_granted_without_prompt",
        run: status_granted,
    },
    LiveTest {
        name: "sck_displays_and_list",
        run: displays_and_list,
    },
    LiveTest {
        name: "sck_current_process_report",
        run: current_process,
    },
    LiveTest {
        name: "sck_each_source_kind_delivers_frames",
        run: each_source_kind,
    },
    LiveTest {
        name: "sck_region_source_rect_sizes",
        run: region_sizes,
    },
    LiveTest {
        name: "sck_non_main_display",
        run: non_main_display,
    },
    LiveTest {
        name: "sck_application_second_window",
        run: app_second_window,
    },
    LiveTest {
        name: "sck_switch_make_before_break",
        run: switch_mbb,
    },
    LiveTest {
        name: "sck_switch_to_static_window",
        run: static_first_frame,
    },
    LiveTest {
        name: "sck_own_windows_excluded",
        run: own_windows_excluded,
    },
    LiveTest {
        name: "sck_snapshot_window",
        run: snapshot_window,
    },
    LiveTest {
        name: "sck_system_audio_app_follow",
        run: system_audio,
    },
    LiveTest {
        name: "sck_picker_configuration_readback",
        run: picker_readback,
    },
    LiveTest {
        name: "sck_closed_window_reports_loss",
        run: window_closed,
    },
];

const FIRST_FRAME: Duration = Duration::from_secs(1);

struct Main {
    id: DisplayId,
    scale: f64,
}

fn main_display() -> Result<Main, String> {
    let src = MacScreenSource::new();
    let d = src
        .displays()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|d| d.is_main)
        .ok_or("no main display")?;
    Ok(Main {
        id: d.id,
        scale: d.scale,
    })
}

fn px(r: [f64; 4], scale: f64) -> [f64; 4] {
    [r[0] * scale, r[1] * scale, r[2] * scale, r[3] * scale]
}

fn inset(r: [f64; 4], by: f64) -> [f64; 4] {
    [r[0] + by, r[1] + by, r[2] - 2.0 * by, r[3] - 2.0 * by]
}

fn rect_pt(r: [f64; 4]) -> RectPt {
    RectPt {
        x: r[0],
        y: r[1],
        w: r[2],
        h: r[3],
    }
}

fn sink(c: &Arc<Collector>) -> Arc<dyn FrameSink> {
    c.clone()
}

fn window_rect(app: &FixtureApp) -> Result<[f64; 4], String> {
    app.rect_pt
        .ok_or_else(|| format!("{} has no window", app.title))
}

fn window_id(app: &FixtureApp) -> Result<WindowId, String> {
    app.window_id
        .map(WindowId)
        .ok_or_else(|| format!("{} has no window id", app.title))
}

fn status_granted() -> R {
    let p = sc_capture_macos::platform();
    let perms = p.permissions();
    let got: Vec<(PermissionKind, PermissionStatus)> = [
        PermissionKind::Screen,
        PermissionKind::Camera,
        PermissionKind::Microphone,
    ]
    .into_iter()
    .map(|k| (k, perms.status(k)))
    .collect();
    measure(
        "permission_status",
        format!("{got:?} (status reads only, no request)"),
    );
    measure("permission_host", format!("{:?}", p.permission_host()));
    ensure(
        got.iter().all(|(_, s)| *s == PermissionStatus::Granted),
        || format!("not all granted: {got:?}"),
    )
}

fn displays_and_list() -> R {
    let fx = fixture()?;
    let src = MacScreenSource::new();
    let displays = src.displays().map_err(|e| e.to_string())?;
    measure("displays", format!("{displays:?}"));
    ensure(displays.iter().filter(|d| d.is_main).count() == 1, || {
        "need exactly one main display".into()
    })?;
    let main = main_display()?;
    ensure(main.id.0 == fx.display_id, || {
        format!("fixture display {} != main {}", fx.display_id, main.id.0)
    })?;
    let list = src.list(&[]).map_err(|e| e.to_string())?;
    measure(
        "list_counts",
        format!(
            "{} displays, {} windows, {} apps",
            list.displays.len(),
            list.windows.len(),
            list.apps.len()
        ),
    );
    let me = std::process::id() as i32;
    ensure(list.windows.iter().all(|w| w.app.pid != me), || {
        "list contains our own window".into()
    })?;
    for t in ["SC-TEST-RED", "SC-TEST-GREEN", "SC-STATIC", "SC-TONE-440"] {
        let a = fx.app(t)?;
        let id = window_id(a)?;
        let w = list
            .windows
            .iter()
            .find(|w| w.id == id)
            .ok_or_else(|| format!("{t} (window {}) not listed", id.0))?;
        measure(&format!("list_{t}"), format!("{w:?}"));
        ensure(
            w.z_index != u32::MAX && w.title == t && w.app.pid == a.pid,
            || format!("{t}: {w:?}"),
        )?;
        ensure(list.apps.iter().any(|x| x.app.pid == a.pid), || {
            format!("{t} app not listed")
        })?;
    }
    let red = window_id(fx.app("SC-TEST-RED")?)?;
    let hidden = src.list(&[red]).map_err(|e| e.to_string())?;
    ensure(hidden.windows.iter().all(|w| w.id != red), || {
        "hide did not drop the window".into()
    })
}

fn current_process() -> R {
    let r = current_process_report().map_err(|e| e.to_string())?;
    measure("current_process", format!("{r:?}"));
    Ok(())
}

struct Case {
    name: &'static str,
    spec: SourceSpec,
    probes: Vec<([f64; 4], Class)>,
}

fn run_case(src: &MacScreenSource, c: &Case, tag: u64) -> R {
    let cfg = cfg();
    let (desc, size) = src
        .describe(&c.spec, &cfg)
        .map_err(|e| format!("{}: describe: {e}", c.name))?;
    let col = Collector::new();
    col.probe(tag, c.probes.iter().map(|p| p.0).collect());
    let t0 = host_ns();
    let s = src
        .start_mac(StreamTag(tag), &c.spec, &cfg, sink(&col))
        .map_err(|e| format!("{}: start: {e}", c.name))?;
    let started = host_ns();
    let frames = col.wait_frames(tag, 3, FIRST_FRAME + Duration::from_millis(500));
    let stats = s.stats();
    Box::new(s).stop().map_err(|e| e.to_string())?;
    let first = frames
        .first()
        .ok_or_else(|| format!("{}: no frame within 1.5 s ({stats:?})", c.name))?;
    let latency_ms = (first.arrive - t0) as f64 / 1e6;
    measure(
        &format!("{}_first_frame", c.name),
        format!(
            "{latency_ms:.1} ms after start() call (start returned after {:.1} ms); size {:?}; describe {size:?} {desc:?}; content_rect {:?}",
            (started - t0) as f64 / 1e6,
            first.size,
            first.content_rect
        ),
    );
    ensure(latency_ms <= 1000.0, || {
        format!("{}: first frame after {latency_ms:.1} ms", c.name)
    })?;
    ensure(first.size == size, || {
        format!("{}: frame {:?} != describe {size:?}", c.name, first.size)
    })?;
    let last = frames.last().ok_or("no frames")?;
    for (i, (_, want)) in c.probes.iter().enumerate() {
        ensure(last.classes.get(i) == Some(want), || {
            format!(
                "{}: probe {i} is {:?}, want {want:?}",
                c.name,
                last.classes.get(i)
            )
        })?;
    }
    Ok(())
}

fn each_source_kind() -> R {
    let fx = fixture()?;
    let m = main_display()?;
    let red = fx.app("SC-TEST-RED")?;
    let green = fx.app("SC-TEST-GREEN")?;
    let red_r = window_rect(red)?;
    let green_r = window_rect(green)?;
    let src = MacScreenSource::new();
    let region = [red_r[0] + 40.0, red_r[1] + 40.0, 300.0, 200.0];
    let cases = [
        Case {
            name: "display",
            spec: SourceSpec::Display {
                display: m.id.clone(),
            },
            probes: vec![
                (px(inset(red_r, 40.0), m.scale), Class::Red),
                (px(inset(green_r, 40.0), m.scale), Class::Green),
            ],
        },
        Case {
            name: "window",
            spec: SourceSpec::Window {
                window: window_id(red)?,
            },
            probes: vec![(px([60.0, 60.0, 520.0, 360.0], m.scale), Class::Red)],
        },
        Case {
            name: "application",
            spec: SourceSpec::Application {
                display: m.id.clone(),
                app: AppRef {
                    pid: green.pid,
                    bundle_id: None,
                },
            },
            probes: vec![(px(inset(green_r, 40.0), m.scale), Class::Green)],
        },
        Case {
            name: "region",
            spec: SourceSpec::Region {
                display: m.id.clone(),
                region_pt: rect_pt(region),
            },
            probes: vec![(px([20.0, 20.0, 260.0, 160.0], m.scale), Class::Red)],
        },
    ];
    let mut failures = Vec::new();
    for (i, c) in cases.iter().enumerate() {
        if let Err(e) = run_case(&src, c, 100 + i as u64) {
            failures.push(e);
        }
    }
    ensure(failures.is_empty(), || failures.join("\n"))
}

fn region_sizes() -> R {
    let fx = fixture()?;
    let m = main_display()?;
    let red_r = window_rect(fx.app("SC-TEST-RED")?)?;
    let green_r = window_rect(fx.app("SC-TEST-GREEN")?)?;
    let src = MacScreenSource::new();
    let s = m.scale;
    let straddle_x = red_r[0] + red_r[2] - 320.0;
    let a = [straddle_x, red_r[1] + 80.0, 617.0, 283.5];
    let right_edge_pt = a[0] + a[2];
    ensure(right_edge_pt > green_r[0] + 20.0, || {
        "fixture layout: region does not reach GREEN".into()
    })?;
    let cases = [
        Case {
            name: "region_617x283.5pt",
            spec: SourceSpec::Region {
                display: m.id.clone(),
                region_pt: rect_pt(a),
            },
            probes: vec![
                (px([10.0, 100.0, 20.0, 20.0], s), Class::Red),
                (px([a[2] - 20.0, 100.0, 10.0, 20.0], s), Class::Green),
            ],
        },
        Case {
            name: "region_1x1pt",
            spec: SourceSpec::Region {
                display: m.id.clone(),
                region_pt: rect_pt([red_r[0] + 200.0, red_r[1] + 200.0, 1.0, 1.0]),
            },
            probes: vec![([0.0, 0.0, 2.0, 2.0], Class::Red)],
        },
        Case {
            name: "region_1121x641px",
            spec: SourceSpec::Region {
                display: m.id.clone(),
                region_pt: rect_pt([red_r[0] + 20.0, red_r[1] + 10.0, 1121.0 / s, 641.0 / s]),
            },
            probes: vec![([20.0, 20.0, 1000.0, 580.0], Class::Red)],
        },
    ];
    measure("region_scale", s);
    let mut failures = Vec::new();
    for (i, c) in cases.iter().enumerate() {
        if let Err(e) = run_case(&src, c, 200 + i as u64) {
            failures.push(e);
        }
    }
    ensure(failures.is_empty(), || failures.join("\n"))
}

fn non_main_display() -> R {
    let src = MacScreenSource::new();
    let ds = src.displays().map_err(|e| e.to_string())?;
    let Some(d) = ds.iter().find(|d| !d.is_main) else {
        println!("SKIPPED: only one display");
        return Ok(());
    };
    run_case(
        &src,
        &Case {
            name: "non_main_display",
            spec: SourceSpec::Display {
                display: d.id.clone(),
            },
            probes: vec![],
        },
        300,
    )
}

fn app_second_window() -> R {
    let fx = fixture()?;
    let m = main_display()?;
    let green = fx.app("SC-TEST-GREEN")?;
    let second = green.second_rect_pt.ok_or("GREEN has no second_rect_pt")?;
    let src = MacScreenSource::new();
    let col = Collector::new();
    let tag = 400;
    col.probe(tag, vec![px(inset(second, 30.0), m.scale)]);
    let s = src
        .start_mac(
            StreamTag(tag),
            &SourceSpec::Application {
                display: m.id.clone(),
                app: AppRef {
                    pid: green.pid,
                    bundle_id: None,
                },
            },
            &cfg(),
            sink(&col),
        )
        .map_err(|e| e.to_string())?;
    col.wait_frames(tag, 2, FIRST_FRAME);
    let before = col
        .frames_of(tag)
        .last()
        .and_then(|f| f.classes.first().copied());
    let t0 = host_ns();
    send_signal(green.pid, libc::SIGUSR2)?;
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut seen = None;
    while Instant::now() < deadline && seen.is_none() {
        seen = col
            .frames_of(tag)
            .into_iter()
            .find(|f| f.pts > t0 && f.classes.first() == Some(&Class::Magenta));
        std::thread::sleep(Duration::from_millis(10));
    }
    Box::new(s).stop().map_err(|e| e.to_string())?;
    measure(
        "application_second_window",
        format!(
            "before signal: {before:?}; magenta second window in frames: {}",
            seen.as_ref()
                .map(|f| format!("yes, {:.1} ms after the signal", (f.pts - t0) as f64 / 1e6))
                .unwrap_or_else(|| "no (within 2 s)".into())
        ),
    );
    ensure(before != Some(Class::Magenta), || {
        "second window present before the signal".into()
    })?;
    ensure(seen.is_some(), || {
        "application capture did not pick up the new window".into()
    })
}

fn switch_mbb() -> R {
    let fx = fixture()?;
    let m = main_display()?;
    let red = fx.app("SC-TEST-RED")?;
    let red_r = window_rect(red)?;
    let green_r = window_rect(fx.app("SC-TEST-GREEN")?)?;
    let region = inset(green_r, 40.0);
    let plan: Vec<(&str, SourceSpec, [f64; 4], Class)> = vec![
        (
            "display",
            SourceSpec::Display {
                display: m.id.clone(),
            },
            px(inset(red_r, 40.0), m.scale),
            Class::Red,
        ),
        (
            "window",
            SourceSpec::Window {
                window: window_id(red)?,
            },
            px([60.0, 60.0, 520.0, 360.0], m.scale),
            Class::Red,
        ),
        (
            "region",
            SourceSpec::Region {
                display: m.id.clone(),
                region_pt: rect_pt(region),
            },
            px([20.0, 20.0, region[2] - 40.0, region[3] - 40.0], m.scale),
            Class::Green,
        ),
        (
            "display",
            SourceSpec::Display {
                display: m.id.clone(),
            },
            px(inset(red_r, 40.0), m.scale),
            Class::Red,
        ),
    ];
    let src = MacScreenSource::new();
    let col = Collector::new();
    let cfg = cfg();
    let mut streams: Vec<sc_capture_macos::sck::MacScreenStream> = Vec::new();
    let mut gaps = Vec::new();
    for (i, (name, spec, probe, _)) in plan.iter().enumerate() {
        let tag = 500 + i as u64;
        col.probe(tag, vec![*probe]);
        let t_issue = host_ns();
        let s = src
            .start_mac(StreamTag(tag), spec, &cfg, sink(&col))
            .map_err(|e| format!("{name}: {e}"))?;
        let frames = col.wait_frames(tag, 1, FIRST_FRAME);
        let first = frames
            .first()
            .ok_or_else(|| format!("{name}: no first frame"))?
            .clone();
        if let Some(prev) = streams.pop() {
            let prev_tag = 500 + i as u64 - 1;
            let last_before = col
                .frames_of(prev_tag)
                .into_iter()
                .filter(|f| f.pts < first.pts)
                .map(|f| f.pts)
                .max();
            let gap = last_before.map(|p| first.pts - p);
            std::thread::sleep(Duration::from_millis(300));
            let old: Vec<i64> = col.frames_of(prev_tag).iter().map(|f| f.pts).collect();
            let old_last = old.iter().copied().max().unwrap_or(0);
            let best = col
                .frames_of(tag)
                .iter()
                .filter(|f| f.pts <= old_last)
                .filter_map(|f| old.iter().filter(|p| **p < f.pts).max().map(|p| f.pts - p))
                .min();
            Box::new(prev).stop().map_err(|e| e.to_string())?;
            gaps.push((
                format!("{}→{name}", plan[i - 1].0),
                gap,
                (first.arrive - t_issue) as f64 / 1e6,
                best,
            ));
        } else {
            std::thread::sleep(Duration::from_millis(300));
        }
        streams.push(s);
        std::thread::sleep(Duration::from_millis(300));
    }
    for s in streams {
        Box::new(s).stop().map_err(|e| e.to_string())?;
    }
    let mut wrong = 0usize;
    let mut total = 0usize;
    for (i, (_, _, _, want)) in plan.iter().enumerate() {
        for f in col.frames_of(500 + i as u64) {
            total += 1;
            if f.classes.first() != Some(want) {
                wrong += 1;
            }
        }
    }
    let ms = |v: Option<i64>| {
        v.map(|g| format!("{:.1}", g as f64 / 1e6))
            .unwrap_or("n/a".into())
    };
    for (what, gap, first_ms, cadence) in &gaps {
        measure(
            "switch",
            format!(
                "{what}: gap at the new stream's first frame {} ms; best switch point in the 300 ms overlap {} ms; new stream first frame {first_ms:.1} ms after start()",
                ms(*gap),
                ms(*cadence)
            ),
        );
    }
    measure("switch_wrong_content_frames", format!("{wrong} of {total}"));
    ensure(wrong == 0, || format!("{wrong} wrong-content frames"))?;
    for (what, gap, _, cadence) in &gaps {
        gap.ok_or_else(|| format!("{what}: old stream delivered nothing before the new one"))?;
        let g = cadence.ok_or_else(|| format!("{what}: streams did not overlap"))?;
        ensure(g <= 34_000_000, || {
            format!("{what}: best switch gap {:.1} ms > 34 ms", g as f64 / 1e6)
        })?;
    }
    Ok(())
}

fn static_first_frame() -> R {
    let fx = fixture()?;
    let m = main_display()?;
    let red = fx.app("SC-TEST-RED")?;
    let stat = fx.app("SC-STATIC")?;
    let src = MacScreenSource::new();
    let col = Collector::new();
    let a = src
        .start_mac(
            StreamTag(600),
            &SourceSpec::Window {
                window: window_id(red)?,
            },
            &cfg(),
            sink(&col),
        )
        .map_err(|e| e.to_string())?;
    col.wait_frames(600, 1, FIRST_FRAME);
    col.probe(601, vec![px([40.0, 40.0, 240.0, 160.0], m.scale)]);
    let t0 = host_ns();
    let b = src
        .start_mac(
            StreamTag(601),
            &SourceSpec::Window {
                window: window_id(stat)?,
            },
            &cfg(),
            sink(&col),
        )
        .map_err(|e| e.to_string())?;
    let frames = col.wait_frames(601, 1, FIRST_FRAME + Duration::from_millis(500));
    std::thread::sleep(Duration::from_millis(500));
    let n = col.frames_of(601).len();
    let stats = b.stats();
    Box::new(a).stop().map_err(|e| e.to_string())?;
    Box::new(b).stop().map_err(|e| e.to_string())?;
    let first = frames
        .first()
        .ok_or_else(|| format!("SC-STATIC: no frame ({stats:?})"))?;
    let ms = (first.arrive - t0) as f64 / 1e6;
    measure(
        "static_first_frame",
        format!(
            "{ms:.1} ms after start(); class {:?}; {n} complete frames in ~1 s; status counts {:?}",
            first.classes, stats.status
        ),
    );
    ensure(ms <= 1000.0, || format!("first frame after {ms:.1} ms"))?;
    ensure(first.classes.first() == Some(&Class::Gray), || {
        format!("SC-STATIC frame is {:?}", first.classes)
    })
}

fn own_windows_excluded() -> R {
    let fx = fixture()?;
    let m = main_display()?;
    let red_r = window_rect(fx.app("SC-TEST-RED")?)?;
    let r1 = [red_r[0] + 40.0, red_r[1] + 40.0, 150.0, 150.0];
    let r2 = [red_r[0] + 240.0, red_r[1] + 40.0, 150.0, 150.0];
    let r3 = [red_r[0] + 440.0, red_r[1] + 40.0, 150.0, 150.0];
    let overlays = MacOverlays::new();
    let w1 = on_main(move |mtm| make_panel(mtm, r1, true, false));
    let ov = MacOverlays::new();
    let (w2, w2_info) = on_main(move |mtm| {
        let p = make_panel(mtm, r2, false, true);
        let before = p.0.windowNumber();
        let isa_before =
            unsafe { objc2::ffi::object_getClass(objc2::rc::Retained::as_ptr(&p.0).cast()) };
        let isa_before =
            unsafe { isa_before.as_ref() }.map(|c| c.name().to_string_lossy().into_owned());
        let ptr = objc2::rc::Retained::as_ptr(&p.0) as *mut std::ffi::c_void;
        let id = unsafe { ov.configure(ptr, OverlayRole::FloatingBar) };
        let class = p.0.class().name().to_string_lossy().into_owned();
        let mask = p.0.styleMask();
        let level = p.0.level();
        (
            p,
            format!(
                "isa before configure {isa_before:?}; windowNumber before configure {before}; configure → {id:?}; class {class}; nonactivating {}; level {level}",
                mask.contains(objc2_app_kit::NSWindowStyleMask::NonactivatingPanel)
            ),
        )
    });
    measure("overlay_configure", &w2_info);
    let w2_id = overlays.registered();
    measure("overlay_registered", format!("{w2_id:?}"));
    ensure(
        w2_info.contains("configure → Ok(")
            && w2_info.contains(MacOverlays::panel_class_name())
            && w2_info.contains("nonactivating true")
            && w2_info.contains("level 25"),
        || format!("overlay configure: {w2_info}"),
    )?;

    let src = MacScreenSource::new();
    let col = Collector::new();
    let tag = 700;
    col.probe(
        tag,
        vec![
            px(inset(r1, 30.0), m.scale),
            px(inset(r2, 30.0), m.scale),
            px(inset(r3, 30.0), m.scale),
        ],
    );
    let s = src
        .start_mac(
            StreamTag(tag),
            &SourceSpec::Display {
                display: m.id.clone(),
            },
            &cfg(),
            sink(&col),
        )
        .map_err(|e| e.to_string())?;
    col.wait_frames(tag, 2, FIRST_FRAME);
    std::thread::sleep(Duration::from_millis(200));
    let at = |since: i64, i: usize| {
        col.frames_of(tag)
            .into_iter()
            .filter(|f| f.pts >= since)
            .filter_map(|f| f.classes.get(i).copied())
            .collect::<Vec<_>>()
    };
    let w1_classes = at(0, 0);

    let t_show = host_ns();
    let w2r = &w2;
    on_main(move |_| w2r.0.orderFrontRegardless());
    std::thread::sleep(Duration::from_millis(400));
    let w2_classes = at(t_show + 100_000_000, 1);

    let t_w3 = host_ns();
    let w3 = on_main(move |mtm| make_panel(mtm, r3, true, false));
    std::thread::sleep(Duration::from_millis(400));
    let w3_before = at(t_w3 + 100_000_000, 2);
    let t_upd = host_ns();
    s.update_exclusions(&[]).map_err(|e| e.to_string())?;
    let upd_ms = (host_ns() - t_upd) as f64 / 1e6;
    std::thread::sleep(Duration::from_millis(400));
    let w3_after = at(host_ns() - 250_000_000, 2);
    let cp = current_process_report().map_err(|e| e.to_string());
    Box::new(s).stop().map_err(|e| e.to_string())?;
    on_main(move |_| {
        for w in [w1, w2, w3] {
            w.0.close();
        }
    });
    let summary = |v: &[Class]| {
        let red = v.iter().filter(|c| **c == Class::Red).count();
        let blue = v.iter().filter(|c| **c == Class::Blue).count();
        format!("{red} red / {blue} blue of {}", v.len())
    };
    measure("own_window_shown_before_start", summary(&w1_classes));
    measure("own_window_hidden_then_shown", summary(&w2_classes));
    measure(
        "own_window_created_after_start_before_update",
        summary(&w3_before),
    );
    measure(
        "own_window_created_after_start_after_update",
        format!(
            "{} (update_exclusions took {upd_ms:.1} ms)",
            summary(&w3_after)
        ),
    );
    measure("current_process_with_own_windows", format!("{cp:?}"));
    let all_red = |v: &[Class]| !v.is_empty() && v.iter().all(|c| *c == Class::Red);
    ensure(all_red(&w1_classes), || {
        format!("own window visible: {}", summary(&w1_classes))
    })?;
    ensure(all_red(&w2_classes), || {
        format!("hidden-then-shown window visible: {}", summary(&w2_classes))
    })?;
    ensure(all_red(&w3_after), || {
        format!(
            "window created after start still visible after update: {}",
            summary(&w3_after)
        )
    })
}

fn snapshot_window() -> R {
    let fx = fixture()?;
    let red = fx.app("SC-TEST-RED")?;
    let src = MacScreenSource::new();
    let img = src
        .snapshot(
            &SourceSpec::Window {
                window: window_id(red)?,
            },
            SizePx { w: 320, h: 320 },
        )
        .map_err(|e| e.to_string())?;
    let (cx, cy) = (img.size.w as usize / 2, img.size.h as usize / 2);
    let i = cy * img.stride + cx * 4;
    let px = &img.data[i..i + 4];
    measure("snapshot", format!("{:?}, centre RGBA {px:?}", img.size));
    ensure(img.size.w <= 320 && img.size.h <= 320, || {
        format!("size {:?}", img.size)
    })?;
    ensure(px[0] > 200 && px[1] < 60 && px[2] < 60, || {
        format!("centre {px:?} is not red")
    })
}

fn tones(recs: &[AudioRec]) -> (f64, f64, f64) {
    let rate = recs.first().map(|r| r.rate).unwrap_or(48_000);
    let x: Vec<f32> = recs.iter().flat_map(|r| r.mono.iter().copied()).collect();
    (
        tone_amplitude(&x, rate, 440.0),
        tone_amplitude(&x, rate, 660.0),
        tone_amplitude(&x, rate, 550.0),
    )
}

/// SCK groups app audio by bundle ID: every unbundled process (all fixture apps) shares one
/// group, so SC-TONE-440 vs SC-TONE-660 cannot be separated (measured, not asserted). The App
/// filter is asserted with a bundled, silent app (Finder): both tones must disappear.
fn system_audio() -> R {
    let fx = fixture()?;
    let t440 = fx.app("SC-TONE-440")?;
    fx.app("SC-TONE-660")?;
    let finder = MacScreenSource::new()
        .list(&[])
        .map_err(|e| e.to_string())?
        .apps
        .into_iter()
        .find(|a| a.app.bundle_id.as_deref() == Some("com.apple.finder"))
        .ok_or("Finder is not listed")?;
    let col = Collector::new();
    let sys = MacSystemAudio::new();
    let s = sys
        .start_mac(SystemAudioMode::All, &[], sink(&col))
        .map_err(|e| e.to_string())?;
    let t0 = host_ns();
    std::thread::sleep(Duration::from_millis(1500));
    let all = tones(&col.audio_since(t0 + 300_000_000));

    s.follow_app(Some(&AppRef {
        pid: t440.pid,
        bundle_id: None,
    }))
    .map_err(|e| e.to_string())?;
    let t1 = host_ns();
    std::thread::sleep(Duration::from_millis(1200));
    let unbundled = tones(&col.audio_since(t1 + 500_000_000));

    let t_follow = host_ns();
    s.follow_app(Some(&finder.app)).map_err(|e| e.to_string())?;
    let follow_ms = (host_ns() - t_follow) as f64 / 1e6;
    std::thread::sleep(Duration::from_millis(1200));
    let around = col.audio_since(t_follow - 300_000_000);
    let gap_ms = max_audio_gap(&around) as f64 / 1e6;
    let bundled = tones(&col.audio_since(t_follow + 500_000_000));

    s.follow_app(None).map_err(|e| e.to_string())?;
    let t2 = host_ns();
    std::thread::sleep(Duration::from_millis(1200));
    let back = tones(&col.audio_since(t2 + 500_000_000));
    let stats = s.stats();
    Box::new(s).stop().map_err(|e| e.to_string())?;
    measure(
        "system_audio_all",
        format!(
            "amp440 {:.4}, amp660 {:.4}, amp550 {:.5}",
            all.0, all.1, all.2
        ),
    );
    measure(
        "system_audio_app_unbundled_440",
        format!(
            "amp440 {:.4}, amp660 {:.4} (unbundled apps share one audio group)",
            unbundled.0, unbundled.1
        ),
    );
    measure(
        "system_audio_app_finder",
        format!("amp440 {:.5}, amp660 {:.5}", bundled.0, bundled.1),
    );
    measure(
        "system_audio_back_to_all",
        format!("amp440 {:.4}, amp660 {:.4}", back.0, back.1),
    );
    measure(
        "system_audio_follow_app",
        format!(
            "follow_app took {follow_ms:.1} ms; largest PTS hole around the filter change {gap_ms:.1} ms; {} buffers",
            stats.audio_buffers
        ),
    );
    ensure(all.0 > 0.005 && all.1 > 0.005, || {
        format!("All mode missing a tone: {all:?}")
    })?;
    ensure(bundled.0 < 0.002 && bundled.1 < 0.002, || {
        format!("App mode (Finder) still has the tones: {bundled:?}")
    })?;
    ensure(back.0 > 0.005 && back.1 > 0.005, || {
        format!("back to All lost a tone: {back:?}")
    })
}

fn picker_readback() -> R {
    let p = MacPicker::new();
    let info = p
        .configuration_readback(&[WindowId(4242)])
        .map_err(|e| e.to_string())?;
    measure("picker_configuration", format!("{info:?} (not presented)"));
    ensure(
        info.allowed_modes_raw == MacPicker::expected_modes_raw(),
        || format!("modes {info:?}"),
    )?;
    ensure(info.maximum_stream_count == Some(0), || {
        format!("maximumStreamCount {info:?}")
    })?;
    ensure(info.excluded_window_ids.contains(&4242), || {
        format!("excludedWindowIDs {info:?}")
    })
}

fn window_closed() -> R {
    let fx = fixture()?;
    let red = fx.app("SC-TEST-RED")?;
    let src = MacScreenSource::new();
    let col = Collector::new();
    let s = src
        .start_mac(
            StreamTag(800),
            &SourceSpec::Window {
                window: window_id(red)?,
            },
            &cfg(),
            sink(&col),
        )
        .map_err(|e| e.to_string())?;
    col.wait_frames(800, 2, FIRST_FRAME);
    let t0 = host_ns();
    send_signal(red.pid, libc::SIGUSR1)?;
    let ev = col.wait_event(Duration::from_secs(2), |e| {
        matches!(
            e,
            SourceEvent::SourceLost { .. } | SourceEvent::StreamStopped { .. }
        )
    });
    std::thread::sleep(Duration::from_millis(200));
    let stats = s.stats();
    let _ = Box::new(s).stop();
    let events = col.events.lock().map(|e| e.clone()).unwrap_or_default();
    measure(
        "closed_window",
        format!(
            "{}; all events {events:?}; status counts {:?}; stop_error {:?}",
            ev.as_ref()
                .map(|(t, e)| format!("{e:?} after {:.1} ms", (t - t0) as f64 / 1e6))
                .unwrap_or_else(|| "no event within 2 s".into()),
            stats.status,
            stats.stop_error
        ),
    );
    let (t, _) = ev.ok_or("no SourceLost/StreamStopped within 2 s")?;
    ensure(t - t0 <= 1_000_000_000, || {
        format!("event after {:.1} ms", (t - t0) as f64 / 1e6)
    })
}
