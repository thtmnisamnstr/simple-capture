//! What an export makes (SPEC Export): container, codecs, encoder, quality, fps, size, GIF
//! options and where the file goes. Stored per format in the app's export settings and as named
//! presets.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Mp4,
    Webm,
    Gif,
    Mov,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoCodec {
    H264,
    Hevc,
    Av1,
    Vp9,
    Vp8,
    Prores,
    Gif,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioCodec {
    Aac,
    Opus,
    Flac,
    Alac,
    Vorbis,
    Pcm,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Encoder {
    Best,
    Fast,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "tier")]
pub enum Quality {
    Small,
    Balanced,
    High,
    /// `crf` for constant quality (lower is better), or `kbps` for a target bitrate.
    Custom {
        crf: Option<u32>,
        kbps: Option<u32>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum SizeChoice {
    Original,
    /// A short-side class (2160, 1440, 1080, 720, 480, 360, 240).
    Class(u32),
    Width(u32),
    Height(u32),
    /// The longer side, whichever way the frame faces.
    Long(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GifMode {
    Gifski,
    Palette,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "times")]
pub enum Loop {
    Forever,
    Once,
    Times(u32),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GifSettings {
    pub mode: GifMode,
    pub quality: u8,
    pub motion_quality: u8,
    pub lossy: u8,
    pub colors: u16,
    pub dither: bool,
    pub repeat: Loop,
}

impl Default for GifSettings {
    fn default() -> GifSettings {
        GifSettings {
            mode: GifMode::Gifski,
            quality: 80,
            motion_quality: 80,
            lossy: 100,
            colors: 256,
            dither: true,
            repeat: Loop::Forever,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportSettings {
    pub format: Format,
    pub video: VideoCodec,
    pub audio: AudioCodec,
    pub encoder: Encoder,
    pub quality: Quality,
    pub fps: u32,
    pub size: SizeChoice,
    pub upscale: bool,
    pub gif: GifSettings,
    /// `None`: next to the project folder.
    pub out_dir: Option<std::path::PathBuf>,
    /// `{title}`, `{date}`, `{time}`, `{size}`, `{fps}`, `{format}`.
    pub name: String,
}

impl Default for ExportSettings {
    fn default() -> ExportSettings {
        ExportSettings::for_format(Format::Mp4)
    }
}

pub const VIDEO_FPS: [u32; 6] = [15, 24, 25, 30, 50, 60];
pub const GIF_FPS: [u32; 9] = [5, 8, 10, 12, 15, 20, 25, 30, 50];

impl ExportSettings {
    pub fn for_format(format: Format) -> ExportSettings {
        let (video, audio, fps, size) = match format {
            Format::Mp4 => (VideoCodec::H264, AudioCodec::Aac, 30, SizeChoice::Original),
            Format::Webm => (VideoCodec::Vp9, AudioCodec::Opus, 30, SizeChoice::Original),
            Format::Gif => (VideoCodec::Gif, AudioCodec::None, 15, SizeChoice::Long(960)),
            Format::Mov => (
                VideoCodec::Prores,
                AudioCodec::Pcm,
                30,
                SizeChoice::Original,
            ),
        };
        ExportSettings {
            format,
            video,
            audio,
            encoder: Encoder::Best,
            quality: Quality::Balanced,
            fps,
            size,
            upscale: false,
            gif: GifSettings::default(),
            out_dir: None,
            name: "{title}".into(),
        }
    }

    pub fn extension(&self) -> &'static str {
        match self.format {
            Format::Mp4 => "mp4",
            Format::Webm => "webm",
            Format::Gif => "gif",
            Format::Mov => "mov",
        }
    }

    pub fn fps_choices(&self) -> &'static [u32] {
        if self.format == Format::Gif {
            &GIF_FPS
        } else {
            &VIDEO_FPS
        }
    }

    /// The largest offered rate not above `min(fps, recording)`; 59.94 counts as 60 (Kap's rule).
    pub fn effective_fps(&self, recording_fps: f64) -> u32 {
        let cap = f64::from(self.fps).min(recording_fps.max(1.0)) + 0.1;
        self.fps_choices()
            .iter()
            .copied()
            .filter(|f| f64::from(*f) <= cap)
            .max()
            .unwrap_or(self.fps_choices()[0])
    }

    /// Codec pairs the container takes (SPEC Export).
    pub fn allowed(format: Format) -> (&'static [VideoCodec], &'static [AudioCodec]) {
        match format {
            Format::Mp4 => (
                &[VideoCodec::H264, VideoCodec::Hevc, VideoCodec::Av1],
                &[
                    AudioCodec::Aac,
                    AudioCodec::Opus,
                    AudioCodec::Flac,
                    AudioCodec::Alac,
                    AudioCodec::None,
                ],
            ),
            Format::Webm => (
                &[VideoCodec::Vp9, VideoCodec::Av1, VideoCodec::Vp8],
                &[AudioCodec::Opus, AudioCodec::Vorbis, AudioCodec::None],
            ),
            Format::Gif => (&[VideoCodec::Gif], &[AudioCodec::None]),
            Format::Mov => (
                &[VideoCodec::Prores, VideoCodec::H264, VideoCodec::Hevc],
                &[AudioCodec::Pcm, AudioCodec::Aac, AudioCodec::None],
            ),
        }
    }

    /// Settings the container accepts: unknown pairs fall back to the format's defaults.
    pub fn normalized(mut self) -> ExportSettings {
        let (v, a) = ExportSettings::allowed(self.format);
        let d = ExportSettings::for_format(self.format);
        if !v.contains(&self.video) {
            self.video = d.video;
        }
        if !a.contains(&self.audio) {
            self.audio = d.audio;
        }
        if !self.fps_choices().contains(&self.fps) {
            self.fps = d.fps;
        }
        self
    }
}

/// Combinations players handle less widely (SPEC: label them).
pub fn less_compatible(
    format: Format,
    video: VideoCodec,
    audio: AudioCodec,
) -> Option<&'static str> {
    match (format, video, audio) {
        (_, VideoCodec::Vp8, _) => Some("VP8 is a legacy codec"),
        (Format::Mp4, VideoCodec::Av1, _) => {
            Some("AV1 needs a recent Mac (M3 or later) or browser")
        }
        (Format::Mp4, VideoCodec::Hevc, _) => {
            Some("HEVC plays on Macs and recent phones; not every browser")
        }
        (Format::Mp4, _, AudioCodec::Opus | AudioCodec::Flac) => {
            Some("Opus or FLAC in MP4 may not play in QuickTime")
        }
        (Format::Mp4, _, AudioCodec::Alac) => {
            Some("ALAC in MP4 plays in Apple apps, not in browsers")
        }
        (Format::Webm, _, _) => Some("WebM plays in browsers and VLC, not in QuickTime"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fps_never_exceeds_the_recording() {
        let s = ExportSettings::for_format(Format::Mp4);
        assert_eq!(s.effective_fps(30.0), 30);
        let s60 = ExportSettings { fps: 60, ..s };
        assert_eq!(s60.effective_fps(30.0), 30);
        assert_eq!(s60.effective_fps(59.94), 60);
        let gif = ExportSettings::for_format(Format::Gif);
        assert_eq!(gif.effective_fps(30.0), 15);
        let gif50 = ExportSettings { fps: 50, ..gif };
        assert_eq!(gif50.effective_fps(30.0), 30);
        assert_eq!(gif50.effective_fps(24.0), 20);
    }

    #[test]
    fn settings_round_trip_and_normalize() {
        let mut s = ExportSettings::for_format(Format::Webm);
        s.quality = Quality::Custom {
            crf: Some(30),
            kbps: None,
        };
        s.size = SizeChoice::Height(720);
        s.gif.repeat = Loop::Times(3);
        let j = serde_json::to_string(&s).expect("json");
        let back: ExportSettings = serde_json::from_str(&j).expect("parse");
        assert_eq!(back, s);
        let odd = ExportSettings {
            video: VideoCodec::H264,
            audio: AudioCodec::Aac,
            ..s
        }
        .normalized();
        assert_eq!((odd.video, odd.audio), (VideoCodec::Vp9, AudioCodec::Opus));
        let partial: ExportSettings = serde_json::from_str(r#"{"format":"gif"}"#).expect("partial");
        assert_eq!(partial.format, Format::Gif);
    }
}
