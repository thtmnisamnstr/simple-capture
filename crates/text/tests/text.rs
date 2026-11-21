use sc_core::image::RgbaImage;
use sc_core::project::{
    Extra, Rgba, TextBackground, TextBackgroundKind, TextFill, TextGlow, TextItem, TextLinearFill,
    TextOutline, TextShadow, TextStyle,
};
use sc_geometry::{PointF, SizePx};
use sc_text::TextEngine;
use sc_text::fonts::{BUNDLED, FontLibrary};
use sc_text::item::{Reveal, prepare};

const HD: SizePx = SizePx { w: 1920, h: 1080 };

fn style(family: &str) -> TextStyle {
    TextStyle {
        font_family: family.into(),
        ..TextStyle::default()
    }
}

fn ink(img: &RgbaImage) -> usize {
    img.data.chunks_exact(4).filter(|p| p[3] > 16).count()
}

fn colorful(img: &RgbaImage) -> usize {
    img.data
        .chunks_exact(4)
        .filter(|p| {
            p[3] > 200 && {
                let (r, g, b) = (i32::from(p[0]), i32::from(p[1]), i32::from(p[2]));
                (r - g).abs() > 40 || (g - b).abs() > 40 || (r - b).abs() > 40
            }
        })
        .count()
}

#[test]
fn every_bundled_family_renders_latin_text_without_missing_glyphs() {
    let mut fonts = FontLibrary::new().expect("fonts");
    for fam in BUNDLED {
        for weight in [400, 700] {
            let p = prepare(
                &mut fonts,
                "Héllo Ångström, ça va? 123",
                &TextStyle {
                    weight,
                    ..style(fam.name)
                },
                HD,
                1.0,
            )
            .unwrap_or_else(|e| panic!("{}: {e}", fam.name));
            assert_eq!(p.unresolved, 0, "{} {weight}", fam.name);
            assert!(ink(&p.text) > 500, "{} drew nothing", fam.name);
        }
    }
}

#[test]
fn heavier_weights_draw_more_ink() {
    let mut fonts = FontLibrary::new().expect("fonts");
    let thin = prepare(
        &mut fonts,
        "Weight",
        &TextStyle {
            weight: 300,
            ..style("Inter")
        },
        HD,
        1.0,
    )
    .expect("thin");
    let black = prepare(
        &mut fonts,
        "Weight",
        &TextStyle {
            weight: 900,
            ..style("Inter")
        },
        HD,
        1.0,
    )
    .expect("black");
    assert!(
        ink(&black.text) > ink(&thin.text) * 5 / 4,
        "{} vs {}",
        ink(&black.text),
        ink(&thin.text)
    );
}

#[test]
fn color_emoji_render_in_color() {
    let mut fonts = FontLibrary::new().expect("fonts");
    let p = prepare(
        &mut fonts,
        "Go 🎉🚀",
        &TextStyle {
            fill: TextFill::Solid(Rgba([1.0, 1.0, 1.0, 1.0])),
            ..style("Inter")
        },
        HD,
        1.0,
    )
    .expect("emoji");
    assert_eq!(p.unresolved, 0);
    assert!(
        colorful(&p.text) > 300,
        "emoji pixels: {}",
        colorful(&p.text)
    );
}

#[test]
fn styles_change_the_pixels() {
    let mut fonts = FontLibrary::new().expect("fonts");
    let plain = prepare(&mut fonts, "Style", &style("Montserrat"), HD, 1.0).expect("plain");
    let mut outlined = style("Montserrat");
    outlined.outline = Some(TextOutline {
        width_1080: 6.0,
        color: Rgba([0.0, 0.0, 0.0, 1.0]),
        extra: Extra::new(),
    });
    let mut shadow = style("Montserrat");
    shadow.shadow = Some(TextShadow {
        blur_1080: 8.0,
        offset_1080: PointF { x: 6.0, y: 6.0 },
        color: Rgba([0.0, 0.0, 0.0, 0.8]),
        extra: Extra::new(),
    });
    let mut glow = style("Tilt Neon");
    glow.glow = Some(TextGlow {
        radius_1080: 14.0,
        color: Rgba([1.0, 0.2, 0.8, 1.0]),
        extra: Extra::new(),
    });
    let mut gradient = style("Montserrat");
    gradient.fill = TextFill::Linear(TextLinearFill {
        stops: [Rgba([1.0, 0.0, 0.0, 1.0]), Rgba([0.0, 0.0, 1.0, 1.0])],
        angle_deg: 0.0,
        extra: Extra::new(),
    });
    for (name, s) in [("outline", outlined), ("shadow", shadow), ("glow", glow)] {
        let p = prepare(&mut fonts, "Style", &s, HD, 1.0).expect(name);
        assert!(
            ink(&p.text) > ink(&plain.text),
            "{name}: {} vs {}",
            ink(&p.text),
            ink(&plain.text)
        );
    }
    let g = prepare(&mut fonts, "Style", &gradient, HD, 1.0).expect("gradient");
    assert!(colorful(&g.text) > 200);
}

