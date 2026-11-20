mod composite;
mod fonts;
mod gpu;
mod specimen;
mod text;

use composite::{Affine, Draw, Image, cpu_composite, diff};
use fonts::{Fonts, MgrKind, load_bundled, load_bundled_with, make_mgr};
use skia_safe::{
    AlphaType, Color, ColorType, Data, ImageInfo, Paint, PaintStyle, Rect, images, surfaces,
    textlayout::TextAlign,
};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;
use text::{BgBox, Fill, Glow, Laid, Raster, Shadow, Stroke, TextSpec, layout, rasterize};

const EMOJI_SAMPLE: &str = "\u{1F600} \u{1F44D}\u{1F3FD} \u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466} \u{1F469}\u{1F3FF}\u{200D}\u{1F4BB} \u{2764}\u{FE0F}\u{200D}\u{1F525} \u{1F1EF}\u{1F1F5} \u{1F1E7}\u{1F1F7} \u{1F3F3}\u{FE0F}\u{200D}\u{1F308} \u{1F3F4}\u{200D}\u{2620}\u{FE0F}";

struct Placed {
    spec: TextSpec,
    cx: f32,
    cy: f32,
    rot: f32,
    opacity: f32,
}

fn hex(s: &str, a: u8) -> Color {
    let v = u32::from_str_radix(s.trim_start_matches('#'), 16).unwrap();
    Color::from_argb(a, (v >> 16) as u8, (v >> 8) as u8, v as u8)
}

fn base(name: &'static str, text: &str, family: &'static str, size: f32) -> TextSpec {
    TextSpec {
        name,
        text: text.to_string(),
        family,
        size,
        align: TextAlign::Center,
        letter_spacing: 0.0,
        line_height: 1.15,
        uppercase: false,
        max_width: 560.0,
        fill: Fill::Solid(Color::WHITE),
        stroke: None,
        shadow: None,
        glow: None,
        bg: None,
    }
}

