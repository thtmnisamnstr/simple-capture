use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use objc2::MainThreadMarker;
use objc2_core_graphics::{CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess};
use serde_json::json;

use crate::avcap::{self, AvCapture};
use crate::capture::{self, Exclude, ScreenCapture, ScreenOpts, PIX_420V};
use crate::testwin::{self, TestWindow};
use crate::util::*;
use crate::Args;

pub fn ensure_screen_access(timeout: Duration) -> bool {
    if CGPreflightScreenCaptureAccess() {
        return true;
    }
    eprintln!(">>> Screen Recording permission needed. macOS will show a prompt or open System Settings (attributed to the shared dev host \"Simple Capture Dev\" when run through tools/macos-dev-host/run.sh). Please allow it. Polling for up to {}s...", timeout.as_secs());
    CGRequestScreenCaptureAccess();
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if CGPreflightScreenCaptureAccess() {
            return true;
        }
        if capture::fetch_content().is_ok() {
            return true;
        }
        pump_for(Duration::from_secs(2));
    }
    false
}

fn stats_intervals(pts: &[f64], from: f64, to: f64) -> serde_json::Value {
    let v: Vec<f64> = pts.iter().copied().filter(|p| *p >= from && *p < to).collect();
    let dur = (to - from).max(1e-9);
    let mut gaps: Vec<f64> = v.windows(2).map(|w| w[1] - w[0]).collect();
    gaps.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct = |q: f64| if gaps.is_empty() { 0.0 } else { gaps[((gaps.len() - 1) as f64 * q).round() as usize] };
    json!({
        "from_s": from, "to_s": to, "frames": v.len(), "fps": v.len() as f64 / dur,
        "interval_ms_p50": pct(0.5) * 1e3, "interval_ms_p95": pct(0.95) * 1e3, "interval_ms_max": pct(1.0) * 1e3,
    })
}

fn late_frames(pts: &[f64], fps: f64, from: f64, to: f64) -> u64 {
    let iv = 1.0 / fps;
    pts.windows(2)
        .filter(|w| w[0] >= from && w[1] < to)
        .map(|w| {
            let gap = w[1] - w[0];
            if gap > iv * 1.5 {
                ((gap / iv).round() as u64).saturating_sub(1)
            } else {
                0
            }
        })
        .sum()
}

