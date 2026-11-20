use crate::frames::PixFmt;
use crate::helper::Container;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Ffmpeg,
    Gifski,
}

#[derive(Clone, Debug)]
pub struct Job {
    pub name: String,
    pub group: &'static str,
    pub label: String,
    pub tool: Tool,
    pub w: usize,
    pub h: usize,
    pub fps: u32,
    pub container: Container,
    pub ext: &'static str,
    pub vargs: Vec<String>,
    pub aargs: Vec<String>,
    pub expect_vcodec: &'static str,
    pub expect_tag: Option<&'static str>,
    pub expect_acodec: Option<&'static str>,
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

// FFmpeg 9 lets unspecified frame properties override -color_primaries/-color_trc, so the frames
// themselves are tagged; the output options stay as a second line of defence.
const TAG_FRAMES: &str = "setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv";
// VP8 is defined as BT.601 and FFmpeg's VP8 decoder ignores container color tags, so VP8 gets a
// BT.601 matrix (primaries/transfer stay BT.709) instead of being mis-decoded.
const VP8_TO_601: &str = "setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv,scale=out_color_matrix=bt601:out_range=tv,setparams=colorspace=smpte170m";
const COLOR_VP8: [&str; 8] =
    ["-color_primaries", "bt709", "-color_trc", "bt709", "-colorspace", "smpte170m", "-color_range", "tv"];
const COLOR: [&str; 8] =
    ["-color_primaries", "bt709", "-color_trc", "bt709", "-colorspace", "bt709", "-color_range", "tv"];

impl Job {
    fn ff(name: &str, group: &'static str, label: &str, w: usize, h: usize, ext: &'static str, vargs: Vec<String>) -> Job {
        Job {
            name: name.into(),
            group,
            label: label.into(),
            tool: Tool::Ffmpeg,
            w,
            h,
            fps: 30,
            container: Container::Y4m,
            ext,
            vargs,
            aargs: vec![],
            expect_vcodec: "",
            expect_tag: None,
            expect_acodec: None,
        }
    }

    fn audio(mut self, codec: &'static str, args: &[&str]) -> Job {
        self.aargs = s(args);
        self.expect_acodec = Some(codec);
        self
    }

    fn expect(mut self, vcodec: &'static str, tag: Option<&'static str>) -> Job {
        self.expect_vcodec = vcodec;
        self.expect_tag = tag;
        self
    }

    fn nv12(mut self) -> Job {
        self.container = Container::Raw(PixFmt::Nv12);
        self.name.push_str("_nv12");
        self.label.push_str(" (NV12 rawvideo in)");
        self
    }

    pub fn is_gif(&self) -> bool {
        self.ext == "gif"
    }

