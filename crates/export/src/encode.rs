//! Helper arguments for every export the settings describe (CODEC_MATRIX §2). Frames arrive as Y4M
//! on stdin at the final size and rate, so no recipe scales or retimes; colour is tagged on the
//! frames and on the output (Pitfall 5, ADR-015). VP8 is the BT.601 exception (ADR-029). The
//! palette GIF runs in two passes with a raw 16×16 palette, because the helper has no PNG decoder.

use std::path::Path;

use crate::settings::{
    AudioCodec, Encoder, ExportSettings, Format, GifMode, Loop, Quality, VideoCodec,
};

const TAG_709: &str = "setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv";
const TO_RGB: &str = "scale=in_color_matrix=bt709:in_range=tv:out_range=pc";
const TAG_601: &str = "scale=out_color_matrix=bt601:out_range=tv,setparams=color_primaries=smpte170m:color_trc=smpte170m:colorspace=smpte170m:range=tv";

#[cfg(target_os = "macos")]
const AAC: &str = "aac_at";
#[cfg(not(target_os = "macos"))]
const AAC: &str = "aac";

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| (*x).to_string()).collect()
}

fn tier(q: Quality) -> usize {
    match q {
        Quality::Small => 0,
        Quality::Balanced | Quality::Custom { .. } | Quality::TargetSize { .. } => 1,
        Quality::High => 2,
    }
}

/// The encoder name the helper uses for `(video, encoder)`, or `None` when there is none.
pub fn video_encoder(video: VideoCodec, enc: Encoder) -> Option<&'static str> {
    let fast = enc == Encoder::Fast;
    Some(match (video, fast) {
        (VideoCodec::H264, false) => "libx264",
        (VideoCodec::Hevc, false) => "libx265",
        (VideoCodec::Av1, false) => "libsvtav1",
        (VideoCodec::Vp9, false) => "libvpx-vp9",
        (VideoCodec::Vp8, false) => "libvpx",
        (VideoCodec::Prores, false) => "prores_ks",
        (VideoCodec::Webp, _) => "libwebp_anim",
        #[cfg(target_os = "macos")]
        (VideoCodec::H264, true) => "h264_videotoolbox",
        #[cfg(target_os = "macos")]
        (VideoCodec::Hevc, true) => "hevc_videotoolbox",
        #[cfg(target_os = "macos")]
        (VideoCodec::Prores, true) => "prores_videotoolbox",
        _ => return None,
    })
}

pub fn audio_encoder(audio: AudioCodec) -> Option<&'static str> {
    Some(match audio {
        AudioCodec::Aac => AAC,
        AudioCodec::Opus => "libopus",
        AudioCodec::Flac => "flac",
        AudioCodec::Alac => "alac",
        AudioCodec::Vorbis => "libvorbis",
        AudioCodec::Pcm => "pcm_s16le",
        AudioCodec::None => return None,
    })
}

fn video_quality(st: &ExportSettings, enc: &str) -> Vec<String> {
    let t = tier(st.quality);
    let custom_crf = match st.quality {
        Quality::Custom { crf: Some(c), .. } => Some(c),
        _ => None,
    };
    let custom_kbps = match st.quality {
        Quality::Custom { kbps: Some(k), .. } => Some(k),
        _ => None,
    };
    let crf = |tiers: [u32; 3]| custom_crf.unwrap_or(tiers[t]).to_string();
    if let Some(k) = custom_kbps
        && enc != "prores_ks"
        && enc != "prores_videotoolbox"
    {
        let rate = format!("{k}k");
        return s(&[
            "-b:v",
            &rate,
            "-maxrate",
            &rate,
            "-bufsize",
            &format!("{}k", k * 2),
        ]);
    }
    match enc {
        "libx264" => {
            let mut a = s(&["-preset", "medium", "-profile:v", "high", "-crf"]);
            a.push(crf([26, 22, 18]));
            a
        }
        "libx265" => {
            let mut a = s(&["-preset", "medium", "-crf"]);
            a.push(crf([28, 25, 22]));
            a.extend(s(&["-x265-params", "log-level=error"]));
            a
        }
        "libsvtav1" => {
            let mut a = s(&["-preset", "8", "-crf"]);
            a.push(crf([38, 32, 26]));
            a
        }
        "libvpx-vp9" => {
            let mut a = s(&[
                "-deadline",
                "good",
                "-cpu-used",
                "4",
                "-row-mt",
                "1",
                "-tile-columns",
                "2",
                "-b:v",
                "0",
                "-crf",
            ]);
            a.push(crf([38, 33, 28]));
            a
        }
        "libvpx" => {
            let caps = ["4M", "8M", "16M"];
            let mut a = s(&["-deadline", "good", "-cpu-used", "4", "-crf"]);
            a.push(crf([20, 10, 6]));
            a.extend(s(&["-b:v", caps[t]]));
            a
        }
        "h264_videotoolbox" | "hevc_videotoolbox" => {
            let q = custom_crf.map_or([50, 60, 70][t], |c| 100u32.saturating_sub(c * 2));
            let mut a = s(&["-allow_sw", "0", "-q:v"]);
            a.push(q.to_string());
            a
        }
        "prores_ks" => {
            let mut a = s(&["-profile:v"]);
            a.push(["1", "2", "3"][t].to_string());
            a.extend(s(&["-vendor", "apl0"]));
            a
        }
        "prores_videotoolbox" => {
            let mut a = s(&["-profile:v"]);
            a.push(["lt", "standard", "hq"][t].to_string());
            a
        }
        "libwebp_anim" => {
            let q = custom_crf.map_or([60, 75, 90][t], |c| 100u32.saturating_sub(c).clamp(1, 100));
            let mut a = s(&["-lossless", "0", "-compression_level", "4", "-quality"]);
            a.push(q.to_string());
            a
        }
        _ => Vec::new(),
    }
}