fn scene(system_family: Option<&str>) -> Vec<Placed> {
    let tiktok = |color: Color| BgBox { color, radius: 16.0, pad_x: 18.0, pad_y: 6.0 };
    let soft_shadow = Shadow { color: hex("000000", 150), dx: 0.0, dy: 5.0, blur: 7.0 };
    let mut v = Vec::new();
    let mut p = |spec: TextSpec, cx: f32, cy: f32, rot: f32, opacity: f32| v.push(Placed { spec, cx, cy, rot, opacity });

    let mut s = base(
        "title_emoji",
        &format!("Ship it today \u{1F680}\nZWJ, tones, flags: {EMOJI_SAMPLE}"),
        "Oswald Bold",
        62.0,
    );
    s.uppercase = true;
    s.max_width = 1750.0;
    s.shadow = Some(soft_shadow.clone());
    p(s, 960.0, 115.0, 0.0, 1.0);

    let mut s = base("outline", "Outline \u{270F}\u{FE0F}\nbehind the fill", "Fredoka SemiBold", 72.0);
    s.stroke = Some(Stroke { color: Color::BLACK, width: 7.0 });
    p(s, 330.0, 335.0, 0.0, 1.0);

    let mut s = base("gradient", "Gradient \u{1F525}", "Oswald Bold", 96.0);
    s.uppercase = true;
    s.letter_spacing = 8.0;
    s.max_width = 900.0;
    s.fill = Fill::Gradient(vec![hex("ff3d77", 255), hex("ff9a3d", 255), hex("ffe53d", 255)]);
    s.shadow = Some(Shadow { color: hex("000000", 110), dx: 0.0, dy: 4.0, blur: 4.0 });
    p(s, 960.0, 340.0, 0.0, 1.0);

    let mut s = base("neon", "Open late \u{2728}", "Fredoka SemiBold", 92.0);
    s.fill = Fill::Solid(hex("fff0fc", 255));
    s.glow = Some(Glow { color: hex("ff2bd6", 255), radius: 12.0 });
    p(s, 1590.0, 335.0, 0.0, 1.0);

    let mut s = base(
        "tiktok_center",
        "This is how TikTok\nmerges line\nbackgrounds together \u{1F389}",
        "Fredoka SemiBold",
        50.0,
    );
    s.fill = Fill::Solid(Color::BLACK);
    s.bg = Some(tiktok(Color::WHITE));
    s.line_height = 1.2;
    p(s, 330.0, 590.0, 0.0, 1.0);

    let mut s = base(
        "tiktok_left",
        "Left aligned boxes\nshort\nconcave corners too \u{1F60E}",
        "Fredoka SemiBold",
        50.0,
    );
    s.align = TextAlign::Left;
    s.fill = Fill::Solid(Color::BLACK);
    s.bg = Some(tiktok(Color::WHITE));
    s.line_height = 1.2;
    p(s, 960.0, 590.0, 0.0, 1.0);

    let mut s = base("inverted", "Inverted style\nwhite on color \u{1F4AC}", "Fredoka SemiBold", 54.0);
    s.bg = Some(tiktok(hex("fe2c55", 255)));
    s.line_height = 1.2;
    p(s, 1590.0, 590.0, 0.0, 1.0);

    let mut s = base("translucent_box", "Line height 1.5\nletter spacing +3\ntranslucent box", "Oswald", 46.0);
    s.align = TextAlign::Left;
    s.line_height = 1.5;
    s.letter_spacing = 3.0;
    s.bg = Some(BgBox { color: hex("000000", 140), radius: 10.0, pad_x: 16.0, pad_y: 2.0 });
    p(s, 330.0, 840.0, 0.0, 1.0);

    let mut s = base("shadow_handwriting", "handwritten,\ndrop shadow \u{270D}\u{1F3FD}", "Caveat Bold", 84.0);
    s.shadow = Some(Shadow { color: hex("000000", 190), dx: 6.0, dy: 8.0, blur: 5.0 });
    s.fill = Fill::Solid(hex("ffe9a8", 255));
    p(s, 960.0, 850.0, 0.0, 1.0);

    let mut s = base("rotated", "Rotated -8\u{00B0}\nat 85% opacity", "Oswald Bold", 64.0);
    s.uppercase = true;
    s.fill = Fill::Solid(hex("ffd400", 255));
    s.stroke = Some(Stroke { color: Color::BLACK, width: 5.0 });
    p(s, 1590.0, 850.0, -8.0, 0.85);

    if let Some(fam) = system_family {
        let mut s = base("fontdb_system", &format!("fontdb system font: {fam}"), "System Pick", 34.0);
        s.max_width = 900.0;
        s.shadow = Some(soft_shadow.clone());
        p(s, 960.0, 1030.0, 0.0, 1.0);
    }
    v
}

fn background(w: u32, h: u32, s: f32) -> Image {
    let mut px = vec![0u8; (w * h * 4) as usize];
    {
        let info = ImageInfo::new((w as i32, h as i32), ColorType::RGBA8888, AlphaType::Premul, None);
        let mut surf = surfaces::wrap_pixels(&info, &mut px, (w * 4) as usize, None).unwrap();
        let c = surf.canvas();
        c.scale((s, s));
        c.clear(hex("1d222b", 255));
        let mut p = Paint::default();
        p.set_anti_alias(true);
        p.set_color(hex("262c37", 255));
        for i in 0..33 {
            c.draw_rect(Rect::from_xywh(i as f32 * 60.0, 0.0, 1.0, 1080.0), &p);
        }
        for i in 0..19 {
            c.draw_rect(Rect::from_xywh(0.0, i as f32 * 60.0, 1920.0, 1.0), &p);
        }
        let blocks = [
            ("3b82f6", 120.0, 220.0, 460.0, 260.0),
            ("f1f1f4", 700.0, 200.0, 520.0, 540.0),
            ("10b981", 1360.0, 470.0, 420.0, 230.0),
            ("f59e0b", 140.0, 700.0, 380.0, 260.0),
            ("e5e7eb", 1340.0, 740.0, 480.0, 240.0),
            ("8b5cf6", 760.0, 780.0, 420.0, 160.0),
        ];
        for (col, x, y, bw, bh) in blocks {
            p.set_color(hex(col, 255));
            c.draw_round_rect(Rect::from_xywh(x, y, bw, bh), 14.0, 14.0, &p);
        }
        p.set_color(hex("9ca3af", 255));
        for i in 0..14 {
            c.draw_rect(Rect::from_xywh(730.0, 250.0 + i as f32 * 34.0, 300.0 + (i % 4) as f32 * 40.0, 12.0), &p);
        }
    }
    Image { w, h, px }
}