    pub fn ffmpeg_args(&self, input: &str, wav: Option<&str>, out: &str) -> Vec<String> {
        let mut a = s(&["-hide_banner", "-loglevel", "warning", "-progress", "pipe:1", "-nostats", "-stats_period", "0.5"]);
        match self.container {
            Container::Y4m => a.extend(s(&["-f", "yuv4mpegpipe", "-i", input])),
            Container::Raw(fmt) => {
                let pf = if fmt == PixFmt::Nv12 { "nv12" } else { "yuv420p" };
                a.extend(s(&["-f", "rawvideo", "-pix_fmt", pf, "-video_size"]));
                a.push(format!("{}x{}", self.w, self.h));
                a.extend(s(&["-framerate"]));
                a.push(self.fps.to_string());
                a.extend(s(&["-i", input]));
            }
        }
        let with_audio = self.expect_acodec.is_some() && wav.is_some();
        if with_audio {
            a.extend(s(&["-i", wav.unwrap()]));
        }
        if !self.is_gif() {
            a.extend(s(&["-map", "0:v:0"]));
            if with_audio {
                a.extend(s(&["-map", "1:a:0"]));
            }
        }
        a.extend(self.vargs.iter().cloned());
        if with_audio {
            a.extend(self.aargs.iter().cloned());
        }
        if self.expect_vcodec == "vp8" {
            a.extend(s(&["-vf", VP8_TO_601]));
            a.extend(s(&COLOR_VP8));
        } else if !self.is_gif() {
            a.extend(s(&["-vf", TAG_FRAMES]));
            a.extend(s(&COLOR));
        }
        match self.ext {
            "mp4" => {
                a.extend(s(&["-movflags", "+faststart", "-f", "mp4"]));
            }
            "webm" => {
                a.extend(s(&["-f", "webm"]));
            }
            _ => a.extend(s(&["-f", "gif"])),
        }
        a.extend(s(&["-y", out]));
        a
    }
}

fn x264(preset: &str, crf: &str, threads: Option<&str>) -> Vec<String> {
    let mut v = s(&["-c:v", "libx264", "-preset", preset, "-crf", crf, "-pix_fmt", "yuv420p", "-profile:v", "high"]);
    if let Some(t) = threads {
        v.extend(s(&["-threads", t]));
    }
    v
}

fn x265(preset: &str, crf: &str, threads: Option<&str>) -> Vec<String> {
    let mut v = s(&["-c:v", "libx265", "-preset", preset, "-crf", crf, "-pix_fmt", "yuv420p", "-tag:v", "hvc1"]);
    match threads {
        Some(t) => {
            v.extend(s(&["-threads", t, "-x265-params"]));
            v.push(format!("log-level=warning:pools={t}"));
        }
        None => v.extend(s(&["-x265-params", "log-level=warning"])),
    }
    v
}

fn svt(preset: &str, crf: &str, lp: Option<&str>) -> Vec<String> {
    let mut v = s(&["-c:v", "libsvtav1", "-preset", preset, "-crf", crf, "-pix_fmt", "yuv420p"]);
    if let Some(l) = lp {
        v.extend(s(&["-svtav1-params"]));
        v.push(format!("lp={l}"));
    }
    v
}

fn vp9(cpu: &str, crf: &str) -> Vec<String> {
    s(&["-c:v", "libvpx-vp9", "-crf", crf, "-b:v", "0", "-deadline", "good", "-cpu-used", cpu, "-row-mt", "1", "-tile-columns", "2", "-pix_fmt", "yuv420p"])
}

fn vp8(cpu: &str) -> Vec<String> {
    s(&["-c:v", "libvpx", "-crf", "10", "-b:v", "8M", "-deadline", "good", "-cpu-used", cpu, "-pix_fmt", "yuv420p"])
}

fn vt(codec: &str, q: &str) -> Vec<String> {
    let mut v = s(&["-c:v", codec, "-q:v", q, "-allow_sw", "0", "-pix_fmt", "yuv420p"]);
    if codec.starts_with("hevc") {
        v.extend(s(&["-tag:v", "hvc1", "-profile:v", "main"]));
    } else {
        v.extend(s(&["-profile:v", "high"]));
    }
    v
}

const AAC_AT: &[&str] = &["-c:a", "aac_at", "-b:a", "160k"];
const AAC: &[&str] = &["-c:a", "aac", "-b:a", "160k"];
const OPUS: &[&str] = &["-c:a", "libopus", "-b:a", "128k"];
const VORBIS: &[&str] = &["-c:a", "libvorbis", "-q:a", "5"];

pub const GIF_W: usize = 960;
pub const GIF_H: usize = 540;
pub const GIF_FPS: u32 = 15;

pub fn gif_palette_args(tag_input: bool, colors: u32) -> Vec<String> {
    let tag = if tag_input { "setparams=color_primaries=bt709:color_trc=iec61966-2-1:colorspace=bt709:range=tv," } else { "" };
    let graph = format!(
        "[0:v]{tag}fps={GIF_FPS},scale={GIF_W}:-2:flags=lanczos,split[a][b];[a]palettegen=max_colors={colors}:stats_mode=full[p];[b][p]paletteuse=dither=sierra2_4a[out]"
    );
    vec!["-filter_complex".into(), graph, "-map".into(), "[out]".into(), "-loop".into(), "0".into()]
}

pub fn gifski_args(out: &str, quality: u32) -> Vec<String> {
    vec![
        "--fps".into(),
        GIF_FPS.to_string(),
        "--quality".into(),
        quality.to_string(),
        "--repeat".into(),
        "0".into(),
        "--y4m-color-override".into(),
        "bt709".into(),
        "--quiet".into(),
        "-o".into(),
        out.into(),
        "-".into(),
    ]
}

pub fn all_jobs() -> Vec<Job> {
    let (w, h) = (1920, 1080);
    let (qw, qh) = (2560, 1440);
    let mut j = vec![
        Job::ff("x264_medium_crf20_aacat", "best", "x264 medium CRF20 + aac_at", w, h, "mp4", x264("medium", "20", None)).audio("aac", AAC_AT).expect("h264", Some("avc1")),
        Job::ff("x264_slow_crf20_aacat", "best", "x264 slow CRF20 + aac_at", w, h, "mp4", x264("slow", "20", None)).audio("aac", AAC_AT).expect("h264", Some("avc1")),
        Job::ff("x264_medium_crf20_aac", "best", "x264 medium CRF20 + native aac", w, h, "mp4", x264("medium", "20", None)).audio("aac", AAC).expect("h264", Some("avc1")),
        Job::ff("x265_medium_crf24_aacat", "best", "x265 medium CRF24 hvc1 + aac_at", w, h, "mp4", x265("medium", "24", None)).audio("aac", AAC_AT).expect("hevc", Some("hvc1")),
        Job::ff("svtav1_p8_crf32_aacat", "best", "SVT-AV1 preset 8 CRF32 + aac_at", w, h, "mp4", svt("8", "32", None)).audio("aac", AAC_AT).expect("av1", Some("av01")),
        Job::ff("svtav1_p6_crf32_aacat", "best", "SVT-AV1 preset 6 CRF32 + aac_at", w, h, "mp4", svt("6", "32", None)).audio("aac", AAC_AT).expect("av1", Some("av01")),
        Job::ff("vp9_good_cpu4_crf32_opus", "best", "libvpx-vp9 good cpu-used 4 CRF32 row-mt + Opus", w, h, "webm", vp9("4", "32")).audio("opus", OPUS).expect("vp9", None),
        Job::ff("vp9_good_cpu2_crf32_opus", "best", "libvpx-vp9 good cpu-used 2 CRF32 row-mt + Opus", w, h, "webm", vp9("2", "32")).audio("opus", OPUS).expect("vp9", None),
        Job::ff("vp8_good_cpu4_vorbis", "best", "libvpx VP8 good cpu-used 4 CRF10 8M cap + Vorbis", w, h, "webm", vp8("4")).audio("vorbis", VORBIS).expect("vp8", None),
        Job::ff("h264_vt_q65_aacat", "fast", "h264_videotoolbox q65 + aac_at", w, h, "mp4", vt("h264_videotoolbox", "65")).audio("aac", AAC_AT).expect("h264", Some("avc1")),
        Job::ff("hevc_vt_q65_aacat", "fast", "hevc_videotoolbox q65 hvc1 + aac_at", w, h, "mp4", vt("hevc_videotoolbox", "65")).audio("aac", AAC_AT).expect("hevc", Some("hvc1")),
        Job::ff("x264_medium_crf20_t4", "proxy4", "x264 medium CRF20 -threads 4", w, h, "mp4", x264("medium", "20", Some("4"))).expect("h264", Some("avc1")),
        Job::ff("x264_slow_crf20_t4", "proxy4", "x264 slow CRF20 -threads 4", w, h, "mp4", x264("slow", "20", Some("4"))).expect("h264", Some("avc1")),
        Job::ff("x265_medium_crf24_t4", "proxy4", "x265 medium CRF24 -threads 4 pools=4", w, h, "mp4", x265("medium", "24", Some("4"))).expect("hevc", Some("hvc1")),
        Job::ff("svtav1_p8_crf32_lp1", "proxy4", "SVT-AV1 preset 8 CRF32 lp=1", w, h, "mp4", svt("8", "32", Some("1"))).expect("av1", Some("av01")),
        Job::ff("svtav1_p8_crf32_lp2", "proxy4", "SVT-AV1 preset 8 CRF32 lp=2", w, h, "mp4", svt("8", "32", Some("2"))).expect("av1", Some("av01")),
        Job::ff("vp9_good_cpu4_crf32_t4", "proxy4", "libvpx-vp9 cpu-used 4 -threads 4", w, h, "webm", { let mut v = vp9("4", "32"); v.extend(s(&["-threads", "4"])); v }).expect("vp9", None),
        Job::ff("x264_medium_crf20", "input", "x264 medium CRF20 (Y4M in, no audio)", w, h, "mp4", x264("medium", "20", None)).expect("h264", Some("avc1")),
        Job::ff("x264_medium_crf20", "input", "x264 medium CRF20", w, h, "mp4", x264("medium", "20", None)).expect("h264", Some("avc1")).nv12(),
        Job::ff("h264_vt_q65", "input", "h264_videotoolbox q65 (Y4M in, no audio)", w, h, "mp4", vt("h264_videotoolbox", "65")).expect("h264", Some("avc1")),
        Job::ff("h264_vt_q65", "input", "h264_videotoolbox q65", w, h, "mp4", vt("h264_videotoolbox", "65")).expect("h264", Some("avc1")).nv12(),
        Job::ff("x264_medium_crf20_1440p", "1440p", "x264 medium CRF20 @1440p", qw, qh, "mp4", x264("medium", "20", None)).audio("aac", AAC_AT).expect("h264", Some("avc1")),
        Job::ff("x265_medium_crf24_1440p", "1440p", "x265 medium CRF24 @1440p", qw, qh, "mp4", x265("medium", "24", None)).audio("aac", AAC_AT).expect("hevc", Some("hvc1")),
        Job::ff("svtav1_p8_crf32_1440p", "1440p", "SVT-AV1 p8 CRF32 @1440p", qw, qh, "mp4", svt("8", "32", None)).audio("aac", AAC_AT).expect("av1", Some("av01")),
        Job::ff("h264_vt_q65_1440p", "1440p", "h264_videotoolbox q65 @1440p", qw, qh, "mp4", vt("h264_videotoolbox", "65")).audio("aac", AAC_AT).expect("h264", Some("avc1")),
        Job::ff("hevc_vt_q65_1440p_nv12", "1440p", "hevc_videotoolbox q65 @1440p (NV12 in)", qw, qh, "mp4", vt("hevc_videotoolbox", "65")).audio("aac", AAC_AT).expect("hevc", Some("hvc1")),
    ];
    if let Some(last) = j.last_mut() {
        last.container = Container::Raw(PixFmt::Nv12);
    }
    let mut g = Job::ff("gif_ffmpeg_palette", "gif", "FFmpeg palettegen/paletteuse (Kap-style, single pass)", GIF_W, GIF_H, "gif", gif_palette_args(true, 256)).expect("gif", None);
    g.fps = GIF_FPS;
    j.push(g);
    let mut g = Job::ff("gif_ffmpeg_palette_untagged", "demo", "same, WITHOUT setparams bt709 (pitfall demo)", GIF_W, GIF_H, "gif", gif_palette_args(false, 256)).expect("gif", None);
    g.fps = GIF_FPS;
    j.push(g);
    for q in [90u32, 70] {
        let mut g = Job::ff(&format!("gif_gifski_q{q}"), "gif", &format!("gifski --quality {q} (Y4M on stdin)"), GIF_W, GIF_H, "gif", vec![]).expect("gif", None);
        g.tool = Tool::Gifski;
        g.fps = GIF_FPS;
        g.vargs = vec![q.to_string()];
        j.push(g);
    }
    j
}
