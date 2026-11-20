use std::collections::BTreeMap;
use std::fmt::Write as _;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::detect::stats;
use super::{EventKind, TARGET_MS};

pub const SYNC_PAIRS: [&str; 5] = [
    "video→system_audio",
    "video→mic",
    "system_audio→mic",
    "camera→system_audio",
    "camera→mic",
];
pub const LATENCY_PAIRS: [&str; 4] = [
    "issue→video",
    "issue→system_audio",
    "issue→mic",
    "issue→camera",
];
pub const NO_COMP_MS: f64 = 10.0;
pub const MAX_SPREAD_MS: f64 = 25.0;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EventResult {
    pub kind: Option<EventKind>,
    pub label: String,
    pub issue_s: f64,
    pub segment: Option<String>,
    pub source: Option<String>,
    pub video_s: Option<f64>,
    pub video_step: Option<f64>,
    pub system_audio_s: Option<f64>,
    pub system_audio_snr: Option<f64>,
    pub mic_s: Option<f64>,
    pub mic_snr: Option<f64>,
    pub camera_s: Option<f64>,
    pub camera_step: Option<f64>,
    pub camera_noise_sd: Option<f64>,
    pub camera_clears_noise: Option<bool>,
    pub offsets_ms: BTreeMap<String, f64>,
    pub notes: Vec<String>,
}

