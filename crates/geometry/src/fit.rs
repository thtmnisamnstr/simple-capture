//! Canvas fit and size rules (ARCHITECTURE §8 and §10).
//!
//! Integer inputs use exact rational arithmetic, so results never depend on floating-point
//! error; `round` is round half up, as `f64::round` is for positive values.

use crate::{Aspect, PointF, Ratio, RectPt, RectPx, SizeClass, SizePx};

const FIXED_SHORT_CAP: u128 = 2160;
const ORIGINAL_LONG_CAP: u128 = 8192;
const EMPTY_SHORT: u128 = 1080;

fn round_div(p: u128, q: u128) -> u128 {
    (2 * p + q) / (2 * q)
}

fn even_round_div(p: u128, q: u128) -> u128 {
    (2 * round_div(p, 2 * q)).max(2)
}

fn sat_u32(v: u128) -> u32 {
    u32::try_from(v).unwrap_or(u32::MAX)
}

fn sat_i32(v: u128) -> i32 {
    i32::try_from(v).unwrap_or(i32::MAX)
}

fn long_short_terms(ratio: Ratio) -> (u128, u128) {
    let (a, b) = (u128::from(ratio.num), u128::from(ratio.den));
    if a >= b { (a, b) } else { (b, a) }
}

/// `2·round(x/2)`, minimum 2. Non-finite or non-positive input gives 2.
pub fn even_round(x: f64) -> u32 {
    if !x.is_finite() || x <= 0.0 {
        return 2;
    }
    let half = (x / 2.0).round().min(f64::from(u32::MAX / 2));
    (half as u32 * 2).max(2)
}

/// Contain fit of `content` into `canvas`, both edges snapped (ARCHITECTURE §8).
///
/// `s = min(Wc/w, Hc/h)`, the content is centred and each edge is rounded on its own, so the
/// limiting dimension always spans the whole canvas. A zero-sized canvas or content gives an
/// empty rect at the origin.
pub fn contain_fit(canvas: SizePx, content: SizePx) -> RectPx {
    if canvas.w == 0 || canvas.h == 0 || content.w == 0 || content.h == 0 {
        return RectPx::default();
    }
    let (wc, hc) = (u128::from(canvas.w), u128::from(canvas.h));
    let (w, h) = (u128::from(content.w), u128::from(content.h));
    if wc * h <= hc * w {
        let y0 = round_div(hc * w - h * wc, 2 * w);
        let y1 = round_div(hc * w + h * wc, 2 * w);
        RectPx {
            x: 0,
            y: sat_i32(y0),
            w: canvas.w,
            h: sat_u32(y1 - y0),
        }
    } else {
        let x0 = round_div(wc * h - w * hc, 2 * h);
        let x1 = round_div(wc * h + w * hc, 2 * h);
        RectPx {
            x: sat_i32(x0),
            y: 0,
            w: sat_u32(x1 - x0),
            h: canvas.h,
        }
    }
}

/// Source rect (px of `image`) for a cover fit of `image` into `dst`, centred on `focus`
/// (normalised 0..1 of the image) and clamped so the crop stays inside the image.
///
/// A zero-sized image or destination gives the whole image; a non-finite focus coordinate
/// counts as 0.5.
pub fn cover_src_rect(image: SizePx, dst: SizePx, focus: PointF) -> RectPt {
    let (iw, ih) = (f64::from(image.w), f64::from(image.h));
    if image.w == 0 || image.h == 0 || dst.w == 0 || dst.h == 0 {
        return RectPt {
            x: 0.0,
            y: 0.0,
            w: iw,
            h: ih,
        };
    }
    let (dw, dh) = (f64::from(dst.w), f64::from(dst.h));
    let dst_wider = u64::from(dst.w) * u64::from(image.h) >= u64::from(dst.h) * u64::from(image.w);
    let (vw, vh) = if dst_wider {
        (iw, (iw * dh / dw).min(ih))
    } else {
        ((ih * dw / dh).min(iw), ih)
    };
    let fx = if focus.x.is_finite() { focus.x } else { 0.5 };
    let fy = if focus.y.is_finite() { focus.y } else { 0.5 };
    let cx = (fx * iw).min(iw - vw / 2.0).max(vw / 2.0);
    let cy = (fy * ih).min(ih - vh / 2.0).max(vh / 2.0);
    RectPt {
        x: (cx - vw / 2.0).max(0.0),
        y: (cy - vh / 2.0).max(0.0),
        w: vw,
        h: vh,
    }
}

