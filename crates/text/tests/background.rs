use std::path::{Path, PathBuf};
use std::sync::Arc;

use sc_core::Error;
use sc_core::image::RgbaImage;
use sc_core::project::{Background, Fit, GradientStop, Rgba};
use sc_geometry::{PointF, SizePx};
use sc_text::{BackgroundCache, build_info, decode_image, encode_png, rasterize_background};
use skia_safe::{AlphaType, ColorType, ImageInfo, Pixmap, jpeg_encoder};

const SIZES: [SizePx; 2] = [SizePx { w: 1920, h: 1080 }, SizePx { w: 1080, h: 1920 }];

fn px(img: &RgbaImage, x: u32, y: u32) -> [u8; 4] {
    let i = y as usize * img.stride + x as usize * 4;
    [
        img.data[i],
        img.data[i + 1],
        img.data[i + 2],
        img.data[i + 3],
    ]
}

fn rgb8(c: [f64; 3]) -> [u8; 4] {
    let q = |v: f64| (v * 255.0).round() as u8;
    [q(c[0]), q(c[1]), q(c[2]), 255]
}

fn assert_near(got: [u8; 4], want: [u8; 4], tol: u8, what: &str) {
    let ok = got.iter().zip(want).all(|(&g, w)| g.abs_diff(w) <= tol);
    assert!(ok, "{what}: got {got:?}, want {want:?} ±{tol}");
}

fn stop(at: f64, c: [f64; 3]) -> GradientStop {
    GradientStop {
        at,
        color: Rgba([c[0], c[1], c[2], 1.0]),
        extra: Default::default(),
    }
}

fn no_dir() -> &'static Path {
    Path::new("/nonexistent-project")
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("sc_text-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets")).expect("create scratch project");
    dir
}

const A: [f64; 3] = [0.9, 0.1, 0.2];
const B: [f64; 3] = [0.1, 0.3, 0.95];

#[test]
fn solid_is_exact_and_opaque() {
    for size in SIZES {
        let bg = Background::Solid {
            color: Rgba([0.2, 0.4, 0.6, 0.25]),
        };
        let img = rasterize_background(&bg, size, no_dir()).expect("rasterize solid");
        assert_eq!(img.size, size);
        assert_eq!(img.stride, size.w as usize * 4);
        assert_eq!(img.data.len(), img.stride * size.h as usize);
        assert!(
            img.data
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [51, 102, 153, 255]),
            "every pixel of the {}x{} solid must be [51,102,153,255]",
            size.w,
            size.h
        );
    }
}

#[test]
fn linear_endpoints_match_stops() {
    for size in SIZES {
        let (w, h) = (size.w, size.h);
        let cases = [
            (90.0, (0, h / 2), (w - 1, h / 2)),
            (180.0, (w / 2, 0), (w / 2, h - 1)),
            (270.0, (w - 1, h / 2), (0, h / 2)),
            (135.0, (0, 0), (w - 1, h - 1)),
        ];
        for (angle, start, end) in cases {
            let bg = Background::Linear {
                stops: vec![stop(1.0, B), stop(0.0, A)],
                angle_deg: angle,
            };
            let img = rasterize_background(&bg, size, no_dir()).expect("rasterize linear");
            let what = format!("{w}x{h} {angle}°");
            assert_near(
                px(&img, start.0, start.1),
                rgb8(A),
                1,
                &format!("{what} start"),
            );
            assert_near(px(&img, end.0, end.1), rgb8(B), 1, &format!("{what} end"));
            assert!(
                img.data.as_chunks::<4>().0.iter().all(|p| p[3] == 255),
                "{what} opaque"
            );
        }
    }
}

#[test]
fn linear_middle_stop_lands_mid_line() {
    let size = SIZES[0];
    let mid = [0.5, 0.9, 0.1];
    let bg = Background::Linear {
        stops: vec![stop(0.0, A), stop(0.5, mid), stop(1.0, B)],
        angle_deg: 90.0,
    };
    let img = rasterize_background(&bg, size, no_dir()).expect("rasterize 3 stops");
    assert_near(px(&img, 960, 540), rgb8(mid), 2, "middle stop");
}

#[test]
fn radial_centre_and_edge() {
    for size in SIZES {
        let (w, h) = (size.w, size.h);
        let bg = Background::Radial {
            stops: vec![stop(0.0, A), stop(1.0, B)],
            center_norm: PointF { x: 0.25, y: 0.75 },
            radius_norm: 0.5,
        };
        let img = rasterize_background(&bg, size, no_dir()).expect("rasterize radial");
        let what = format!("{w}x{h}");
        assert_near(
            px(&img, w / 4, h * 3 / 4),
            rgb8(A),
            1,
            &format!("{what} centre"),
        );
        assert_near(
            px(&img, w - 1, 0),
            rgb8(B),
            1,
            &format!("{what} far corner"),
        );
        let r = (f64::from(w).hypot(f64::from(h)) / 4.0) as u32;
        let inside = px(&img, w / 4 + r / 2, h * 3 / 4);
        assert!(
            inside != rgb8(A) && inside != rgb8(B),
            "{what} half radius is a blend: {inside:?}"
        );
    }
}

