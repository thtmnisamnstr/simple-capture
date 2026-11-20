//! Live av tests. Bodies run under the dev host via `just test-live simple-capture-capture-macos av_`.

#[cfg(target_os = "macos")]
#[path = "../support/live_runner.rs"]
mod live_runner;

#[cfg(target_os = "macos")]
mod live {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use sc_platform::{
        Append, AudioBuffer, AudioTrackKind, AudioWriterSpec, CameraLimit, FrameSink, Platform,
        SourceEvent, VideoFrame,
    };

    type AudioRow = (i64, i64, u32, u32, u16);

    #[derive(Default)]
    struct Probe {
        camera: Mutex<Vec<(i64, i64, sc_geometry::SizePx)>>,
        audio: Mutex<Vec<AudioRow>>,
        keep: Mutex<Vec<AudioBuffer>>,
        keep_audio: AtomicBool,
    }

    impl FrameSink for Probe {
        fn on_screen(&self, _f: VideoFrame) {}
        fn on_camera(&self, f: VideoFrame) {
            let now = host_now();
            if let Ok(mut v) = self.camera.lock() {
                v.push((f.pts.0, now, f.size));
            }
        }
        fn on_audio(&self, t: AudioTrackKind, a: AudioBuffer) {
            if t != AudioTrackKind::Mic {
                return;
            }
            let now = host_now();
            if let Ok(mut v) = self.audio.lock() {
                v.push((a.pts.0, now, a.frames, a.sample_rate, a.channels));
            }
            if self.keep_audio.load(Ordering::Relaxed)
                && let Ok(mut k) = self.keep.lock()
            {
                k.push(a);
            }
        }
        fn on_event(&self, e: SourceEvent) {
            println!("event: {e:?}");
        }
    }

    fn platform() -> Arc<dyn Platform> {
        static P: std::sync::OnceLock<Arc<dyn Platform>> = std::sync::OnceLock::new();
        P.get_or_init(sc_capture_macos::platform).clone()
    }

    fn host_now() -> i64 {
        platform().host_now().0
    }

    fn check_host_time(
        what: &str,
        samples: &[(i64, i64)],
        max_latency_ns: i64,
    ) -> Result<(), String> {
        let mut worst = 0i64;
        for (i, (pts, arrival)) in samples.iter().enumerate() {
            let lat = arrival - pts;
            if !(-5_000_000..=max_latency_ns).contains(&lat) {
                return Err(format!(
                    "{what} sample {i}: pts {pts} vs arrival {arrival} ({lat} ns)"
                ));
            }
            worst = worst.max(lat);
            if i > 0 && *pts <= samples[i - 1].0 {
                return Err(format!("{what}: pts not increasing at {i}"));
            }
        }
        println!(
            "{what}: {} samples on the host clock, worst delivery latency {:.1} ms",
            samples.len(),
            worst as f64 / 1e6
        );
        Ok(())
    }

    pub fn device_lists_non_empty() -> Result<(), String> {
        let p = platform();
        let cams = p.camera().devices().map_err(|e| e.to_string())?;
        let mics = p.mic().devices().map_err(|e| e.to_string())?;
        for d in cams.iter().chain(mics.iter()) {
            println!("device: {d:?}");
        }
        if cams.is_empty() || mics.is_empty() {
            return Err(format!("{} cameras, {} mics", cams.len(), mics.len()));
        }
        Ok(())
    }

    pub fn camera_delivers_host_time_frames() -> Result<(), String> {
        let p = platform();
        let cams = p.camera().devices().map_err(|e| e.to_string())?;
        let cam = cams
            .iter()
            .find(|d| d.is_default)
            .or(cams.first())
            .ok_or("no camera")?;
        let probe = Arc::new(Probe::default());
        let limit = CameraLimit {
            max: sc_geometry::SizePx { w: 1920, h: 1080 },
            max_fps: 30,
        };
        let t0 = Instant::now();
        let s = p
            .camera()
            .start(&cam.id, limit, probe.clone())
            .map_err(|e| format!("start {}: {e}", cam.name))?;
        let started_ms = t0.elapsed().as_millis();
        std::thread::sleep(Duration::from_secs(3));
        let format = s.format();
        s.stop().map_err(|e| e.to_string())?;
        let v = probe.camera.lock().map_err(|e| e.to_string())?.clone();
        let first = v.first().ok_or("camera delivered no frames")?;
        let span = (v.last().map_or(0, |l| l.0) - first.0) as f64 / 1e9;
        println!(
            "camera {} ({}): start {started_ms} ms, {} frames {}x{} in {span:.2} s ({:.1} fps)",
            cam.name,
            format,
            v.len(),
            first.2.w,
            first.2.h,
            (v.len().saturating_sub(1)) as f64 / span.max(1e-9)
        );
        if v.len() < 10 || first.2.w > 1920 || first.2.h > 1080 {
            return Err(format!("{} frames of {:?}", v.len(), first.2));
        }
        let pairs: Vec<(i64, i64)> = v.iter().map(|x| (x.0, x.1)).collect();
        check_host_time("camera", &pairs, 250_000_000)
    }