impl EventResult {
    pub fn fill_offsets(&mut self) {
        let t = |name: &str| -> Option<f64> {
            match name {
                "issue" => Some(self.issue_s),
                "video" => self.video_s,
                "system_audio" => self.system_audio_s,
                "mic" => self.mic_s,
                "camera" => self.camera_s,
                _ => None,
            }
        };
        let mut m = BTreeMap::new();
        for pair in SYNC_PAIRS.iter().chain(LATENCY_PAIRS.iter()) {
            let Some((a, b)) = pair.split_once('→') else {
                continue;
            };
            if let (Some(ta), Some(tb)) = (t(a), t(b)) {
                m.insert((*pair).to_string(), ((tb - ta) * 1e4).round() / 10.0);
            }
        }
        self.offsets_ms = m;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PairSummary {
    pub pair: String,
    pub n: usize,
    pub median_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
    pub within_target: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Report {
    pub project: String,
    pub log: String,
    pub duration_s: f64,
    pub tracks: Vec<String>,
    pub events: Vec<EventResult>,
    pub summary: Vec<PairSummary>,
    pub mic: String,
    pub camera: String,
    pub recommendation: String,
    pub notes: Vec<String>,
}

pub fn summarize(events: &[EventResult]) -> Vec<PairSummary> {
    SYNC_PAIRS
        .iter()
        .chain(LATENCY_PAIRS.iter())
        .filter_map(|pair| {
            let v: Vec<f64> = events
                .iter()
                .filter_map(|e| e.offsets_ms.get(*pair).copied())
                .collect();
            let (median_ms, min_ms, max_ms) = stats(&v)?;
            Some(PairSummary {
                pair: (*pair).to_string(),
                n: v.len(),
                median_ms,
                min_ms,
                max_ms,
                within_target: v.iter().filter(|x| x.abs() <= TARGET_MS).count(),
            })
        })
        .collect()
}

pub fn recommendation(summary: &[PairSummary]) -> String {
    let get = |pair: &str| summary.iter().find(|s| s.pair == pair);
    let Some(s) = get("video→system_audio") else {
        return "no video→system_audio pairs measured: no recommendation".into();
    };
    if s.n < 3 {
        return format!(
            "only {} video→system_audio pairs: too few for a recommendation",
            s.n
        );
    }
    let spread = s.max_ms - s.min_ms;
    let target = if s.within_target == s.n {
        format!("all {} within ±{TARGET_MS} ms", s.n)
    } else {
        format!(
            "{} of {} outside ±{TARGET_MS} ms",
            s.n - s.within_target,
            s.n
        )
    };
    let mut why = format!(
        "median video→system_audio {:+.1} ms, spread {spread:.1} ms ({target})",
        s.median_ms
    );
    for (pair, what) in [
        ("issue→video", "the stimulus's display latency"),
        ("issue→system_audio", "system audio stamped at mix time"),
        ("system_audio→mic", "speaker output latency + air"),
    ] {
        if let Some(p) = get(pair) {
            let _ = write!(why, "; {pair} {:+.1} ms ({what}, n {})", p.median_ms, p.n);
        }
    }
    if s.within_target == s.n || s.median_ms.abs() <= NO_COMP_MS {
        format!("no fixed system-audio compensation: {why}")
    } else if spread <= MAX_SPREAD_MS {
        let dir = if s.median_ms > 0.0 {
            "earlier"
        } else {
            "later"
        };
        format!(
            "fixed system-audio compensation of {:.0} ms (shift system audio {dir}): {why}",
            s.median_ms.abs()
        )
    } else {
        format!("no fixed compensation, video→system_audio varies too much: {why}")
    }
}

pub fn mic_verdict(events: &[EventResult]) -> String {
    let heard = events.iter().filter(|e| e.mic_s.is_some()).count();
    match heard {
        0 => "the mic heard none of the beeps (speakers muted, headphones, or mic too far): \
              mic sync not measured; volume left unchanged"
            .into(),
        n if n < events.len() => format!("the mic heard {n} of {} beeps", events.len()),
        n => format!("the mic heard all {n} beeps"),
    }
}

pub fn camera_verdict(events: &[EventResult]) -> String {
    let full: Vec<&EventResult> = events
        .iter()
        .filter(|e| e.kind == Some(EventKind::Fullscreen))
        .collect();
    if full.is_empty() {
        return "no full-screen events".into();
    }
    let parts: Vec<String> = full
        .iter()
        .map(
            |e| match (e.camera_step, e.camera_noise_sd, e.camera_clears_noise) {
                (Some(st), Some(sd), Some(c)) => format!(
                    "{}: step {st:.1} levels vs noise σ {sd:.2} ({})",
                    e.label,
                    if c {
                        "clears the noise floor"
                    } else {
                        "below the noise floor"
                    }
                ),
                _ => format!("{}: no camera track or no data", e.label),
            },
        )
        .collect();
    parts.join("; ")
}

fn ms(v: Option<&f64>) -> String {
    v.map_or("-".into(), |x| format!("{x:+.1}"))
}

pub fn render_text(r: &Report) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "sync report: {}", r.project);
    let _ = writeln!(
        s,
        "{:<16} {:>7} {:<8} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9}",
        "event", "at s", "source", "v→sys", "v→mic", "sys→mic", "cam→sys", "cam→mic", "iss→v"
    );
    for e in &r.events {
        let o = &e.offsets_ms;
        let _ = writeln!(
            s,
            "{:<16} {:>7.2} {:<8} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9}",
            e.label,
            e.issue_s,
            e.source.as_deref().unwrap_or("-"),
            ms(o.get("video→system_audio")),
            ms(o.get("video→mic")),
            ms(o.get("system_audio→mic")),
            ms(o.get("camera→system_audio")),
            ms(o.get("camera→mic")),
            ms(o.get("issue→video")),
        );
        for n in &e.notes {
            let _ = writeln!(s, "    note: {n}");
        }
    }
    for n in &r.notes {
        let _ = writeln!(s, "note: {n}");
    }
    s.push_str(&render_summary(&r.summary));
    let _ = writeln!(s, "mic: {}", r.mic);
    let _ = writeln!(s, "camera: {}", r.camera);
    let _ = writeln!(s, "recommendation: {}", r.recommendation);
    s
}

pub fn render_summary(summary: &[PairSummary]) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "{:<22} {:>3} {:>9} {:>9} {:>9}  within ±{TARGET_MS} ms",
        "pair (ms)", "n", "median", "min", "max"
    );
    for p in summary {
        let _ = writeln!(
            s,
            "{:<22} {:>3} {:>+9.1} {:>+9.1} {:>+9.1}  {}/{}",
            p.pair, p.n, p.median_ms, p.min_ms, p.max_ms, p.within_target, p.n
        );
    }
    s
}

