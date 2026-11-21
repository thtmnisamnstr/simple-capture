//! A text item laid out and rasterized (ADR-021): Skia paragraph layout in 1080p units (1080 =
//! the canvas's short side), drawn at output scale into premultiplied RGBA with its box, glow,
//! shadow, outline and fill. The box and the text are separate layers so a reveal never cuts the
//! box. Reveal order comes from glyph clusters, never from `utf8Starts`.

use std::f32::consts::FRAC_1_SQRT_2;

use sc_core::image::RgbaImage;
use sc_core::project::{Rgba, TextAlign as Align, TextBackgroundKind, TextFill, TextStyle};
use sc_core::{Error, Result};
use sc_geometry::SizePx;
use skia_safe::gradient::{Colors, Gradient, Interpolation, shaders};
use skia_safe::textlayout::{
    ParagraphBuilder, ParagraphStyle, RectHeightStyle, RectWidthStyle, TextAlign, TextStyle as Ts,
};
use skia_safe::{
    AlphaType, BlurStyle, Color, Color4f, ColorType, Font, FontHinting, GlyphId, ImageInfo,
    MaskFilter, Paint, PaintJoin, PaintStyle, Path, PathBuilder, Point, Rect, TileMode, font,
    surfaces,
};

use crate::fonts::{EMOJI_FAMILY, FontLibrary};

struct Run {
    font: Font,
    glyphs: Vec<GlyphId>,
    pos: Vec<Point>,
    emoji: bool,
}

#[derive(Debug, Clone, Copy)]
struct LineBox {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
}

#[derive(Debug, Clone, Copy)]
struct GlyphBox {
    cluster: u32,
    line: usize,
    pen_x: f32,
}

