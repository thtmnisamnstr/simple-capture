//! Runs the bundled helpers when they are built (`just helpers`); `SC_REQUIRE_HELPERS=1`
//! turns a missing helper into a failure instead of a skip.

use std::path::PathBuf;
use std::process::Command;
use std::time::SystemTime;

use sc_geometry::SizePx;
use sc_helpers::part::PartFile;
use sc_helpers::y4m::{self, Y4mHeader};
use sc_helpers::{HelperKind, HelperProcess, SpawnSpec, helper_info, locate};

fn find(kind: HelperKind) -> Option<PathBuf> {
    match locate(kind) {
        Ok(p) => Some(p),
        Err(e) if std::env::var("SC_REQUIRE_HELPERS").as_deref() == Ok("1") => {
            panic!("SC_REQUIRE_HELPERS=1 but {e}")
        }
        Err(e) => {
            eprintln!("SKIPPED: {e}");
            None
        }
    }
}

fn pattern(size: SizePx, n: u32, out: &mut Vec<u8>) {
    let (w, h) = (size.w as usize, size.h as usize);
    out.clear();
    let bar = (n as usize * 4) % w;
    for y in 0..h {
        for x in 0..w {
            let on = x >= bar && x < bar + 16;
            out.push(if on { 235 } else { 16 + ((x + y) % 200) as u8 });
        }
    }
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    out.extend(std::iter::repeat_n(90u8, cw * ch));
    out.extend(std::iter::repeat_n(170u8, cw * ch));
}

#[test]
fn x264_encodes_two_seconds_of_y4m_into_a_part_file() {
    let Some(ffmpeg) = find(HelperKind::Ffmpeg) else {
        return;
    };
    let dir = std::env::temp_dir().join(format!(
        "sc-helpers-x264-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let size = SizePx { w: 320, h: 240 };
    let (fps, frames) = (30u32, 60u32);
    let fin = dir.join("pattern.mp4");
    let part = PartFile::create(&fin).expect("part");
    let args: Vec<String> = [
        "-hide_banner",
        "-nostdin",
        "-loglevel",
        "error",
        "-nostats",
        "-progress",
        "pipe:2",
        "-f",
        "yuv4mpegpipe",
        "-i",
        "pipe:0",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-pix_fmt",
        "yuv420p",
        "-f",
        "mp4",
        "-y",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .chain([part.path().display().to_string()])
    .collect();
    let h = HelperProcess::spawn(SpawnSpec {
        program: ffmpeg,
        args,
        frame_bytes: y4m::FRAME.len() + y4m::i420_frame_bytes(size),
        queue_frames: 3,
    })
    .expect("spawn ffmpeg");
    let header = y4m::header(&Y4mHeader {
        size,
        fps_num: fps,
        fps_den: 1,
    });
    h.write_bytes(header.as_bytes()).expect("header");
    let mut buf = Vec::new();
    for n in 0..frames {
        pattern(size, n, &mut buf);
        h.write_frame_parts(&[y4m::FRAME, &buf]).expect("frame");
    }
    let r = h.finish().expect("finish");
    assert!(r.success(), "{r:?}");
    assert_eq!(r.frames_written, frames as u64);
    let out = part.commit().expect("commit");

    if let Some(ffprobe) = find(HelperKind::Ffprobe) {
        let o = Command::new(ffprobe)
            .args([
                "-v",
                "error",
                "-count_frames",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=codec_name,width,height,nb_read_frames",
                "-of",
                "default=nw=1",
            ])
            .arg(&out)
            .output()
            .expect("ffprobe");
        let text = String::from_utf8_lossy(&o.stdout);
        assert!(o.status.success(), "{text}");
        for want in [
            "codec_name=h264",
            "width=320",
            "height=240",
            "nb_read_frames=60",
        ] {
            assert!(text.lines().any(|l| l == want), "missing {want} in {text}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ffmpeg_info_reports_network_disabled() {
    let Some(ffmpeg) = find(HelperKind::Ffmpeg) else {
        return;
    };
    let i = helper_info(&ffmpeg).expect("info");
    assert!(i.network_disabled, "{:?}", i.buildconf);
    assert!(
        i.version_line.starts_with("ffmpeg version "),
        "{}",
        i.version_line
    );
    assert_eq!(i.sha256.len(), 64);
    assert!(
        i.buildconf.iter().any(|f| f == "--enable-libx264"),
        "{:?}",
        i.buildconf
    );
    if let Ok(gifski) = locate(HelperKind::Gifski) {
        let g = helper_info(&gifski).expect("gifski info");
        assert!(g.version_line.starts_with("gifski "), "{}", g.version_line);
        assert!(g.buildconf.is_empty());
    }
}
