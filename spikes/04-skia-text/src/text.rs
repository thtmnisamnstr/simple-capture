use crate::fonts::{EMOJI_FAMILY, Fonts};
use skia_safe::{
    AlphaType, BlurStyle, Color, ColorType, Font, FontHinting, GlyphId, ImageInfo, MaskFilter, Paint,
    PaintJoin, PaintStyle, Path, PathBuilder, Point, Rect, Shader, TileMode, font, surfaces,
    textlayout::{
        ParagraphBuilder, ParagraphStyle, RectHeightStyle, RectWidthStyle, TextAlign,
        TextStyle,
    },
};
use std::f32::consts::FRAC_1_SQRT_2;

#[derive(Clone)]
pub enum Fill {
    Solid(Color),
    Gradient(Vec<Color>),
}

#[derive(Clone)]
pub struct Stroke {
    pub color: Color,
    pub width: f32,
}

#[derive(Clone)]
pub struct Shadow {
    pub color: Color,
    pub dx: f32,
    pub dy: f32,
    pub blur: f32,
}

#[derive(Clone)]
pub struct Glow {
    pub color: Color,
    pub radius: f32,
}

#[derive(Clone)]
pub struct BgBox {
    pub color: Color,
    pub radius: f32,
    pub pad_x: f32,
    pub pad_y: f32,
}

#[derive(Clone)]
pub struct TextSpec {
    pub name: &'static str,
    pub text: String,
    pub family: &'static str,
    pub size: f32,
    pub align: TextAlign,
    pub letter_spacing: f32,
    pub line_height: f32,
    pub uppercase: bool,
    pub max_width: f32,
    pub fill: Fill,
    pub stroke: Option<Stroke>,
    pub shadow: Option<Shadow>,
    pub glow: Option<Glow>,
    pub bg: Option<BgBox>,
}