fn raster_image(r: &Raster) -> Image {
    Image { w: r.w, h: r.h, px: r.px.clone() }
}

fn place(r: &Raster, pl: &Placed, s: f32) -> Affine {
    let mut xf = Affine::translate(-(r.w as f32) / 2.0, -(r.h as f32) / 2.0)
        .then(Affine::rotate_deg(pl.rot))
        .then(Affine::translate(pl.cx * s, pl.cy * s));
    if pl.rot == 0.0 {
        xf.tx = xf.tx.round();
        xf.ty = xf.ty.round();
    }
    xf
}

fn full_draw(i: usize, r: &Raster, pl: &Placed, s: f32) -> Draw {
    Draw { tex: i, src: [0.0, 0.0, r.w as f32, r.h as f32], xf: place(r, pl, s), opacity: pl.opacity }
}

fn write_png(path: &Path, img: &Image) {
    let opaque = img.px.chunks(4).all(|p| p[3] == 255);
    let f = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), img.w, img.h);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::High);
    if opaque {
        enc.set_color(png::ColorType::Rgb);
        let rgb: Vec<u8> = img.px.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
        enc.write_header().unwrap().write_image_data(&rgb).unwrap();
    } else {
        enc.set_color(png::ColorType::Rgba);
        let un: Vec<u8> = img
            .px
            .chunks(4)
            .flat_map(|p| {
                if p[3] == 0 {
                    [0, 0, 0, 0]
                } else {
                    let a = p[3] as f32 / 255.0;
                    [
                        (p[0] as f32 / a).round().min(255.0) as u8,
                        (p[1] as f32 / a).round().min(255.0) as u8,
                        (p[2] as f32 / a).round().min(255.0) as u8,
                        p[3],
                    ]
                }
            })
            .collect();
        enc.write_header().unwrap().write_image_data(&un).unwrap();
    }
}

fn crop(img: &Image, x: u32, y: u32, w: u32, h: u32) -> Image {
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for row in y..y + h {
        let s = ((row * img.w + x) * 4) as usize;
        px.extend_from_slice(&img.px[s..s + (w * 4) as usize]);
    }
    Image { w, h, px }
}

fn stack(imgs: &[Image], gap: u32, bgc: [u8; 4]) -> Image {
    let w = imgs.iter().map(|i| i.w).max().unwrap();
    let h = imgs.iter().map(|i| i.h).sum::<u32>() + gap * (imgs.len() as u32 - 1);
    let mut px: Vec<u8> = bgc.iter().copied().cycle().take((w * h * 4) as usize).collect();
    let mut y0 = 0;
    for im in imgs {
        for row in 0..im.h {
            let d = (((y0 + row) * w) * 4) as usize;
            let s = (row * im.w * 4) as usize;
            px[d..d + (im.w * 4) as usize].copy_from_slice(&im.px[s..s + (im.w * 4) as usize]);
        }
        y0 += im.h + gap;
    }
    Image { w, h, px }
}