fn boxed(kind: TextBackgroundKind) -> TextStyle {
    let mut s = style("Inter");
    s.background = Some(TextBackground {
        kind,
        color: Rgba([0.1, 0.1, 0.1, 1.0]),
        radius_1080: 16.0,
        padding_1080: PointF { x: 18.0, y: 8.0 },
        extra: Extra::new(),
    });
    s
}

#[test]
fn line_boxes_merge_and_invert() {
    let mut fonts = FontLibrary::new().expect("fonts");
    let text = "A longer first line\nshort\nand a third one";
    let per = prepare(
        &mut fonts,
        text,
        &boxed(TextBackgroundKind::PerLine),
        HD,
        1.0,
    )
    .expect("per");
    let merged = prepare(
        &mut fonts,
        text,
        &boxed(TextBackgroundKind::Merged),
        HD,
        1.0,
    )
    .expect("merged");
    let inv = prepare(
        &mut fonts,
        text,
        &boxed(TextBackgroundKind::Inverted),
        HD,
        1.0,
    )
    .expect("inv");
    let (pb, mb) = (
        per.boxes.expect("per boxes"),
        merged.boxes.expect("merged boxes"),
    );
    assert_eq!(pb.size, mb.size);
    let differ = pb
        .data
        .iter()
        .zip(&mb.data)
        .filter(|(a, b)| a.abs_diff(**b) > 64)
        .count();
    assert!(
        differ > 400,
        "per-line and merged boxes differ ({differ} bytes)"
    );
    let ib = inv.boxes.expect("inverted boxes");
    let light = |img: &RgbaImage| {
        img.data
            .chunks_exact(4)
            .filter(|p| p[3] > 200 && p[0] > 200)
            .count()
    };
    assert!(
        light(&ib) > light(&mb),
        "inverted box takes the text colour"
    );
}

#[test]
fn reveals_grow_and_never_cut_the_box() {
    let mut fonts = FontLibrary::new().expect("fonts");
    let p = prepare(
        &mut fonts,
        "One two three\nfour five",
        &boxed(TextBackgroundKind::Merged),
        HD,
        1.0,
    )
    .expect("prepare");
    let boxes = ink(p.boxes.as_ref().expect("boxes"));
    let none = p.compose(Reveal::Clusters(0));
    assert_eq!(
        ink(&none),
        boxes,
        "nothing but the box before the first letter"
    );
    let mut last = 0;
    for n in [1, 4, 9, p.cluster_count()] {
        let img = p.compose(Reveal::Clusters(n));
        let shown: usize = img.data.chunks_exact(4).filter(|x| x[0] > 128).count();
        assert!(shown >= last, "typewriter at {n}");
        last = shown;
    }
    assert_eq!(p.word_count(), 5);
    let w1 = p.compose(Reveal::Words(1));
    let w5 = p.compose(Reveal::Words(5));
    let white = |img: &RgbaImage| img.data.chunks_exact(4).filter(|x| x[0] > 128).count();
    assert!(white(&w1) < white(&w5));
    assert_eq!(white(&w5), white(&p.compose(Reveal::All)));
}

#[test]
fn layout_scales_with_the_canvas_and_the_item() {
    let mut fonts = FontLibrary::new().expect("fonts");
    let hd = prepare(&mut fonts, "Scale", &style("Inter"), HD, 1.0).expect("hd");
    let uhd = prepare(
        &mut fonts,
        "Scale",
        &style("Inter"),
        SizePx { w: 3840, h: 2160 },
        1.0,
    )
    .expect("4k");
    let big = prepare(&mut fonts, "Scale", &style("Inter"), HD, 1.5).expect("big");
    let ratio = |a: u32, b: u32| f64::from(a) / f64::from(b);
    assert!((ratio(uhd.size.w, hd.size.w) - 2.0).abs() < 0.03);
    assert!((ratio(big.size.w, hd.size.w) - 1.5).abs() < 0.03);
}

#[test]
fn engine_places_the_block_centre_and_rotates() {
    let mut e = TextEngine::new().expect("engine");
    let mut item = TextItem::new(0, 3_000_000_000, "Placed");
    item.transform.center_norm = PointF { x: 0.25, y: 0.75 };
    let f = e
        .frame(&item, 1_000_000_000, HD)
        .expect("frame")
        .expect("visible");
    let cx = f.quad.iter().map(|p| p.0).sum::<f32>() / 4.0;
    let cy = f.quad.iter().map(|p| p.1).sum::<f32>() / 4.0;
    assert!(
        (cx - 480.0).abs() < 0.5 && (cy - 810.0).abs() < 0.5,
        "{cx},{cy}"
    );
    let w = f.quad[1].0 - f.quad[0].0;
    item.transform.rotation_deg = 90.0;
    let r = e
        .frame(&item, 1_000_000_000, HD)
        .expect("frame")
        .expect("visible");
    assert!(
        (r.quad[1].1 - r.quad[0].1 - w).abs() < 0.5,
        "rotated 90°: {:?}",
        r.quad
    );
    let shown = e
        .frames(std::slice::from_ref(&item), 3_000_000_000, HD)
        .expect("frames");
    assert!(shown.is_empty(), "hidden at its end time");
}
