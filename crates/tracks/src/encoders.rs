//! Which encoder the helper uses for raw tracks on this machine: the first hardware encoder that
//! opens, else x264. Probed once per process with a real encode of a test pattern.

use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

use sc_geometry::SizePx;
use sc_platform::VideoCodec;

pub const X264: &str = "libx264";

pub fn candidates(codec: VideoCodec) -> Vec<&'static str> {
    let hevc_first = |hevc: &'static [&'static str], h264: &'static [&'static str]| {
        let mut v: Vec<&'static str> = Vec::new();
        if codec == VideoCodec::Hevc {
            v.extend_from_slice(hevc);
        }
        v.extend_from_slice(h264);
        v.push(X264);
        v
    };
    if cfg!(target_os = "windows") {
        hevc_first(
            &["hevc_nvenc", "hevc_qsv", "hevc_amf", "hevc_mf"],
            &["h264_nvenc", "h264_qsv", "h264_amf", "h264_mf"],
        )
    } else if cfg!(target_os = "macos") {
        hevc_first(&["hevc_videotoolbox"], &["h264_videotoolbox"])
    } else {
        hevc_first(&["hevc_vaapi", "hevc_nvenc"], &["h264_vaapi", "h264_nvenc"])
    }
}

pub fn is_hevc(encoder: &str) -> bool {
    encoder.starts_with("hevc")
}

/// Arguments before `-i` (device setup).
pub fn pre_input(encoder: &str) -> Vec<String> {
    if encoder.ends_with("_vaapi") {
        let dev = std::env::var("SC_VAAPI_DEVICE").unwrap_or_else(|_| "/dev/dri/renderD128".into());
        vec!["-vaapi_device".into(), dev]
    } else {
        Vec::new()
    }
}

/// Output arguments: filter, encoder, rate control, keyframes, colour tags.
pub fn output_args(
    encoder: &str,
    size: SizePx,
    fps: u32,
    gop: u32,
    bits_per_pixel: f64,
) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
    if encoder.ends_with("_vaapi") {
        a.extend(["-vf".into(), "format=nv12,hwupload".into()]);
    }
    a.extend(["-c:v".into(), encoder.into()]);
    let bpp = if is_hevc(encoder) {
        bits_per_pixel
    } else {
        bits_per_pixel * 1.6
    };
    let rate = (f64::from(size.w) * f64::from(size.h) * f64::from(fps) * bpp).max(500_000.0) as u64;
    match encoder {
        X264 => {
            let preset = if u64::from(size.w) * u64::from(size.h) > 2560 * 1440 {
                "ultrafast"
            } else {
                "veryfast"
            };
            a.extend(["-preset", preset, "-crf", "20", "-pix_fmt", "yuv420p"].map(String::from));
        }
        e if e.ends_with("_mf") => {
            a.extend(["-hw_encoding", "1", "-rate_control", "cbr", "-b:v"].map(String::from));
            a.push(rate.to_string());
        }
        e if e.ends_with("_nvenc") => {
            a.extend(["-preset", "p4", "-b:v"].map(String::from));
            a.push(rate.to_string());
        }
        _ => {
            a.push("-b:v".into());
            a.push(rate.to_string());
        }
    }
    a.extend(["-g".into(), gop.max(1).to_string()]);
    a.extend(
        [
            "-color_primaries",
            "bt709",
            "-color_trc",
            "bt709",
            "-colorspace",
            "bt709",
            "-color_range",
            "tv",
        ]
        .map(String::from),
    );
    a
}

/// Matroska with clusters of at most a second, flushed as written: a killed helper leaves a file
/// readable up to its last cluster.
pub fn container_args() -> Vec<String> {
    [
        "-f",
        "matroska",
        "-cluster_time_limit",
        "1000",
        "-flush_packets",
        "1",
    ]
    .map(String::from)
    .to_vec()
}

pub fn works(ffmpeg: &Path, encoder: &str) -> bool {
    let size = SizePx { w: 320, h: 240 };
    let mut args = vec![
        "-hide_banner".to_string(),
        "-loglevel".into(),
        "error".into(),
    ];
    args.extend(pre_input(encoder));
    args.extend(
        [
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x240:rate=30",
            "-frames:v",
            "5",
            "-pix_fmt",
            "nv12",
        ]
        .map(String::from),
    );
    args.extend(output_args(encoder, size, 30, 30, 0.1));
    args.extend(["-f".into(), "null".into(), "-".into()]);
    sc_helpers::hide_console(&mut Command::new(ffmpeg))
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn cache() -> &'static Mutex<HashMap<(String, bool), &'static str>> {
    static C: OnceLock<Mutex<HashMap<(String, bool), &'static str>>> = OnceLock::new();
    C.get_or_init(Default::default)
}

/// `SC_RECORD_ENCODER` forces one (tests, diagnostics).
pub fn pick(ffmpeg: &Path, codec: VideoCodec) -> &'static str {
    if let Ok(forced) = std::env::var("SC_RECORD_ENCODER")
        && let Some(e) = candidates(codec).into_iter().find(|c| *c == forced)
    {
        return e;
    }
    let key = (ffmpeg.display().to_string(), codec == VideoCodec::Hevc);
    if let Some(e) = cache().lock().ok().and_then(|c| c.get(&key).copied()) {
        return e;
    }
    let chosen = candidates(codec)
        .into_iter()
        .find(|e| *e == X264 || works(ffmpeg, e))
        .unwrap_or(X264);
    tracing::info!(encoder = chosen, "raw-track encoder");
    if let Ok(mut c) = cache().lock() {
        c.insert(key, chosen);
    }
    chosen
}

/// Every candidate with whether it opened (Diagnostics).
pub fn survey(ffmpeg: &Path) -> Vec<(&'static str, bool)> {
    candidates(VideoCodec::Hevc)
        .into_iter()
        .map(|e| (e, works(ffmpeg, e)))
        .collect()
}