fn to_skimage(img: &Image) -> skia_safe::Image {
    let info = ImageInfo::new((img.w as i32, img.h as i32), ColorType::RGBA8888, AlphaType::Premul, None);
    images::raster_from_data(&info, Data::new_copy(&img.px), (img.w * 4) as usize).unwrap()
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn ms<F: FnMut()>(reps: usize, mut f: F) -> f64 {
    let mut t = Vec::with_capacity(reps);
    for _ in 0..reps {
        let s = Instant::now();
        f();
        t.push(s.elapsed().as_secs_f64() * 1000.0);
    }
    median(t)
}

fn colored_pixels(r: &Raster) -> usize {
    r.px
        .chunks(4)
        .filter(|p| {
            let (a, b, c) = (p[0] as i32, p[1] as i32, p[2] as i32);
            p[3] > 128 && ((a - b).abs() > 40 || (b - c).abs() > 40 || (a - c).abs() > 40)
        })
        .count()
}

fn emoji_matrix(dir: &Path, report: &mut String) -> Image {
    let mut rows = Vec::new();
    let label_fonts = load_bundled(dir, "Noto-COLRv1.ttf");
    for kind in [MgrKind::FreeType, MgrKind::System] {
        for file in ["NotoColorEmoji-CBDT.ttf", "Noto-COLRv1.ttf"] {
            let tag = format!("{kind:?} fontmgr + {file}");
            let Some(mgr) = make_mgr(kind) else {
                let _ = writeln!(report, "| {tag} | fontmgr unavailable in this Skia build | - | - | - |");
                continue;
            };
            let f = load_bundled_with(dir, file, mgr, kind);
            let mut s = base("emoji", EMOJI_SAMPLE, "Oswald", 56.0);
            s.max_width = 1200.0;
            s.align = TextAlign::Left;
            s.fill = Fill::Solid(Color::BLACK);
            let laid = layout(&f, &s);
            let r = rasterize(&laid, 1.0);
            let colored = colored_pixels(&r);
            let w1 = ms(15, || {
                let _ = rasterize(&laid, 1.0);
            });
            let c1 = ms(15, || {
                skia_safe::graphics::purge_font_cache();
                let _ = rasterize(&laid, 1.0);
            });
            let c2 = ms(15, || {
                skia_safe::graphics::purge_font_cache();
                let _ = rasterize(&laid, 2.0);
            });
            let _ = writeln!(
                report,
                "| {tag} | unresolved glyphs: {} | colored px: {} | {} | {w1:.2} / {c1:.2} / {c2:.2} |",
                laid.unresolved,
                colored,
                if colored > 2000 { "renders" } else { "NO color emoji" }
            );
            let mut l = base("label", &tag, "Oswald", 26.0);
            l.max_width = 1200.0;
            l.align = TextAlign::Left;
            l.fill = Fill::Solid(hex("333333", 255));
            let lr = rasterize(&layout(&label_fonts, &l), 1.0);
            let w = 1240u32;
            let h = lr.h + r.h + 8;
            let mut px = vec![255u8; (w * h * 4) as usize];
            {
                let info = ImageInfo::new((w as i32, h as i32), ColorType::RGBA8888, AlphaType::Premul, None);
                let mut surf = surfaces::wrap_pixels(&info, &mut px, (w * 4) as usize, None).unwrap();
                let c = surf.canvas();
                c.draw_image(to_skimage(&raster_image(&lr)), (10, 0), None);
                c.draw_image(to_skimage(&raster_image(&r)), (10, lr.h as i32 + 4), None);
            }
            rows.push(Image { w, h, px });
        }
    }
    let mut big = Vec::new();
    for file in ["NotoColorEmoji-CBDT.ttf", "Noto-COLRv1.ttf"] {
        let Some(mgr) = make_mgr(MgrKind::FreeType) else { continue };
        let f = load_bundled_with(dir, file, mgr, MgrKind::FreeType);
        let mut s = base("emoji_big", "\u{1F600}\u{1F1EF}\u{1F1F5}", "Oswald", 110.0);
        s.max_width = 1000.0;
        s.fill = Fill::Solid(Color::BLACK);
        let r = rasterize(&layout(&f, &s), 2.0);
        big.push(crop(&raster_image(&r), 0, 0, r.w.min(620), r.h));
    }
    if big.len() == 2 {
        let w = big[0].w + big[1].w + 20;
        let h = big[0].h.max(big[1].h);
        let mut px = vec![255u8; (w * h * 4) as usize];
        {
            let info = ImageInfo::new((w as i32, h as i32), ColorType::RGBA8888, AlphaType::Premul, None);
            let mut surf = surfaces::wrap_pixels(&info, &mut px, (w * 4) as usize, None).unwrap();
            let c = surf.canvas();
            c.draw_image(to_skimage(&big[0]), (0, 0), None);
            c.draw_image(to_skimage(&big[1]), (big[0].w as i32 + 20, 0), None);
        }
        rows.push(Image { w, h, px });
    }
    stack(&rows, 6, [220, 220, 220, 255])
}

fn rects_debug(laid: &Laid) -> Image {
    let s = 1.0;
    let r = rasterize(laid, s);
    let (w, h) = (r.w, r.h);
    let mut px = vec![0u8; (w * h * 4) as usize];
    {
        let info = ImageInfo::new((w as i32, h as i32), ColorType::RGBA8888, AlphaType::Premul, None);
        let mut surf = surfaces::wrap_pixels(&info, &mut px, (w * 4) as usize, None).unwrap();
        let c = surf.canvas();
        c.clear(hex("2b3340", 255));
        c.draw_image(to_skimage(&raster_image(&r)), (0, 0), None);
        let mut p = Paint::default();
        p.set_style(PaintStyle::Stroke);
        p.set_stroke_width(1.0);
        p.set_anti_alias(true);
        for (col, rects) in [
            ("22c55e", laid.lines.iter().map(|l| Rect::new(l.left, l.top, l.right, l.bottom)).collect::<Vec<_>>()),
            ("3b82f6", laid.words.iter().map(|w| w.rect).collect()),
            ("ef4444", laid.glyphs.iter().map(|g| g.ink).collect()),
        ] {
            p.set_color(hex(col, 255));
            for rr in rects {
                c.draw_rect(r.rect_to_texel(rr), &p);
            }
        }
    }
    Image { w, h, px }
}

fn ease_out_back(t: f32) -> f32 {
    let c1 = 1.70158;
    let c3 = c1 + 1.0;
    1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
}

fn anim_draws(kind: &str, laid: &Laid, r: &Raster, tex: usize, pl: &Placed, s: f32, t: f32) -> Vec<Draw> {
    let base_xf = place(r, pl, s);
    let mut out = Vec::new();
    match kind {
        "typewriter" => {
            let n = (text::cluster_count(laid) as f32 * t).floor() as usize;
            for tile in text::typewriter_tiles(laid, n) {
                let tr = r.rect_to_texel(tile);
                out.push(Draw { tex, src: [tr.left, tr.top, tr.right, tr.bottom], xf: base_xf, opacity: pl.opacity });
            }
        }
        _ => {
            let tiles = text::word_tiles(laid);
            let n = tiles.len() as f32;
            for (i, tile) in tiles.iter().enumerate() {
                let start = i as f32 / n * 0.8;
                let lt = ((t - start) / 0.2).clamp(0.0, 1.0);
                if lt <= 0.0 {
                    continue;
                }
                let tr = r.rect_to_texel(*tile);
                let (cx, cy) = (tr.center_x(), tr.center_y());
                let sc = 0.5 + 0.5 * ease_out_back(lt);
                let local = Affine::translate(-cx, -cy).then(Affine::scale(sc)).then(Affine::translate(cx, cy));
                out.push(Draw {
                    tex,
                    src: [tr.left, tr.top, tr.right, tr.bottom],
                    xf: local.then(base_xf),
                    opacity: pl.opacity * lt.min(1.0),
                });
            }
        }
    }
    out
}

struct Args {
    reps: usize,
    label: String,
    png: bool,
    threads: Vec<usize>,
}

fn parse_args() -> Args {
    let mut a = Args { reps: 20, label: "default".into(), png: true, threads: vec![1, 4] };
    let v: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < v.len() {
        match v[i].as_str() {
            "--reps" => {
                a.reps = v[i + 1].parse().unwrap();
                i += 1;
            }
            "--label" => {
                a.label = v[i + 1].clone();
                i += 1;
            }
            "--no-png" => a.png = false,
            "--threads" => {
                a.threads = v[i + 1].split(',').map(|x| x.parse().unwrap()).collect();
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    a
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    if let Some(pos) = argv.iter().position(|a| a == "--specimen") {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let filter: Vec<String> = argv.get(pos + 1).filter(|a| !a.starts_with("--")).map(|a| a.split(',').map(String::from).collect()).unwrap_or_default();
        let rep = specimen::run(&root.join("fonts"), &root.join("out"), &filter, specimen::CANDIDATES);
        print!("{rep}");
        std::fs::write(root.join("out/specimens/specimen-results.txt"), &rep).unwrap();
        return;
    }
    let args = parse_args();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fdir = root.join("fonts");
    let out = root.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let mut rep = String::new();

    let t = Instant::now();
    let mut fonts: Fonts = load_bundled(&fdir, "Noto-COLRv1.ttf");
    let font_load_ms = t.elapsed().as_secs_f64() * 1000.0;
    let probe = fonts::fontdb_probe(&mut fonts);
    let _ = writeln!(rep, "# Skia text spike bench ({})\n", args.label);
    let _ = writeln!(rep, "- Skia font manager for bundled fonts: {:?} (emoji file: {})", fonts.kind, fonts.emoji_file);
    let _ = writeln!(rep, "- bundled font load + provider setup: {font_load_ms:.1} ms");
    let _ = writeln!(
        rep,
        "- fontdb: {} faces / {} families scanned in {:.1} ms; first families: {:?}",
        probe.faces, probe.families, probe.scan_ms, probe.sample_families
    );
    let _ = writeln!(rep, "- fontdb pick (Georgia/serif/sans, bold): {:?}", probe.chosen);
    let system_family = probe.chosen.as_ref().map(|c| c.0.clone());

    let placed = scene(system_family.as_deref());
    let reps = args.reps;

    let _ = writeln!(rep, "\n## Per-item layout + raster (median of {reps}, single thread)\n");
    let _ = writeln!(rep, "Warm = Skia glyph cache populated (re-raster after an edit). Cold = `purge_font_cache()` before every rep (first raster of new glyphs/size). Layout = paragraph build + shape + layout + rect/run extraction, paragraph cache off.\n");
    let _ = writeln!(rep, "| item | lines | glyphs | unresolved | layout ms (warm/cold) | raster 1080p ms (warm/cold) | tex 1080p | raster 4K ms (warm/cold) | tex 4K |");
    let _ = writeln!(rep, "|---|---|---|---|---|---|---|---|---|");
    let mut laid_all = Vec::new();
    let (mut sum_l, mut sum_r1, mut sum_r2) = (0.0, 0.0, 0.0);
    let mut sum_c = (0.0f64, 0.0f64, 0.0f64);
    for pl in &placed {
        let lt = ms(reps, || {
            let _ = layout(&fonts, &pl.spec);
        });
        let ltc = ms(reps, || {
            skia_safe::graphics::purge_font_cache();
            let _ = layout(&fonts, &pl.spec);
        });
        let laid = layout(&fonts, &pl.spec);
        let r1t = ms(reps, || {
            let _ = rasterize(&laid, 1.0);
        });
        let r1c = ms(reps, || {
            skia_safe::graphics::purge_font_cache();
            let _ = rasterize(&laid, 1.0);
        });
        let r2t = ms(reps, || {
            let _ = rasterize(&laid, 2.0);
        });
        let r2c = ms(reps, || {
            skia_safe::graphics::purge_font_cache();
            let _ = rasterize(&laid, 2.0);
        });
        sum_c.0 += ltc;
        sum_c.1 += r1c;
        sum_c.2 += r2c;
        let r1 = rasterize(&laid, 1.0);
        let r2 = rasterize(&laid, 2.0);
        sum_l += lt;
        sum_r1 += r1t;
        sum_r2 += r2t;
        let _ = writeln!(
            rep,
            "| {} | {} | {} | {} | {:.3} / {:.3} | {:.3} / {:.3} | {}x{} | {:.3} / {:.3} | {}x{} |",
            pl.spec.name,
            laid.lines.len(),
            laid.glyphs.len(),
            laid.unresolved,
            lt,
            ltc,
            r1t,
            r1c,
            r1.w,
            r1.h,
            r2t,
            r2c,
            r2.w,
            r2.h
        );
        laid_all.push(laid);
    }
    let _ = writeln!(rep, "| **sum ({} items)** | | | | {:.2} / {:.2} | {:.2} / {:.2} | | {:.2} / {:.2} | |", placed.len(), sum_l, sum_c.0, sum_r1, sum_c.1, sum_r2, sum_c.2);

    let _ = writeln!(rep, "\n## Parallel layout+raster of all items at 4K scale (throughput proxy)\n");
    for &n in &args.threads {
        let per = 4usize;
        let specs: Vec<TextSpec> = placed.iter().map(|p| p.spec.clone()).collect();
        let barrier = std::sync::Barrier::new(n + 1);
        let mut t0 = Instant::now();
        std::thread::scope(|sc| {
            for _ in 0..n {
                let specs = &specs;
                let fdir = &fdir;
                let barrier = &barrier;
                sc.spawn(move || {
                    let f = load_bundled(fdir, "Noto-COLRv1.ttf");
                    barrier.wait();
                    for _ in 0..per {
                        for s in specs {
                            let l = layout(&f, s);
                            let _ = rasterize(&l, 2.0);
                        }
                    }
                });
            }
            barrier.wait();
            t0 = Instant::now();
        });
        let el = t0.elapsed().as_secs_f64();
        let items = (n * per * specs.len()) as f64;
        let _ = writeln!(
            rep,
            "- {n} thread(s): {items} items in {:.1} ms -> {:.1} items/s ({:.2} ms/item/thread)",
            el * 1000.0,
            items / el,
            el * 1000.0 * n as f64 / items
        );
    }

    let gpu = gpu::Gpu::new();
    let _ = writeln!(rep, "\n## GPU ({})\n", gpu.adapter_info);

    for (res_label, s, w, h) in [("1080p", 1.0f32, 1920u32, 1080u32), ("4K", 2.0, 3840, 2160)] {
        let bg = background(w, h, s);
        let rasters: Vec<Raster> = laid_all.iter().map(|l| rasterize(l, s)).collect();
        let imgs: Vec<Image> = rasters.iter().map(raster_image).collect();
        let bytes: usize = imgs.iter().map(|i| i.px.len()).sum();
        let up = ms(reps, || {
            let t: Vec<_> = imgs.iter().map(|i| gpu.upload(i)).collect();
            gpu.queue.submit([]);
            gpu.wait();
            drop(t);
        });
        let gbg = gpu.upload(&bg);
        let texs: Vec<gpu::GpuTex> = imgs.iter().map(|i| gpu.upload(i)).collect();
        gpu.wait();
        let draws: Vec<Draw> = rasters.iter().zip(&placed).enumerate().map(|(i, (r, pl))| full_draw(i, r, pl, s)).collect();
        let target = gpu.target(w, h);
        gpu.composite(&gbg, &target, &texs, &draws);
        gpu.wait();
        let comp = ms(reps.max(30), || {
            gpu.composite(&gbg, &target, &texs, &draws);
            gpu.wait();
        });
        let rb = ms(reps.min(10).max(3), || {
            let _ = gpu.readback(&target);
        });
        let g = gpu.readback(&target);
        let ct0 = Instant::now();
        let c = cpu_composite(&bg, &imgs, &draws);
        let cpu_ms = ct0.elapsed().as_secs_f64() * 1000.0;
        let (st, vis) = diff(&g, &c);
        let _ = writeln!(rep, "### {res_label} ({w}x{h}), {} text items\n", draws.len());
        let _ = writeln!(rep, "- upload all item textures ({:.2} MB RGBA): {up:.3} ms (create + write_texture + submit + wait)", bytes as f64 / 1e6);
        let _ = writeln!(rep, "- GPU composite (bg copy + {} quads, submit + wait): {comp:.3} ms", draws.len());
        let _ = writeln!(rep, "- readback {w}x{h}: {rb:.3} ms");
        let _ = writeln!(rep, "- CPU reference composite: {cpu_ms:.1} ms");
        let _ = writeln!(
            rep,
            "- GPU vs CPU per-channel diff: max {} / mean {:.5}; channels >1: {}, >2: {}, >4: {} of {}",
            st.max, st.mean, st.over1, st.over2, st.over4, st.channels
        );
        if args.png && res_label == "1080p" {
            write_png(&out.join("composite_gpu_1080p.png"), &g);
            write_png(&out.join("diff_gpu_vs_cpu_x32_1080p.png"), &vis);
        }
        if args.png && res_label == "4K" {
            write_png(&out.join("composite_gpu_4k_crop.png"), &crop(&g, 1160, 1100, 1520, 560));
        }
        let _ = writeln!(rep);
    }

    {
        let s = 1.0;
        let bg = background(1920, 1080, s);
        let gbg = gpu.upload(&bg);
        let target = gpu.target(1920, 1080);
        let mut worst = 0u8;
        let mut strips = Vec::new();
        for (kind, idx) in [("typewriter", 4usize), ("words", 0usize)] {
            let pl = &placed[idx];
            let laid = &laid_all[idx];
            let r = rasterize(laid, s);
            let img = raster_image(&r);
            let tex = vec![gpu.upload(&img)];
            let mut frames = Vec::new();
            for t in [0.15f32, 0.35, 0.55, 0.75, 1.0] {
                let draws = anim_draws(kind, laid, &r, 0, pl, s, t);
                gpu.composite(&gbg, &target, &tex, &draws);
                let g = gpu.readback(&target);
                let c = cpu_composite(&bg, std::slice::from_ref(&img), &draws);
                let (st, _) = diff(&g, &c);
                worst = worst.max(st.max);
                let xf = place(&r, pl, s);
                let (x0, y0) = (xf.tx.max(0.0) as u32, xf.ty.max(0.0) as u32);
                frames.push(crop(&g, x0, y0, r.w.min(1920 - x0), r.h.min(1080 - y0)));
            }
            let _ = writeln!(
                rep,
                "- anim `{kind}` on `{}`: {} draws at t=1, {} of {} glyph clusters revealed at t=0.75",
                pl.spec.name,
                anim_draws(kind, laid, &r, 0, pl, s, 1.0).len(),
                (text::cluster_count(laid) as f32 * 0.75).floor() as usize,
                text::cluster_count(laid)
            );
            strips.push((kind, stack(&frames, 4, [255, 255, 255, 255])));
        }
        let _ = writeln!(rep, "- animation frames GPU vs CPU max channel diff: {worst}");
        if args.png {
            for (k, im) in strips {
                write_png(&out.join(format!("anim_{k}.png")), &im);
            }
        }
    }

    if args.png {
        write_png(&out.join("rects_debug.png"), &rects_debug(&laid_all[0]));
        write_png(&out.join("rects_debug_tiktok.png"), &rects_debug(&laid_all[5]));
        let _ = writeln!(rep, "\n## Emoji font format x Skia font manager\n");
        let _ = writeln!(rep, "| combo | shaping | pixels | verdict | raster ms 1080p warm / cold / 4K cold |\n|---|---|---|---|---|");
        let em = emoji_matrix(&fdir, &mut rep);
        write_png(&out.join("emoji_matrix.png"), &em);

        let idx = 2usize;
        let pl = &placed[idx];
        let r1 = rasterize(&laid_all[idx], 1.0);
        let r2 = rasterize(&laid_all[idx], 2.0);
        let w = r2.w * 2 + 20;
        let h = r2.h;
        let bgc = Image { w, h, px: [40u8, 44, 52, 255].iter().copied().cycle().take((w * h * 4) as usize).collect() };
        let target = gpu.target(w, h);
        let gbg = gpu.upload(&bgc);
        let texs = vec![gpu.upload(&raster_image(&r1)), gpu.upload(&raster_image(&r2))];
        let draws = vec![
            Draw { tex: 0, src: [0.0, 0.0, r1.w as f32, r1.h as f32], xf: Affine::scale(2.0), opacity: 1.0 },
            Draw {
                tex: 1,
                src: [0.0, 0.0, r2.w as f32, r2.h as f32],
                xf: Affine::translate((r2.w + 20) as f32, 0.0),
                opacity: 1.0,
            },
        ];
        gpu.composite(&gbg, &target, &texs, &draws);
        let g = gpu.readback(&target);
        let (cw, ch) = (300u32, 150u32);
        let (cx, cy) = (40u32, 60u32);
        let a = crop(&g, cx, cy, cw, ch);
        let b2 = crop(&g, cx + r2.w + 20, cy, cw, ch);
        let zoom = |im: &Image| {
            let z = 3u32;
            let mut px = Vec::with_capacity((im.w * z * im.h * z * 4) as usize);
            for y in 0..im.h * z {
                for x in 0..im.w * z {
                    let i = (((y / z) * im.w + x / z) * 4) as usize;
                    px.extend_from_slice(&im.px[i..i + 4]);
                }
            }
            Image { w: im.w * z, h: im.h * z, px }
        };
        let za = zoom(&a);
        let zb = zoom(&b2);
        let w = za.w * 2 + 12;
        let mut px: Vec<u8> = [255u8, 255, 255, 255].iter().copied().cycle().take((w * za.h * 4) as usize).collect();
        for y in 0..za.h {
            let d = (y * w * 4) as usize;
            let s1 = (y * za.w * 4) as usize;
            px[d..d + (za.w * 4) as usize].copy_from_slice(&za.px[s1..s1 + (za.w * 4) as usize]);
            let d2 = d + ((za.w + 12) * 4) as usize;
            px[d2..d2 + (zb.w * 4) as usize].copy_from_slice(&zb.px[s1..s1 + (zb.w * 4) as usize]);
        }
        write_png(&out.join("crisp_upscaled_vs_native.png"), &Image { w, h: za.h, px });
        let _ = pl;
    }

    print!("{rep}");
    std::fs::write(out.join(format!("bench-{}.md", args.label)), &rep).unwrap();
}