pub fn record(a: &Args) -> Result<()> {
    let mtm = MainThreadMarker::new().ok_or_else(|| anyhow!("must run on main thread"))?;
    let dir = PathBuf::from(a.get("out").ok_or_else(|| anyhow!("--out DIR required"))?);
    std::fs::create_dir_all(&dir)?;
    let secs_total: f64 = a.num("secs", 10.0);
    let fps: f64 = a.num("fps", 30.0);
    let qd: isize = a.num("queue-depth", 6.0) as isize;
    let bpp: f64 = a.num("bpp", 0.05);
    let frag: f64 = a.num("frag", 1.0);
    let hevc = a.get("codec").map(|c| c != "h264").unwrap_or(true);
    let pixfmt = if a.get("pixfmt") == Some("bgra") { capture::PIX_BGRA } else { PIX_420V };
    let exclude = match a.get("exclude").unwrap_or("windows") {
        "none" => Exclude::None,
        "app" => Exclude::SelfApp,
        _ => Exclude::SelfWindows,
    };
    let want_cam = !a.flag("no-camera");
    let want_mic = !a.flag("no-mic");
    let want_sys = !a.flag("no-sysaudio");
    let want_window = !a.flag("no-test-window");
    let want_activity = !a.flag("no-activity");
    let active_from = a.num("active-from", secs_total * 0.4);
    let active_to = a.num("active-to", secs_total * 0.8);
    let flash_at = a.num("flash-at", secs_total * 0.2);
    let self_sound_at = a.num("self-sound-at", secs_total * 0.6);

    let target_window = a.get("target") == Some("activity-window");
    let camera_mode = a.get("camera-mode").unwrap_or("auto").to_string();
    let start_delay = a.num("start-delay", 3.0);

    let _app = testwin::init_app(mtm);
    let scale = testwin::main_screen_scale(mtm);
    let test_win = if want_window { Some(TestWindow::new(mtm, 200.0, 200.0, 400.0, 300.0, (1.0, 0.0, 1.0))) } else { None };
    pump_for(Duration::from_millis(400));

    if !ensure_screen_access(Duration::from_secs(180)) {
        return Err(anyhow!("Screen Recording permission not granted"));
    }
    let t0 = host_now_secs() + start_delay;
    let mut child = None;
    if want_activity || target_window {
        let exe = std::env::current_exe()?;
        let mut cmd = Command::new(exe);
        cmd.args([
            "activity",
            "--secs",
            &format!("{}", secs_total + 1.0),
            "--active-from",
            &format!("{active_from}"),
            "--active-to",
            &format!("{active_to}"),
            "--flash-at",
            &format!("{flash_at}"),
            "--t0",
            &format!("{t0}"),
        ]);
        cmd.stdout(Stdio::piped());
        child = Some(cmd.spawn()?);
    }
    let child_pid = child.as_ref().map(|c| c.id() as i32);

    let mut content = capture::fetch_content().map_err(|e| anyhow!("SCShareableContent: {e}"))?;
    let display = capture::main_display(&content).ok_or_else(|| anyhow!("no display"))?;
    let fi = if target_window {
        let pid = child_pid.unwrap();
        let mut found = None;
        for _ in 0..40 {
            found = unsafe { content.windows() }
                .iter()
                .find(|w| unsafe { w.owningApplication().map(|a| a.processID() == pid).unwrap_or(false) && w.isOnScreen() });
            if found.is_some() {
                break;
            }
            pump_for(Duration::from_millis(100));
            content = capture::fetch_content().map_err(|e| anyhow!("SCShareableContent: {e}"))?;
        }
        let w = found.ok_or_else(|| anyhow!("activity window not found in SCShareableContent"))?;
        capture::window_filter(&w)?
    } else {
        match capture::build_filter(&content, &display, &exclude) {
            Ok(f) => f,
            Err(e) if matches!(exclude, Exclude::SelfApp) => {
                eprintln!("app-level exclusion unavailable ({e}); falling back to excluding this process's windows");
                capture::build_filter(&content, &display, &Exclude::SelfWindows)?
            }
            Err(e) => return Err(e),
        }
    };
    let (pw, ph) = fi.pixel_size;
    let bitrate = (pw * ph) as f64 * fps * bpp;
    eprintln!("capture {}x{} px (scale {}), {}", pw, ph, fi.point_pixel_scale, fi.excluded_desc);

    let cam_dev = if want_cam && camera_mode != "off" {
        if avcap::ensure_access(true, Duration::from_secs(180)) {
            let d = pick_camera(a.get("camera"));
            if d.is_none() {
                eprintln!("no camera found");
            }
            d
        } else {
            eprintln!("camera permission not granted; continuing without camera");
            None
        }
    } else {
        None
    };
    let mic_dev = if want_mic {
        if avcap::ensure_access(false, Duration::from_secs(180)) {
            unsafe { objc2_av_foundation::AVCaptureDevice::defaultDeviceWithMediaType(objc2_av_foundation::AVMediaTypeAudio.unwrap()) }
        } else {
            eprintln!("microphone permission not granted; continuing without mic");
            None
        }
    } else {
        None
    };

    let sc = ScreenCapture::new(
        &dir,
        &fi,
        &ScreenOpts { fps, pixel_format: pixfmt, queue_depth: qd, capture_audio: want_sys, bitrate, hevc, frag },
    )?;
    let cam = match &cam_dev {
        Some(d) => match AvCapture::camera(&dir, d, 30.0, frag) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("camera setup failed: {e}");
                None
            }
        },
        None => None,
    };
    let mic = match &mic_dev {
        Some(d) => match AvCapture::mic(&dir, d, frag) {
            Ok(m) => Some(m),
            Err(e) => {
                eprintln!("mic setup failed: {e}");
                None
            }
        },
        None => None,
    };

    let win_rect_pts = test_win.as_ref().map(|w| w.rect_top_left_points(mtm));
    let mut project = json!({
        "schema_version": 1,
        "permission_host": std::env::var("__CFBundleIdentifier").unwrap_or_else(|_| "unknown".into()),
        "status": "recording",
        "clock": "mach host time (CMClockGetHostTimeClock), seconds",
        "display": {
            "id": unsafe { display.displayID() },
            "points": [unsafe { display.width() }, unsafe { display.height() }],
            "pixels": [pw, ph],
            "point_pixel_scale": fi.point_pixel_scale,
            "nsscreen_backing_scale": scale,
        },
        "filter": fi.excluded_desc,
        "test_window": win_rect_pts.map(|r| json!({
            "rect_points_top_left": [r.0, r.1, r.2, r.3],
            "rect_pixels_top_left": [r.0 * fi.point_pixel_scale, r.1 * fi.point_pixel_scale, r.2 * fi.point_pixel_scale, r.3 * fi.point_pixel_scale],
            "color_srgb": [255, 0, 255],
        })),
        "config": {
            "fps": fps, "pixel_format": fourcc(pixfmt), "queue_depth": qd, "bitrate": bitrate, "bpp": bpp,
            "codec": if hevc {"hevc"} else {"h264"}, "movie_fragment_interval_s": frag,
            "shows_cursor": true, "excludes_current_process_audio": want_sys,
        },
        "pid": std::process::id(),
        "t0_host_s": t0,
        "target": if target_window { "activity-window (desktop independent window filter)" } else { "main display" },
    });
    write_json(&dir.join("project.json"), &project)?;

    let cpu_names = ["replayd", "VTEncoderXPCService", "WindowServer", "screencaptureui", "coreaudiod"];
    if let Some(c) = &cam {
        c.start();
    }
    if let Some(m) = &mic {
        m.start();
    }
    let cpu0 = cpu_snapshot(&cpu_names);
    let wall0 = Instant::now();
    while host_now_secs() < t0 {
        pump(Duration::from_millis(2));
    }
    sc.start()?;
    let t_start = host_now_secs();
    project["recording_start_host_s"] = json!(t_start);
    write_json(&dir.join("project.json"), &project)?;
    eprintln!("recording {secs_total}s ...");

    let mut self_sound_host = None;
    let mut _player = None;
    let mut rss_max = 0u64;
    let mut sync_probes = vec![];
    while host_now_secs() < t_start + secs_total {
        pump(Duration::from_millis(5));
        if self_sound_host.is_none() && host_now_secs() >= t_start + self_sound_at {
            self_sound_host = Some(host_now_secs());
            _player = testwin::play_sound_in_process("Ping");
        }
        rss_max = rss_max.max(self_rss());
        if sync_probes.len() < 3 && host_now_secs() > t_start + 1.0 + sync_probes.len() as f64 {
            let mut p = json!({});
            if let Some((c, h)) = sc.sync_clock_vs_host() {
                p["scstream_sync_clock_minus_host_ms"] = json!((c - h) * 1e3);
            }
            if let Some((c, h)) = cam.as_ref().and_then(|c| c.sync_clock_vs_host()) {
                p["camera_session_clock_minus_host_ms"] = json!((c - h) * 1e3);
            }
            if let Some((c, h)) = mic.as_ref().and_then(|c| c.sync_clock_vs_host()) {
                p["mic_session_clock_minus_host_ms"] = json!((c - h) * 1e3);
            }
            sync_probes.push(p);
        }
    }
    let t_stop = host_now_secs();
    let wall = wall0.elapsed().as_secs_f64();
    let cpu1 = cpu_snapshot(&cpu_names);
    sc.stop()?;
    if let Some(c) = &cam {
        c.stop();
    }
    if let Some(m) = &mic {
        m.stop();
    }

    let ext_pts = t_stop - 1.0 / fps;
    let extended = capture::extend_screen_to(&sc.shared, cmtime_secs(ext_pts));
    let mut tracks = vec![];
    let mut errors = vec![];
    {
        let mut s = sc.shared.screen.lock().unwrap();
        if let Some(w) = s.writer.as_mut() {
            if let Err(e) = w.finish(Some(cmtime_secs(t_stop))) {
                errors.push(e.to_string());
            }
            let mut v = w.summary();
            v["offset_from_start_s"] = json!(w.first_pts.map(|p| p - t_start));
            v["end_host_s"] = json!(t_stop);
            v["pixel_format"] = json!(s.pix_fmt.map(fourcc));
            v["dims"] = json!(s.dims);
            tracks.push(v);
        }
        if let Some(e) = &s.writer_error {
            errors.push(format!("screen writer: {e}"));
        }
    }
    {
        let mut st = sc.shared.sys.lock().unwrap();
        if let Some(w) = st.writer.as_mut() {
            if let Err(e) = w.finish(None) {
                errors.push(e.to_string());
            }
            let mut v = w.summary();
            v["offset_from_start_s"] = json!(w.first_pts.map(|p| p - t_start));
            tracks.push(v);
        }
        if let Some(e) = &st.writer_error {
            errors.push(format!("system audio writer: {e}"));
        }
    }
    for c in [&cam, &mic].into_iter().flatten() {
        let mut t = c.shared.track.lock().unwrap();
        if let Some(w) = t.writer.as_mut() {
            if let Err(e) = w.finish(None) {
                errors.push(e.to_string());
            }
            let mut v = w.summary();
            v["offset_from_start_s"] = json!(w.first_pts.map(|p| p - t_start));
            v["device"] = json!(c.device_name);
            v["device_type"] = json!(c.device_type);
            v["format"] = json!(c.format);
            v["capture_dropped"] = json!(t.dropped_by_capture);
            v["received"] = json!(t.received);
            tracks.push(v);
        }
        if let Some(e) = &t.writer_error {
            errors.push(format!("{} writer: {e}", t.kind));
        }
    }

    let mut events = vec![];
    if let Some(mut ch) = child {
        let _ = ch.wait();
        if let Some(out) = ch.stdout.take() {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                    events.push(v);
                }
            }
        }
    }
    if let Some(t) = self_sound_host {
        events.push(json!({"event": "self_sound_ping", "host_s": t, "note": "played by the recording process via in-process AVAudioPlayer; should be absent from system_audio"}));
    }

    let s = sc.shared.screen.lock().unwrap();
    let rel = |p: f64| p - t0;
    let pts_rel: Vec<f64> = s.complete_pts.iter().map(|p| rel(*p)).collect();
    let probe = &s.clock_probe;
    let lat: Vec<f64> = probe.iter().map(|(p, n, _)| (n - p) * 1e3).collect();
    let disp_vs_pts: Vec<f64> = probe.iter().filter_map(|(p, _, d)| d.map(|d| (d - p) * 1e3)).collect();
    let sys = sc.shared.sys.lock().unwrap();
    let sys_lat: Vec<f64> = sys.clock_probe.iter().map(|(p, n)| (n - p) * 1e3).collect();
    let mut clocks = json!({
        "screen_callback_minus_pts_ms": summarize(&lat),
        "screen_displaytime_minus_pts_ms": summarize(&disp_vs_pts),
        "system_audio_callback_minus_pts_ms": summarize(&sys_lat),
        "sync_clock_probes": sync_probes,
    });
    for c in [&cam, &mic].into_iter().flatten() {
        let t = c.shared.track.lock().unwrap();
        let l: Vec<f64> = t.clock_probe.iter().map(|(p, n)| (n - p) * 1e3).collect();
        clocks[format!("{}_callback_minus_pts_ms", t.kind)] = summarize(&l);
    }
    let cam_stats = cam.as_ref().map(|c| {
        let t = c.shared.track.lock().unwrap();
        let p: Vec<f64> = t.pts.iter().map(|x| rel(*x)).collect();
        json!({"all": stats_intervals(&p, 0.5, secs_total - 0.2), "late_or_missing_frames": late_frames(&p, 30.0, 0.5, secs_total - 0.2), "capture_dropped": t.dropped_by_capture})
    });
    let cpu: Vec<serde_json::Value> = cpu0
        .iter()
        .zip(cpu1.iter())
        .map(|(a, b)| json!({"process": a.0, "cpu_s": b.1 - a.1, "cpu_pct_of_one_core": (b.1 - a.1) / wall * 100.0}))
        .collect();
    let status_names = ["complete", "idle", "blank", "suspended", "started", "stopped", "?", "missing"];
    let mut status = serde_json::Map::new();
    for (i, n) in status_names.iter().enumerate() {
        if s.status_counts[i] > 0 {
            status.insert(n.to_string(), json!(s.status_counts[i]));
        }
    }
    let delivered = json!({
        "status_counts": status,
        "idle_phase_1": stats_intervals(&pts_rel, 0.5, active_from),
        "active_phase": stats_intervals(&pts_rel, active_from + 0.2, active_to),
        "idle_phase_2": stats_intervals(&pts_rel, active_to + 0.3, secs_total),
        "active_phase_late_or_missing_frames": late_frames(&pts_rel, fps, active_from + 0.2, active_to),
        "first_frame_after_start_ms": pts_rel.first().map(|p| p * 1e3),
        "last_real_frame_before_stop_ms": pts_rel.last().map(|p| (secs_total - p) * 1e3),
        "stop_extension_appended_gap_s": extended,
    });
    project["status"] = json!("finished");
    project["recording_stop_host_s"] = json!(t_stop);
    project["duration_s"] = json!(t_stop - t_start);
    project["tracks"] = json!(tracks);
    project["events"] = json!(events);
    project["clocks"] = clocks;
    project["screen_delivery"] = delivered;
    project["camera_delivery"] = json!(cam_stats);
    project["system_audio_buffers"] = json!(sys.buffers);
    project["cpu"] = json!(cpu);
    project["self_rss_max_mb"] = json!(rss_max as f64 / 1048576.0);
    project["errors"] = json!(errors);
    project["phases"] = json!({"active_from_s": active_from, "active_to_s": active_to, "flash_at_s": flash_at, "self_sound_at_s": self_sound_at});
    project["stream_stopped_with_error"] = json!(*sc.shared.stopped_with_error.lock().unwrap());
    write_json(&dir.join("project.json"), &project)?;
    println!("{}", serde_json::to_string_pretty(&project)?);
    drop(test_win);
    Ok(())
}