pub struct Run {
    pub font: Font,
    pub glyphs: Vec<GlyphId>,
    pub pos: Vec<Point>,
    pub emoji: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct LineBox {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct GlyphBox {
    pub utf8: u32,
    pub line: usize,
    pub pen_x: f32,
    pub ink: Rect,
}

#[derive(Clone, Copy, Debug)]
pub struct WordBox {
    pub line: usize,
    pub rect: Rect,
}

pub struct Laid {
    pub spec: TextSpec,
    pub runs: Vec<Run>,
    pub lines: Vec<LineBox>,
    pub words: Vec<WordBox>,
    pub glyphs: Vec<GlyphBox>,
    pub bg_path: Option<Path>,
    pub bounds: Rect,
    pub unresolved: usize,
}

pub struct Raster {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
    pub scale: f32,
    pub origin: Point,
}

impl Raster {
    pub fn layout_to_texel(&self, p: Point) -> Point {
        Point::new((p.x - self.origin.x) * self.scale, (p.y - self.origin.y) * self.scale)
    }
    pub fn rect_to_texel(&self, r: Rect) -> Rect {
        let a = self.layout_to_texel(Point::new(r.left, r.top));
        let b = self.layout_to_texel(Point::new(r.right, r.bottom));
        Rect::new(a.x, a.y, b.x, b.y)
    }
}

fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

pub fn layout(fonts: &Fonts, spec: &TextSpec) -> Laid {
    let shown = if spec.uppercase { spec.text.to_uppercase() } else { spec.text.clone() };
    let mut ps = ParagraphStyle::new();
    ps.set_text_align(spec.align);
    let mut ts = TextStyle::new();
    ts.set_font_families(&[spec.family, EMOJI_FAMILY]);
    ts.set_font_size(spec.size);
    ts.set_letter_spacing(spec.letter_spacing);
    ts.set_height(spec.line_height);
    ts.set_height_override(true);
    ts.set_half_leading(true);
    ts.set_color(Color::BLACK);
    ts.set_font_edging(font::Edging::AntiAlias);
    ts.set_subpixel(true);
    ts.set_font_hinting(FontHinting::None);
    ps.set_text_style(&ts);
    let mut b = ParagraphBuilder::new(&ps, fonts.collection.clone());
    b.push_style(&ts);
    b.add_text(&shown);
    let mut para = b.build();
    para.layout(spec.max_width);
    let unresolved = para.unresolved_glyphs().unwrap_or(0);

    let metrics = para.get_line_metrics();
    let mut lines = Vec::new();
    let mut y = 0.0f32;
    for m in &metrics {
        let top = y;
        let bottom = y + m.height as f32;
        y = bottom;
        lines.push(LineBox {
            left: m.left as f32,
            right: (m.left + m.width) as f32,
            top,
            bottom,
        });
    }

    let mut runs = Vec::new();
    let mut glyphs = Vec::new();
    para.extended_visit(|line, info| {
        let Some(info) = info else { return };
        let font = info.font().clone();
        let emoji = font.typeface().family_name() == EMOJI_FAMILY;
        let o = info.origin();
        let pos: Vec<Point> = info.positions().iter().map(|p| Point::new(p.x + o.x, p.y + o.y)).collect();
        for (i, p) in pos.iter().enumerate() {
            let b = info.bounds()[i];
            glyphs.push(GlyphBox {
                utf8: info.utf8_starts()[i],
                line,
                pen_x: p.x,
                ink: Rect::new(b.left + p.x, b.top + p.y, b.right + p.x, b.bottom + p.y),
            });
        }
        runs.push(Run { font, glyphs: info.glyphs().to_vec(), pos, emoji });
    });
    for g in &mut glyphs {
        let l = &lines[g.line];
        if let Some(c) = para.get_closest_glyph_cluster_at((g.pen_x + 0.5, (l.top + l.bottom) / 2.0)) {
            g.utf8 = c.text_range.start as u32;
        }
    }
    glyphs.sort_by_key(|g| g.utf8);

    let mut words = Vec::new();
    let mut u16_off = 0usize;
    let mut start: Option<usize> = None;
    let push_word = |s: usize, e: usize, words: &mut Vec<WordBox>| {
        let boxes = para.get_rects_for_range(s..e, RectHeightStyle::Max, RectWidthStyle::Tight);
        if boxes.is_empty() {
            return;
        }
        let mut r = boxes[0].rect;
        for b in &boxes[1..] {
            r.join(b.rect);
        }
        let cy = r.center_y();
        let line = lines.iter().position(|l| cy >= l.top && cy < l.bottom).unwrap_or(lines.len() - 1);
        words.push(WordBox { line, rect: r });
    };
    for ch in shown.chars() {
        let ws = ch.is_whitespace();
        match (ws, start) {
            (false, None) => start = Some(u16_off),
            (true, Some(s)) => {
                push_word(s, u16_off, &mut words);
                start = None;
            }
            _ => {}
        }
        u16_off += ch.len_utf16();
    }
    if let Some(s) = start {
        push_word(s, utf16_len(&shown), &mut words);
    }

    let bg_path = spec.bg.as_ref().map(|bg| merged_box_path(&lines, bg));
    let mut bounds = Rect::new_empty();
    for l in &lines {
        if l.right > l.left {
            bounds.join(Rect::new(l.left, l.top, l.right, l.bottom));
        }
    }
    for g in &glyphs {
        bounds.join(g.ink);
    }
    if let Some(p) = &bg_path {
        bounds.join(p.bounds());
    }
    let mut m = 2.0f32;
    if let Some(s) = &spec.stroke {
        m = m.max(s.width + 2.0);
    }
    if let Some(g) = &spec.glow {
        m = m.max(g.radius * 3.0 + 2.0);
    }
    if let Some(s) = &spec.shadow {
        m = m.max(s.dx.abs().max(s.dy.abs()) + s.blur * 3.0 + 2.0);
    }
    let bounds = bounds.with_outset((m, m));

    Laid { spec: spec.clone(), runs, lines, words, glyphs, bg_path, bounds, unresolved }
}

pub fn merged_box_path(lines: &[LineBox], bg: &BgBox) -> Path {
    let mut pb = PathBuilder::new();
    let mut groups: Vec<Vec<(f32, f32, f32, f32)>> = Vec::new();
    let mut cur: Vec<(f32, f32, f32, f32)> = Vec::new();
    for l in lines {
        let empty = l.right - l.left < 0.5;
        let r = (l.left - bg.pad_x, l.right + bg.pad_x, l.top, l.bottom);
        let overlaps = cur.last().is_some_and(|p: &(f32, f32, f32, f32)| r.0 < p.1 && r.1 > p.0);
        if empty || (!cur.is_empty() && !overlaps) {
            if !cur.is_empty() {
                groups.push(std::mem::take(&mut cur));
            }
            if empty {
                continue;
            }
        }
        cur.push(r);
    }
    if !cur.is_empty() {
        groups.push(cur);
    }
    for mut g in groups {
        let n = g.len();
        g[0].2 -= bg.pad_y;
        g[n - 1].3 += bg.pad_y;
        for i in 1..n {
            if (g[i].1 - g[i - 1].1).abs() < bg.radius * 0.5 {
                let v = g[i].1.max(g[i - 1].1);
                g[i].1 = v;
                g[i - 1].1 = v;
            }
            if (g[i].0 - g[i - 1].0).abs() < bg.radius * 0.5 {
                let v = g[i].0.min(g[i - 1].0);
                g[i].0 = v;
                g[i - 1].0 = v;
            }
        }
        let mut pts: Vec<Point> = Vec::new();
        pts.push(Point::new(g[0].0, g[0].2));
        pts.push(Point::new(g[0].1, g[0].2));
        for i in 0..n {
            pts.push(Point::new(g[i].1, g[i].3));
            if i + 1 < n {
                pts.push(Point::new(g[i + 1].1, g[i].3));
            }
        }
        pts.push(Point::new(g[n - 1].0, g[n - 1].3));
        for i in (0..n).rev() {
            pts.push(Point::new(g[i].0, g[i].2));
            if i > 0 {
                pts.push(Point::new(g[i - 1].0, g[i].2));
            }
        }
        pts.pop();
        let mut clean: Vec<Point> = Vec::new();
        for p in pts {
            if clean.last().is_none_or(|q: &Point| (q.x - p.x).abs() > 0.01 || (q.y - p.y).abs() > 0.01) {
                clean.push(p);
            }
        }
        while clean.len() > 1 && (clean[0] - clean[clean.len() - 1]).length() < 0.01 {
            clean.pop();
        }
        let k = clean.len();
        let keep: Vec<Point> = (0..k)
            .filter(|&i| {
                let p = clean[(i + k - 1) % k];
                let v = clean[i];
                let nx = clean[(i + 1) % k];
                let c = (v.x - p.x) * (nx.y - v.y) - (v.y - p.y) * (nx.x - v.x);
                c.abs() > 1e-3
            })
            .map(|i| clean[i])
            .collect();
        let k = keep.len();
        for i in 0..k {
            let p = keep[(i + k - 1) % k];
            let v = keep[i];
            let nx = keep[(i + 1) % k];
            let dp = p - v;
            let dn = nx - v;
            let r = bg.radius.min(dp.length() / 2.0).min(dn.length() / 2.0);
            let a = v + Point::new(dp.x / dp.length() * r, dp.y / dp.length() * r);
            let bpt = v + Point::new(dn.x / dn.length() * r, dn.y / dn.length() * r);
            if i == 0 {
                pb.move_to(a);
            } else {
                pb.line_to(a);
            }
            pb.conic_to(v, bpt, FRAC_1_SQRT_2);
        }
        pb.close();
    }
    pb.detach()
}

fn draw_runs(canvas: &skia_safe::Canvas, runs: &[Run], paint: &Paint, dx: f32, dy: f32, emoji: Option<&Paint>) {
    for r in runs {
        let p = if r.emoji {
            match emoji {
                Some(p) => p,
                None => continue,
            }
        } else {
            paint
        };
        canvas.draw_glyphs_at(&r.glyphs, r.pos.as_slice(), (dx, dy), &r.font, p);
    }
}

pub fn rasterize(laid: &Laid, scale: f32) -> Raster {
    let b = laid.bounds;
    let w = (b.width() * scale).ceil().max(1.0) as u32;
    let h = (b.height() * scale).ceil().max(1.0) as u32;
    let mut px = vec![0u8; (w * h * 4) as usize];
    {
        let info = ImageInfo::new((w as i32, h as i32), ColorType::RGBA8888, AlphaType::Premul, None);
        let mut surface = surfaces::wrap_pixels(&info, &mut px, (w * 4) as usize, None).expect("surface");
        let c = surface.canvas();
        c.scale((scale, scale));
        c.translate((-b.left, -b.top));
        paint_item(c, laid);
    }
    Raster { w, h, px, scale, origin: Point::new(b.left, b.top) }
}

#[allow(deprecated)]
pub fn paint_item(c: &skia_safe::Canvas, laid: &Laid) {
    let s = &laid.spec;
    let mut aa = Paint::default();
    aa.set_anti_alias(true);

    if let (Some(bg), Some(path)) = (&s.bg, &laid.bg_path) {
        let mut p = aa.clone();
        p.set_color(bg.color);
        c.draw_path(path, &p);
    }
    if let Some(sh) = &s.shadow {
        let mut p = aa.clone();
        p.set_color(sh.color);
        p.set_mask_filter(MaskFilter::blur(BlurStyle::Normal, sh.blur.max(0.01), true));
        if let Some(st) = &s.stroke {
            p.set_style(PaintStyle::StrokeAndFill);
            p.set_stroke_width(st.width * 2.0);
            p.set_stroke_join(PaintJoin::Round);
        }
        let mut pe = aa.clone();
        pe.set_image_filter(skia_safe::image_filters::drop_shadow_only(
            (0.0, 0.0),
            (sh.blur, sh.blur),
            sh.color,
            None,
            None,
            None,
        ));
        draw_runs(c, &laid.runs, &p, sh.dx, sh.dy, Some(&pe));
    }
    if let Some(g) = &s.glow {
        for (sigma, width) in [(g.radius, g.radius * 0.6), (g.radius * 0.35, g.radius * 0.25)] {
            let mut p = aa.clone();
            p.set_color(g.color);
            p.set_style(PaintStyle::StrokeAndFill);
            p.set_stroke_width(width);
            p.set_stroke_join(PaintJoin::Round);
            p.set_mask_filter(MaskFilter::blur(BlurStyle::Normal, sigma, true));
            draw_runs(c, &laid.runs, &p, 0.0, 0.0, None);
        }
    }
    if let Some(st) = &s.stroke {
        let mut p = aa.clone();
        p.set_color(st.color);
        p.set_style(PaintStyle::Stroke);
        p.set_stroke_width(st.width * 2.0);
        p.set_stroke_join(PaintJoin::Round);
        draw_runs(c, &laid.runs, &p, 0.0, 0.0, None);
    }
    let mut fill = aa.clone();
    match &s.fill {
        Fill::Solid(col) => {
            fill.set_color(*col);
        }
        Fill::Gradient(cols) => {
            let ink = laid.lines.iter().fold(Rect::new_empty(), |mut r, l| {
                r.join(Rect::new(l.left, l.top, l.right, l.bottom));
                r
            });
            let sh = Shader::linear_gradient(
                ((ink.left, ink.top), (ink.right, ink.bottom)),
                cols.as_slice(),
                None,
                TileMode::Clamp,
                None,
                None,
            );
            fill.set_shader(sh);
        }
    }
    let emoji_paint = aa.clone();
    draw_runs(c, &laid.runs, &fill, 0.0, 0.0, Some(&emoji_paint));
}

pub fn word_tiles(laid: &Laid) -> Vec<Rect> {
    let b = laid.bounds;
    let mut tiles = Vec::new();
    let nl = laid.lines.len();
    for (li, l) in laid.lines.iter().enumerate() {
        let top = if li == 0 { b.top } else { (laid.lines[li - 1].bottom + l.top) / 2.0 };
        let bottom = if li + 1 == nl { b.bottom } else { (l.bottom + laid.lines[li + 1].top) / 2.0 };
        let mut ws: Vec<&WordBox> = laid.words.iter().filter(|w| w.line == li).collect();
        ws.sort_by(|a, c| a.rect.left.total_cmp(&c.rect.left));
        for (i, w) in ws.iter().enumerate() {
            let left = if i == 0 { b.left } else { (ws[i - 1].rect.right + w.rect.left) / 2.0 };
            let right = if i + 1 == ws.len() { b.right } else { (w.rect.right + ws[i + 1].rect.left) / 2.0 };
            tiles.push(Rect::new(left, top, right, bottom));
        }
    }
    tiles
}

pub fn typewriter_tiles(laid: &Laid, n: usize) -> Vec<Rect> {
    let b = laid.bounds;
    let mut clusters: Vec<u32> = laid.glyphs.iter().map(|g| g.utf8).collect();
    clusters.dedup();
    let cut_cluster = clusters.get(n).copied();
    let nl = laid.lines.len();
    let mut tiles = Vec::new();
    for (li, l) in laid.lines.iter().enumerate() {
        let top = if li == 0 { b.top } else { (laid.lines[li - 1].bottom + l.top) / 2.0 };
        let bottom = if li + 1 == nl { b.bottom } else { (l.bottom + laid.lines[li + 1].top) / 2.0 };
        let Some(cut) = cut_cluster else {
            tiles.push(Rect::new(b.left, top, b.right, bottom));
            continue;
        };
        let line_glyphs: Vec<&GlyphBox> = laid.glyphs.iter().filter(|g| g.line == li).collect();
        if line_glyphs.is_empty() {
            continue;
        }
        if line_glyphs.iter().all(|g| g.utf8 < cut) {
            tiles.push(Rect::new(b.left, top, b.right, bottom));
        } else if line_glyphs.iter().any(|g| g.utf8 < cut) {
            let x = line_glyphs.iter().filter(|g| g.utf8 >= cut).map(|g| g.pen_x).fold(f32::MAX, f32::min);
            tiles.push(Rect::new(b.left, top, x, bottom));
            break;
        } else {
            break;
        }
    }
    tiles
}

pub fn cluster_count(laid: &Laid) -> usize {
    let mut c: Vec<u32> = laid.glyphs.iter().map(|g| g.utf8).collect();
    c.dedup();
    c.len()
}