/// What the compositor needs to place a laid-out item, in image pixels.
#[derive(Debug, Clone)]
pub struct PreparedText {
    pub size: SizePx,
    /// Pixels per 1080 unit.
    pub scale: f32,
    /// The text block's centre in the image.
    pub anchor: (f32, f32),
    /// The text block (all lines, without effects) in 1080 units.
    pub block: (f32, f32),
    pub boxes: Option<RgbaImage>,
    pub text: RgbaImage,
    pub unresolved: usize,
    clusters: Vec<(u32, usize, f32)>,
    words: Vec<(usize, f32, f32)>,
    lines: Vec<(f32, f32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reveal {
    All,
    Clusters(usize),
    Words(usize),
}

fn color(c: &Rgba) -> Color4f {
    let [r, g, b, a] = c.0;
    Color4f::new(r as f32, g as f32, b as f32, a as f32)
}

fn layout_width(canvas: SizePx) -> f32 {
    let short = canvas.w.min(canvas.h).max(1) as f32;
    canvas.w as f32 / short * 1080.0 * 0.9
}

/// Lays out and rasterizes `content` in `style` for a canvas, `scale` times the canvas's own
/// 1080-unit size (the item's transform scale).
pub fn prepare(
    fonts: &mut FontLibrary,
    content: &str,
    style: &TextStyle,
    canvas: SizePx,
    scale: f32,
) -> Result<PreparedText> {
    let shown: String = if style.uppercase {
        content.to_uppercase()
    } else {
        content.to_string()
    };
    let shown = if shown.is_empty() {
        " ".to_string()
    } else {
        shown
    };
    let family = fonts.resolve(&style.font_family, style.weight, style.italic)?;
    let collection = fonts.collection();
    let mut ps = ParagraphStyle::new();
    ps.set_text_align(match style.align {
        Align::Left => TextAlign::Left,
        Align::Center => TextAlign::Center,
        Align::Right => TextAlign::Right,
    });
    let mut ts = Ts::new();
    ts.set_font_families(&[family.as_str(), EMOJI_FAMILY]);
    ts.set_font_size(style.size_1080 as f32);
    ts.set_letter_spacing(style.letter_spacing as f32);
    ts.set_height(style.line_height as f32);
    ts.set_height_override(true);
    ts.set_half_leading(true);
    ts.set_color(Color::BLACK);
    ts.set_font_edging(font::Edging::AntiAlias);
    ts.set_subpixel(true);
    ts.set_font_hinting(FontHinting::None);
    ps.set_text_style(&ts);
    let mut b = ParagraphBuilder::new(&ps, collection);
    b.push_style(&ts);
    b.add_text(&shown);
    let mut para = b.build();
    para.layout(layout_width(canvas));
    let unresolved = para.unresolved_glyphs().unwrap_or(0);

    let mut lines = Vec::new();
    let mut y = 0.0f32;
    for m in &para.get_line_metrics() {
        let top = y;
        y += m.height as f32;
        lines.push(LineBox {
            left: m.left as f32,
            right: (m.left + m.width) as f32,
            top,
            bottom: y,
        });
    }
    if lines.is_empty() {
        return Err(Error::Invalid("sc_text: layout produced no lines".into()));
    }

    let mut runs = Vec::new();
    let mut glyphs = Vec::new();
    let mut ink = Rect::new_empty();
    para.extended_visit(|line, info| {
        let Some(info) = info else { return };
        let font = info.font().clone();
        let emoji = font.typeface().family_name() == EMOJI_FAMILY;
        let o = info.origin();
        let pos: Vec<Point> = info
            .positions()
            .iter()
            .map(|p| Point::new(p.x + o.x, p.y + o.y))
            .collect();
        for (i, p) in pos.iter().enumerate() {
            let bb = info.bounds()[i];
            ink.join(Rect::new(
                bb.left + p.x,
                bb.top + p.y,
                bb.right + p.x,
                bb.bottom + p.y,
            ));
            glyphs.push(GlyphBox {
                cluster: 0,
                line,
                pen_x: p.x,
            });
        }
        runs.push(Run {
            font,
            glyphs: info.glyphs().to_vec(),
            pos,
            emoji,
        });
    });
    for g in &mut glyphs {
        let l = &lines[g.line.min(lines.len() - 1)];
        if let Some(c) =
            para.get_closest_glyph_cluster_at((g.pen_x + 0.5, (l.top + l.bottom) / 2.0))
        {
            g.cluster = c.text_range.start as u32;
        }
    }
    glyphs.sort_by_key(|g| g.cluster);

    let mut words = Vec::new();
    let mut u16_off = 0usize;
    let mut start: Option<usize> = None;
    let mut push_word = |s: usize, e: usize| {
        let rects = para.get_rects_for_range(s..e, RectHeightStyle::Max, RectWidthStyle::Tight);
        if let Some(first) = rects.first() {
            let mut r = first.rect;
            for x in &rects[1..] {
                r.join(x.rect);
            }
            let cy = r.center_y();
            let line = lines
                .iter()
                .position(|l| cy >= l.top && cy < l.bottom)
                .unwrap_or(lines.len() - 1);
            words.push((line, r.left, r.right));
        }
    };
    for ch in shown.chars() {
        match (ch.is_whitespace(), start) {
            (false, None) => start = Some(u16_off),
            (true, Some(s)) => {
                push_word(s, u16_off);
                start = None;
            }
            _ => {}
        }
        u16_off += ch.len_utf16();
    }
    if let Some(s) = start {
        push_word(s, u16_off);
    }

    let block = lines.iter().fold(Rect::new_empty(), |mut r, l| {
        r.join(Rect::new(
            l.left.min(l.right),
            l.top,
            l.right.max(l.left),
            l.bottom,
        ));
        r
    });
    let bg = style.background.as_ref();
    let bg_path = bg.map(|bg| {
        let pad = (bg.padding_1080.x as f32, bg.padding_1080.y as f32);
        box_path(
            &lines,
            pad,
            bg.radius_1080 as f32,
            bg.kind != TextBackgroundKind::PerLine,
        )
    });
    let mut bounds = block;
    bounds.join(ink);
    if let Some(p) = &bg_path {
        bounds.join(p.bounds());
    }
    let mut m = 2.0f32;
    if let Some(o) = &style.outline {
        m = m.max(o.width_1080 as f32 + 2.0);
    }
    if let Some(g) = &style.glow {
        m = m.max(g.radius_1080 as f32 * 3.0 + 2.0);
    }
    if let Some(s) = &style.shadow {
        let off = (s.offset_1080.x.abs()).max(s.offset_1080.y.abs()) as f32;
        m = m.max(off + s.blur_1080 as f32 * 3.0 + 2.0);
    }
    let bounds = bounds.with_outset((m, m));

    let px_per_unit = canvas.w.min(canvas.h).max(1) as f32 / 1080.0 * scale.max(0.01);
    let w = (bounds.width() * px_per_unit).ceil().clamp(1.0, 8192.0) as u32;
    let h = (bounds.height() * px_per_unit).ceil().clamp(1.0, 8192.0) as u32;
    let size = SizePx { w, h };
    let draw = |paint_fn: &dyn Fn(&skia_safe::Canvas)| -> Result<RgbaImage> {
        let mut px = vec![0u8; (w * h * 4) as usize];
        {
            let info = ImageInfo::new(
                (w as i32, h as i32),
                ColorType::RGBA8888,
                AlphaType::Premul,
                None,
            );
            let mut surface = surfaces::wrap_pixels(&info, &mut px, (w * 4) as usize, None)
                .ok_or_else(|| Error::Other("sc_text: cannot wrap a text surface".into()))?;
            let c = surface.canvas();
            c.scale((px_per_unit, px_per_unit));
            c.translate((-bounds.left, -bounds.top));
            paint_fn(c);
        }
        Ok(RgbaImage {
            size,
            stride: w as usize * 4,
            data: px,
        })
    };
    let inverted = bg.is_some_and(|b| b.kind == TextBackgroundKind::Inverted);
    let boxes = match (bg, &bg_path) {
        (Some(bg), Some(path)) => {
            let fill = if inverted {
                match &style.fill {
                    TextFill::Solid(c) => color(c),
                    TextFill::Linear(l) => color(&l.stops[0]),
                }
            } else {
                color(&bg.color)
            };
            Some(draw(&|c| {
                let mut p = Paint::new(fill, None);
                p.set_anti_alias(true);
                c.draw_path(path, &p);
            })?)
        }
        _ => None,
    };
    let text_fill = if inverted {
        TextFill::Solid(bg.map_or(Rgba([1.0, 1.0, 1.0, 1.0]), |b| b.color))
    } else {
        style.fill.clone()
    };
    let text = draw(&|c| paint_text(c, &runs, style, &text_fill, block))?;
    let to_px = |x: f32, y: f32| {
        (
            (x - bounds.left) * px_per_unit,
            (y - bounds.top) * px_per_unit,
        )
    };
    let anchor = to_px(block.center_x(), block.center_y());
    Ok(PreparedText {
        size,
        scale: px_per_unit,
        anchor,
        block: (block.width(), block.height()),
        boxes,
        text,
        unresolved,
        clusters: glyphs
            .iter()
            .map(|g| (g.cluster, g.line, to_px(g.pen_x, 0.0).0))
            .collect(),
        words: words
            .iter()
            .map(|(l, a, b)| (*l, to_px(*a, 0.0).0, to_px(*b, 0.0).0))
            .collect(),
        lines: lines
            .iter()
            .map(|l| (to_px(0.0, l.top).1, to_px(0.0, l.bottom).1))
            .collect(),
    })
}

fn draw_runs(c: &skia_safe::Canvas, runs: &[Run], paint: &Paint, dx: f32, dy: f32, emoji: bool) {
    for r in runs {
        if r.emoji && !emoji {
            continue;
        }
        c.draw_glyphs_at(&r.glyphs, r.pos.as_slice(), (dx, dy), &r.font, paint);
    }
}

fn paint_text(c: &skia_safe::Canvas, runs: &[Run], s: &TextStyle, fill: &TextFill, block: Rect) {
    let mut aa = Paint::default();
    aa.set_anti_alias(true);
    if let Some(sh) = &s.shadow {
        let blur = (sh.blur_1080 as f32).max(0.01);
        let mut p = aa.clone();
        p.set_color4f(color(&sh.color), None);
        p.set_mask_filter(MaskFilter::blur(BlurStyle::Normal, blur, true));
        if let Some(o) = &s.outline {
            p.set_style(PaintStyle::StrokeAndFill);
            p.set_stroke_width(o.width_1080 as f32 * 2.0);
            p.set_stroke_join(PaintJoin::Round);
        }
        draw_runs(
            c,
            runs,
            &p,
            sh.offset_1080.x as f32,
            sh.offset_1080.y as f32,
            true,
        );
    }
    if let Some(g) = &s.glow {
        let r = g.radius_1080 as f32;
        for (sigma, width) in [(r, r * 0.6), (r * 0.35, r * 0.25)] {
            let mut p = aa.clone();
            p.set_color4f(color(&g.color), None);
            p.set_style(PaintStyle::StrokeAndFill);
            p.set_stroke_width(width);
            p.set_stroke_join(PaintJoin::Round);
            p.set_mask_filter(MaskFilter::blur(BlurStyle::Normal, sigma.max(0.01), true));
            draw_runs(c, runs, &p, 0.0, 0.0, false);
        }
    }
    if let Some(o) = &s.outline {
        let mut p = aa.clone();
        p.set_color4f(color(&o.color), None);
        p.set_style(PaintStyle::Stroke);
        p.set_stroke_width(o.width_1080 as f32 * 2.0);
        p.set_stroke_join(PaintJoin::Round);
        draw_runs(c, runs, &p, 0.0, 0.0, false);
    }
    let mut paint = aa.clone();
    match fill {
        TextFill::Solid(col) => {
            paint.set_color4f(color(col), None);
        }
        TextFill::Linear(l) => {
            let a = (l.angle_deg as f32).to_radians();
            let (cx, cy) = (block.center_x(), block.center_y());
            let half = 0.5 * (block.width() * a.cos().abs() + block.height() * a.sin().abs());
            let (dx, dy) = (a.cos() * half, a.sin() * half);
            let cols = [color(&l.stops[0]), color(&l.stops[1])];
            let grad = Gradient::new(
                Colors::new(&cols, None, TileMode::Clamp, None),
                Interpolation::default(),
            );
            if let Some(sh) =
                shaders::linear_gradient(((cx - dx, cy - dy), (cx + dx, cy + dy)), &grad, None)
            {
                paint.set_shader(sh);
            }
        }
    }
    draw_runs(c, runs, &paint, 0.0, 0.0, false);
    for r in runs.iter().filter(|r| r.emoji) {
        c.draw_glyphs_at(&r.glyphs, r.pos.as_slice(), (0.0, 0.0), &r.font, &aa);
    }
}

/// Rounded boxes behind the lines: one per line, or merged into one outline per run of
/// overlapping lines (TikTok style), corners rounded and near-equal edges aligned.
fn box_path(lines: &[LineBox], pad: (f32, f32), radius: f32, merge: bool) -> Path {
    let mut pb = PathBuilder::new();
    let mut groups: Vec<Vec<(f32, f32, f32, f32)>> = Vec::new();
    let mut cur: Vec<(f32, f32, f32, f32)> = Vec::new();
    for l in lines {
        let empty = l.right - l.left < 0.5;
        let r = (l.left - pad.0, l.right + pad.0, l.top, l.bottom);
        let overlaps = cur
            .last()
            .is_some_and(|p: &(f32, f32, f32, f32)| r.0 < p.1 && r.1 > p.0);
        if empty || !merge || (!cur.is_empty() && !overlaps) {
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
        g[0].2 -= pad.1;
        g[n - 1].3 += pad.1;
        for i in 1..n {
            if (g[i].1 - g[i - 1].1).abs() < radius * 0.5 {
                let v = g[i].1.max(g[i - 1].1);
                g[i].1 = v;
                g[i - 1].1 = v;
            }
            if (g[i].0 - g[i - 1].0).abs() < radius * 0.5 {
                let v = g[i].0.min(g[i - 1].0);
                g[i].0 = v;
                g[i - 1].0 = v;
            }
        }
        let mut pts: Vec<Point> = vec![Point::new(g[0].0, g[0].2), Point::new(g[0].1, g[0].2)];
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
            if clean
                .last()
                .is_none_or(|q: &Point| (q.x - p.x).abs() > 0.01 || (q.y - p.y).abs() > 0.01)
            {
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
                ((v.x - p.x) * (nx.y - v.y) - (v.y - p.y) * (nx.x - v.x)).abs() > 1e-3
            })
            .map(|i| clean[i])
            .collect();
        let k = keep.len();
        if k < 3 {
            continue;
        }
        for i in 0..k {
            let p = keep[(i + k - 1) % k];
            let v = keep[i];
            let nx = keep[(i + 1) % k];
            let dp = p - v;
            let dn = nx - v;
            let r = radius.min(dp.length() / 2.0).min(dn.length() / 2.0);
            let a = v + Point::new(dp.x / dp.length() * r, dp.y / dp.length() * r);
            let b = v + Point::new(dn.x / dn.length() * r, dn.y / dn.length() * r);
            if i == 0 {
                pb.move_to(a);
            } else {
                pb.line_to(a);
            }
            pb.conic_to(v, b, FRAC_1_SQRT_2);
        }
        pb.close();
    }
    pb.detach()
}

impl PreparedText {
    pub fn cluster_count(&self) -> usize {
        let mut c: Vec<u32> = self.clusters.iter().map(|g| g.0).collect();
        c.dedup();
        c.len()
    }

    pub fn word_count(&self) -> usize {
        self.words.len()
    }

    /// Strips of the image that show at `reveal`, as `(x0, y0, x1, y1)` pixels.
    fn tiles(&self, reveal: Reveal) -> Vec<(f32, f32, f32, f32)> {
        let (w, h) = (self.size.w as f32, self.size.h as f32);
        let nl = self.lines.len();
        let band = |li: usize| {
            let top = if li == 0 {
                0.0
            } else {
                (self.lines[li - 1].1 + self.lines[li].0) / 2.0
            };
            let bottom = if li + 1 == nl {
                h
            } else {
                (self.lines[li].1 + self.lines[li + 1].0) / 2.0
            };
            (top, bottom)
        };
        let mut tiles = Vec::new();
        match reveal {
            Reveal::All => tiles.push((0.0, 0.0, w, h)),
            Reveal::Clusters(n) => {
                let mut cl: Vec<u32> = self.clusters.iter().map(|g| g.0).collect();
                cl.dedup();
                let Some(&cut) = cl.get(n) else {
                    tiles.push((0.0, 0.0, w, h));
                    return tiles;
                };
                for li in 0..nl {
                    let (top, bottom) = band(li);
                    let on: Vec<&(u32, usize, f32)> =
                        self.clusters.iter().filter(|g| g.1 == li).collect();
                    if on.is_empty() {
                        continue;
                    }
                    if on.iter().all(|g| g.0 < cut) {
                        tiles.push((0.0, top, w, bottom));
                    } else {
                        if on.iter().any(|g| g.0 < cut) {
                            let x = on
                                .iter()
                                .filter(|g| g.0 >= cut)
                                .map(|g| g.2)
                                .fold(f32::MAX, f32::min);
                            tiles.push((0.0, top, x, bottom));
                        }
                        break;
                    }
                }
            }
            Reveal::Words(n) => {
                let mut order: Vec<(usize, f32, f32)> = self.words.clone();
                order.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
                for (i, wd) in order.iter().enumerate().take(n) {
                    let (top, bottom) = band(wd.0);
                    let first_on_line = i == 0 || order[i - 1].0 != wd.0;
                    let last_on_line = i + 1 == order.len() || order[i + 1].0 != wd.0;
                    let left = if first_on_line {
                        0.0
                    } else {
                        (order[i - 1].2 + wd.1) / 2.0
                    };
                    let right = if last_on_line {
                        w
                    } else {
                        (wd.2 + order[i + 1].1) / 2.0
                    };
                    tiles.push((left, top, right, bottom));
                }
            }
        }
        tiles
    }

    /// The image to draw: boxes, then the revealed part of the text over them.
    pub fn compose(&self, reveal: Reveal) -> RgbaImage {
        if reveal == Reveal::All && self.boxes.is_none() {
            return self.text.clone();
        }
        let mut out = self.boxes.clone().unwrap_or(RgbaImage {
            size: self.size,
            stride: self.size.w as usize * 4,
            data: vec![0; (self.size.w * self.size.h * 4) as usize],
        });
        let w = self.size.w as usize;
        for (x0, y0, x1, y1) in self.tiles(reveal) {
            let (x0, x1) = (x0.max(0.0) as usize, (x1.ceil() as usize).min(w));
            let (y0, y1) = (
                y0.max(0.0) as usize,
                (y1.ceil() as usize).min(self.size.h as usize),
            );
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = y * w * 4 + x * 4;
                    let s = &self.text.data[i..i + 4];
                    let ia = 255 - u32::from(s[3]);
                    for (o, sc) in out.data[i..i + 4].iter_mut().zip(s) {
                        *o = (u32::from(*sc) + (u32::from(*o) * ia + 127) / 255).min(255) as u8;
                    }
                }
            }
        }
        out
    }
}
