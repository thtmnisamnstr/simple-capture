use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use sc_core::project::{ProjectV1, SourceDescriptor, Track, TrackKind};
use sc_decode::TrackAudio;
use sc_platform::{Decoder, FrameBuffer};

use super::detect::{
    CAMERA_MIN_STEP, CAMERA_SIGMAS, FLASH_MIN_STEP, Found, GOERTZEL_HOP_S, GOERTZEL_WIN_S, TONE_HZ,
    goertzel_series, step_onset, tone_onset,
};
use super::report::{
    EventResult, Report, camera_verdict, mic_verdict, recommendation, render_text, summarize,
};
use super::{EventKind, FlashLog, Opts};

pub const AUDIO_RATE: u32 = 48_000;
const S: i64 = 1_000_000_000;
const MS: i64 = 1_000_000;

pub fn run(o: &Opts) -> Result<()> {
    let pos = o.positional();
    let Some(dir) = pos.first() else {
        bail!("usage: sync analyze PROJECT --log LOG.json [--json OUT.json]");
    };
    let log_path = o
        .get("log")
        .ok_or_else(|| anyhow!("--log LOG.json is required"))?;
    let text = std::fs::read_to_string(log_path).with_context(|| format!("reading {log_path}"))?;
    let log: FlashLog =
        serde_json::from_str(&text).with_context(|| format!("parsing {log_path}"))?;
    let dir = PathBuf::from(dir);
    let project = load(&dir)?;
    let mut r = analyze(&dir, &project, &log, sc_capture_macos::decoder())?;
    r.log = log_path.to_string();
    print!("{}", render_text(&r));
    if let Some(out) = o.get("json") {
        std::fs::write(out, serde_json::to_string_pretty(&r)?)
            .with_context(|| format!("writing {out}"))?;
    }
    Ok(())
}

pub fn load(dir: &Path) -> Result<ProjectV1> {
    ProjectV1::load(dir).map_err(|e| anyhow!("loading {}: {e}", dir.display()))
}

struct Seg {
    id: String,
    start: i64,
    end: Option<i64>,
    source: &'static str,
}

fn source_name(s: &SourceDescriptor) -> &'static str {
    match s {
        SourceDescriptor::Display { .. } => "display",
        SourceDescriptor::Window { .. } => "window",
        SourceDescriptor::Application { .. } => "app",
        _ => "region",
    }
}

type Rect = (usize, usize, usize, usize);
type Series = Vec<(i64, f64)>;

fn inset(x: f64, y: f64, w: f64, h: f64) -> Rect {
    let (dx, dy) = (w * 0.1, h * 0.1);
    (
        (x + dx).max(0.0) as usize,
        (y + dy).max(0.0) as usize,
        (w - 2.0 * dx).max(1.0) as usize,
        (h - 2.0 * dy).max(1.0) as usize,
    )
}

pub fn mean_luma(f: &FrameBuffer, rect: Option<Rect>, step: usize) -> Result<f64> {
    let mut out = 0.0;
    f.0.with_nv12(&mut |v| {
        let (fw, fh) = (v.size.w as usize, v.size.h as usize);
        let (x, y, w, h) = rect.unwrap_or((0, 0, fw, fh));
        let (x1, y1) = ((x + w).min(fw), (y + h).min(fh));
        let (mut sum, mut n) = (0u64, 0u64);
        for row in (y.min(y1)..y1).step_by(step) {
            let line = &v.y[row * v.y_stride..];
            for col in (x.min(x1)..x1).step_by(step) {
                sum += u64::from(line[col]);
                n += 1;
            }
        }
        out = if n > 0 { sum as f64 / n as f64 } else { 0.0 };
    })
    .map_err(|e| anyhow!("reading frame: {e}"))?;
    Ok(out)
}

pub fn luma_sample(f: &FrameBuffer, step: usize) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    f.0.with_nv12(&mut |v| {
        for row in (0..v.size.h as usize).step_by(step) {
            let line = &v.y[row * v.y_stride..];
            out.extend((0..v.size.w as usize).step_by(step).map(|c| line[c]));
        }
    })
    .map_err(|e| anyhow!("reading frame: {e}"))?;
    Ok(out)
}

