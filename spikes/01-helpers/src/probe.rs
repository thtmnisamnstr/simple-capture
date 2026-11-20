use crate::frames::Synth;
use crate::jobs::Job;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Default)]
pub struct Validation {
    pub checks: Vec<Check>,
    pub fields: BTreeMap<String, String>,
}

impl Validation {
    pub fn ok(&self) -> bool {
        !self.checks.is_empty() && self.checks.iter().all(|c| c.ok)
    }
    fn add(&mut self, name: &str, ok: bool, detail: String) {
        self.checks.push(Check { name: name.into(), ok, detail });
    }
    pub fn failures(&self) -> Vec<String> {
        self.checks.iter().filter(|c| !c.ok).map(|c| format!("{}: {}", c.name, c.detail)).collect()
    }
}

fn flat(ffprobe: &Path, args: &[&str], file: &Path) -> Result<BTreeMap<String, String>, String> {
    let out = Command::new(ffprobe)
        .args(["-v", "error"])
        .args(args)
        .args(["-of", "flat"])
        .arg(file)
        .output()
        .map_err(|e| format!("ffprobe: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    let mut m = BTreeMap::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some((k, v)) = line.split_once('=') {
            m.insert(k.to_string(), v.trim_matches('"').to_string());
        }
    }
    Ok(m)
}

pub fn gif_loop_count(path: &Path) -> Option<u16> {
    let d = std::fs::read(path).ok()?;
    let pat = b"NETSCAPE2.0";
    let i = d.windows(pat.len()).position(|w| w == pat)?;
    let b = d.get(i + pat.len()..i + pat.len() + 4)?;
    (b[0] == 3 && b[1] == 1).then(|| u16::from_le_bytes([b[2], b[3]]))
}

