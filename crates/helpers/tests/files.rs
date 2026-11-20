use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use sc_geometry::SizePx;
use sc_helpers::part::{PartFile, sweep_older_than, sweep_stale};
use sc_helpers::wav::{WavFormat, WavWriter};
use sc_helpers::y4m::{self, Y4mHeader};
use sc_helpers::{HelperKind, locate};

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        let p = std::env::temp_dir().join(format!(
            "sc-helpers-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&p).expect("temp dir");
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn y4m_header_bytes() {
    let h = Y4mHeader {
        size: SizePx { w: 1920, h: 1080 },
        fps_num: 30,
        fps_den: 1,
    };
    assert_eq!(
        y4m::header(&h).as_bytes(),
        b"YUV4MPEG2 W1920 H1080 F30:1 Ip A1:1 C420mpeg2 XYSCSS=420MPEG2 XCOLORRANGE=LIMITED\n"
    );
    let ntsc = Y4mHeader {
        size: SizePx { w: 3, h: 5 },
        fps_num: 30000,
        fps_den: 1001,
    };
    assert!(y4m::header(&ntsc).starts_with("YUV4MPEG2 W3 H5 F30000:1001 "));
    assert_eq!(y4m::FRAME, b"FRAME\n");
    assert_eq!(
        y4m::i420_frame_bytes(SizePx { w: 1920, h: 1080 }),
        3_110_400
    );
    assert_eq!(y4m::i420_frame_bytes(SizePx { w: 3, h: 5 }), 15 + 2 * 2 * 3);
}

#[test]
fn part_file_commit_never_overwrites() {
    let d = TempDir::new("part");
    let fin = d.0.join("out.mp4");
    let part = PartFile::create(&fin).expect("create");
    assert_eq!(part.path(), d.0.join("out.mp4.part"));
    assert!(part.path().is_file());
    assert!(
        PartFile::create(&fin).is_err(),
        "a second .part for the same target must fail"
    );
    std::fs::write(part.path(), b"video").expect("write");
    let got = part.commit().expect("commit");
    assert_eq!(got, fin);
    assert_eq!(std::fs::read(&fin).expect("read"), b"video");
    assert!(!d.0.join("out.mp4.part").exists());

    assert!(
        PartFile::create(&fin).is_err(),
        "create must refuse an existing target"
    );

    let fin2 = d.0.join("race.mp4");
    let part2 = PartFile::create(&fin2).expect("create");
    std::fs::write(part2.path(), b"new").expect("write");
    std::fs::write(&fin2, b"someone else's").expect("write");
    let part2_path = part2.path().to_path_buf();
    let e = part2.commit().expect_err("commit must not overwrite");
    assert!(e.to_string().contains("race.mp4"), "{e}");
    assert_eq!(std::fs::read(&fin2).expect("read"), b"someone else's");
    assert!(
        !part2_path.exists(),
        "a failed commit drops (deletes) the .part"
    );
}

#[test]
fn part_file_discard_and_drop_delete_the_part() {
    let d = TempDir::new("discard");
    let a = PartFile::create(&d.0.join("a.mp4")).expect("create");
    let ap = a.path().to_path_buf();
    a.discard();
    assert!(!ap.exists());
    {
        let _b = PartFile::create(&d.0.join("b.gif")).expect("create");
    }
    assert!(!d.0.join("b.gif.part").exists());
    assert!(!d.0.join("a.mp4").exists());
}

fn age(p: &Path, by: Duration) {
    let f = std::fs::File::options().write(true).open(p).expect("open");
    f.set_modified(SystemTime::now() - by).expect("set mtime");
}

#[test]
fn sweep_removes_only_old_export_parts() {
    let d = TempDir::new("sweep");
    for name in [
        "old.mp4.part",
        "old2.gif.part",
        "fresh.mp4.part",
        "download.dmg.part",
        "keep.mp4",
    ] {
        std::fs::write(d.0.join(name), b"x").expect("write");
    }
    std::fs::create_dir(d.0.join("dir.mp4.part")).expect("mkdir");
    for name in ["old.mp4.part", "old2.gif.part", "download.dmg.part"] {
        age(&d.0.join(name), Duration::from_secs(3600));
    }
    assert_eq!(sweep_stale(&d.0).expect("sweep"), 2);
    let mut left: Vec<String> = std::fs::read_dir(&d.0)
        .expect("ls")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(
        left,
        [
            "dir.mp4.part",
            "download.dmg.part",
            "fresh.mp4.part",
            "keep.mp4"
        ]
    );
    assert_eq!(sweep_older_than(&d.0, Duration::ZERO).expect("sweep"), 1);
    assert!(sweep_stale(&d.0.join("missing")).is_err());
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

#[test]
fn wav_s16_header_is_patched_on_finish() {
    let d = TempDir::new("wav16");
    let p = d.0.join("mix.wav");
    let mut w = WavWriter::create(&p, 48_000, 2, WavFormat::S16).expect("create");
    w.write_f32(&[0.0, 1.0, -1.0, 2.0, 0.5, -0.5])
        .expect("write");
    assert_eq!(w.frames(), 3);
    assert!(w.write_f32(&[0.0]).is_err(), "half a frame must fail");
    assert_eq!(w.finish().expect("finish"), 3);
    let b = std::fs::read(&p).expect("read");
    assert_eq!(b.len(), 44 + 12);
    assert_eq!(&b[0..4], b"RIFF");
    assert_eq!(u32_at(&b, 4), 36 + 12);
    assert_eq!(&b[8..16], b"WAVEfmt ");
    assert_eq!(u32_at(&b, 16), 16);
    assert_eq!(u16_at(&b, 20), 1);
    assert_eq!(u16_at(&b, 22), 2);
    assert_eq!(u32_at(&b, 24), 48_000);
    assert_eq!(u32_at(&b, 28), 48_000 * 4);
    assert_eq!(u16_at(&b, 32), 4);
    assert_eq!(u16_at(&b, 34), 16);
    assert_eq!(&b[36..40], b"data");
    assert_eq!(u32_at(&b, 40), 12);
    let s: Vec<i16> = b[44..]
        .chunks(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    assert_eq!(s, [0, 32767, -32767, 32767, 16384, -16384]);
}

#[test]
fn wav_f32_header_is_patched_on_finish() {
    let d = TempDir::new("wavf");
    let p = d.0.join("mix.wav");
    let mut w = WavWriter::create(&p, 44_100, 1, WavFormat::F32).expect("create");
    let samples: Vec<f32> = (0..1000).map(|i| i as f32 / 1000.0).collect();
    w.write_f32(&samples).expect("write");
    w.write_f32(&samples[..10]).expect("write");
    assert_eq!(w.finish().expect("finish"), 1010);
    let b = std::fs::read(&p).expect("read");
    assert_eq!(b.len(), 58 + 1010 * 4);
    assert_eq!(u32_at(&b, 4), (b.len() - 8) as u32);
    assert_eq!(u32_at(&b, 16), 18);
    assert_eq!(u16_at(&b, 20), 3);
    assert_eq!(u16_at(&b, 22), 1);
    assert_eq!(u32_at(&b, 24), 44_100);
    assert_eq!(u32_at(&b, 28), 44_100 * 4);
    assert_eq!(u16_at(&b, 32), 4);
    assert_eq!(u16_at(&b, 34), 32);
    assert_eq!(u16_at(&b, 36), 0);
    assert_eq!(&b[38..42], b"fact");
    assert_eq!(u32_at(&b, 46), 1010);
    assert_eq!(&b[50..54], b"data");
    assert_eq!(u32_at(&b, 54), 4040);
    assert_eq!(
        f32::from_le_bytes([b[58 + 4], b[59 + 4], b[60 + 4], b[61 + 4]]),
        0.001
    );
}

#[test]
fn locate_error_names_every_path_tried() {
    if std::env::var_os("SC_GIFSKI").is_some() {
        eprintln!("SKIPPED: SC_GIFSKI is set");
        return;
    }
    match locate(HelperKind::Gifski) {
        Ok(p) => assert!(p.ends_with(format!("gifski{}", std::env::consts::EXE_SUFFIX))),
        Err(e) => {
            let msg = e.to_string();
            assert!(msg.contains("$PATH"), "{msg}");
            let exe_dir = std::env::current_exe().expect("exe");
            let exe_dir = exe_dir.parent().expect("parent");
            assert!(msg.contains(&exe_dir.display().to_string()), "{msg}");
            assert!(
                msg.replace('\\', "/").contains(&format!(
                    "third_party/out/{}",
                    sc_helpers::platform_dir_name()
                )),
                "{msg}"
            );
        }
    }
}
