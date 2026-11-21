//! Capability probe v1 (ARCHITECTURE §11, plan §10 item 11). Cache file
//! `<cache_dir>/probe-v1.json`, keyed by helper sha256 + OS version + display sizes.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use sc_geometry::{SizePx, even_round};
use sc_helpers::{HelperInfo, HelperKind};
use sc_platform::{DisplayId, DisplayInfo, Error, Platform, Resolution, Result, VideoCodec};
use serde::{Deserialize, Serialize};

pub const CACHE_FILE: &str = "probe-v1.json";
/// Holds the key of the step running now; found at the next run, that step is quarantined.
pub const IN_PROGRESS_FILE: &str = "probe-v1.inprogress";
pub const VT_H264_SIZES: [SizePx; 3] = [
    SizePx { w: 3840, h: 2160 },
    SizePx { w: 4096, h: 2160 },
    SizePx { w: 4098, h: 2160 },
];
pub const HEVC_PROBE_FPS: u32 = 60;
pub const HEVC_PROBE_FRAMES: u32 = 90;
/// ADR-009: 60 fps needs 30 % headroom.
pub const HEADROOM: f64 = 1.3;
const VT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayProbe {
    pub display_id: String,
    pub native_px: SizePx,
    pub one_x_px: SizePx,
    pub hevc_fps_native: Option<f64>,
    pub hevc_fps_1x: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbeV1 {
    /// Always 1.
    pub schema: u32,
    pub created_at: String,
    pub os_version: String,
    pub helper: Option<HelperInfo>,
    pub vt_h264_opens: Vec<(SizePx, bool)>,
    pub displays: Vec<DisplayProbe>,
    pub quarantined: Vec<String>,
    /// Helper encoders that encoded a short test clip (`name`, worked).
    #[serde(default)]
    pub encoders: Vec<(String, bool)>,
}

impl ProbeV1 {
    /// Measured ≥ 1.3 × 60 fps; `None` when the display or that resolution was not measured.
    pub fn allows_60fps(&self, display: &DisplayId, res: Resolution) -> Option<bool> {
        let d = self.displays.iter().find(|d| d.display_id == display.0)?;
        let fps = match res {
            Resolution::Native => d.hevc_fps_native,
            Resolution::OneX => d.hevc_fps_1x,
        }?;
        Some(fps >= HEADROOM * f64::from(HEVC_PROBE_FPS))
    }

    /// `None` when the encoder was not tried.
    pub fn encoder_ok(&self, name: &str) -> Option<bool> {
        self.encoders
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, ok)| *ok)
    }

    /// `None` when that size was not tried.
    pub fn vt_h264_opens_at(&self, size: SizePx) -> Option<bool> {
        self.vt_h264_opens
            .iter()
            .find(|(s, _)| *s == size)
            .map(|(_, ok)| *ok)
    }
}

pub fn one_x_px(d: &DisplayInfo) -> SizePx {
    SizePx {
        w: even_round(d.frame_pt.w),
        h: even_round(d.frame_pt.h),
    }
}

pub fn os_version() -> String {
    #[cfg(target_os = "macos")]
    {
        let get = |arg: &str| {
            Command::new("/usr/bin/sw_vers")
                .arg(arg)
                .stdin(Stdio::null())
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        };
        if let Some(v) = get("-productVersion") {
            return match get("-buildVersion") {
                Some(b) => format!("macOS {v} ({b})"),
                None => format!("macOS {v}"),
            };
        }
    }
    std::env::consts::OS.to_string()
}

struct Steps<'a> {
    marker: Option<&'a Path>,
    quarantined: &'a [String],
}

impl Steps<'_> {
    fn run<T>(&self, key: &str, f: impl FnOnce() -> T) -> Result<Option<T>> {
        if self.quarantined.iter().any(|q| q == key) {
            tracing::warn!(key, "probe step quarantined; skipped");
            return Ok(None);
        }
        if let Some(m) = self.marker {
            sc_core::fsio::atomic_write(m, key.as_bytes())?;
        }
        let out = f();
        if let Some(m) = self.marker {
            std::fs::remove_file(m)
                .map_err(|e| Error::Other(format!("probe: remove marker {}: {e}", m.display())))?;
        }
        Ok(Some(out))
    }
}