/// The smallest canvas of `ratio` that shows `content` at 1:1: `(w, w/A)` when `w/h ≥ A`, else
/// `(h·A, h)`.
pub fn natural_canvas(ratio: Ratio, content: SizePx) -> (f64, f64) {
    let ratio = Ratio::new(ratio.num, ratio.den);
    let (w, h) = (f64::from(content.w), f64::from(content.h));
    let (a, b) = (f64::from(ratio.num), f64::from(ratio.den));
    if u64::from(content.w) * u64::from(ratio.den) >= u64::from(content.h) * u64::from(ratio.num) {
        (w, w * b / a)
    } else {
        (h * a / b, h)
    }
}

#[derive(Clone, Copy)]
struct Rational {
    p: u128,
    q: u128,
}

impl Rational {
    fn int(v: u128) -> Rational {
        Rational { p: v, q: 1 }
    }

    fn gt(self, o: Rational) -> bool {
        self.p * o.q > o.p * self.q
    }

    fn min(self, o: Rational) -> Rational {
        if self.gt(o) { o } else { self }
    }
}

fn natural_short(ratio: Ratio, content: SizePx) -> Rational {
    let (lt, st) = long_short_terms(ratio);
    let (w, h) = (u128::from(content.w), u128::from(content.h));
    let (own, other) = if ratio.num >= ratio.den {
        (h, w)
    } else {
        (w, h)
    };
    if other * st > own * lt {
        Rational {
            p: other * st,
            q: lt,
        }
    } else {
        Rational::int(own)
    }
}

fn from_short(ratio: Ratio, short: u128, gif: bool) -> SizePx {
    let (lt, st) = long_short_terms(ratio);
    let long = if gif {
        round_div(short * lt, st).max(1)
    } else {
        even_round_div(short * lt, st)
    };
    let (w, h) = if ratio.num >= ratio.den {
        (long, short)
    } else {
        (short, long)
    };
    SizePx {
        w: sat_u32(w),
        h: sat_u32(h),
    }
}

/// Reference canvas size (ARCHITECTURE §8; derived, never stored).
///
/// - Fixed aspects: short side = the largest natural short side over `segments`, capped at 2160.
/// - `Original` with one segment: that segment's size rounded down to even.
/// - `Original` with several segments, using `ratio` (the stored `original_ratio`): the largest
///   natural short side, with the long side capped at 8192 (also for ratios beyond 4096:1,
///   whose short side is already the minimum of 2).
/// - The short side is rounded to the nearest even number and the long side follows from it and
///   the exact ratio, also even, as for presets.
///
/// `ratio` is ignored for fixed aspects. With no segments the short side is 1080.
pub fn reference_canvas_size(aspect: Aspect, ratio: Ratio, segments: &[SizePx]) -> SizePx {
    if aspect == Aspect::Original
        && let [only] = segments
    {
        return SizePx {
            w: (only.w & !1).max(2),
            h: (only.h & !1).max(2),
        };
    }
    let ratio = aspect
        .ratio()
        .unwrap_or_else(|| Ratio::new(ratio.num, ratio.den));
    let (lt, st) = long_short_terms(ratio);
    let natural = segments
        .iter()
        .map(|&s| natural_short(ratio, s))
        .reduce(|m, s| if s.gt(m) { s } else { m })
        .unwrap_or(Rational::int(EMPTY_SHORT));
    let capped = if aspect == Aspect::Original {
        natural.min(Rational {
            p: ORIGINAL_LONG_CAP * st,
            q: lt,
        })
    } else {
        natural.min(Rational::int(FIXED_SHORT_CAP))
    };
    let mut short = even_round_div(capped.p, capped.q);
    if aspect == Aspect::Original {
        while short > 2 && even_round_div(short * lt, st) > ORIGINAL_LONG_CAP {
            short -= 2;
        }
        let out = from_short(ratio, short, false);
        let cap = ORIGINAL_LONG_CAP as u32;
        return SizePx {
            w: out.w.min(cap),
            h: out.h.min(cap),
        };
    }
    from_short(ratio, short, false)
}

/// Export preset size for a short-side class (ARCHITECTURE §10). Video sizes are even
/// (`2·round(x/2)`); GIF sizes are not even-rounded (427, not 426).
pub fn preset_size(class: SizeClass, ratio: Ratio, gif: bool) -> SizePx {
    let short = match class {
        SizeClass::S2160 => 2160,
        SizeClass::S1440 => 1440,
        SizeClass::S1080 => 1080,
        SizeClass::S720 => 720,
        SizeClass::S480 => 480,
        SizeClass::S360 => 360,
        SizeClass::S240 => 240,
    };
    from_short(Ratio::new(ratio.num, ratio.den), short, gif)
}

/// True when the fitted content is under 25 % of the canvas width or height.
pub fn small_content(canvas: SizePx, content: SizePx) -> bool {
    let dst = contain_fit(canvas, content);
    u64::from(dst.w) * 4 < u64::from(canvas.w) || u64::from(dst.h) * 4 < u64::from(canvas.h)
}