pub fn read_audio(
    project: &ProjectV1,
    dir: &Path,
    track: &Track,
    dec: Arc<dyn Decoder>,
    duration_ns: i64,
) -> Result<Vec<f32>> {
    let mut a = TrackAudio::open(project, dir, &track.id, dec, AUDIO_RATE, 1)
        .map_err(|e| anyhow!("{e}"))?;
    let total =
        usize::try_from(duration_ns.max(0) as i128 * i128::from(AUDIO_RATE) / i128::from(S))?;
    let mut out = vec![0f32; total];
    let mut at = 0;
    while at < total {
        let end = (at + AUDIO_RATE as usize).min(total);
        a.read(&mut out[at..end]).map_err(|e| anyhow!("{e}"))?;
        at = end;
    }
    Ok(out)
}

pub fn camera_track(project: &ProjectV1) -> Option<&Track> {
    project
        .recording
        .tracks
        .iter()
        .find(|t| t.kind == TrackKind::Camera)
}

pub fn each_frame(
    dec: &Arc<dyn Decoder>,
    path: &Path,
    mut f: impl FnMut(i64, &FrameBuffer) -> Result<bool>,
) -> Result<()> {
    let mut rd = dec
        .open_video(path)
        .map_err(|e| anyhow!("opening {}: {e}", path.display()))?;
    while let Some(fr) = rd
        .next_frame()
        .map_err(|e| anyhow!("decoding {}: {e}", path.display()))?
    {
        if !f(fr.file_ns, &fr.frame)? {
            break;
        }
    }
    Ok(())
}

fn screen_series(
    dir: &Path,
    project: &ProjectV1,
    log: &FlashLog,
    dec: &Arc<dyn Decoder>,
) -> Result<(Series, Vec<Seg>)> {
    let mut segs: Vec<_> = project.recording.screen.segments.iter().collect();
    segs.sort_by_key(|s| s.start_rec_ns);
    let mut series = Vec::new();
    let mut out = Vec::new();
    for s in segs {
        let track = project
            .recording
            .tracks
            .iter()
            .find(|t| t.id == s.track_id)
            .ok_or_else(|| anyhow!("segment {}: track {} missing", s.id, s.track_id))?;
        let c = s.content_px;
        let rect = match s.source {
            SourceDescriptor::Window { .. } => inset(
                f64::from(c.x),
                f64::from(c.y),
                f64::from(c.w),
                f64::from(c.h),
            ),
            _ => {
                let k = f64::from(c.w) / log.screen_pt[0];
                let [x, y, w, h] = log.rect_pt;
                inset(f64::from(c.x) + x * k, f64::from(c.y) + y * k, w * k, h * k)
            }
        };
        let end = s.end_rec_ns;
        each_frame(dec, &dir.join(&track.file), |file_ns, fb| {
            let rec = s.start_rec_ns + file_ns;
            if end.is_some_and(|e| rec >= e) {
                return Ok(false);
            }
            series.push((rec, mean_luma(fb, Some(rect), 4)?));
            Ok(true)
        })?;
        out.push(Seg {
            id: s.id.clone(),
            start: s.start_rec_ns,
            end,
            source: source_name(&s.source),
        });
    }
    series.sort_by_key(|p| p.0);
    Ok((series, out))
}

fn camera_series(dir: &Path, track: &Track, dec: &Arc<dyn Decoder>) -> Result<Series> {
    let mut series = Vec::new();
    each_frame(dec, &dir.join(&track.file), |file_ns, fb| {
        series.push((
            file_ns + track.first_sample_offset_ns,
            mean_luma(fb, None, 8)?,
        ));
        Ok(true)
    })?;
    Ok(series)
}

struct Tone {
    series: Vec<f64>,
}

impl Tone {
    fn new(x: &[f32]) -> Tone {
        let win = (GOERTZEL_WIN_S * f64::from(AUDIO_RATE)) as usize;
        let hop = (GOERTZEL_HOP_S * f64::from(AUDIO_RATE)) as usize;
        Tone {
            series: goertzel_series(x, AUDIO_RATE, TONE_HZ, win, hop),
        }
    }

    fn onset(&self, e: i64) -> Found<super::detect::ToneOnset> {
        tone_onset(
            &self.series,
            (GOERTZEL_HOP_S * 1e9) as i64,
            (GOERTZEL_WIN_S * 1e9) as i64,
            e - S,
            e - 300 * MS,
            e + 800 * MS,
        )
    }
}

fn db(peak: f64, noise: f64) -> f64 {
    (20.0 * (peak / noise.max(1e-7)).log10() * 10.0).round() / 10.0
}

fn secs(ns: i64) -> f64 {
    ns as f64 / 1e9
}

