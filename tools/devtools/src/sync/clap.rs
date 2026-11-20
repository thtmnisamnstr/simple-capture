use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use sc_core::project::TrackKind;
use serde_json::json;

use super::Opts;
use super::analyze::{AUDIO_RATE, camera_track, each_frame, load, luma_sample, read_audio};
use super::detect::{loudest_transient, motion_energy, rms_blocks, stats};

const BLOCK: usize = 96;
const BLOCK_NS: i64 = 2_000_000;

fn dbfs(v: f64) -> f64 {
    (20.0 * v.max(1e-9).log10() * 10.0).round() / 10.0
}

pub fn run(o: &Opts) -> Result<()> {
    let pos = o.positional();
    let Some(dir) = pos.first() else {
        bail!("usage: sync clap PROJECT [--from S] [--to S] [--json OUT.json]");
    };
    let dir = PathBuf::from(dir);
    let project = load(&dir)?;
    let dec = sc_capture_macos::decoder();
    let duration = project
        .duration_ns()
        .ok_or_else(|| anyhow!("project has no duration (not finalized?)"))?;
    let mic = project
        .recording
        .tracks
        .iter()
        .find(|t| t.kind == TrackKind::Mic)
        .ok_or_else(|| anyhow!("no mic track"))?;
    let x = read_audio(&project, &dir, mic, dec.clone(), duration)?;
    let env = rms_blocks(&x, BLOCK);
    let block_at = |s: f64| (s * 1e9 / BLOCK_NS as f64).max(0.0) as usize;
    let from = o.num("from")?.map_or(0, block_at);
    let to = o.num("to")?.map_or(env.len(), block_at);
    let t = loudest_transient(&env, from, to, 50)
        .ok_or_else(|| anyhow!("no mic audio in the chosen range"))?;
    let clap_ns = t.onset_block as i64 * BLOCK_NS;
    let mut out = json!({
        "project": dir.display().to_string(),
        "mic_onset_s": clap_ns as f64 / 1e9,
        "mic_peak_dbfs": dbfs(t.peak),
        "mic_floor_dbfs": dbfs(t.floor),
        "audio_rate": AUDIO_RATE,
    });
    println!(
        "mic: loudest transient starts at {:.3} s (peak {:.1} dBFS, floor {:.1} dBFS)",
        clap_ns as f64 / 1e9,
        dbfs(t.peak),
        dbfs(t.floor)
    );

    if let Some(cam) = camera_track(&project) {
        let path = dir.join(&cam.file);
        let (lo, hi) = (clap_ns - 1_000_000_000, clap_ns + 1_000_000_000);
        let mut prev: Option<Vec<u8>> = None;
        let mut motion: Vec<(i64, i64, usize, f64)> = Vec::new();
        let mut index = 0usize;
        each_frame(&dec, &path, |file_ns, fb| {
            let rec = file_ns + cam.first_sample_offset_ns;
            index += 1;
            if rec < lo {
                return Ok(true);
            }
            if rec > hi {
                return Ok(false);
            }
            let cur = luma_sample(fb, 8)?;
            if let Some(p) = &prev {
                motion.push((rec, file_ns, index - 1, motion_energy(p, &cur)));
            }
            prev = Some(cur);
            Ok(true)
        })?;
        let near: Vec<_> = motion
            .iter()
            .filter(|m| (m.0 - clap_ns).abs() <= 500_000_000)
            .collect();
        let all: Vec<f64> = motion.iter().map(|m| m.3).collect();
        let median = stats(&all).map_or(0.0, |s| s.0);
        if let Some(best) = near.iter().max_by(|a, b| a.3.total_cmp(&b.3)) {
            let off_ms = ((clap_ns - best.0) as f64 / 1e5).round() / 10.0;
            println!(
                "camera: most motion at {:.3} s (frame #{} in {}, file time {:.3} s; motion {:.2} \
                 vs median {:.2})",
                best.0 as f64 / 1e9,
                best.2,
                cam.file,
                best.1 as f64 / 1e9,
                best.3,
                median
            );
            println!("camera→mic offset: {off_ms:+.1} ms (positive: the mic hears the clap later)");
            println!(
                "confirm by eye (MANUAL_TEST): open {} in QuickTime, step to {:.3} s and check \
                 that the hands meet in that frame",
                path.display(),
                best.1 as f64 / 1e9
            );
            out["camera_frame_s"] = json!(best.0 as f64 / 1e9);
            out["camera_frame_index"] = json!(best.2);
            out["camera_file"] = json!(cam.file);
            out["camera_file_time_s"] = json!(best.1 as f64 / 1e9);
            out["camera_motion"] = json!(best.3);
            out["camera_motion_median"] = json!(median);
            out["camera_to_mic_ms"] = json!(off_ms);
        } else {
            println!("camera: no frames within 0.5 s of the clap");
        }
    } else {
        println!("camera: no camera track");
    }
    if let Some(p) = o.get("json") {
        std::fs::write(p, serde_json::to_string_pretty(&out)?)
            .with_context(|| format!("writing {p}"))?;
    }
    Ok(())
}