fn audio_quality(st: &ExportSettings, enc: &str) -> Vec<String> {
    if let Some(k) = st.audio_kbps
        && matches!(enc, "aac_at" | "aac" | "libopus" | "libvorbis")
    {
        return s(&["-b:a", &format!("{k}k")]);
    }
    let t = tier(st.quality);
    match enc {
        "aac_at" | "aac" => s(&["-b:a", ["128k", "160k", "256k"][t]]),
        "libopus" => s(&["-b:a", ["96k", "128k", "192k"][t]]),
        "libvorbis" => s(&["-q:a", ["3", "5", "7"][t]]),
        _ => Vec::new(),
    }
}

fn ffmpeg_prefix() -> Vec<String> {
    s(&[
        "-hide_banner",
        "-loglevel",
        "warning",
        "-nostats",
        "-progress",
        "pipe:2",
        "-stats_period",
        "0.5",
        "-f",
        "yuv4mpegpipe",
        "-i",
        "pipe:0",
    ])
}

/// ffmpeg arguments for a video export: Y4M on stdin, the mixed WAV at `wav` when there is one,
/// the file at `out`.
pub fn video_args(st: &ExportSettings, wav: Option<&Path>, out: &Path) -> Option<Vec<String>> {
    let venc = video_encoder(st.video, st.encoder)?;
    let aenc = wav.and(audio_encoder(st.audio));
    let mut a = ffmpeg_prefix();
    if let (Some(w), Some(_)) = (wav, aenc) {
        a.extend(s(&["-f", "wav", "-i"]));
        a.push(w.to_string_lossy().into_owned());
    }
    a.extend(s(&["-map", "0:v:0"]));
    if aenc.is_some() {
        a.extend(s(&["-map", "1:a:0"]));
    }
    if let Some(rgb) = rgb_pix(venc) {
        a.extend(s(&["-vf", &format!("{TAG_709},{TO_RGB}"), "-c:v", venc]));
        a.extend(video_quality(st, venc));
        a.extend(s(&["-pix_fmt", rgb]));
    } else {
        let (filter, tags) = if st.video == VideoCodec::Vp8 {
            (TAG_601, ["smpte170m", "smpte170m", "smpte170m"])
        } else {
            (TAG_709, ["bt709", "bt709", "bt709"])
        };
        a.extend(s(&["-vf", filter, "-c:v", venc]));
        a.extend(video_quality(st, venc));
        let pix = match venc {
            "prores_ks" => "yuv422p10le",
            "prores_videotoolbox" => "p210le",
            _ => "yuv420p",
        };
        a.extend(s(&[
            "-pix_fmt",
            pix,
            "-color_primaries",
            tags[0],
            "-color_trc",
            tags[1],
            "-colorspace",
            tags[2],
            "-color_range",
            "tv",
        ]));
    }
    if st.video == VideoCodec::Hevc {
        a.extend(s(&["-tag:v", "hvc1"]));
    }
    match aenc {
        Some(enc) => {
            a.extend(s(&["-c:a", enc]));
            a.extend(audio_quality(st, enc));
        }
        None => a.push("-an".into()),
    }
    match st.format {
        Format::Mp4 => a.extend(s(&["-movflags", "+faststart", "-f", "mp4"])),
        Format::Mov => a.extend(s(&["-movflags", "+faststart", "-f", "mov"])),
        Format::Webm => a.extend(s(&["-f", "webm"])),
        Format::Webp => {
            a.extend(s(&["-loop", &plays(st.gif.repeat), "-f", "webp"]));
        }
        Format::Gif => return None,
    }
    a.push("-y".into());
    a.push(out.to_string_lossy().into_owned());
    Some(a)
}

