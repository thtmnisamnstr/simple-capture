use super::{background, base, colored_pixels, hex, place, raster_image, stack, to_skimage, write_png, Placed};
use crate::composite::{Draw, Image, cpu_composite};
use crate::fonts::{EMOJI_FAMILY, Fonts, MgrKind, load_typeface};
use crate::text::{BgBox, Fill, Glow, Laid, Raster, Shadow, Stroke, TextSpec, layout, rasterize};
use skia_safe::{
    AlphaType, Color, ColorType, FontArguments, FontMgr, FontStyle, FourByteTag, ImageInfo, Paint, PaintStyle,
    Rect, Typeface,
    font_arguments::{VariationPosition, variation_position::Coordinate},
    surfaces,
    textlayout::{FontCollection, ParagraphBuilder, ParagraphStyle, TextAlign, TextStyle, TypefaceFontProvider},
};
use std::fmt::Write as _;
use std::path::Path;

pub struct Fam {
    pub name: &'static str,
    pub slug: &'static str,
    pub license: &'static str,
    pub candidate: bool,
    pub upright: &'static [(&'static str, f32)],
    pub italic: &'static [(&'static str, f32)],
}

const V: f32 = 0.0;

pub const FAMS: &[Fam] = &[
    Fam { name: "Inter", slug: "inter", license: "OFL-1.1", candidate: false, upright: &[("Inter[opsz,wght].ttf", V)], italic: &[("Inter-Italic[opsz,wght].ttf", V)] },
    Fam { name: "Montserrat", slug: "montserrat", license: "OFL-1.1", candidate: false, upright: &[("Montserrat[wght].ttf", V)], italic: &[("Montserrat-Italic[wght].ttf", V)] },
    Fam { name: "Oswald", slug: "oswald", license: "OFL-1.1", candidate: false, upright: &[("Oswald[wght].ttf", V)], italic: &[] },
    Fam { name: "Bebas Neue", slug: "bebasneue", license: "OFL-1.1", candidate: false, upright: &[("BebasNeue-Regular.ttf", 400.0)], italic: &[] },
    Fam { name: "Playfair Display", slug: "playfairdisplay", license: "OFL-1.1", candidate: false, upright: &[("PlayfairDisplay[wght].ttf", V)], italic: &[("PlayfairDisplay-Italic[wght].ttf", V)] },
    Fam { name: "DM Serif Display", slug: "dmserifdisplay", license: "OFL-1.1", candidate: false, upright: &[("DMSerifDisplay-Regular.ttf", 400.0)], italic: &[("DMSerifDisplay-Italic.ttf", 400.0)] },
    Fam { name: "Courier Prime", slug: "courierprime", license: "OFL-1.1", candidate: false, upright: &[("CourierPrime-Regular.ttf", 400.0), ("CourierPrime-Bold.ttf", 700.0)], italic: &[("CourierPrime-Italic.ttf", 400.0), ("CourierPrime-BoldItalic.ttf", 700.0)] },
    Fam { name: "Special Elite", slug: "specialelite", license: "Apache-2.0", candidate: false, upright: &[("SpecialElite-Regular.ttf", 400.0)], italic: &[] },
    Fam { name: "Caveat", slug: "caveat", license: "OFL-1.1", candidate: false, upright: &[("Caveat[wght].ttf", V)], italic: &[] },
    Fam { name: "Permanent Marker", slug: "permanentmarker", license: "Apache-2.0", candidate: false, upright: &[("PermanentMarker-Regular.ttf", 400.0)], italic: &[] },
    Fam { name: "Pacifico", slug: "pacifico", license: "OFL-1.1", candidate: false, upright: &[("Pacifico-Regular.ttf", 400.0)], italic: &[] },
    Fam { name: "Tilt Neon", slug: "tiltneon", license: "OFL-1.1", candidate: false, upright: &[("TiltNeon[XROT,YROT].ttf", V)], italic: &[] },
    Fam { name: "Fredoka", slug: "fredoka", license: "OFL-1.1", candidate: false, upright: &[("Fredoka[wdth,wght].ttf", V)], italic: &[] },
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Req {
    Core,
    Accents,
    ExtA,
    Info,
}

struct LineDef {
    key: &'static str,
    text: String,
    req: Req,
}

const SAMPLE: &str = "\u{00C6}r\u{00F8}sk\u{00F8}bing, S\u{00E3}o Paulo, Krak\u{00F3}w, \u{0110}\u{1EB7}ng, na\u{00EF}ve caf\u{00E9}";
const EMOJI_LINE: &str = "Emoji \u{1F600} \u{1F44D}\u{1F3FD} \u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466} \u{2764}\u{FE0F}\u{200D}\u{1F525} \u{1F1EF}\u{1F1F5} \u{1F3F3}\u{FE0F}\u{200D}\u{1F308} \u{2728} mixed with caf\u{00E9} \u{201C}ok\u{201D} \u{1F389}";

fn lines() -> Vec<LineDef> {
    let latin1: String = (0xC0u32..=0xFF).filter(|&c| c != 0xD7 && c != 0xF7).filter_map(char::from_u32).collect();
    let ext_a: String = (0x100u32..=0x17F).filter_map(char::from_u32).collect();
    vec![
        LineDef { key: "ascii_upper", text: "ABCDEFGHIJKLMNOPQRSTUVWXYZ".into(), req: Req::Core },
        LineDef { key: "ascii_lower", text: "abcdefghijklmnopqrstuvwxyz".into(), req: Req::Core },
        LineDef { key: "digits", text: "0123456789".into(), req: Req::Core },
        LineDef { key: "ascii_punct", text: "! \" # $ % & ' ( ) * + , - . / : ; < = > ? @ [ \\ ] ^ _ ` { | } ~".into(), req: Req::Core },
        LineDef {
            key: "typographic",
            text: "\u{2018}single\u{2019} \u{201C}double\u{201D} it\u{2019}s \u{2013} en \u{2014} em \u{2026} \u{2022} \u{20AC} \u{00A3} \u{00A5}".into(),
            req: Req::Core,
        },
        LineDef { key: "sample", text: SAMPLE.into(), req: Req::Accents },
        LineDef { key: "latin1_letters", text: latin1, req: Req::Accents },
        LineDef { key: "latin_ext_a", text: ext_a, req: Req::ExtA },
        LineDef {
            key: "nfd_combining_info",
            text: "NFD: c\u{0301} z\u{0301} s\u{0301} n\u{0301} e\u{0328} a\u{0306} e\u{0300} n\u{0303} o\u{0308} u\u{030A} C\u{0327} A\u{0301} w\u{0302} y\u{0302}".into(),
            req: Req::Info,
        },
        LineDef {
            key: "extras_info",
            text: "\u{00A9} \u{00AE} \u{2122} \u{00B0} \u{00B1} \u{00D7} \u{00F7} \u{00A2} \u{00BF} \u{00A1} \u{00AB} \u{00BB} \u{2039} \u{203A} \u{201A} \u{201E} \u{2020} \u{2021} \u{2030} \u{00BD} \u{00BC} \u{00BE} \u{00B2} \u{00B3} \u{00B5} \u{00B6} \u{00A7} \u{2032} \u{2033} \u{2212}".into(),
            req: Req::Info,
        },
    ]
}

struct Inst {
    label: String,
    weight: f32,
    italic: bool,
    file: &'static str,
    tf: Typeface,
    alias: String,
}

fn tag(s: &str) -> FourByteTag {
    let b = s.as_bytes();
    FourByteTag::from_chars(b[0] as char, b[1] as char, b[2] as char, b[3] as char)
}

fn with_axes(tf: &Typeface, coords: &[(&str, f32)]) -> Typeface {
    let c: Vec<Coordinate> = coords.iter().map(|(t, v)| Coordinate { axis: tag(t), value: *v }).collect();
    let args = FontArguments::new().set_variation_design_position(VariationPosition { coordinates: &c });
    tf.clone_with_arguments(&args).expect("variation instance")
}

fn wght_axis(tf: &Typeface) -> Option<(f32, f32, f32)> {
    tf.variation_design_parameters()?.into_iter().find(|a| a.tag == tag("wght")).map(|a| (a.min, a.def, a.max))
}

fn axes_desc(tf: &Typeface) -> String {
    match tf.variation_design_parameters() {
        Some(v) if !v.is_empty() => v
            .iter()
            .map(|a| {
                let s = String::from_utf8_lossy(&[a.tag.a(), a.tag.b(), a.tag.c(), a.tag.d()]).to_string();
                format!("{s} {}..{} (default {})", a.min, a.max, a.def)
            })
            .collect::<Vec<_>>()
            .join(", "),
        _ => "none (static)".into(),
    }
}

fn instances(mgr: &FontMgr, dir: &Path, fam: &Fam) -> Vec<Inst> {
    let mut out = Vec::new();
    for (italic, faces) in [(false, fam.upright), (true, fam.italic)] {
        for &(file, w) in faces {
            let tf = load_typeface(mgr, &dir.join(file), 0).unwrap_or_else(|| panic!("FreeType could not load {file}"));
            let it = if italic { " Italic" } else { "" };
            if w != V {
                out.push(Inst { label: format!("{w}{it}"), weight: w, italic, file, alias: format!("{} {w}{it}", fam.slug), tf });
                continue;
            }
            match wght_axis(&tf) {
                Some((min, _def, max)) => {
                    let mut ws: Vec<f32> = (1..=9).map(|i| i as f32 * 100.0).filter(|w| *w >= min && *w <= max).collect();
                    if !ws.contains(&min) {
                        ws.insert(0, min);
                    }
                    if !ws.contains(&max) {
                        ws.push(max);
                    }
                    for w in ws {
                        out.push(Inst {
                            label: format!("wght {w}{it}"),
                            weight: w,
                            italic,
                            file,
                            alias: format!("{} {w}{it}", fam.slug),
                            tf: with_axes(&tf, &[("wght", w)]),
                        });
                    }
                }
                None => out.push(Inst { label: format!("default{it}"), weight: 400.0, italic, file, alias: format!("{} default{it}", fam.slug), tf }),
            }
        }
    }
    out
}

fn make_fonts(mgr: &FontMgr, faces: &[(Typeface, String)], emoji: Option<&Typeface>) -> Fonts {
    let mut provider = TypefaceFontProvider::new();
    for (tf, alias) in faces {
        provider.register_typeface(tf.clone(), alias.as_str());
    }
    if let Some(e) = emoji {
        provider.register_typeface(e.clone(), EMOJI_FAMILY);
    }
    let mut collection = FontCollection::new();
    collection.set_asset_font_manager(Some(provider.clone().into()));
    collection.set_default_font_manager(None, None);
    collection.disable_font_fallback();
    collection.paragraph_cache_mut().turn_on(false);
    Fonts { collection, provider, mgr: mgr.clone(), kind: MgrKind::FreeType, emoji_file: "Noto-COLRv1.ttf".into() }
}

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

fn spec(text: &str, family: &str, size: f32, width: f32) -> TextSpec {
    let mut s = base("specimen", text, leak(family), size);
    s.align = TextAlign::Left;
    s.max_width = width;
    s.fill = Fill::Solid(Color::BLACK);
    s.line_height = 1.25;
    s
}

fn notdef_marks(laid: &Laid, r: &Raster) -> Vec<Rect> {
    let sz = laid.spec.size;
    let mut v = Vec::new();
    for run in &laid.runs {
        for (g, p) in run.glyphs.iter().zip(&run.pos) {
            if *g == 0 {
                v.push(r.rect_to_texel(Rect::new(p.x - 2.0, p.y - sz * 0.9, p.x + sz * 0.6, p.y + sz * 0.25)));
            }
        }
    }
    v
}

fn notdef_count(laid: &Laid) -> usize {
    laid.runs.iter().flat_map(|r| r.glyphs.iter()).filter(|g| **g == 0).count()
}

fn flatten(r: &Raster, min_w: u32, pad: u32, bgc: Color, marks: &[Rect]) -> Image {
    let w = min_w.max(r.w + pad * 2);
    let h = r.h + pad * 2;
    let mut px = vec![0u8; (w * h * 4) as usize];
    {
        let info = ImageInfo::new((w as i32, h as i32), ColorType::RGBA8888, AlphaType::Premul, None);
        let mut surf = surfaces::wrap_pixels(&info, &mut px, (w * 4) as usize, None).unwrap();
        let c = surf.canvas();
        c.clear(bgc);
        c.draw_image(to_skimage(&raster_image(r)), (pad as i32, pad as i32), None);
        let mut p = Paint::default();
        p.set_anti_alias(true);
        p.set_style(PaintStyle::Stroke);
        p.set_stroke_width(3.0);
        p.set_color(Color::from_argb(255, 230, 20, 20));
        for m in marks {
            c.draw_rect(m.with_offset((pad as f32, pad as f32)), &p);
        }
    }
    Image { w, h, px }
}

struct Labeler {
    fonts: Fonts,
}

impl Labeler {
    fn new(mgr: &FontMgr, dir: &Path) -> Self {
        let sys = Path::new("/System/Library/Fonts/Supplemental/Arial.ttf");
        let tf = load_typeface(mgr, sys, 0).or_else(|| load_typeface(mgr, &dir.join("Oswald[wght].ttf"), 0)).unwrap();
        Labeler { fonts: make_fonts(mgr, &[(tf, "Label".into())], None) }
    }
    fn row(&self, text: &str, w: u32, color: Color) -> Image {
        let mut s = spec(text, "Label", 22.0, w as f32 - 40.0);
        s.fill = Fill::Solid(color);
        s.line_height = 1.1;
        let r = rasterize(&layout(&self.fonts, &s), 1.0);
        flatten(&r, w, 6, Color::from_argb(255, 236, 238, 242), &[])
    }
}

fn line_row(fonts: &Fonts, text: &str, family: &str, size: f32, w: u32) -> (Image, Laid) {
    let laid = layout(fonts, &spec(text, family, size, w as f32 - 40.0));
    let r = rasterize(&laid, 1.0);
    let marks = notdef_marks(&laid, &r);
    (flatten(&r, w, 14, Color::WHITE, &marks), laid)
}

struct Coverage {
    missing: Vec<char>,
    blank: Vec<char>,
}

fn coverage(tf: &Typeface, text: &str) -> Coverage {
    let font = skia_safe::Font::from_typeface(tf.clone(), 64.0);
    let mut missing = Vec::new();
    let mut blank = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for ch in text.chars() {
        if ch.is_whitespace() || !seen.insert(ch) {
            continue;
        }
        let g = tf.unichar_to_glyph(ch as i32);
        if g == 0 {
            missing.push(ch);
            continue;
        }
        let mut b = [Rect::default()];
        font.get_bounds(&[g], &mut b, None);
        if b[0].is_empty() {
            blank.push(ch);
        }
    }
    Coverage { missing, blank }
}

fn fmt_chars(v: &[char]) -> String {
    if v.is_empty() {
        return "none".into();
    }
    v.iter().map(|c| format!("{c} U+{:04X}", *c as u32)).collect::<Vec<_>>().join(", ")
}

const DIFF_TEXT: &str = "Handgloves 0123 Krak\u{00F3}w \u{201C}Hamburg\u{201D}";

fn para_alpha(collection: &FontCollection, family: &str, style: FontStyle, wght: Option<f32>, size: f32) -> Vec<u8> {
    let (w, h) = (1500i32, (size * 1.8) as i32);
    let mut ps = ParagraphStyle::new();
    ps.set_text_align(TextAlign::Left);
    let mut ts = TextStyle::new();
    ts.set_font_families(&[family]);
    ts.set_font_size(size);
    ts.set_color(Color::BLACK);
    ts.set_font_style(style);
    ts.set_font_edging(skia_safe::font::Edging::AntiAlias);
    ts.set_subpixel(true);
    ts.set_font_hinting(skia_safe::FontHinting::None);
    let coords;
    let args;
    if let Some(v) = wght {
        coords = [Coordinate { axis: tag("wght"), value: v }];
        args = FontArguments::new().set_variation_design_position(VariationPosition { coordinates: &coords });
        ts.set_font_arguments(&args);
    }
    let mut b = ParagraphBuilder::new(&ps, collection.clone());
    b.push_style(&ts);
    b.add_text(DIFF_TEXT);
    let mut para = b.build();
    para.layout(w as f32 - 20.0);
    let mut px = vec![0u8; (w * h * 4) as usize];
    {
        let info = ImageInfo::new((w, h), ColorType::RGBA8888, AlphaType::Premul, None);
        let mut surf = surfaces::wrap_pixels(&info, &mut px, (w * 4) as usize, None).unwrap();
        para.paint(surf.canvas(), (10.0, 10.0));
    }
    px.chunks(4).map(|p| p[3]).collect()
}

#[derive(Clone, Copy)]
struct PxDiff {
    frac: f64,
    ink_ratio: f64,
}

fn alpha_diff(a: &[u8], b: &[u8]) -> PxDiff {
    let (mut ink_a, mut ink_b, mut union, mut differ) = (0u64, 0u64, 0u64, 0u64);
    for (x, y) in a.iter().zip(b) {
        ink_a += *x as u64;
        ink_b += *y as u64;
        if *x > 0 || *y > 0 {
            union += 1;
            if (*x as i32 - *y as i32).abs() > 32 {
                differ += 1;
            }
        }
    }
    PxDiff { frac: differ as f64 / union.max(1) as f64, ink_ratio: ink_b as f64 / ink_a.max(1) as f64 }
}

pub struct FamResult {
    pub name: &'static str,
    pub license: &'static str,
    pub candidate: bool,
    pub grade: String,
    pub notes: Vec<String>,
}

fn styles_frame(fonts: &Fonts, fam: &Fam, reg: &str, bold: &str) -> Image {
    let (reg, bold) = (leak(reg), leak(bold));
    let tiktok = |color: Color| BgBox { color, radius: 16.0, pad_x: 18.0, pad_y: 6.0 };
    let mut items: Vec<Placed> = Vec::new();
    let mut p = |spec: TextSpec, cx: f32, cy: f32, rot: f32| items.push(Placed { spec, cx, cy, rot, opacity: 1.0 });

    let mut s = base("title", &format!("{} \u{2014} styles at 1080p \u{1F3AC}", fam.name), bold, 64.0);
    s.uppercase = true;
    s.max_width = 1800.0;
    s.shadow = Some(Shadow { color: hex("000000", 150), dx: 0.0, dy: 5.0, blur: 7.0 });
    p(s, 960.0, 105.0, 0.0);

    let mut s = base("outline", "Outline \u{00C6}r\u{00F8}sk\u{00F8}bing\nKrak\u{00F3}w \u{0141}\u{00F3}d\u{017A} \u{270F}\u{FE0F}", bold, 74.0);
    s.max_width = 880.0;
    s.stroke = Some(Stroke { color: Color::BLACK, width: 7.0 });
    p(s, 480.0, 320.0, 0.0);

    let mut s = base("shadow", "Drop shadow\n\u{201C}na\u{00EF}ve caf\u{00E9}\u{201D} \u{2026} \u{270D}\u{1F3FD}", reg, 74.0);
    s.max_width = 880.0;
    s.fill = Fill::Solid(hex("ffe9a8", 255));
    s.shadow = Some(Shadow { color: hex("000000", 190), dx: 6.0, dy: 8.0, blur: 5.0 });
    p(s, 1440.0, 320.0, 0.0);

    let mut s = base("tiktok", "This is how TikTok\nmerges line\nbackgrounds \u{2014} S\u{00E3}o Paulo \u{1F389}", bold, 50.0);
    s.max_width = 880.0;
    s.fill = Fill::Solid(Color::BLACK);
    s.bg = Some(tiktok(Color::WHITE));
    s.line_height = 1.2;
    p(s, 480.0, 620.0, 0.0);

    let mut s = base("inverted", "Inverted, left aligned\nshort\n\u{0110}\u{1EB7}ng \u{0141}\u{00F3}d\u{017A} \u{20AC}5 \u{1F4AC}", bold, 50.0);
    s.max_width = 880.0;
    s.align = TextAlign::Left;
    s.bg = Some(tiktok(hex("fe2c55", 255)));
    s.line_height = 1.2;
    p(s, 1440.0, 620.0, 0.0);

    let mut s = base("glow", "Open late \u{2728}", reg, 84.0);
    s.max_width = 880.0;
    s.fill = Fill::Solid(hex("fff0fc", 255));
    s.glow = Some(Glow { color: hex("ff2bd6", 255), radius: 12.0 });
    p(s, 480.0, 900.0, 0.0);

    let mut s = base("gradient", "Gradient \u{00A3}\u{00A5} \u{1F525}", bold, 80.0);
    s.max_width = 880.0;
    s.uppercase = true;
    s.letter_spacing = 4.0;
    s.fill = Fill::Gradient(vec![hex("ff3d77", 255), hex("ff9a3d", 255), hex("ffe53d", 255)]);
    s.stroke = Some(Stroke { color: Color::BLACK, width: 4.0 });
    p(s, 1440.0, 900.0, -6.0);

    let bg = background(1920, 1080, 1.0);
    let mut imgs = Vec::new();
    let mut draws = Vec::new();
    for (i, pl) in items.iter().enumerate() {
        let laid = layout(fonts, &pl.spec);
        let r = rasterize(&laid, 1.0);
        draws.push(Draw { tex: i, src: [0.0, 0.0, r.w as f32, r.h as f32], xf: place(&r, pl, 1.0), opacity: pl.opacity });
        imgs.push(raster_image(&r));
    }
    cpu_composite(&bg, &imgs, &draws)
}

fn hollow_row(fonts: &Fonts, family: &str, w: u32) -> Image {
    let mut s = spec("Overlap check: Handgloves \u{00C6}\u{0110}\u{0141}&@8", family, 72.0, w as f32 - 40.0);
    s.fill = Fill::Solid(Color::TRANSPARENT);
    s.stroke = Some(Stroke { color: Color::BLACK, width: 1.0 });
    let r = rasterize(&layout(fonts, &s), 1.0);
    flatten(&r, w, 14, Color::WHITE, &[])
}

pub fn run(dir: &Path, out: &Path, filter: &[String], extra: &[Fam]) -> String {
    let outdir = out.join("specimens");
    std::fs::create_dir_all(&outdir).unwrap();
    let mgr = FontMgr::custom_empty().expect("FreeType custom_empty font manager (build with .cargo/config.toml)");
    let emoji = load_typeface(&mgr, &dir.join("Noto-COLRv1.ttf"), 0).expect("emoji font");
    let labeler = Labeler::new(&mgr, dir);
    let grey = Color::from_argb(255, 70, 74, 84);
    let red = Color::from_argb(255, 200, 20, 20);
    let mut rep = String::new();
    let mut results = Vec::new();
    let mut nfd_rows = Vec::new();
    let all: Vec<&Fam> = FAMS.iter().chain(extra.iter()).filter(|f| filter.is_empty() || filter.iter().any(|x| x == f.slug)).collect();
    for fam in all {
        let insts = instances(&mgr, dir, fam);
        let faces: Vec<(Typeface, String)> = insts.iter().map(|i| (i.tf.clone(), i.alias.clone())).collect();
        let plain = make_fonts(&mgr, &faces, None);
        let with_emoji = make_fonts(&mgr, &faces, Some(&emoji));
        let upright: Vec<&Inst> = insts.iter().filter(|i| !i.italic).collect();
        let reg = *upright.iter().min_by(|a, b| (a.weight - 400.0).abs().total_cmp(&(b.weight - 400.0).abs())).unwrap();
        let bold = *upright.iter().min_by(|a, b| (a.weight - 700.0).abs().total_cmp(&(b.weight - 700.0).abs())).unwrap();
        let w = 1800u32;
        let _ = writeln!(rep, "\n## {} ({}{})\n", fam.name, fam.license, if fam.candidate { ", replacement candidate" } else { "" });

        let mut files: Vec<&'static str> = insts.iter().map(|i| i.file).collect();
        files.dedup();
        let mut notes = Vec::new();
        let mut core_missing = Vec::new();
        let mut accent_missing = Vec::new();
        let mut exta_missing = Vec::new();
        let mut viet_missing = Vec::new();
        let mut info_missing = Vec::new();
        let defs = lines();
        for file in &files {
            let inst = insts.iter().find(|i| i.file == *file).unwrap();
            let tf = &inst.tf;
            let _ = writeln!(rep, "- `{file}` axes: {}", axes_desc(&load_typeface(&mgr, &dir.join(file), 0).unwrap()));
            for d in &defs {
                let cov = coverage(tf, &d.text);
                let mut miss: Vec<char> = cov.missing.iter().chain(cov.blank.iter()).copied().collect();
                miss.dedup();
                for c in miss {
                    let bucket = match (d.req, c) {
                        (_, '\u{1EB7}') => &mut viet_missing,
                        (Req::Core, _) => &mut core_missing,
                        (Req::Accents, _) => &mut accent_missing,
                        (Req::ExtA, _) => &mut exta_missing,
                        (Req::Info, _) => &mut info_missing,
                    };
                    let one = layout(&plain, &spec(&c.to_string(), &inst.alias, 46.0, 400.0));
                    let tofu = cov.blank.contains(&c) || one.unresolved > 0 || notdef_count(&one) > 0;
                    if !bucket.iter().any(|(x, _, _): &(char, &str, bool)| *x == c) {
                        bucket.push((c, *file, tofu));
                    }
                }
                if !cov.blank.is_empty() {
                    let _ = writeln!(rep, "  - `{}` blank (empty-outline) glyphs for: {}", d.key, fmt_chars(&cov.blank));
                }
            }
        }
        let lower_as_upper = {
            let tf = &reg.tf;
            ('a'..='z').filter(|c| tf.unichar_to_glyph(*c as i32) != 0 && tf.unichar_to_glyph(*c as i32) == tf.unichar_to_glyph(c.to_ascii_uppercase() as i32)).count()
        };

        let mut rows = Vec::new();
        rows.push(labeler.row(&format!("{} \u{2014} {} \u{2014} FreeType FontMgr::custom_empty, instance {} ({}), no fallback font for text lines; red boxes = glyph id 0", fam.name, fam.license, reg.label, reg.file), w, grey));
        let (img, _) = line_row(&plain, &format!("{} {}", fam.name, reg.label), &reg.alias, 64.0, w);
        rows.push(img);
        let _ = writeln!(rep, "\n| line | paragraph unresolved | glyph-id-0 in runs | cmap missing (regular face) |\n|---|---|---|---|");
        for d in &defs {
            let (img, laid) = line_row(&plain, &d.text, &reg.alias, 46.0, w);
            let cov = coverage(&reg.tf, &d.text);
            let label = format!(
                "{} \u{2014} paragraph unresolved {} \u{2014} glyph-id-0 {} \u{2014} missing in cmap: {}{}",
                d.key,
                laid.unresolved,
                notdef_count(&laid),
                fmt_chars(&cov.missing),
                if cov.blank.is_empty() { String::new() } else { format!(" \u{2014} blank: {}", fmt_chars(&cov.blank)) }
            );
            rows.push(labeler.row(&label, w, if cov.missing.is_empty() && cov.blank.is_empty() { grey } else { red }));
            rows.push(img);
            let _ = writeln!(rep, "| {} | {} | {} | {} |", d.key, laid.unresolved, notdef_count(&laid), fmt_chars(&cov.missing));
        }
        let composed: String = [&core_missing, &accent_missing, &exta_missing, &viet_missing, &info_missing]
            .iter()
            .flat_map(|v| v.iter().filter(|x| !x.2 && x.1 == reg.file).map(|x| x.0))
            .collect();
        if !composed.is_empty() {
            rows.push(labeler.row("ZOOM: characters with no precomposed glyph that the shaper (HarfBuzz) built from base letter + combining mark, at 80 px", w, red));
            let (img, _) = line_row(&plain, &composed, &reg.alias, 80.0, w);
            rows.push(img);
        }
        let el = layout(&with_emoji, &spec(EMOJI_LINE, &reg.alias, 46.0, w as f32 - 40.0));
        let er = rasterize(&el, 1.0);
        let ecol = colored_pixels(&er);
        rows.push(labeler.row(&format!("emoji line through fallback {EMOJI_FAMILY} (Noto-COLRv1) \u{2014} paragraph unresolved {} \u{2014} colored px {ecol}", el.unresolved), w, if el.unresolved == 0 { grey } else { red }));
        let marks = notdef_marks(&el, &er);
        rows.push(flatten(&er, w, 14, Color::WHITE, &marks));
        let _ = writeln!(rep, "| emoji (with {EMOJI_FAMILY} fallback) | {} | {} | colored px {ecol} |", el.unresolved, notdef_count(&el));
        rows.push(labeler.row("stroke-only diagnostic (fill transparent): shows overlapping contours; not a spec style, the spec outline is drawn behind an opaque fill", w, grey));
        rows.push(hollow_row(&plain, &reg.alias, w));
        write_png(&outdir.join(format!("{}_coverage.png", fam.slug)), &stack(&rows, 0, [255, 255, 255, 255]));

        let mut wrows = Vec::new();
        wrows.push(labeler.row(&format!("{} \u{2014} every instance (A route: clone_with_arguments wght=N, registered as its own alias)", fam.name), w, grey));
        let mut alphas: Vec<(String, bool, f32, Vec<u8>)> = Vec::new();
        for i in &insts {
            alphas.push((i.label.clone(), i.italic, i.weight, para_alpha(&plain.collection, &i.alias, FontStyle::normal(), None, 48.0)));
        }
        let mut weights_ok = true;
        for (k, i) in insts.iter().enumerate() {
            let prev = if k > 0 && insts[k - 1].italic == i.italic { Some(&alphas[k - 1].3) } else { None };
            let d = prev.map(|p| alpha_diff(p, &alphas[k].3));
            let lbl = match d {
                Some(d) => {
                    if d.frac < 0.05 || d.ink_ratio <= 1.0 {
                        weights_ok = false;
                    }
                    format!("{} \u{2014} vs previous weight: {:.1}% of inked px differ, ink ratio {:.3}", i.label, d.frac * 100.0, d.ink_ratio)
                }
                None => i.label.clone(),
            };
            let _ = writeln!(rep, "- instance {lbl}");
            wrows.push(labeler.row(&lbl, w, if d.is_some_and(|d| d.frac < 0.05 || d.ink_ratio <= 1.0) { red } else { grey }));
            let (img, _) = line_row(&plain, &format!("{DIFF_TEXT} \u{2026} \u{2014} \u{20AC}"), &i.alias, 48.0, w);
            wrows.push(img);
        }
        let ia = alphas.iter().position(|a| !a.1 && a.0 == reg.label).unwrap();
        let ib = alphas.iter().position(|a| !a.1 && a.0 == bold.label).unwrap();
        let rb = alpha_diff(&alphas[ia].3, &alphas[ib].3);
        let distinct_weights = upright.len() > 1;
        if distinct_weights && (rb.frac < 0.05 || rb.ink_ratio <= 1.02) {
            weights_ok = false;
        }
        let _ = writeln!(
            rep,
            "- Regular ({}) vs Bold ({}): {:.1}% of inked px differ, ink ratio {:.3}{}",
            reg.label,
            bold.label,
            rb.frac * 100.0,
            rb.ink_ratio,
            if distinct_weights { "" } else { " (single weight family, same instance)" }
        );
        if let Some(it) = insts.iter().filter(|i| i.italic).min_by(|a, b| (a.weight - reg.weight).abs().total_cmp(&(b.weight - reg.weight).abs())) {
            let ai = alphas.iter().position(|a| a.1 && a.0 == it.label).unwrap();
            let di = alpha_diff(&alphas[ia].3, &alphas[ai].3);
            let _ = writeln!(rep, "- Upright {} vs {}: {:.1}% of inked px differ", reg.label, it.label, di.frac * 100.0);
            notes.push(format!("italic vs upright differ {:.0}% px", di.frac * 100.0));
        }
        let base_file = upright[0].file;
        if wght_axis(&load_typeface(&mgr, &dir.join(base_file), 0).unwrap()).is_some() {
            let base_tf = load_typeface(&mgr, &dir.join(base_file), 0).unwrap();
            let naive = make_fonts(&mgr, &[(base_tf, "Naive".into())], None);
            let b_route = para_alpha(&naive.collection, "Naive", FontStyle::bold(), None, 48.0);
            let c_route = para_alpha(&naive.collection, "Naive", FontStyle::normal(), Some(bold.weight), 48.0);
            let def_route = para_alpha(&naive.collection, "Naive", FontStyle::normal(), None, 48.0);
            let db = alpha_diff(&alphas[ib].3, &b_route);
            let dc = alpha_diff(&alphas[ib].3, &c_route);
            let dd = alpha_diff(&def_route, &b_route);
            let _ = writeln!(
                rep,
                "- Route B (one variable face registered, TextStyle FontStyle::bold()): vs A-bold {:.1}% px differ, ink ratio {:.3}; vs default instance {:.1}% differ, ink ratio {:.3}",
                db.frac * 100.0,
                db.ink_ratio,
                dd.frac * 100.0,
                dd.ink_ratio
            );
            let _ = writeln!(rep, "- Route C (one variable face, TextStyle font_arguments wght={}): vs A-bold {:.1}% px differ, ink ratio {:.3}", bold.weight, dc.frac * 100.0, dc.ink_ratio);
            notes.push(format!("route B vs A-bold {:.1}%, route C vs A-bold {:.1}%", db.frac * 100.0, dc.frac * 100.0));
        }
        if let Some(ax) = reg.tf.variation_design_parameters() {
            if let Some(op) = ax.iter().find(|a| a.tag == tag("opsz")) {
                let raw = load_typeface(&mgr, &dir.join(reg.file), 0).unwrap();
                let hi = with_axes(&raw, &[("wght", reg.weight), ("opsz", op.max)]);
                let f2 = make_fonts(&mgr, &[(hi, "OpszMax".into())], None);
                let a_hi = para_alpha(&f2.collection, "OpszMax", FontStyle::normal(), None, 48.0);
                let d = alpha_diff(&alphas[ia].3, &a_hi);
                let _ = writeln!(rep, "- opsz axis: default {} is used unless set; opsz {} vs default at wght {}: {:.1}% of inked px differ, ink ratio {:.3}", op.def, op.max, reg.weight, d.frac * 100.0, d.ink_ratio);
                wrows.push(labeler.row(&format!("opsz {} (display optical size) at {} \u{2014} vs default opsz {}: {:.1}% px differ", op.max, reg.label, op.def, d.frac * 100.0), w, grey));
                let fx = make_fonts(&mgr, &[(with_axes(&raw, &[("wght", reg.weight), ("opsz", op.max)]), "OpszMax".into())], None);
                let (img, _) = line_row(&fx, &format!("{DIFF_TEXT} \u{2026} \u{2014} \u{20AC}"), "OpszMax", 48.0, w);
                wrows.push(img);
            }
        }
        write_png(&outdir.join(format!("{}_weights.png", fam.slug)), &stack(&wrows, 0, [255, 255, 255, 255]));

        {
            let nfd = lines().into_iter().find(|d| d.key == "nfd_combining_info").unwrap().text;
            let probe = format!("{nfd}   precomposed: \u{0107} \u{017A} \u{015B} \u{0144} \u{0119} \u{0103} \u{1EB7} \u{0175} \u{0177}");
            let laid = layout(&plain, &spec(&probe, &reg.alias, 60.0, 2400.0));
            nfd_rows.push(labeler.row(&format!("{} \u{2014} NFD (base + combining mark) then precomposed \u{2014} paragraph unresolved {}", fam.name, laid.unresolved), 2440, grey));
            let r = rasterize(&laid, 1.0);
            let marks = notdef_marks(&laid, &r);
            nfd_rows.push(flatten(&r, 2440, 10, Color::WHITE, &marks));
        }
        let frame = styles_frame(&with_emoji, fam, &reg.alias, &bold.alias);
        write_png(&outdir.join(format!("{}_styles_1080p.png", fam.slug)), &frame);

        let fmt_b = |v: &Vec<(char, &str, bool)>, want_tofu: bool| {
            let sel: Vec<String> = v
                .iter()
                .filter(|x| x.2 == want_tofu)
                .map(|(c, f, _)| format!("{c} U+{:04X}{}", *c as u32, if *f == reg.file { String::new() } else { format!(" [{f}]") }))
                .collect();
            if sel.is_empty() { "none".to_string() } else { format!("{} ({})", sel.join(", "), sel.len()) }
        };
        let tofu_n = |v: &Vec<(char, &str, bool)>| v.iter().filter(|x| x.2).count();
        for (lbl, v) in [
            ("core (ASCII + common punctuation)", &core_missing),
            ("Latin-1 letters / sample", &accent_missing),
            ("Latin Extended-A", &exta_missing),
            ("Vietnamese \u{1EB7}", &viet_missing),
            ("extras (informational)", &info_missing),
        ] {
            let _ = writeln!(rep, "- {lbl}: TOFU {} | no precomposed glyph but composed by the shaper from base + mark: {}", fmt_b(v, true), fmt_b(v, false));
        }
        let _ = writeln!(rep, "- lowercase a-z mapped to the same glyph as uppercase: {lower_as_upper} of 26");
        let _ = writeln!(rep, "- weights distinct and monotonic: {weights_ok}");
        let grade = if tofu_n(&core_missing) > 0 || !weights_ok {
            "fail".to_string()
        } else if tofu_n(&accent_missing) + tofu_n(&exta_missing) + tofu_n(&viet_missing) == 0 {
            "pass".to_string()
        } else {
            "pass with gaps".to_string()
        };
        let _ = writeln!(rep, "- automatic grade (before visual review): **{grade}**");
        notes.insert(
            0,
            format!(
                "tofu: core {}, latin1/sample {}, extA {}, viet {}, extras {}; composed-from-marks: {}",
                tofu_n(&core_missing),
                tofu_n(&accent_missing),
                tofu_n(&exta_missing),
                tofu_n(&viet_missing),
                tofu_n(&info_missing),
                [&core_missing, &accent_missing, &exta_missing, &viet_missing].iter().map(|v| v.len() - tofu_n(v)).sum::<usize>()
            ),
        );
        results.push(FamResult { name: fam.name, license: fam.license, candidate: fam.candidate, grade, notes });
        eprintln!("specimen: {} done", fam.name);
    }
    if !nfd_rows.is_empty() {
        write_png(&outdir.join("nfd_marks_all.png"), &stack(&nfd_rows, 0, [255, 255, 255, 255]));
    }
    let mut head = String::from("# Font specimen results (FreeType FontMgr::custom_empty, Noto-COLRv1 emoji fallback)\n\n| family | licence | candidate | automatic grade | notes |\n|---|---|---|---|---|\n");
    for r in &results {
        let _ = writeln!(head, "| {} | {} | {} | {} | {} |", r.name, r.license, r.candidate, r.grade, r.notes.join("; "));
    }
    head + &rep
}

pub const CANDIDATES: &[Fam] = &[
    Fam { name: "Nunito", slug: "nunito", license: "OFL-1.1", candidate: true, upright: &[("Nunito[wght].ttf", V)], italic: &[("Nunito-Italic[wght].ttf", V)] },
    Fam { name: "Baloo 2", slug: "baloo2", license: "OFL-1.1", candidate: true, upright: &[("Baloo2[wght].ttf", V)], italic: &[] },
    Fam { name: "Quicksand", slug: "quicksand", license: "OFL-1.1", candidate: true, upright: &[("Quicksand[wght].ttf", V)], italic: &[] },
    Fam { name: "Shantell Sans", slug: "shantellsans", license: "OFL-1.1", candidate: true, upright: &[("ShantellSans[BNCE,INFM,SPAC,wght].ttf", V)], italic: &[("ShantellSans-Italic[BNCE,INFM,SPAC,wght].ttf", V)] },
    Fam { name: "Sedgwick Ave Display", slug: "sedgwickavedisplay", license: "OFL-1.1", candidate: true, upright: &[("SedgwickAveDisplay-Regular.ttf", 400.0)], italic: &[] },
    Fam { name: "Knewave", slug: "knewave", license: "OFL-1.1", candidate: true, upright: &[("Knewave-Regular.ttf", 400.0)], italic: &[] },
    Fam { name: "Caveat Brush", slug: "caveatbrush", license: "OFL-1.1", candidate: true, upright: &[("CaveatBrush-Regular.ttf", 400.0)], italic: &[] },
    Fam { name: "Abril Fatface", slug: "abrilfatface", license: "OFL-1.1", candidate: true, upright: &[("AbrilFatface-Regular.ttf", 400.0)], italic: &[] },
    Fam { name: "Gloock", slug: "gloock", license: "OFL-1.1", candidate: true, upright: &[("Gloock-Regular.ttf", 400.0)], italic: &[] },
];
