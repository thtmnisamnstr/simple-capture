//! The Windows backend on Windows; elsewhere the stub must say it is unsupported.

#[cfg(not(windows))]
#[test]
fn elsewhere_the_stub_is_unsupported() {
    let p = sc_capture_windows::platform();
    assert_eq!(p.name(), "windows");
    assert!(matches!(
        p.screen().displays(),
        Err(sc_platform::Error::Unsupported(_))
    ));
}

#[cfg(windows)]
mod windows {
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use sc_platform::{
        AudioBuffer, AudioTrackKind, FrameSink, ScreenStreamConfig, SourceEvent, SourceSpec,
        StreamTag, VideoFrame,
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

    fn required() -> bool {
        std::env::var("SC_REQUIRE_DISPLAY").as_deref() == Ok("1")
    }

    #[derive(Default)]
    struct Frames(Mutex<Vec<i64>>);

    impl FrameSink for Frames {
        fn on_screen(&self, f: VideoFrame) {
            if let Ok(mut l) = self.0.lock() {
                l.push(f.pts.0);
            }
        }
        fn on_camera(&self, _: VideoFrame) {}
        fn on_audio(&self, _: AudioTrackKind, _: AudioBuffer) {}
        fn on_event(&self, _: SourceEvent) {}
    }

    #[test]
    fn platform_basics() {
        let p = sc_capture_windows::platform();
        assert_eq!(p.name(), "windows");
        assert!(p.capabilities().application_source);
        assert!(
            p.default_projects_dir()
                .expect("dir")
                .ends_with("simple-capture")
        );
        assert!(p.free_disk_bytes(Path::new(".")).expect("disk") > 0);
        let (a, b) = (p.host_now(), p.host_now());
        assert!(b >= a && a.0 > 0);
        assert!(p.local_now().y >= 2026);
        assert!(p.camera().devices().is_ok());
        assert!(p.screen().list(&[]).is_ok());
        assert_eq!(
            p.writers().extension(),
            "mp4",
            "Media Foundation records on a system that has it"
        );
    }

    /// Windows.Graphics.Capture on the main display. A runner without a desktop may list no
    /// display; `SC_REQUIRE_DISPLAY=1` makes that a failure.
    #[test]
    fn main_display_delivers_frames() {
        let p = sc_capture_windows::platform();
        let displays = p.screen().displays().expect("displays");
        eprintln!("displays: {displays:?}");
        let Some(main) = displays.iter().find(|d| d.is_main).or(displays.first()) else {
            assert!(!required(), "no display");
            eprintln!("SKIPPED: no display on this machine");
            return;
        };
        let spec = SourceSpec::Display {
            display: main.id.clone(),
        };
        let (desc, size) = p.screen().describe(&spec, &cfg()).expect("describe");
        eprintln!("{desc:?} {size:?}");
        assert_eq!(size.w % 2, 0);
        let sink = Arc::new(Frames::default());
        let s = match p.screen().start(StreamTag(1), &spec, &cfg(), sink.clone()) {
            Ok(s) => s,
            Err(e) if !required() => {
                eprintln!("SKIPPED: capture did not start: {e}");
                return;
            }
            Err(e) => panic!("start: {e}"),
        };
        std::thread::sleep(std::time::Duration::from_millis(1500));
        s.stop().expect("stop");
        let n = sink.0.lock().expect("frames").len();
        eprintln!("{n} frames in 1.5 s");
        assert!(!required() || n > 0, "no frames");
    }
}