pub fn summarize(v: &[f64]) -> serde_json::Value {
    if v.is_empty() {
        return json!(null);
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    json!({"n": s.len(), "min": s[0], "median": s[s.len() / 2], "max": s[s.len() - 1]})
}

pub fn activity(a: &Args) -> Result<()> {
    let mtm = MainThreadMarker::new().unwrap();
    let _app = testwin::init_app(mtm);
    let secs_total = a.num("secs", 10.0);
    let af = a.num("active-from", 4.0);
    let at = a.num("active-to", 8.0);
    let flash = a.num("flash-at", 2.0);
    let t0: f64 = a.num("t0", host_now_secs());
    let w = TestWindow::new(mtm, 900.0, 200.0, 360.0, 240.0, (0.1, 0.1, 0.1));
    let r = w.rect_top_left_points(mtm);
    emit(json!({"event": "activity_window", "rect_points_top_left": [r.0, r.1, r.2, r.3], "pid": std::process::id()}));
    let mut flashed = false;
    let mut keep = None;
    let mut toggle = false;
    let mut last_toggle = 0.0;
    loop {
        let t = host_now_secs() - t0;
        if t > secs_total {
            break;
        }
        if !flashed && t >= flash {
            w.set_color((1.0, 1.0, 1.0));
            keep = testwin::play_sound_in_process("Glass");
            emit(json!({"event": "flash_and_glass_sound", "host_s": host_now_secs(), "note": "separate process: white flash + Glass sound (in-process AVAudioPlayer of that child), should be present in screen and system_audio"}));
            flashed = true;
            pump_for(Duration::from_millis(200));
            w.set_color((0.1, 0.1, 0.1));
            continue;
        }
        if t >= af && t < at {
            if t - last_toggle >= 0.008 {
                toggle = !toggle;
                last_toggle = t;
                w.set_color(if toggle { (0.0, 0.6, 0.2) } else { (0.0, 0.2, 0.6) });
                w.window.display();
            }
        }
        pump(Duration::from_millis(2));
    }
    drop(keep);
    Ok(())
}

pub fn killtest(a: &Args) -> Result<()> {
    let dir = PathBuf::from(a.get("out").ok_or_else(|| anyhow!("--out DIR"))?);
    let after = a.num("after", 6.0);
    let exe = std::env::current_exe()?;
    let mut extra: Vec<String> = vec!["record".into(), "--out".into(), dir.to_string_lossy().into(), "--secs".into(), "30".into()];
    for k in ["no-camera", "no-mic", "no-sysaudio", "no-activity"] {
        if a.flag(k) {
            extra.push(format!("--{k}"));
        }
    }
    let mut ch = Command::new(exe).args(&extra).stdout(Stdio::null()).spawn()?;
    let start = Instant::now();
    let pj = dir.join("project.json");
    let mut rec_start = None;
    while start.elapsed() < Duration::from_secs(60) {
        if let Ok(s) = std::fs::read_to_string(&pj) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                if let Some(t) = v["recording_start_host_s"].as_f64() {
                    rec_start = Some(t);
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let rs = rec_start.ok_or_else(|| anyhow!("recording never started"))?;
    while host_now_secs() < rs + after {
        std::thread::sleep(Duration::from_millis(5));
    }
    let kill_host = host_now_secs();
    unsafe { libc::kill(ch.id() as i32, libc::SIGKILL) };
    let st = ch.wait()?;
    let info = json!({"killed_at_host_s": kill_host, "killed_after_s": kill_host - rs, "exit_status": format!("{st:?}")});
    std::fs::write(dir.join("kill.json"), serde_json::to_vec_pretty(&info)?)?;
    println!("{info}");
    Ok(())
}

pub fn list(_a: &Args) -> Result<()> {
    let mtm = MainThreadMarker::new().unwrap();
    let _app = testwin::init_app(mtm);
    let mut out = json!({});
    out["screen_permission_preflight"] = json!(CGPreflightScreenCaptureAccess());
    out["camera_permission"] = json!(avcap::status_name(avcap::auth_status(true)));
    out["mic_permission"] = json!(avcap::status_name(avcap::auth_status(false)));
    match capture::fetch_content() {
        Ok(c) => unsafe {
            let displays: Vec<_> = c
                .displays()
                .iter()
                .map(|d| {
                    let f = capture::build_filter(&c, &d, &Exclude::None).ok();
                    json!({"id": d.displayID(), "points": [d.width(), d.height()], "frame": [d.frame().origin.x, d.frame().origin.y, d.frame().size.width, d.frame().size.height],
                           "point_pixel_scale": f.as_ref().map(|f| f.point_pixel_scale), "pixels": f.as_ref().map(|f| f.pixel_size)})
                })
                .collect();
            let apps: Vec<_> = c
                .applications()
                .iter()
                .map(|a| json!({"pid": a.processID(), "bundle_id": a.bundleIdentifier().to_string(), "name": a.applicationName().to_string()}))
                .collect();
            let wins: Vec<_> = c
                .windows()
                .iter()
                .filter(|w| w.isOnScreen())
                .map(|w| {
                    json!({"id": w.windowID(), "title": w.title().map(|t| t.to_string()), "layer": w.windowLayer(),
                       "app": w.owningApplication().map(|a| a.applicationName().to_string()),
                       "frame": [w.frame().origin.x, w.frame().origin.y, w.frame().size.width, w.frame().size.height]})
                })
                .collect();
            out["displays"] = json!(displays);
            out["applications_count"] = json!(apps.len());
            out["applications_sample"] = json!(apps.iter().take(15).collect::<Vec<_>>());
            out["on_screen_windows_count"] = json!(wins.len());
            out["on_screen_windows_sample"] = json!(wins.iter().take(15).collect::<Vec<_>>());
        },
        Err(e) => out["shareable_content_error"] = json!(e),
    }
    let cams: Vec<_> = avcap::discover(true)
        .iter()
        .map(|d| unsafe {
            let best = avcap::best_format(d, 1920, 1080, 30.0).map(|b| b.1);
            json!({"name": d.localizedName().to_string(), "type": d.deviceType().to_string(), "unique_id": d.uniqueID().to_string(),
                   "formats": d.formats().count(), "best_le_1080p30": best})
        })
        .collect();
    let mics: Vec<_> = avcap::discover(false)
        .iter()
        .map(|d| unsafe { json!({"name": d.localizedName().to_string(), "type": d.deviceType().to_string()}) })
        .collect();
    out["cameras"] = json!(cams);
    out["microphones"] = json!(mics);
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}


pub fn perms(a: &Args) -> Result<()> {
    let mtm = MainThreadMarker::new().unwrap();
    let _app = testwin::init_app(mtm);
    let t = Duration::from_secs(a.num("timeout", 180.0) as u64);
    let mut out = json!({});
    if !a.flag("skip-mic") {
        out["mic"] = json!(avcap::ensure_access(false, t));
    }
    if !a.flag("skip-screen") {
        out["screen"] = json!(ensure_screen_access(t));
    }
    if !a.flag("skip-camera") {
        out["camera"] = json!(avcap::ensure_access(true, t));
    }
    println!("{out}");
    Ok(())
}

fn pick_camera(name: Option<&str>) -> Option<objc2::rc::Retained<objc2_av_foundation::AVCaptureDevice>> {
    let devs = avcap::discover(true);
    match name.map(|s| s.to_lowercase()) {
        Some(p) => devs.iter().find(|d| unsafe { d.localizedName() }.to_string().to_lowercase().contains(&p)).cloned(),
        None => devs.iter().find(|d| unsafe { d.localizedName() }.to_string().contains("FaceTime")).cloned().or_else(|| devs.first().cloned()),
    }
}

fn emit(v: serde_json::Value) {
    use std::io::Write;
    let mut o = std::io::stdout();
    let _ = writeln!(o, "{v}");
    let _ = o.flush();
}
