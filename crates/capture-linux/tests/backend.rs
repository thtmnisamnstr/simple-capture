//! The Linux backend on Linux; elsewhere the stub must say it is unsupported.

#[cfg(not(target_os = "linux"))]
#[test]
fn elsewhere_the_stub_is_unsupported() {
    let p = sc_capture_linux::platform();
    assert_eq!(p.name(), "linux");
    assert!(matches!(
        p.screen().displays(),
        Err(sc_platform::Error::Unsupported(_))
    ));
}

#[cfg(target_os = "linux")]
mod linux {
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use sc_platform::{
        AudioBuffer, AudioTrackKind, Error, FrameSink, PermissionKind, PermissionStatus,
        PickerRequest, ScreenStreamConfig, SourceEvent, SourceSpec, StreamTag, VideoFrame,
    };

    fn cfg() -> ScreenStreamConfig {
        ScreenStreamConfig {
            fps: 30,
            resolution: sc_platform::Resolution::Native,
            show_cursor: true,
            show_clicks: false,
            queue_depth: 3,
            exclude: Vec::new(),
        }
    }

    #[test]
    fn platform_basics() {
        let p = sc_capture_linux::platform();
        assert_eq!(p.name(), "linux");
        let caps = p.capabilities();
        assert!(caps.system_picker);
        assert_eq!(caps.exclusion, sc_platform::ExclusionKind::AutoHide);
        assert!(
            p.default_projects_dir()
                .expect("dir")
                .ends_with("Simple Capture")
        );
        assert!(p.free_disk_bytes(Path::new(".")).expect("disk") > 0);
        let (a, b) = (p.host_now(), p.host_now());
        assert!(b >= a && a.0 > 0);
        assert!(p.local_now().y >= 2026);
        assert_eq!(
            p.permissions().status(PermissionKind::Camera),
            PermissionStatus::Granted
        );
        assert!(p.camera().devices().is_ok());
        assert!(p.screen().displays().expect("displays").is_empty());
        let spec = SourceSpec::Display {
            display: sc_platform::DisplayId("x".into()),
        };
        assert!(matches!(
            p.screen().describe(&spec, &cfg()),
            Err(Error::Unsupported(_))
        ));
        assert_eq!(p.writers().extension(), "mkv");
    }

    #[derive(Default)]
    struct Frames(Mutex<Vec<(i64, u64)>>);

    impl FrameSink for Frames {
        fn on_screen(&self, f: VideoFrame) {
            let mut sum = 0u64;
            let _ = f.buffer.0.with_nv12(&mut |v| {
                sum = v.y.iter().step_by(97).map(|y| u64::from(*y)).sum();
            });
            if let Ok(mut l) = self.0.lock() {
                l.push((f.pts.0, sum));
            }
        }
        fn on_camera(&self, _: VideoFrame) {}
        fn on_audio(&self, _: AudioTrackKind, _: AudioBuffer) {}
        fn on_event(&self, _: SourceEvent) {}
    }

    /// Needs a session whose ScreenCast portal picks without asking (tools/linux-dev/portal.sh).
    #[test]
    #[ignore = "needs a portal session: tools/linux-dev/portal.sh"]
    fn portal_screencast_delivers_frames() {
        let p = sc_capture_linux::platform();
        let (tx, rx) = std::sync::mpsc::channel();
        p.picker().present(
            PickerRequest::default(),
            Box::new(move |r| {
                let _ = tx.send(r);
            }),
        );
        let picked = rx
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("picker answered")
            .expect("picker ok")
            .expect("a source was picked");
        let spec = SourceSpec::Picked(picked);
        let (desc, size) = p.screen().describe(&spec, &cfg()).expect("describe");
        eprintln!("picked {desc:?} {size:?}");
        let sink = Arc::new(Frames::default());
        let s = p
            .screen()
            .start(StreamTag(1), &spec, &cfg(), sink.clone())
            .expect("start");
        std::thread::sleep(std::time::Duration::from_secs(2));
        s.stop().expect("stop");
        let frames = sink.0.lock().expect("frames").clone();
        eprintln!("{} frames", frames.len());
        assert!(!frames.is_empty(), "no frames in 2 s");
        assert!(frames.windows(2).all(|w| w[1].0 >= w[0].0));
    }
}

/// Our PipeWire consumer against a GStreamer test source on the PipeWire daemon
/// (tools/linux-dev/portal.sh provides one); checks format negotiation and conversion.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "needs a PipeWire daemon with a video source: tools/linux-dev/portal.sh"]
fn pipewire_test_source_delivers_converted_frames() {
    use std::sync::Arc;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Luma(Mutex<Vec<f64>>);
    impl sc_platform::FrameSink for Luma {
        fn on_screen(&self, f: sc_platform::VideoFrame) {
            let mut mean = 0.0;
            let _ = f.buffer.0.with_nv12(&mut |v| {
                mean = v.y.iter().map(|y| f64::from(*y)).sum::<f64>() / v.y.len() as f64;
            });
            if let Ok(mut l) = self.0.lock() {
                l.push(mean);
            }
        }
        fn on_camera(&self, _: sc_platform::VideoFrame) {}
        fn on_audio(&self, _: sc_platform::AudioTrackKind, _: sc_platform::AudioBuffer) {}
        fn on_event(&self, _: sc_platform::SourceEvent) {}
    }

    let node: u32 = std::env::var("SC_PW_TEST_NODE")
        .expect("SC_PW_TEST_NODE")
        .parse()
        .expect("node id");
    let sink = Arc::new(Luma::default());
    let out = sc_geometry::SizePx { w: 640, h: 360 };
    let s = sc_capture_linux::node_stream(node, sc_platform::StreamTag(1), out, 30, sink.clone())
        .expect("stream");
    std::thread::sleep(std::time::Duration::from_secs(2));
    s.stop().expect("stop");
    let l = sink.0.lock().expect("frames").clone();
    eprintln!("{} frames, mean luma {:?}", l.len(), l.first());
    assert!(l.len() >= 10, "{} frames", l.len());
    assert!(l.iter().all(|m| *m > 20.0), "frames are not black: {l:?}");
}