#[test]
fn gradient_stop_count_is_checked() {
    let bg = Background::Linear {
        stops: vec![stop(0.0, A)],
        angle_deg: 90.0,
    };
    let err = rasterize_background(&bg, SIZES[0], no_dir()).expect_err("one stop");
    assert!(matches!(err, Error::Invalid(_)), "{err}");
}

const TL: [u8; 4] = [230, 20, 20, 255];
const TR: [u8; 4] = [20, 210, 20, 255];
const BL: [u8; 4] = [20, 20, 230, 255];
const BR: [u8; 4] = [240, 240, 240, 255];

fn quadrants(w: u32, h: u32) -> RgbaImage {
    let mut img = RgbaImage {
        size: SizePx { w, h },
        stride: w as usize * 4,
        data: vec![0; (w * h * 4) as usize],
    };
    for y in 0..h {
        for x in 0..w {
            let c = match (x < w / 2, y < h / 2) {
                (true, true) => TL,
                (false, true) => TR,
                (true, false) => BL,
                (false, false) => BR,
            };
            let i = y as usize * img.stride + x as usize * 4;
            img.data[i..i + 4].copy_from_slice(&c);
        }
    }
    img
}

fn encode_jpeg(img: &RgbaImage) -> Vec<u8> {
    let info = ImageInfo::new(
        (img.size.w as i32, img.size.h as i32),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    let mut pixels = img.data.clone();
    let pixmap = Pixmap::new(&info, &mut pixels, img.stride).expect("pixmap");
    let mut out = Vec::new();
    let opts = jpeg_encoder::Options {
        quality: 95,
        ..Default::default()
    };
    assert!(
        jpeg_encoder::encode(&pixmap, &mut out, &opts),
        "jpeg encode"
    );
    out
}

fn image_bg(asset: &str, focus: PointF) -> Background {
    Background::Image {
        asset: asset.to_string(),
        sha256: "test".to_string(),
        fit: Fit::Cover,
        focus_norm: focus,
    }
}

#[test]
fn image_cover_picks_the_focused_quadrant() {
    let dir = scratch("cover");
    let src = quadrants(800, 200);
    std::fs::write(dir.join("assets/bg.png"), encode_png(&src).expect("png")).expect("write png");
    std::fs::write(dir.join("assets/bg.jpg"), encode_jpeg(&src)).expect("write jpg");
    for (asset, tol) in [("assets/bg.png", 2), ("assets/bg.jpg", 12)] {
        for size in SIZES {
            let (w, h) = (size.w, size.h);
            let probe = |focus: PointF| {
                let img = rasterize_background(&image_bg(asset, focus), size, &dir)
                    .expect("rasterize image");
                assert!(
                    img.data.as_chunks::<4>().0.iter().all(|p| p[3] == 255),
                    "opaque"
                );
                (px(&img, w / 2, h / 4), px(&img, w / 2, h * 3 / 4))
            };
            let what = format!("{asset} {w}x{h}");
            let (top, bottom) = probe(PointF { x: 0.0, y: 0.5 });
            assert_near(top, TL, tol, &format!("{what} focus left, top"));
            assert_near(bottom, BL, tol, &format!("{what} focus left, bottom"));
            let (top, bottom) = probe(PointF { x: 1.0, y: 0.5 });
            assert_near(top, TR, tol, &format!("{what} focus right, top"));
            assert_near(bottom, BR, tol, &format!("{what} focus right, bottom"));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn image_cover_vertical_focus_on_a_tall_source() {
    let dir = scratch("tall");
    let src = quadrants(200, 800);
    std::fs::write(dir.join("assets/tall.png"), encode_png(&src).expect("png")).expect("write");
    let size = SIZES[0];
    let img = rasterize_background(
        &image_bg("assets/tall.png", PointF { x: 0.5, y: 1.0 }),
        size,
        &dir,
    )
    .expect("rasterize");
    assert_near(px(&img, 480, 540), BL, 2, "bottom focus, left");
    assert_near(px(&img, 1440, 540), BR, 2, "bottom focus, right");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn image_asset_errors() {
    let dir = scratch("errors");
    let size = SizePx { w: 64, h: 64 };
    let centre = PointF { x: 0.5, y: 0.5 };
    let err = rasterize_background(&image_bg("../escape.png", centre), size, &dir).expect_err("..");
    assert!(matches!(err, Error::Invalid(_)), "{err}");
    let err = rasterize_background(&image_bg("/etc/hosts", centre), size, &dir).expect_err("abs");
    assert!(matches!(err, Error::Invalid(_)), "{err}");
    let err = rasterize_background(&image_bg("assets/missing.png", centre), size, &dir)
        .expect_err("missing");
    assert!(matches!(err, Error::Io(_)), "{err}");
    std::fs::write(dir.join("assets/junk.png"), b"not an image").expect("write junk");
    let err =
        rasterize_background(&image_bg("assets/junk.png", centre), size, &dir).expect_err("junk");
    assert!(matches!(err, Error::Invalid(_)), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn zero_size_is_invalid() {
    let bg = Background::Solid {
        color: Rgba([0.0, 0.0, 0.0, 1.0]),
    };
    let err = rasterize_background(&bg, SizePx { w: 0, h: 10 }, no_dir()).expect_err("zero");
    assert!(matches!(err, Error::Invalid(_)), "{err}");
}

#[test]
fn cache_hits_share_the_arc_and_evict_lru() {
    let a = Background::Solid {
        color: Rgba([0.1, 0.2, 0.3, 1.0]),
    };
    let b = Background::Linear {
        stops: vec![stop(0.0, A), stop(1.0, B)],
        angle_deg: 45.0,
    };
    let s = SizePx { w: 320, h: 180 };
    let mut cache = BackgroundCache::new(2);
    let a1 = cache.get(&a, s, no_dir()).expect("a");
    let a2 = cache.get(&a, s, no_dir()).expect("a again");
    assert!(Arc::ptr_eq(&a1, &a2), "hit returns the same Arc");
    let a_other_size = cache
        .get(&a, SizePx { w: 180, h: 320 }, no_dir())
        .expect("a 9:16");
    assert!(!Arc::ptr_eq(&a1, &a_other_size), "size is part of the key");
    let a3 = cache.get(&a, s, no_dir()).expect("a third");
    assert!(Arc::ptr_eq(&a1, &a3), "still cached at capacity 2");
    let _b = cache.get(&b, s, no_dir()).expect("b");
    let a_portrait = cache
        .get(&a, SizePx { w: 180, h: 320 }, no_dir())
        .expect("evicted");
    assert!(
        !Arc::ptr_eq(&a_other_size, &a_portrait),
        "LRU entry was evicted"
    );
}

#[test]
fn png_round_trip_with_srgb_chunk() {
    let mut img = quadrants(37, 23);
    for (i, p) in img.data.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        p[0] = (i * 7) as u8;
        p[1] = (i * 13) as u8;
    }
    let png = encode_png(&img).expect("encode");
    let chunks = chunk_types(&png);
    assert_eq!(
        chunks.first().map(String::as_str),
        Some("IHDR"),
        "{chunks:?}"
    );
    assert_eq!(
        chunks.get(1).map(String::as_str),
        Some("sRGB"),
        "{chunks:?}"
    );
    assert_eq!(
        chunks.iter().filter(|c| *c == "sRGB").count(),
        1,
        "{chunks:?}"
    );
    assert!(!chunks.iter().any(|c| c == "iCCP"), "{chunks:?}");
    assert_eq!(
        chunks.last().map(String::as_str),
        Some("IEND"),
        "{chunks:?}"
    );
    let back = decode_image(&png).expect("decode");
    assert_eq!(back, img);
}

#[test]
fn png_encode_respects_stride() {
    let src = quadrants(10, 4);
    let mut padded = RgbaImage {
        size: src.size,
        stride: 48,
        data: vec![7; 48 * 4],
    };
    for y in 0..4 {
        padded.data[y * 48..y * 48 + 40].copy_from_slice(&src.data[y * 40..y * 40 + 40]);
    }
    let back = decode_image(&encode_png(&padded).expect("encode")).expect("decode");
    assert_eq!(back, src);
    let short = RgbaImage {
        size: src.size,
        stride: 40,
        data: vec![0; 10],
    };
    assert!(matches!(encode_png(&short), Err(Error::Invalid(_))));
}

#[test]
fn decode_jpeg_and_reject_garbage() {
    let src = quadrants(64, 64);
    let img = decode_image(&encode_jpeg(&src)).expect("decode jpeg");
    assert_eq!(img.size, src.size);
    assert_near(px(&img, 8, 8), TL, 12, "jpeg TL");
    assert_near(px(&img, 56, 56), BR, 12, "jpeg BR");
    assert!(matches!(decode_image(b"garbage"), Err(Error::Invalid(_))));
}

#[test]
fn build_info_reports_the_freetype_build() {
    let info = build_info();
    assert!(info.freetype, "{info:?}");
    assert_eq!(info.skia_milestone, 157, "{info:?}");
    assert!(info.jpeg_decode, "{info:?}");
    assert!(!info.webp_decode, "{info:?}");
}

fn chunk_types(png: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut pos = 8;
    while pos + 8 <= png.len() {
        let len = u32::from_be_bytes([png[pos], png[pos + 1], png[pos + 2], png[pos + 3]]) as usize;
        out.push(String::from_utf8_lossy(&png[pos + 4..pos + 8]).into_owned());
        pos += 12 + len;
    }
    out
}