    pub fn mic_delivers_host_time_audio_and_writes_aac() -> Result<(), String> {
        let p = platform();
        let mics = p.mic().devices().map_err(|e| e.to_string())?;
        let mic = mics
            .iter()
            .find(|d| d.is_default)
            .or(mics.first())
            .ok_or("no mic")?;
        let probe = Arc::new(Probe::default());
        probe.keep_audio.store(true, Ordering::Relaxed);
        let s = p
            .mic()
            .start(&mic.id, probe.clone())
            .map_err(|e| format!("start {}: {e}", mic.name))?;
        std::thread::sleep(Duration::from_secs(2));
        let format = s.format();
        s.stop().map_err(|e| e.to_string())?;
        let v = probe.audio.lock().map_err(|e| e.to_string())?.clone();
        let first = *v.first().ok_or("mic delivered no audio")?;
        let frames: u64 = v.iter().map(|x| u64::from(x.2)).sum();
        let span_ns = v.last().map_or(0, |l| l.0) - first.0;
        println!(
            "mic {} ({format}): {} buffers, {frames} frames at {} Hz x {} ch over {:.3} s of pts",
            mic.name,
            v.len(),
            first.3,
            first.4,
            span_ns as f64 / 1e9
        );
        let pairs: Vec<(i64, i64)> = v.iter().map(|x| (x.0, x.1)).collect();
        check_host_time("mic", &pairs, 250_000_000)?;

        let dir = std::env::temp_dir().join(format!("sc-av-live-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join("mic.mov");
        let spec = AudioWriterSpec {
            sample_rate: 48_000,
            channels: 1,
            bitrate: 96_000,
            fragment_interval_s: 1.0,
        };
        let mut w = p.writers().audio(&path, &spec).map_err(|e| e.to_string())?;
        let kept = std::mem::take(&mut *probe.keep.lock().map_err(|e| e.to_string())?);
        let base = kept.first().map_or(0, |a| a.pts.0);
        for a in &kept {
            let deadline = Instant::now() + Duration::from_secs(5);
            while w
                .append_audio(a, a.pts.0 - base, 0)
                .map_err(|e| e.to_string())?
                == Append::NotReady
            {
                if Instant::now() > deadline {
                    return Err("writer stayed NotReady".into());
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        let fin = w.finish(None).map_err(|e| e.to_string())?;
        let probe_res = p.decoder().probe(&path).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_dir_all(&dir);
        println!("mic -> AAC: {fin:?}; probe {probe_res:?}");
        if !probe_res.readable {
            return Err(format!("mic AAC not readable: {probe_res:?}"));
        }
        Ok(())
    }

    pub fn show_mic_modes_returns() -> Result<(), String> {
        if std::env::var("SC_LIVE_MIC_MODES").ok().as_deref() != Some("1") {
            println!(
                "show_mic_modes: not presented (it opens Control Center's mic-mode panel on the owner's screen); set SC_LIVE_MIC_MODES=1 to call it"
            );
            return Ok(());
        }
        platform()
            .mic()
            .show_mic_modes()
            .map_err(|e| e.to_string())?;
        std::thread::sleep(Duration::from_millis(500));
        Ok(())
    }

    pub fn device_watcher_starts_and_stops() -> Result<(), String> {
        let s = sc_capture_macos::av::watch_devices(Arc::new(Probe::default()))
            .map_err(|e| e.to_string())?;
        println!("watcher: {}", s.format());
        s.stop().map_err(|e| e.to_string())
    }
}

#[cfg(target_os = "macos")]
fn main() {
    live_runner::main(&[
        live_runner::LiveTest {
            name: "av_devices_listed",
            run: live::device_lists_non_empty,
        },
        live_runner::LiveTest {
            name: "av_camera_host_time",
            run: live::camera_delivers_host_time_frames,
        },
        live_runner::LiveTest {
            name: "av_mic_host_time_and_aac",
            run: live::mic_delivers_host_time_audio_and_writes_aac,
        },
        live_runner::LiveTest {
            name: "av_show_mic_modes",
            run: live::show_mic_modes_returns,
        },
        live_runner::LiveTest {
            name: "av_device_watcher",
            run: live::device_watcher_starts_and_stops,
        },
    ])
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("SKIPPED: macOS only");
    println!();
    println!(
        "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s"
    );
}
