//! Point/pixel conversion and region editing.

use crate::{Handle, PointF, Ratio, RectPt, RectPx};

const MIN_SIDE: f64 = 1.0;

fn sane_scale(scale: f64) -> f64 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

fn finite(r: RectPt) -> bool {
    r.x.is_finite() && r.y.is_finite() && r.w.is_finite() && r.h.is_finite()
}

/// Converts a point rect to pixels at `scale` px per pt, rounding both edges (no seams).
/// A non-positive or non-finite scale counts as 1.
pub fn pt_to_px(r: RectPt, scale: f64) -> RectPx {
    let s = sane_scale(scale);
    let x0 = (r.x * s).round();
    let y0 = (r.y * s).round();
    let x1 = ((r.x + r.w) * s).round();
    let y1 = ((r.y + r.h) * s).round();
    RectPx {
        x: x0 as i32,
        y: y0 as i32,
        w: (x1 - x0).max(0.0) as u32,
        h: (y1 - y0).max(0.0) as u32,
    }
}

/// Flips between bottom-left and top-left origins (Pitfall 3); applying it twice is the identity.
pub fn flip_y(r: RectPt, container_h_pt: f64) -> RectPt {
    RectPt {
        y: container_h_pt - r.y - r.h,
        ..r
    }
}

/// Snaps a region so its origin sits on whole pixels and its pixel size is even (at least 2),
/// never growing past its own pixel-rounded edges. A non-positive or non-finite scale counts as 1.
pub fn snap_region_even_px(r: RectPt, scale: f64) -> RectPt {
    let s = sane_scale(scale);
    let snap = |o: f64, len: f64| {
        let p0 = (o * s).round();
        let p1 = ((o + len) * s).round();
        let even = (((p1 - p0) / 2.0).floor() * 2.0).max(2.0);
        (p0 / s, even / s)
    };
    let (x, w) = snap(r.x, r.w);
    let (y, h) = snap(r.y, r.h);
    RectPt { x, y, w, h }
}

/// The union of an application's window rects (global points), each clipped to the display, as
/// display-local points (ADR-039). `None` when nothing is left or a side is under 2 pt.
pub fn app_bounds_pt(windows: &[RectPt], display_frame_pt: RectPt) -> Option<RectPt> {
    let d = display_frame_pt;
    if !finite(d) {
        return None;
    }
    let mut acc: Option<(f64, f64, f64, f64)> = None;
    for w in windows.iter().filter(|w| finite(**w)) {
        let x0 = w.x.max(d.x);
        let y0 = w.y.max(d.y);
        let x1 = (w.x + w.w).min(d.x + d.w);
        let y1 = (w.y + w.h).min(d.y + d.h);
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        acc = Some(match acc {
            None => (x0, y0, x1, y1),
            Some((a, b, c, e)) => (a.min(x0), b.min(y0), c.max(x1), e.max(y1)),
        });
    }
    let (x0, y0, x1, y1) = acc?;
    let (w, h) = (x1 - x0, y1 - y0);
    (w >= 2.0 && h >= 2.0).then_some(RectPt {
        x: x0 - d.x,
        y: y0 - d.y,
        w,
        h,
    })
}

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    v.min(hi).max(lo)
}

fn clamp_into(r: RectPt, b: RectPt) -> RectPt {
    let w = clamp(r.w, 0.0, b.w);
    let h = clamp(r.h, 0.0, b.h);
    RectPt {
        x: clamp(r.x, b.x, b.x + b.w - w),
        y: clamp(r.y, b.y, b.y + b.h - h),
        w,
        h,
    }
}

struct Axis {
    origin: f64,
    len: f64,
    avail: f64,
    want: f64,
}

/// `dir` is +1 when the far edge moves, −1 when the near edge moves, 0 when neither does.
fn axis(dir: i8, origin: f64, len: f64, lo: f64, span: f64, to: f64) -> Axis {
    let hi = lo + span;
    match dir {
        1 => Axis {
            origin,
            len,
            avail: hi - origin,
            want: if to.is_finite() { to - origin } else { len },
        },
        -1 => {
            let anchor = origin + len;
            Axis {
                origin,
                len,
                avail: anchor - lo,
                want: if to.is_finite() { anchor - to } else { len },
            }
        }
        _ => Axis {
            origin,
            len,
            avail: span,
            want: len,
        },
    }
}

fn place(dir: i8, a: &Axis, new_len: f64, lo: f64, span: f64) -> f64 {
    match dir {
        1 => a.origin,
        -1 => a.origin + a.len - new_len,
        _ => clamp(
            a.origin + a.len / 2.0 - new_len / 2.0,
            lo,
            lo + span - new_len,
        ),
    }
}

/// Resizes region `r` by dragging `handle` to `to`, keeping it inside `bounds`.
///
/// Edge and corner handles keep the opposite edge or corner fixed; an edge handle with a locked
/// ratio grows the other dimension about the region's centre, shifted to stay in `bounds`. A
/// corner follows whichever of the pointer's axes asks for the larger region. `Move` places the
/// region's top-left corner at `to`. `ratio` is `None` for Free, a preset's ratio, or the canvas
/// ratio for "Match frame". Sides never shrink below 1 pt unless `bounds` is smaller.
pub fn aspect_locked_resize(
    r: RectPt,
    ratio: Option<Ratio>,
    handle: Handle,
    to: PointF,
    bounds: RectPt,
) -> RectPt {
    if !finite(r) || !finite(bounds) || bounds.w < 0.0 || bounds.h < 0.0 {
        return r;
    }
    let r = clamp_into(r, bounds);
    let b = bounds;
    if handle == Handle::Move {
        let moved = RectPt {
            x: if to.x.is_finite() { to.x } else { r.x },
            y: if to.y.is_finite() { to.y } else { r.y },
            ..r
        };
        return clamp_into(moved, b);
    }
    let (dx, dy): (i8, i8) = match handle {
        Handle::N => (0, -1),
        Handle::S => (0, 1),
        Handle::E => (1, 0),
        Handle::W => (-1, 0),
        Handle::NE => (1, -1),
        Handle::NW => (-1, -1),
        Handle::SE => (1, 1),
        Handle::SW => (-1, 1),
        Handle::Move => (0, 0),
    };
    let ax = axis(dx, r.x, r.w, b.x, b.w, to.x);
    let ay = axis(dy, r.y, r.h, b.y, b.h, to.y);
    let (w, h) = match ratio {
        None => {
            let free = |dir: i8, a: &Axis| {
                if dir == 0 {
                    a.len
                } else {
                    clamp(a.want, MIN_SIDE.min(a.avail), a.avail)
                }
            };
            (free(dx, &ax), free(dy, &ay))
        }
        Some(q) => {
            let k = Ratio::new(q.num, q.den).as_f64();
            let max_w = ax.avail.min(ay.avail * k);
            let min_w = MIN_SIDE.max(MIN_SIDE * k).min(max_w);
            let target = match (dx, dy) {
                (0, _) => ay.want * k,
                (_, 0) => ax.want,
                _ => ax.want.max(ay.want * k),
            };
            let w = clamp(target, min_w, max_w);
            (w, w / k)
        }
    };
    RectPt {
        x: place(dx, &ax, w, b.x, b.w),
        y: place(dy, &ay, h, b.y, b.h),
        w,
        h,
    }
}