pub fn summarize_files(paths: &[&str]) -> Result<()> {
    if paths.is_empty() {
        bail!("usage: sync summarize REPORT.json...");
    }
    let mut all = Vec::new();
    for p in paths {
        let text = std::fs::read_to_string(p).with_context(|| format!("reading {p}"))?;
        let r: Report = serde_json::from_str(&text).with_context(|| format!("parsing {p}"))?;
        println!("run {p}:");
        print!("{}", render_summary(&r.summary));
        all.extend(r.events);
    }
    let summary = summarize(&all);
    println!("all {} runs ({} events):", paths.len(), all.len());
    print!("{}", render_summary(&summary));
    println!("mic: {}", mic_verdict(&all));
    println!("camera: {}", camera_verdict(&all));
    println!("recommendation: {}", recommendation(&summary));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(video: f64, sys: f64, mic: Option<f64>) -> EventResult {
        let mut e = EventResult {
            kind: Some(EventKind::Flash),
            label: "start".into(),
            issue_s: video - 0.03,
            video_s: Some(video),
            system_audio_s: Some(sys),
            mic_s: mic,
            ..EventResult::default()
        };
        e.fill_offsets();
        e
    }

    #[test]
    fn offsets_are_b_minus_a_in_ms() {
        let e = ev(2.0, 2.045, Some(2.06));
        assert_eq!(e.offsets_ms["video→system_audio"], 45.0);
        assert_eq!(e.offsets_ms["system_audio→mic"], 15.0);
        assert_eq!(e.offsets_ms["issue→video"], 30.0);
        assert!(!e.offsets_ms.contains_key("camera→mic"));
    }

    #[test]
    fn recommendation_cases() {
        let small: Vec<_> = [0.004, -0.002, 0.006]
            .iter()
            .map(|d| ev(1.0, 1.0 + d, None))
            .collect();
        assert!(recommendation(&summarize(&small)).starts_with("no fixed system-audio"));
        let inside: Vec<_> = [-0.011, -0.031, -0.02]
            .iter()
            .map(|d| ev(1.0, 1.0 + d, None))
            .collect();
        let r = recommendation(&summarize(&inside));
        assert!(r.starts_with("no fixed system-audio"), "{r}");
        assert!(r.contains("issue→video +30.0 ms"), "{r}");
        let fixed: Vec<_> = [0.045, 0.05, 0.04, 0.055]
            .iter()
            .map(|d| ev(1.0, 1.0 + d, None))
            .collect();
        let r = recommendation(&summarize(&fixed));
        assert!(
            r.starts_with("fixed system-audio compensation of 48 ms (shift system audio earlier)"),
            "{r}"
        );
        assert!(r.contains("4 of 4 outside"), "{r}");
        let wild: Vec<_> = [0.0, 0.08, 0.03]
            .iter()
            .map(|d| ev(1.0, 1.0 + d, None))
            .collect();
        assert!(recommendation(&summarize(&wild)).contains("varies too much"));
        assert!(recommendation(&summarize(&small[..2])).contains("too few"));
    }

    #[test]
    fn verdicts_and_text() {
        let evs = vec![ev(1.0, 1.04, None), ev(2.0, 2.04, Some(2.05))];
        assert_eq!(mic_verdict(&evs), "the mic heard 1 of 2 beeps");
        assert!(mic_verdict(&evs[..1]).contains("none"));
        let mut cam = ev(5.0, 5.04, None);
        cam.kind = Some(EventKind::Fullscreen);
        cam.camera_step = Some(0.8);
        cam.camera_noise_sd = Some(0.5);
        cam.camera_clears_noise = Some(false);
        assert!(camera_verdict(&[cam]).contains("below the noise floor"));
        let summary = summarize(&evs);
        let r = Report {
            project: "p".into(),
            summary: summary.clone(),
            events: evs,
            ..Report::default()
        };
        let text = render_text(&r);
        assert!(text.contains("video→system_audio"), "{text}");
        assert_eq!(summary[0].within_target, 0);
    }
}
