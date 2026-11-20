use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

pub const FRAME_MS_30: f64 = 1000.0 / 30.0;
pub const MAX_DROP_PCT: f64 = 0.5;
pub const MAX_RSS_GROWTH_MB: f64 = 50.0;
pub const DECISION_MIN_S: f64 = 29.5 * 60.0;
pub const STALL_S: f64 = 2.0;

pub fn linear_fit(pts: &[(f64, f64)]) -> Option<(f64, f64)> {
    let n = pts.len() as f64;
    if pts.len() < 2 {
        return None;
    }
    let mx = pts.iter().map(|p| p.0).sum::<f64>() / n;
    let my = pts.iter().map(|p| p.1).sum::<f64>() / n;
    let sxx: f64 = pts.iter().map(|p| (p.0 - mx).powi(2)).sum();
    if sxx <= f64::EPSILON {
        return None;
    }
    let sxy: f64 = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let slope = sxy / sxx;
    Some((slope, my - slope * mx))
}

/// Normalised so a full-block sine of amplitude A gives A².
pub fn goertzel(block: &[f32], rate: u32, hz: f64) -> f64 {
    let w = 2.0 * std::f64::consts::PI * hz / f64::from(rate);
    let c = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &x in block {
        let s0 = f64::from(x) + c * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    let p = s1 * s1 + s2 * s2 - c * s1 * s2;
    let n = block.len().max(1) as f64;
    4.0 * p / (n * n)
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Onset {
    pub t_ns: i64,
    pub strength: f64,
}

/// The onset is the centre of the first block whose amplitude reaches halfway from the noise
/// floor (median before the peak) to the peak: half the block then overlaps the tone.
pub fn tone_onset(
    samples: &[f32],
    rate: u32,
    t0_ns: i64,
    hz: f64,
    block: usize,
    hop: usize,
    min_ratio: f64,
) -> Option<Onset> {
    if samples.len() < block || block == 0 || hop == 0 {
        return None;
    }
    let amps: Vec<f64> = (0..=(samples.len() - block) / hop)
        .map(|i| goertzel(&samples[i * hop..i * hop + block], rate, hz).sqrt())
        .collect();
    let (peak_i, peak) = amps
        .iter()
        .copied()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    let mut floor: Vec<f64> = amps[..peak_i].to_vec();
    if floor.len() < 5 {
        return None;
    }
    floor.sort_by(f64::total_cmp);
    let noise = floor[floor.len() / 2].max(1e-9);
    if peak / noise < min_ratio {
        return None;
    }
    let half = noise + (peak - noise) / 2.0;
    let i = amps[..=peak_i].iter().position(|&a| a >= half)?;
    let centre = i * hop + block / 2;
    let t = t0_ns + (centre as i128 * 1_000_000_000 / i128::from(rate)) as i64;
    Some(Onset {
        t_ns: t,
        strength: peak / noise,
    })
}

pub fn luma_onset(frames: &[(i64, f64)], min_step: f64) -> Option<Onset> {
    let (peak_i, peak) = frames
        .iter()
        .map(|f| f.1)
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    let mut before: Vec<f64> = frames[..peak_i].iter().map(|f| f.1).collect();
    if before.len() < 3 {
        return None;
    }
    before.sort_by(f64::total_cmp);
    let base = before[before.len() / 2];
    if peak - base < min_step {
        return None;
    }
    let half = base + (peak - base) / 2.0;
    let i = frames[..=peak_i].iter().position(|f| f.1 >= half)?;
    Some(Onset {
        t_ns: frames[i].0,
        strength: peak - base,
    })
}

pub fn cpu_pct(prev_s: f64, now_s: f64, wall_s: f64) -> f64 {
    if wall_s <= 0.0 {
        return 0.0;
    }
    ((now_s - prev_s).max(0.0) / wall_s) * 100.0
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub t_s: f64,
    pub our_rss_mb: Option<f64>,
    pub our_cpu_pct: Option<f64>,
    pub replayd_cpu_pct: Option<f64>,
    pub vtenc_cpu_pct: Option<f64>,
    pub replayd_rss_mb: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RssTrend {
    pub from_s: f64,
    pub to_s: f64,
    pub start_mb: f64,
    pub end_mb: f64,
    pub growth_mb: f64,
    pub slope_mb_per_min: f64,
    pub peak_mb: f64,
}

/// Growth compares the means of the first and last three samples so one 5 s spike at either
/// end does not decide the threshold.
pub fn rss_trend(samples: &[Sample], from_s: f64) -> Option<RssTrend> {
    let pts: Vec<(f64, f64)> = samples
        .iter()
        .filter(|s| s.t_s >= from_s)
        .filter_map(|s| s.our_rss_mb.map(|r| (s.t_s, r)))
        .collect();
    if pts.len() < 2 {
        return None;
    }
    let k = pts.len().min(3);
    let mean = |s: &[(f64, f64)]| s.iter().map(|p| p.1).sum::<f64>() / s.len() as f64;
    let start_mb = mean(&pts[..k]);
    let end_mb = mean(&pts[pts.len() - k..]);
    let (slope, _) = linear_fit(&pts)?;
    Some(RssTrend {
        from_s: pts[0].0,
        to_s: pts[pts.len() - 1].0,
        start_mb,
        end_mb,
        growth_mb: end_mb - start_mb,
        slope_mb_per_min: slope * 60.0,
        peak_mb: pts.iter().map(|p| p.1).fold(f64::MIN, f64::max),
    })
}

/// `ticks` are 1 Hz (rec_ns, screen frames so far). Returns the longest run without a new frame
/// and the lowest rate over any 10 s window.
pub fn screen_delivery(ticks: &[(i64, u64)], skip_first_s: f64) -> (f64, Option<f64>) {
    let t: Vec<&(i64, u64)> = ticks
        .iter()
        .filter(|x| x.0 as f64 / 1e9 >= skip_first_s)
        .collect();
    let mut longest = 0.0f64;
    let mut since: Option<i64> = None;
    for w in t.windows(2) {
        if w[1].1 == w[0].1 {
            let s = *since.get_or_insert(w[0].0);
            longest = longest.max((w[1].0 - s) as f64 / 1e9);
        } else {
            since = None;
        }
    }
    let mut min_fps: Option<f64> = None;
    let mut j = 0;
    for i in 0..t.len() {
        while j < t.len() && t[j].0 - t[i].0 < 10_000_000_000 {
            j += 1;
        }
        if j >= t.len() {
            break;
        }
        let dt = (t[j].0 - t[i].0) as f64 / 1e9;
        let fps = t[j].1.saturating_sub(t[i].1) as f64 / dt;
        min_fps = Some(min_fps.map_or(fps, |m: f64| m.min(fps)));
    }
    (longest, min_fps)
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TrackRow {
    pub id: String,
    pub kind: String,
    pub source: String,
    pub frames: u64,
    pub dropped: u64,
    pub writer_not_ready: u64,
    pub gaps: u64,
    pub overlaps: Option<u64>,
    pub gap_total_ms: f64,
    pub duration_s: Option<f64>,
    pub drift_ms: Option<f64>,
    pub device: Option<String>,
}

impl TrackRow {
    pub fn dropped_pct(&self) -> f64 {
        let all = self.frames + self.dropped;
        if all == 0 {
            0.0
        } else {
            self.dropped as f64 * 100.0 / all as f64
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SyncRow {
    pub label: String,
    pub at_s: f64,
    pub video_latency_ms: Option<f64>,
    pub video_step: Option<f64>,
    /// (track kind, audio onset − screen onset in ms; positive = audio late)
    pub offsets_ms: Vec<(String, Option<f64>)>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProcRow {
    pub name: String,
    pub cpu_avg: Option<f64>,
    pub cpu_max: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub minutes_requested: f64,
    pub duration_s: f64,
    pub machine: String,
    pub os: String,
    pub display: String,
    pub fps_requested: u32,
    pub segments: Vec<String>,
    pub tracks: Vec<TrackRow>,
    pub screen_dropped_pct: f64,
    pub rss: Option<RssTrend>,
    pub procs: Vec<ProcRow>,
    pub stream_stopped: u64,
    pub source_lost: u64,
    pub switch_failed: u64,
    pub notices: Vec<String>,
    pub longest_stall_s: f64,
    pub min_screen_fps_10s: Option<f64>,
    pub sync: Vec<SyncRow>,
    pub exit_code: Option<i32>,
    pub stop_reason: Option<String>,
    pub kept: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    Pass,
    Fail,
    NotMeasured,
}

impl Verdict {
    fn text(self) -> &'static str {
        match self {
            Verdict::Pass => "PASS",
            Verdict::Fail => "FAIL",
            Verdict::NotMeasured => "n/a",
        }
    }
}

impl Report {
    pub fn decision_run(&self) -> bool {
        self.duration_s >= DECISION_MIN_S
    }

    pub fn collapse(&self) -> bool {
        self.longest_stall_s >= STALL_S
            || self
                .min_screen_fps_10s
                .is_some_and(|f| f < f64::from(self.fps_requested) / 2.0)
    }

    /// The start offset holds output and capture latencies that are constant; what a long run
    /// can break is the change from the first flash to the last.
    pub fn sync_drift_ms(&self) -> Vec<(String, Option<f64>)> {
        let (Some(a), Some(b)) = (self.sync.first(), self.sync.last()) else {
            return Vec::new();
        };
        if self.sync.len() < 2 {
            return Vec::new();
        }
        a.offsets_ms
            .iter()
            .map(|(k, s)| {
                let e = b.offsets_ms.iter().find(|x| &x.0 == k).and_then(|x| x.1);
                (k.clone(), s.zip(e).map(|(s, e)| e - s))
            })
            .collect()
    }

    pub fn checks(&self) -> Vec<(String, String, String, Verdict)> {
        let mut v = Vec::new();
        let drops = self.screen_dropped_pct;
        v.push((
            "Screen drops".into(),
            format!("< {MAX_DROP_PCT} %"),
            format!("{drops:.3} %"),
            if drops < MAX_DROP_PCT {
                Verdict::Pass
            } else {
                Verdict::Fail
            },
        ));
        match &self.rss {
            Some(r) => v.push((
                "RSS growth from minute 2".into(),
                format!("≤ {MAX_RSS_GROWTH_MB} MB"),
                format!(
                    "{:+.1} MB ({:.1} → {:.1} MB, slope {:+.2} MB/min)",
                    r.growth_mb, r.start_mb, r.end_mb, r.slope_mb_per_min
                ),
                if r.growth_mb <= MAX_RSS_GROWTH_MB {
                    Verdict::Pass
                } else {
                    Verdict::Fail
                },
            )),
            None => v.push((
                "RSS growth from minute 2".into(),
                format!("≤ {MAX_RSS_GROWTH_MB} MB"),
                "run shorter than 2 min or RSS unreadable".into(),
                Verdict::NotMeasured,
            )),
        }
        v.push((
            "StreamStopped / source lost".into(),
            "0".into(),
            format!(
                "{} StreamStopped, {} source lost",
                self.stream_stopped, self.source_lost
            ),
            if self.stream_stopped == 0 && self.source_lost == 0 {
                Verdict::Pass
            } else {
                Verdict::Fail
            },
        ));
        let drift = self.sync_drift_ms();
        let end = self.sync.last().filter(|_| self.sync.len() >= 2);
        for (k, d) in &drift {
            let abs_end = end
                .and_then(|e| e.offsets_ms.iter().find(|x| &x.0 == k))
                .and_then(|x| x.1);
            v.push((
                format!("End sync, {k} vs screen"),
                format!("within one frame ({FRAME_MS_30:.1} ms) of the start offset"),
                match (d, abs_end) {
                    (Some(d), Some(e)) => format!("{d:+.1} ms change (end offset {e:+.1} ms)"),
                    _ => "flash or tone not detected".into(),
                },
                match d {
                    Some(d) if d.abs() <= FRAME_MS_30 => Verdict::Pass,
                    Some(_) => Verdict::Fail,
                    None => Verdict::NotMeasured,
                },
            ));
        }
        if drift.is_empty() {
            v.push((
                "End sync".into(),
                format!("within one frame ({FRAME_MS_30:.1} ms)"),
                "no start/end flash pair measured".into(),
                Verdict::NotMeasured,
            ));
        }
        let drifts: Vec<String> = self
            .tracks
            .iter()
            .filter(|t| t.kind == "mic" || t.kind == "system_audio")
            .map(|t| match t.drift_ms {
                Some(d) => format!("{} {d:+.1} ms", t.id),
                None => format!("{} n/a", t.id),
            })
            .collect();
        v.push((
            "Audio drift (count-based − host time)".into(),
            "reported".into(),
            if drifts.is_empty() {
                "no audio tracks".into()
            } else {
                drifts.join(", ")
            },
            if drifts.is_empty() {
                Verdict::NotMeasured
            } else {
                Verdict::Pass
            },
        ));
        v
    }

    pub fn all_pass(&self) -> bool {
        self.checks().iter().all(|c| c.3 != Verdict::Fail)
    }

    pub fn adr010(&self) -> String {
        let mut why = Vec::new();
        if self.stream_stopped > 0 {
            why.push(format!("{} StreamStopped", self.stream_stopped));
        }
        if self.longest_stall_s >= STALL_S {
            why.push(format!("a {:.1} s screen stall", self.longest_stall_s));
        }
        if let Some(f) = self
            .min_screen_fps_10s
            .filter(|f| *f < f64::from(self.fps_requested) / 2.0)
        {
            why.push(format!("delivery collapse to {f:.1} fps over 10 s"));
        }
        let rec = if why.is_empty() {
            "no copy: SCK delivered without stalls, StreamStopped or collapse".to_string()
        } else {
            format!("copy frames: {}", why.join(", "))
        };
        if self.decision_run() {
            rec
        } else {
            format!("{rec} (regression only: the run lasted under 30 min)")
        }
    }
}

fn opt(v: Option<f64>, digits: usize) -> String {
    v.map_or("n/a".into(), |x| format!("{x:.digits$}"))
}

/// Format 1. MANUAL_TEST's owner soak runs the same command, so both runs print this text.
pub fn summary_markdown(r: &Report) -> String {
    let mut s = String::new();
    let kind = if r.decision_run() {
        "ADR-010 decision run (≥ 30 min)"
    } else {
        "regression only (under 30 min)"
    };
    let _ = writeln!(s, "## Soak summary (format 1)\n");
    let _ = writeln!(
        s,
        "- Run: {:.1} min recorded of {} requested; {kind}",
        r.duration_s / 60.0,
        r.minutes_requested
    );
    let _ = writeln!(s, "- Machine: {}; {}", r.machine, r.os);
    let _ = writeln!(
        s,
        "- Display: {}; {} fps requested; stop {}; exit {}",
        r.display,
        r.fps_requested,
        r.stop_reason.as_deref().unwrap_or("?"),
        r.exit_code.map_or("?".into(), |c| c.to_string())
    );
    let _ = writeln!(s, "- Segments: {}", r.segments.join(" → "));
    let _ = writeln!(
        s,
        "- Overall: {}\n",
        if r.all_pass() { "PASS" } else { "FAIL" }
    );
    let _ = writeln!(s, "| Check | Threshold | Measured | Result |");
    let _ = writeln!(s, "|---|---|---|---|");
    for (name, th, m, v) in r.checks() {
        let _ = writeln!(s, "| {name} | {th} | {m} | {} |", v.text());
    }
    let _ = writeln!(s, "\n### Tracks\n");
    let _ = writeln!(
        s,
        "| Track | Kind | Source/device | Frames | Dropped | Dropped % | Writer not ready | Gaps | Overlaps | Gap total ms | Duration s | Drift ms |"
    );
    let _ = writeln!(s, "|---|---|---|---|---|---|---|---|---|---|---|---|");
    for t in &r.tracks {
        let _ = writeln!(
            s,
            "| {} | {} | {} | {} | {} | {:.3} | {} | {} | {} | {:.1} | {} | {} |",
            t.id,
            t.kind,
            if t.source.is_empty() {
                t.device.clone().unwrap_or_default()
            } else {
                t.source.clone()
            },
            t.frames,
            t.dropped,
            t.dropped_pct(),
            t.writer_not_ready,
            t.gaps,
            t.overlaps.map_or("n/a".into(), |o| o.to_string()),
            t.gap_total_ms,
            opt(t.duration_s, 1),
            opt(t.drift_ms, 1)
        );
    }
    let _ = writeln!(s, "\n### Sync (flash + 1 kHz beep)\n");
    let _ = writeln!(
        s,
        "| Flash | At s | Screen latency ms | Luma step | Offsets, audio − screen ms | Note |"
    );
    let _ = writeln!(s, "|---|---|---|---|---|---|");
    for y in &r.sync {
        let offs: Vec<String> = y
            .offsets_ms
            .iter()
            .map(|(k, v)| format!("{k} {}", v.map_or("n/a".into(), |x| format!("{x:+.1}"))))
            .collect();
        let _ = writeln!(
            s,
            "| {} | {:.1} | {} | {} | {} | {} |",
            y.label,
            y.at_s,
            opt(y.video_latency_ms, 1),
            opt(y.video_step, 0),
            offs.join(", "),
            y.note.as_deref().unwrap_or("")
        );
    }
    let _ = writeln!(s, "\n### Load (samples every 5 s)\n");
    let _ = writeln!(s, "| Process | CPU avg % | CPU max % |");
    let _ = writeln!(s, "|---|---|---|");
    for p in &r.procs {
        let _ = writeln!(
            s,
            "| {} | {} | {} |",
            p.name,
            opt(p.cpu_avg, 1),
            opt(p.cpu_max, 1)
        );
    }
    if let Some(t) = &r.rss {
        let _ = writeln!(
            s,
            "\nRSS (simple-capture): {:.1} MB at {:.0} s, {:.1} MB at {:.0} s, peak {:.1} MB, slope {:+.2} MB/min.",
            t.start_mb, t.from_s, t.end_mb, t.to_s, t.peak_mb, t.slope_mb_per_min
        );
    }
    let _ = writeln!(
        s,
        "\nScreen delivery: longest stall {:.1} s; lowest 10 s rate {} fps; collapse {}. Switch failures: {}.",
        r.longest_stall_s,
        opt(r.min_screen_fps_10s, 1),
        if r.collapse() { "yes" } else { "no" },
        r.switch_failed
    );
    if !r.notices.is_empty() {
        let _ = writeln!(s, "Notices: {}.", r.notices.join("; "));
    }
    let _ = writeln!(s, "\n**ADR-010 frame copy:** {}", r.adr010());
    if let Some(k) = &r.kept {
        let _ = writeln!(s, "\nRecording kept at {k}.");
    } else {
        let _ = writeln!(s, "\nRecording deleted after the summary.");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_recovers_a_line() {
        let pts: Vec<(f64, f64)> = (0..10).map(|i| (i as f64, 3.0 + 2.0 * i as f64)).collect();
        let (m, b) = linear_fit(&pts).expect("fit");
        assert!((m - 2.0).abs() < 1e-9 && (b - 3.0).abs() < 1e-9);
        assert!(linear_fit(&[(1.0, 1.0), (1.0, 2.0)]).is_none());
    }

    fn tone(rate: u32, len_s: f64, on_s: f64, dur_s: f64, noise: f32) -> Vec<f32> {
        let n = (len_s * f64::from(rate)) as usize;
        let mut seed = 12345u32;
        (0..n)
            .map(|i| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let r = ((seed >> 16) as f32 / 32768.0 - 1.0) * noise;
                let t = i as f64 / f64::from(rate);
                let v = if t >= on_s && t < on_s + dur_s {
                    (2.0 * std::f64::consts::PI * 1000.0 * t).sin() as f32 * 0.2
                } else {
                    0.0
                };
                v + r
            })
            .collect()
    }

    #[test]
    fn tone_onset_within_two_ms() {
        let x = tone(48_000, 2.0, 1.2345, 0.1, 0.02);
        let o = tone_onset(&x, 48_000, 1_000_000_000, 1000.0, 480, 96, 8.0).expect("onset");
        let want = 1_000_000_000 + 1_234_500_000;
        assert!((o.t_ns - want).abs() < 2_000_000, "{} vs {want}", o.t_ns);
        assert!(o.strength > 8.0);
    }

    #[test]
    fn no_tone_is_not_detected() {
        let x = tone(48_000, 2.0, 5.0, 0.1, 0.05);
        assert!(tone_onset(&x, 48_000, 0, 1000.0, 480, 96, 8.0).is_none());
    }

    #[test]
    fn luma_onset_picks_first_bright_frame() {
        let mut f: Vec<(i64, f64)> = (0..60).map(|i| (i * 33_333_333, 40.0)).collect();
        for x in f.iter_mut().skip(30).take(3) {
            x.1 = 230.0;
        }
        let o = luma_onset(&f, 60.0).expect("onset");
        assert_eq!(o.t_ns, 30 * 33_333_333);
        assert!(luma_onset(&f[..29], 60.0).is_none());
        let flat: Vec<(i64, f64)> = (0..60).map(|i| (i, 40.0 + (i % 3) as f64)).collect();
        assert!(luma_onset(&flat, 60.0).is_none());
    }

    #[test]
    fn rss_trend_smooths_ends() {
        let s: Vec<Sample> = (0..100)
            .map(|i| Sample {
                t_s: i as f64 * 5.0,
                our_rss_mb: Some(if i < 24 { 50.0 } else { 300.0 + 0.1 * i as f64 }),
                ..Sample::default()
            })
            .collect();
        let r = rss_trend(&s, 120.0).expect("trend");
        assert!((r.start_mb - 302.5).abs() < 0.01, "{}", r.start_mb);
        assert!((r.growth_mb - 7.3).abs() < 0.01, "{}", r.growth_mb);
        assert!((r.slope_mb_per_min - 1.2).abs() < 1e-6);
        assert!(rss_trend(&s, 1e9).is_none());
    }

    #[test]
    fn delivery_finds_stalls_and_collapse() {
        let mut ticks = Vec::new();
        let mut frames = 0u64;
        for i in 0..60i64 {
            if !(20..24).contains(&i) {
                frames += if (40..52).contains(&i) { 10 } else { 30 };
            }
            ticks.push((i * 1_000_000_000, frames));
        }
        let (stall, min_fps) = screen_delivery(&ticks, 2.0);
        assert!((stall - 4.0).abs() < 1e-9, "{stall}");
        assert!(min_fps.expect("fps") <= 10.0 + 1e-9);
    }

    #[test]
    fn cpu_pct_from_cumulative() {
        assert!((cpu_pct(1.0, 3.5, 5.0) - 50.0).abs() < 1e-9);
        assert_eq!(cpu_pct(2.0, 1.0, 5.0), 0.0);
        assert_eq!(cpu_pct(0.0, 1.0, 0.0), 0.0);
    }

    fn report() -> Report {
        Report {
            minutes_requested: 30.0,
            duration_s: 1800.0,
            fps_requested: 30,
            tracks: vec![
                TrackRow {
                    id: "screen-s0".into(),
                    kind: "screen".into(),
                    frames: 9990,
                    dropped: 10,
                    ..TrackRow::default()
                },
                TrackRow {
                    id: "mic".into(),
                    kind: "mic".into(),
                    drift_ms: Some(-0.4),
                    ..TrackRow::default()
                },
            ],
            screen_dropped_pct: 0.1,
            rss: rss_trend(
                &(0..50)
                    .map(|i| Sample {
                        t_s: 120.0 + i as f64 * 5.0,
                        our_rss_mb: Some(200.0),
                        ..Sample::default()
                    })
                    .collect::<Vec<_>>(),
                120.0,
            ),
            min_screen_fps_10s: Some(29.0),
            sync: vec![
                SyncRow {
                    label: "start".into(),
                    offsets_ms: vec![("mic".into(), Some(20.0))],
                    ..SyncRow::default()
                },
                SyncRow {
                    label: "end".into(),
                    offsets_ms: vec![("mic".into(), Some(30.0))],
                    ..SyncRow::default()
                },
            ],
            ..Report::default()
        }
    }

    #[test]
    fn thresholds_pass_and_fail() {
        let r = report();
        assert!(r.all_pass(), "{:?}", r.checks());
        assert!(r.adr010().starts_with("no copy"));
        assert!(!r.adr010().contains("regression only"));
        assert_eq!(r.sync_drift_ms(), vec![("mic".to_string(), Some(10.0))]);

        let mut bad = report();
        bad.screen_dropped_pct = 0.6;
        assert!(!bad.all_pass());
        let mut bad = report();
        bad.sync[1].offsets_ms[0].1 = Some(60.0);
        assert!(!bad.all_pass());
        let mut bad = report();
        bad.stream_stopped = 1;
        assert!(!bad.all_pass());
        assert!(bad.adr010().starts_with("copy frames"));
        let mut short = report();
        short.duration_s = 600.0;
        assert!(short.adr010().contains("regression only"));
        let mut slow = report();
        slow.longest_stall_s = 3.0;
        assert!(slow.collapse() && slow.adr010().starts_with("copy frames"));
    }

    #[test]
    fn summary_has_every_section() {
        let s = summary_markdown(&report());
        for want in [
            "## Soak summary (format 1)",
            "| Screen drops | < 0.5 % | 0.100 % | PASS |",
            "### Tracks",
            "### Sync",
            "### Load",
            "**ADR-010 frame copy:** no copy",
            "ADR-010 decision run",
        ] {
            assert!(s.contains(want), "missing {want:?} in\n{s}");
        }
    }
}
