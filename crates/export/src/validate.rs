//! ffprobe validation of an exported file.

use std::path::Path;
use std::process::{Command, Stdio};

use sc_geometry::SizePx;
use sc_helpers::HelperKind;
use sc_platform::{Error, Result};
use serde_json::Value;

/// One AAC frame at 48 kHz.
pub const AAC_FRAME_NS: i64 = 1024 * 1_000_000_000 / 48_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expect {
    pub size: SizePx,
    pub fps: u32,
    pub duration_ns: i64,
    pub audio: bool,
    pub vcodec: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProbeReport {
    pub vcodec: String,
    pub size: SizePx,
    pub fps: f64,
    pub duration_ns: i64,
    pub frames: Option<u64>,
    pub acodec: Option<String>,
    pub audio_duration_ns: Option<i64>,
    pub color_primaries: Option<String>,
    pub color_transfer: Option<String>,
    pub color_space: Option<String>,
    pub color_range: Option<String>,
}

fn ffprobe_json(file: &Path) -> Result<Value> {
    let ffprobe = sc_helpers::locate(HelperKind::Ffprobe)?;
    let out = Command::new(&ffprobe)
        .args([
            "-v",
            "error",
            "-count_packets",
            "-show_streams",
            "-show_format",
        ])
        .args(["-of", "json"])
        .arg(file)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Error::Other(format!("run {}: {e}", ffprobe.display())))?;
    if !out.status.success() {
        return Err(Error::Other(format!(
            "ffprobe {}: {}",
            file.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    serde_json::from_slice(&out.stdout)
        .map_err(|e| Error::Other(format!("ffprobe {}: bad json: {e}", file.display())))
}

fn text(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_owned)
}

fn secs_ns(v: Option<&Value>) -> Option<i64> {
    let s: f64 = v?.as_str()?.parse().ok()?;
    Some((s * 1e9).round() as i64)
}

fn rate(v: &Value, k: &str) -> Option<f64> {
    let (n, d) = v.get(k)?.as_str()?.split_once('/')?;
    let (n, d): (f64, f64) = (n.parse().ok()?, d.parse().ok()?);
    (d != 0.0).then_some(n / d)
}

fn first_of<'a>(streams: &'a [Value], kind: &str) -> Option<&'a Value> {
    streams
        .iter()
        .find(|s| s.get("codec_type").and_then(Value::as_str) == Some(kind))
}

/// Reads the first video and audio stream and checks codec, size, fps, frame count, duration
/// (±1 frame), audio presence and length (video ±1 AAC frame ±1 frame) and BT.709 limited tags.
/// Every failed check is listed in the error.
pub fn ffprobe_check(file: &Path, e: &Expect) -> Result<ProbeReport> {
    let j = ffprobe_json(file)?;
    let streams = j
        .get("streams")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let format_dur = secs_ns(j.get("format").and_then(|f| f.get("duration")));
    let v = first_of(&streams, "video")
        .ok_or_else(|| Error::Other(format!("{}: no video stream", file.display())))?;
    let a = first_of(&streams, "audio");
    let dim = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0) as u32;
    let r = ProbeReport {
        vcodec: text(v, "codec_name").unwrap_or_default(),
        size: SizePx {
            w: dim("width"),
            h: dim("height"),
        },
        fps: rate(v, "avg_frame_rate")
            .or_else(|| rate(v, "r_frame_rate"))
            .unwrap_or(0.0),
        duration_ns: secs_ns(v.get("duration")).or(format_dur).unwrap_or(-1),
        frames: v
            .get("nb_read_packets")
            .and_then(Value::as_str)
            .and_then(|s| s.parse().ok()),
        acodec: a.and_then(|a| text(a, "codec_name")),
        audio_duration_ns: a.and_then(|a| secs_ns(a.get("duration"))),
        color_primaries: text(v, "color_primaries"),
        color_transfer: text(v, "color_transfer"),
        color_space: text(v, "color_space"),
        color_range: text(v, "color_range"),
    };
    let mut bad = Vec::new();
    let frame_ns = 1_000_000_000 / i64::from(e.fps.max(1));
    if r.vcodec != e.vcodec {
        bad.push(format!("vcodec {} (want {})", r.vcodec, e.vcodec));
    }
    if r.size != e.size {
        bad.push(format!(
            "size {}x{} (want {}x{})",
            r.size.w, r.size.h, e.size.w, e.size.h
        ));
    }
    if (r.fps - f64::from(e.fps)).abs() > 0.01 {
        bad.push(format!("fps {:.3} (want {})", r.fps, e.fps));
    }
    if (r.duration_ns - e.duration_ns).abs() > frame_ns {
        bad.push(format!(
            "duration {} ns (want {} ± {frame_ns})",
            r.duration_ns, e.duration_ns
        ));
    }
    let want_frames = sc_decode::cfr_frame_times(e.duration_ns, e.fps).count() as u64;
    if let Some(n) = r.frames
        && n.abs_diff(want_frames) > 1
    {
        bad.push(format!("frames {n} (want {want_frames} ± 1)"));
    }
    match (e.audio, &r.acodec) {
        (true, None) => bad.push("no audio stream (want one)".into()),
        (false, Some(c)) => bad.push(format!("audio stream {c} (want none)")),
        _ => {}
    }
    if e.audio {
        match r.audio_duration_ns {
            Some(ad) if (ad - r.duration_ns).abs() <= AAC_FRAME_NS + frame_ns => {}
            other => bad.push(format!(
                "audio duration {other:?} ns vs video {} (want ± {})",
                r.duration_ns,
                AAC_FRAME_NS + frame_ns
            )),
        }
    }
    if matches!(e.vcodec.as_str(), "h264" | "hevc") {
        let tags = [
            &r.color_primaries,
            &r.color_transfer,
            &r.color_space,
            &r.color_range,
        ]
        .map(|t| t.as_deref().unwrap_or("-"));
        if tags != ["bt709", "bt709", "bt709", "tv"] {
            bad.push(format!(
                "colour tags {} (want bt709/bt709/bt709/tv)",
                tags.join("/")
            ));
        }
    }
    if !bad.is_empty() {
        return Err(Error::Other(format!(
            "{}: {}",
            file.display(),
            bad.join("; ")
        )));
    }
    Ok(r)
}