/// The WebP loop count: how many times it plays, 0 for ever.
fn plays(r: Loop) -> String {
    match r {
        Loop::Forever => "0".into(),
        Loop::Once => "1".into(),
        Loop::Times(n) => n.max(1).to_string(),
    }
}

/// WebP takes RGB: libwebp does its own (BT.601) conversion from it.
fn rgb_pix(venc: &str) -> Option<&'static str> {
    match venc {
        "libwebp_anim" => Some("bgra"),
        _ => None,
    }
}

fn gifski_repeat(r: Loop) -> String {
    match r {
        Loop::Forever => "0".into(),
        Loop::Once => "-1".into(),
        Loop::Times(n) => n.max(1).to_string(),
    }
}

fn ffmpeg_loop(r: Loop) -> String {
    match r {
        Loop::Forever => "0".into(),
        Loop::Once => "-1".into(),
        Loop::Times(n) => n.max(1).saturating_sub(1).to_string(),
    }
}

/// gifski arguments: Y4M on stdin, the GIF at `out`.
pub fn gifski_args(st: &ExportSettings, fps: u32, out: &Path) -> Vec<String> {
    let g = &st.gif;
    let mut a = s(&["--quiet", "--fps"]);
    a.push(fps.to_string());
    a.extend(s(&["--quality"]));
    a.push(g.quality.clamp(1, 100).to_string());
    a.extend(s(&["--motion-quality"]));
    a.push(g.motion_quality.clamp(1, 100).to_string());
    a.extend(s(&["--lossy-quality"]));
    a.push(g.lossy.clamp(1, 100).to_string());
    a.extend(s(&["--repeat"]));
    a.push(gifski_repeat(g.repeat));
    a.extend(s(&["--y4m-color-override", "bt709", "-o"]));
    a.push(out.to_string_lossy().into_owned());
    a.push("-".into());
    a
}

/// Palette GIF pass 1: frames on stdin → a 16×16 raw RGB32 palette at `palette`.
pub fn palette_gen_args(st: &ExportSettings, palette: &Path) -> Vec<String> {
    let mut a = ffmpeg_prefix();
    let graph = format!(
        "{TAG_709},palettegen=max_colors={}:stats_mode=full",
        st.gif.colors.clamp(2, 256)
    );
    a.extend(s(&[
        "-vf",
        &graph,
        "-frames:v",
        "1",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgb32",
        "-y",
    ]));
    a.push(palette.to_string_lossy().into_owned());
    a
}

/// Palette GIF pass 2: frames on stdin and the palette from pass 1 → the GIF at `out`.
pub fn palette_use_args(st: &ExportSettings, palette: &Path, out: &Path) -> Vec<String> {
    let mut a = ffmpeg_prefix();
    a.extend(s(&[
        "-f", "rawvideo", "-pix_fmt", "rgb32", "-s", "16x16", "-i",
    ]));
    a.push(palette.to_string_lossy().into_owned());
    let dither = if st.gif.dither { "sierra2_4a" } else { "none" };
    let graph = format!("[0:v]{TAG_709}[v];[v][1:v]paletteuse=dither={dither}[out]");
    a.extend(s(&["-filter_complex", &graph, "-map", "[out]", "-loop"]));
    a.push(ffmpeg_loop(st.gif.repeat));
    a.extend(s(&["-f", "gif", "-y"]));
    a.push(out.to_string_lossy().into_owned());
    a
}