pub fn analyze(
    dir: &Path,
    project: &ProjectV1,
    log: &FlashLog,
    dec: Arc<dyn Decoder>,
) -> Result<Report> {
    let start = project.recording.start_host_ns;
    let duration = project
        .duration_ns()
        .ok_or_else(|| anyhow!("project has no duration (not finalized?)"))?;
    let mut notes = Vec::new();
    let (screen, segs) = screen_series(dir, project, log, &dec)?;
    let audio = |kind: TrackKind, notes: &mut Vec<String>| -> Option<Tone> {
        let t = project.recording.tracks.iter().find(|t| t.kind == kind)?;
        match read_audio(project, dir, t, dec.clone(), duration) {
            Ok(x) => Some(Tone::new(&x)),
            Err(e) => {
                notes.push(format!("{kind:?} track {}: {e:#}", t.id));
                None
            }
        }
    };
    let sys = audio(TrackKind::SystemAudio, &mut notes);
    let mic = audio(TrackKind::Mic, &mut notes);
    let cam = match camera_track(project) {
        Some(t) => match camera_series(dir, t, &dec) {
            Ok(s) => Some(s),
            Err(e) => {
                notes.push(format!("camera track {}: {e:#}", t.id));
                None
            }
        },
        None => None,
    };

    let mut events = Vec::new();
    for ie in &log.events {
        let e = ie.host_ns - start;
        let mut r = EventResult {
            kind: Some(ie.kind),
            label: ie.label.clone(),
            issue_s: secs(e),
            ..EventResult::default()
        };
        if let Some(s) = segs
            .iter()
            .rev()
            .find(|s| s.start <= e && s.end.is_none_or(|x| e < x))
        {
            r.segment = Some(s.id.clone());
            r.source = Some(s.source.to_string());
        }
        match step_onset(
            &screen,
            e - 350 * MS,
            e - 300 * MS,
            e + 800 * MS,
            FLASH_MIN_STEP,
            0.0,
        ) {
            Found::Yes(o) => {
                r.video_s = Some(secs(o.t_ns));
                r.video_step = Some(o.step());
            }
            Found::No { peak, .. } => r
                .notes
                .push(format!("no flash in the screen video (step {peak:.1})")),
            Found::NoData => r.notes.push("no screen frames near the event".into()),
        }
        for (name, tone, s_out, snr_out) in [
            (
                "system audio",
                &sys,
                &mut r.system_audio_s,
                &mut r.system_audio_snr,
            ),
            ("mic", &mic, &mut r.mic_s, &mut r.mic_snr),
        ] {
            let Some(tone) = tone else { continue };
            match tone.onset(e) {
                Found::Yes(o) => {
                    *s_out = Some(secs(o.t_ns));
                    *snr_out = Some(db(o.peak, o.noise));
                }
                Found::No { peak, noise } => r.notes.push(format!(
                    "{name}: no beep (peak {peak:.5}, noise {noise:.5}, {:.1} dB)",
                    db(peak, noise)
                )),
                Found::NoData => r.notes.push(format!("{name}: no audio near the event")),
            }
        }
        if ie.kind == EventKind::Fullscreen
            && let Some(cam) = &cam
        {
            match step_onset(
                cam,
                e - S,
                e - 200 * MS,
                e + S,
                CAMERA_MIN_STEP,
                CAMERA_SIGMAS,
            ) {
                Found::Yes(o) => {
                    r.camera_s = Some(secs(o.t_ns));
                    r.camera_step = Some(o.step());
                    r.camera_noise_sd = Some(o.noise_sd);
                    r.camera_clears_noise = Some(true);
                }
                Found::No { peak, noise } => {
                    r.camera_step = Some(peak);
                    r.camera_noise_sd = Some(noise);
                    r.camera_clears_noise = Some(false);
                    r.notes.push(format!(
                        "camera step {peak:.2} levels is below the noise floor (σ {noise:.2})"
                    ));
                }
                Found::NoData => r.notes.push("camera: no frames near the event".into()),
            }
        }
        r.fill_offsets();
        events.push(r);
    }
    let summary = summarize(&events);
    let mut tracks: Vec<String> = project
        .recording
        .tracks
        .iter()
        .map(|t| format!("{:?}:{}", t.kind, t.id))
        .collect();
    tracks.sort();
    Ok(Report {
        project: dir.display().to_string(),
        log: String::new(),
        duration_s: secs(duration),
        tracks,
        mic: if mic.is_some() {
            mic_verdict(&events)
        } else {
            "no mic track".into()
        },
        camera: if cam.is_some() {
            camera_verdict(&events)
        } else {
            "no camera track".into()
        },
        recommendation: recommendation(&summary),
        summary,
        events,
        notes,
    })
}