fn decode_rgb_frame(ffmpeg: &Path, file: &Path, n: u64, w: usize, h: usize) -> Result<Vec<u8>, String> {
    let out = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(file)
        .args(["-vf", &format!("select=eq(n\\,{n})"), "-frames:v", "1", "-pix_fmt", "rgb24", "-f", "rawvideo", "pipe:1"])
        .output()
        .map_err(|e| format!("ffmpeg decode: {e}"))?;
    if out.stdout.len() != w * h * 3 {
        return Err(format!("decoded {} bytes, wanted {} ({})", out.stdout.len(), w * h * 3, String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(out.stdout)
}

pub fn color_error(ffmpeg: &Path, file: &Path, synth: &Synth, n: u64) -> Result<(u32, String), String> {
    let rgb = decode_rgb_frame(ffmpeg, file, n, synth.w, synth.h)?;
    let mut worst = 0u32;
    let mut parts = vec![];
    for (name, want, r) in synth.patch_rects() {
        let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
        let mut acc = [0u64; 3];
        let mut cnt = 0u64;
        for y in cy - 8..cy + 8 {
            for x in cx - 8..cx + 8 {
                let i = (y * synth.w + x) * 3;
                for c in 0..3 {
                    acc[c] += rgb[i + c] as u64;
                }
                cnt += 1;
            }
        }
        let got = acc.map(|v| (v as f64 / cnt as f64).round() as i32);
        let err = (0..3).map(|c| (got[c] - want[c] as i32).unsigned_abs()).max().unwrap();
        worst = worst.max(err);
        parts.push(format!("{name}:{:?}->{:?}", want, got));
    }
    Ok((worst, parts.join(" ")))
}

pub fn validate(ffprobe: &Path, ffmpeg_ref: &Path, job: &Job, file: &Path, frames: u64, synth: &Synth) -> Validation {
    let mut v = Validation::default();
    let entries = "stream=codec_name,codec_tag_string,width,height,r_frame_rate,avg_frame_rate,pix_fmt,color_range,color_space,color_transfer,color_primaries,nb_read_frames,duration";
    let m = match flat(ffprobe, &["-count_frames", "-select_streams", "v:0", "-show_entries", entries, "-show_entries", "format=duration,format_name,size,bit_rate"], file) {
        Ok(m) => m,
        Err(e) => {
            v.add("ffprobe", false, e);
            return v;
        }
    };
    let g = |k: &str| m.get(&format!("streams.stream.0.{k}")).cloned().unwrap_or_default();
    for k in ["codec_name", "codec_tag_string", "width", "height", "r_frame_rate", "avg_frame_rate", "pix_fmt", "color_range", "color_space", "color_transfer", "color_primaries", "nb_read_frames", "duration"] {
        v.fields.insert(k.into(), g(k));
    }
    for k in ["duration", "format_name", "size", "bit_rate"] {
        v.fields.insert(format!("format.{k}"), m.get(&format!("format.{k}")).cloned().unwrap_or_default());
    }
    let fps = job.fps as f64;
    let expected_s = frames as f64 / fps;
    v.add("codec", g("codec_name") == job.expect_vcodec, format!("{} (want {})", g("codec_name"), job.expect_vcodec));
    if let Some(tag) = job.expect_tag {
        v.add("tag", g("codec_tag_string") == tag, format!("{} (want {tag})", g("codec_tag_string")));
    }
    v.add("size", g("width") == job.w.to_string() && g("height") == job.h.to_string(), format!("{}x{} (want {}x{})", g("width"), g("height"), job.w, job.h));
    let nb: u64 = g("nb_read_frames").parse().unwrap_or(0);
    v.add("frames", nb == frames, format!("{nb} (want {frames})"));
    let dur = g("duration").parse::<f64>().ok().or_else(|| m.get("format.duration").and_then(|d| d.parse().ok())).unwrap_or(-1.0);
    v.add("duration", (dur - expected_s).abs() <= 1.0 / fps + 1e-6, format!("{dur:.4}s (want {expected_s:.4}s +- 1 frame)"));
    if job.is_gif() {
        match flat(ffprobe, &["-select_streams", "v:0", "-show_entries", "frame=duration_time"], file) {
            Ok(fm) => {
                let delays: Vec<f64> = fm.iter().filter(|(k, _)| k.ends_with("duration_time")).filter_map(|(_, v)| v.parse().ok()).collect();
                let cs: Vec<u32> = delays.iter().map(|d| (d * 100.0).round() as u32).collect();
                let lo = (100 / job.fps) as u32;
                let hi = lo + u32::from(100 % job.fps != 0);
                let bad = cs.iter().filter(|c| **c < lo || **c > hi).count();
                let total: f64 = delays.iter().sum();
                let mut hist = BTreeMap::new();
                for c in &cs {
                    *hist.entry(*c).or_insert(0) += 1;
                }
                v.fields.insert("gif_delays_cs".into(), format!("{hist:?}"));
                v.fields.insert("gif_total_s".into(), format!("{total:.3}"));
                v.add("gif_delays", bad == 0 && !cs.is_empty(), format!("{} frames, delays(cs) {hist:?}, total {total:.3}s", cs.len()));
                v.add("gif_total", (total - expected_s).abs() <= 1.0 / fps + 1e-6, format!("{total:.3}s (want {expected_s:.3}s)"));
            }
            Err(e) => v.add("gif_delays", false, e),
        }
        let lc = gif_loop_count(file);
        v.fields.insert("gif_loop".into(), format!("{lc:?}"));
        v.add("gif_loop", lc == Some(0), format!("{lc:?} (want Some(0) = forever)"));
    } else {
        v.add("pix_fmt", g("pix_fmt") == "yuv420p", g("pix_fmt"));
        let tags = [g("color_primaries"), g("color_transfer"), g("color_space"), g("color_range")];
        let matrix = if job.expect_vcodec == "vp8" { "smpte170m" } else { "bt709" };
        v.add("color_tags", tags == ["bt709", "bt709", matrix, "tv"], format!("{} (want bt709/bt709/{matrix}/tv)", tags.join("/")));
    }
    match color_error(ffmpeg_ref, file, synth, frames / 2) {
        Ok((worst, detail)) => {
            let tol = if job.is_gif() { 12 } else { 6 };
            v.fields.insert("color_max_err".into(), worst.to_string());
            v.add("color_patches", worst <= tol, format!("max |err| {worst} (tol {tol}) {detail}"));
        }
        Err(e) => v.add("color_patches", false, e),
    }
    if let Some(ac) = job.expect_acodec {
        match flat(ffprobe, &["-select_streams", "a:0", "-show_entries", "stream=codec_name,sample_rate,channels,duration"], file) {
            Ok(am) => {
                let a = |k: &str| am.get(&format!("streams.stream.0.{k}")).cloned().unwrap_or_default();
                v.fields.insert("audio".into(), format!("{} {}Hz {}ch {}s", a("codec_name"), a("sample_rate"), a("channels"), a("duration")));
                v.add("audio_codec", a("codec_name") == ac, format!("{} (want {ac})", a("codec_name")));
                let ad = a("duration").parse::<f64>().ok().or_else(|| m.get("format.duration").and_then(|d| d.parse().ok())).unwrap_or(-1.0);
                v.add("audio_duration", (ad - expected_s).abs() <= 1.0 / fps + 1e-6, format!("{ad:.4}s"));
            }
            Err(e) => v.add("audio", false, e),
        }
    }
    v
}