pub fn uses_gifski(st: &ExportSettings) -> bool {
    st.format == Format::Gif && st.gif.mode == GifMode::Gifski
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::GifSettings;
    use std::path::PathBuf;

    fn has(a: &[String], pair: [&str; 2]) -> bool {
        a.windows(2).any(|w| w[0] == pair[0] && w[1] == pair[1])
    }

    #[test]
    fn mp4_h264_best_and_fast_follow_the_matrix() {
        let mut st = ExportSettings::for_format(Format::Mp4);
        let out = PathBuf::from("/tmp/x.mp4");
        let wav = PathBuf::from("/tmp/x.wav");
        let a = video_args(&st, Some(&wav), &out).expect("args");
        assert!(has(&a, ["-c:v", "libx264"]) && has(&a, ["-crf", "22"]));
        assert!(has(&a, ["-movflags", "+faststart"]) && has(&a, ["-pix_fmt", "yuv420p"]));
        assert!(has(&a, ["-c:a", AAC]) && has(&a, ["-b:a", "160k"]));
        st.quality = Quality::High;
        assert!(has(
            &video_args(&st, None, &out).expect("a"),
            ["-crf", "18"]
        ));
        st.encoder = Encoder::Fast;
        if cfg!(target_os = "macos") {
            let f = video_args(&st, None, &out).expect("fast");
            assert!(has(&f, ["-c:v", "h264_videotoolbox"]) && has(&f, ["-allow_sw", "0"]));
            assert!(f.contains(&"-an".to_string()));
        }
    }

    #[test]
    fn hevc_is_tagged_hvc1_and_vp8_is_bt601() {
        let mut st = ExportSettings::for_format(Format::Mp4);
        st.video = VideoCodec::Hevc;
        let a = video_args(&st, None, Path::new("o.mp4")).expect("hevc");
        assert!(has(&a, ["-tag:v", "hvc1"]) && has(&a, ["-c:v", "libx265"]));
        let mut w = ExportSettings::for_format(Format::Webm);
        w.video = VideoCodec::Vp8;
        w.audio = AudioCodec::Vorbis;
        let a = video_args(&w, Some(Path::new("a.wav")), Path::new("o.webm")).expect("vp8");
        assert!(has(&a, ["-colorspace", "smpte170m"]) && has(&a, ["-b:v", "8M"]));
        assert!(has(&a, ["-c:a", "libvorbis"]) && has(&a, ["-q:a", "5"]));
    }

    #[test]
    fn custom_quality_uses_crf_or_bitrate() {
        let mut st = ExportSettings::for_format(Format::Webm);
        st.quality = Quality::Custom {
            crf: Some(40),
            kbps: None,
        };
        assert!(has(
            &video_args(&st, None, Path::new("o")).expect("a"),
            ["-crf", "40"]
        ));
        st.quality = Quality::Custom {
            crf: None,
            kbps: Some(2500),
        };
        assert!(has(
            &video_args(&st, None, Path::new("o")).expect("a"),
            ["-b:v", "2500k"]
        ));
    }

    #[test]
    fn gif_loops_map_to_each_tool() {
        let mut st = ExportSettings::for_format(Format::Gif);
        st.gif = GifSettings {
            repeat: Loop::Times(3),
            ..GifSettings::default()
        };
        let g = gifski_args(&st, 15, Path::new("o.gif"));
        assert!(has(&g, ["--repeat", "3"]) && has(&g, ["--fps", "15"]));
        assert!(has(&g, ["--y4m-color-override", "bt709"]));
        let u = palette_use_args(&st, Path::new("p.raw"), Path::new("o.gif"));
        assert!(has(&u, ["-loop", "2"]));
        st.gif.repeat = Loop::Once;
        assert!(has(
            &gifski_args(&st, 15, Path::new("o")),
            ["--repeat", "-1"]
        ));
        assert!(video_args(&st, None, Path::new("o.gif")).is_none());
    }

    #[test]
    fn webp_takes_rgb_and_a_loop_count() {
        let mut st = ExportSettings::for_format(Format::Webp);
        st.gif.repeat = Loop::Times(3);
        let a = video_args(&st, Some(Path::new("a.wav")), Path::new("o.webp")).expect("webp");
        assert!(has(&a, ["-c:v", "libwebp_anim"]) && has(&a, ["-pix_fmt", "bgra"]));
        assert!(has(&a, ["-quality", "75"]) && has(&a, ["-loop", "3"]) && has(&a, ["-f", "webp"]));
        assert!(a.contains(&"-an".to_string()) && !a.contains(&"-colorspace".to_string()));
    }
}