pub const VIDEO_ENCODERS: [&str; 9] = [
    "libx264",
    "libx265",
    "libsvtav1",
    "libvpx-vp9",
    "libvpx",
    "prores_ks",
    "h264_videotoolbox",
    "hevc_videotoolbox",
    "prores_videotoolbox",
];
pub const AUDIO_ENCODERS: [&str; 7] = [
    "aac_at",
    "aac",
    "libopus",
    "flac",
    "alac",
    "libvorbis",
    "pcm_s16le",
];

fn wait_ok(child: std::io::Result<std::process::Child>) -> bool {
    let Ok(mut child) = child else {
        return false;
    };
    let t0 = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return st.success(),
            Ok(None) if t0.elapsed() < VT_TIMEOUT => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

/// Encodes 0.2 s of a test pattern (or tone) with `name`; hardware encoders may not fall back
/// to software.
pub fn encoder_works(ffmpeg: &Path, name: &str) -> bool {
    let audio = AUDIO_ENCODERS.contains(&name);
    let mut cmd = Command::new(ffmpeg);
    cmd.args([
        "-hide_banner",
        "-v",
        "error",
        "-nostdin",
        "-f",
        "lavfi",
        "-i",
    ]);
    if audio {
        cmd.args(["sine=d=0.2:sample_rate=48000", "-c:a", name]);
    } else {
        cmd.args(["testsrc2=s=320x240:r=30:d=0.2", "-c:v", name]);
        if name.ends_with("_videotoolbox") {
            cmd.args(["-allow_sw", "0"]);
        }
        if name == "prores_ks" || name == "prores_videotoolbox" {
            cmd.args([
                "-pix_fmt",
                if name == "prores_ks" {
                    "yuv422p10le"
                } else {
                    "p210le"
                },
            ]);
        } else {
            cmd.args(["-pix_fmt", "yuv420p"]);
        }
    }
    cmd.args(["-f", "null", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    wait_ok(cmd.spawn())
}

fn vt_h264_opens(ffmpeg: &Path, size: SizePx) -> bool {
    let child = Command::new(ffmpeg)
        .args([
            "-hide_banner",
            "-v",
            "error",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
        ])
        .arg(format!("testsrc2=s={}x{}:r=30:d=0.2", size.w, size.h))
        .args([
            "-c:v",
            "h264_videotoolbox",
            "-allow_sw",
            "0",
            "-f",
            "null",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        return false;
    };
    let t0 = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return st.success(),
            Ok(None) if t0.elapsed() < VT_TIMEOUT => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

fn hevc_fps(platform: &dyn Platform, size: SizePx) -> Option<f64> {
    match platform.writers().encode_benchmark(
        size,
        HEVC_PROBE_FPS,
        HEVC_PROBE_FRAMES,
        VideoCodec::Hevc,
    ) {
        Ok(f) if f.is_finite() && f > 0.0 => Some((f * 100.0).round() / 100.0),
        Ok(f) => {
            tracing::warn!(w = size.w, h = size.h, f, "encode_benchmark: no rate");
            None
        }
        Err(e) => {
            tracing::warn!(w = size.w, h = size.h, %e, "encode_benchmark failed");
            None
        }
    }
}

pub fn run(platform: &dyn Platform, displays: &[DisplayInfo]) -> Result<ProbeV1> {
    run_steps(
        platform,
        displays,
        &Steps {
            marker: None,
            quarantined: &[],
        },
    )
}

fn run_steps(platform: &dyn Platform, displays: &[DisplayInfo], st: &Steps) -> Result<ProbeV1> {
    let ffmpeg = sc_helpers::locate(HelperKind::Ffmpeg).ok();
    let helper = match &ffmpeg {
        Some(p) => match sc_helpers::helper_info(p) {
            Ok(i) => Some(i),
            Err(e) => {
                tracing::warn!(%e, "probe: helper info");
                None
            }
        },
        None => None,
    };
    let mut vt = Vec::new();
    if cfg!(target_os = "macos")
        && let Some(ff) = &ffmpeg
    {
        for size in VT_H264_SIZES {
            let key = format!("vt_h264:{}x{}", size.w, size.h);
            let ok = st.run(&key, || vt_h264_opens(ff, size))?.unwrap_or(false);
            vt.push((size, ok));
        }
    }
    let mut encoders = Vec::new();
    if let Some(ff) = &ffmpeg {
        for name in VIDEO_ENCODERS.iter().chain(AUDIO_ENCODERS.iter()) {
            if !cfg!(target_os = "macos") && (name.ends_with("_videotoolbox") || *name == "aac_at")
            {
                continue;
            }
            let key = format!("enc:{name}");
            let ok = st.run(&key, || encoder_works(ff, name))?.unwrap_or(false);
            encoders.push(((*name).to_string(), ok));
        }
    }
    let mut probes = Vec::new();
    for d in displays {
        let native = d.size_px;
        let one_x = one_x_px(d);
        let key = format!("hevc:{}:{}x{}", d.id.0, native.w, native.h);
        let fps_native = st.run(&key, || hevc_fps(platform, native))?.flatten();
        let fps_1x = if one_x == native {
            fps_native
        } else {
            let key = format!("hevc:{}:{}x{}", d.id.0, one_x.w, one_x.h);
            st.run(&key, || hevc_fps(platform, one_x))?.flatten()
        };
        probes.push(DisplayProbe {
            display_id: d.id.0.clone(),
            native_px: native,
            one_x_px: one_x,
            hevc_fps_native: fps_native,
            hevc_fps_1x: fps_1x,
        });
    }
    Ok(ProbeV1 {
        schema: 1,
        created_at: sc_core::folder::rfc3339(&platform.local_now()),
        os_version: os_version(),
        helper,
        vt_h264_opens: vt,
        displays: probes,
        quarantined: st.quarantined.to_vec(),
        encoders,
    })
}

fn matches_cache(p: &ProbeV1, displays: &[DisplayInfo], sha: Option<&str>, os: &str) -> bool {
    p.schema == 1
        && (p.helper.is_none() || !p.encoders.is_empty())
        && p.os_version == os
        && p.helper.as_ref().map(|h| h.sha256.as_str()) == sha
        && p.displays.len() == displays.len()
        && p.displays.iter().zip(displays).all(|(a, b)| {
            a.display_id == b.id.0 && a.native_px == b.size_px && a.one_x_px == one_x_px(b)
        })
}

fn read_cache(path: &Path) -> Option<ProbeV1> {
    let bytes = std::fs::read(path).ok()?;
    match serde_json::from_slice(&bytes) {
        Ok(p) => Some(p),
        Err(e) => {
            tracing::warn!(path = %path.display(), %e, "probe cache unreadable; re-probing");
            None
        }
    }
}

pub fn cache_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join(CACHE_FILE)
}

/// Returns the cache when it matches this helper, OS and displays and no step crashed since;
/// otherwise probes again, skipping quarantined steps, and rewrites the cache.
pub fn load_or_run(
    platform: &dyn Platform,
    cache_dir: &Path,
    displays: &[DisplayInfo],
) -> Result<ProbeV1> {
    std::fs::create_dir_all(cache_dir)
        .map_err(|e| Error::Other(format!("probe: create {}: {e}", cache_dir.display())))?;
    let path = cache_path(cache_dir);
    let marker = cache_dir.join(IN_PROGRESS_FILE);
    let prev = read_cache(&path);
    let mut quarantined = prev
        .as_ref()
        .map(|p| p.quarantined.clone())
        .unwrap_or_default();
    let crashed = std::fs::read_to_string(&marker)
        .ok()
        .map(|k| k.trim().to_string());
    if let Some(k) = &crashed {
        tracing::warn!(key = %k, "probe step crashed last time; quarantining it");
        if !k.is_empty() && !quarantined.contains(k) {
            quarantined.push(k.clone());
        }
        std::fs::remove_file(&marker)
            .map_err(|e| Error::Other(format!("probe: remove {}: {e}", marker.display())))?;
    }
    if crashed.is_none()
        && let Some(p) = prev
    {
        let sha = sc_helpers::locate(HelperKind::Ffmpeg)
            .ok()
            .and_then(|f| sc_core::sha::sha256_file(&f).ok());
        if matches_cache(&p, displays, sha.as_deref(), &os_version()) {
            return Ok(p);
        }
    }
    let probe = run_steps(
        platform,
        displays,
        &Steps {
            marker: Some(&marker),
            quarantined: &quarantined,
        },
    )?;
    let mut bytes = serde_json::to_vec_pretty(&probe)
        .map_err(|e| Error::Other(format!("probe: serialize: {e}")))?;
    bytes.push(b'\n');
    sc_core::fsio::atomic_write(&path, &bytes)?;
    Ok(probe)
}
