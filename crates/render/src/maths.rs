//! Fixed-point integer maths shared by the WGSL shaders and the CPU reference: WGSL `i32` and
//! Rust `i32` evaluate it identically, so both renderers agree to the bit. `gpu.rs` mirrors
//! these functions line for line.

use sc_geometry::{RectPx, SizePx};
use sc_platform::{Error, Result};

/// Keeps every intermediate below `i32::MAX`.
pub(crate) const MAX_SIDE: u32 = 16_384;

/// Bounds the per-pixel box loop; larger ratios alias slightly in the bilinear step instead.
pub(crate) const MAX_BOX: u32 = 64;

/// YUV→RGB coefficients in 1/65536 units.
#[derive(Debug, Clone, Copy)]
pub(crate) struct YuvCoeffs {
    pub y_mul: i32,
    pub y_off: i32,
    pub rv: i32,
    pub gu: i32,
    pub gv: i32,
    pub bu: i32,
}

pub(crate) const BT709_LIMITED: YuvCoeffs = YuvCoeffs {
    y_mul: 76_309,
    y_off: 16,
    rv: 117_489,
    gu: 13_975,
    gv: 34_925,
    bu: 138_438,
};

pub(crate) const BT709_FULL: YuvCoeffs = YuvCoeffs {
    y_mul: 65_536,
    y_off: 0,
    rv: 103_206,
    gu: 12_276,
    gv: 30_679,
    bu: 121_609,
};

pub(crate) fn coeffs(full_range: bool) -> YuvCoeffs {
    if full_range {
        BT709_FULL
    } else {
        BT709_LIMITED
    }
}

#[inline]
pub(crate) fn yuv_to_rgb(c: &YuvCoeffs, y: i32, u: i32, v: i32) -> [i32; 3] {
    let l = c.y_mul * (y - c.y_off) + 32_768;
    let (du, dv) = (u - 128, v - 128);
    [
        ((l + c.rv * dv) >> 16).clamp(0, 255),
        ((l - c.gu * du - c.gv * dv) >> 16).clamp(0, 255),
        ((l + c.bu * du) >> 16).clamp(0, 255),
    ]
}

/// BT.709 limited-range luma.
#[inline]
pub(crate) fn rgb_to_y(r: i32, g: i32, b: i32) -> i32 {
    ((11_966 * r + 40_254 * g + 4_064 * b + 1_081_344) >> 16).clamp(0, 255)
}

/// BT.709 limited-range chroma from the channel sums of a 2×2 block (centre-sited 4:2:0).
#[inline]
pub(crate) fn rgb4_to_uv(sr: i32, sg: i32, sb: i32) -> (i32, i32) {
    let u = (-6_596 * sr - 22_189 * sg + 28_785 * sb + 33_685_504) >> 18;
    let v = (28_785 * sr - 26_145 * sg - 2_640 * sb + 33_685_504) >> 18;
    (u.clamp(0, 255), v.clamp(0, 255))
}

/// Prefiltered texel pair and 8-bit weight for output pixel `rel` (relative to the dst origin)
/// along one axis: `s` src length, `d` dst length, `k` box factor, `p` prefiltered length.
/// Pixel centres map to `(rel + ½)·s/(d·k) − ½` prefiltered texels.
#[inline]
pub(crate) fn axis(rel: i32, s: i32, d: i32, k: i32, p: i32) -> (i32, i32, i32) {
    let den = 2 * d * k;
    let n = (2 * rel + 1) * s - d * k;
    let i0 = n.div_euclid(den);
    let rem = n - i0 * den;
    let w = (rem * 256 + d * k) / den;
    (i0.clamp(0, p - 1), (i0 + 1).clamp(0, p - 1), w)
}

#[inline]
pub(crate) fn bilerp(p00: i32, p10: i32, p01: i32, p11: i32, wx: i32, wy: i32) -> i32 {
    let top = p00 * (256 - wx) + p10 * wx;
    let bot = p01 * (256 - wx) + p11 * wx;
    (top * (256 - wy) + bot * wy + 32_768) >> 16
}

/// The screen layer, validated and clipped, as both renderers consume it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScreenPlan {
    /// x, y, w, h inside the frame.
    pub src: [i32; 4],
    /// x, y, w, h on the canvas, unclipped.
    pub dst: [i32; 4],
    /// `dst ∩ canvas` as x0, y0, x1, y1.
    pub clip: [i32; 4],
    pub k: [i32; 2],
    pub p: [i32; 2],
    pub frame: SizePx,
    pub full_range: bool,
}

pub(crate) fn check_size(what: &str, s: SizePx) -> Result<()> {
    if s.w == 0 || s.h == 0 || s.w > MAX_SIDE || s.h > MAX_SIDE {
        return Err(Error::Other(format!(
            "sc_render: {what} {}x{} is outside 1..={MAX_SIDE}",
            s.w, s.h
        )));
    }
    Ok(())
}

/// `None` when nothing of the layer is visible.
pub(crate) fn plan_screen(
    canvas: SizePx,
    frame: SizePx,
    full_range: bool,
    src: RectPx,
    dst: RectPx,
) -> Result<Option<ScreenPlan>> {
    check_size("frame", frame)?;
    let sx0 = src.x.max(0);
    let sy0 = src.y.max(0);
    let sx1 = (i64::from(src.x) + i64::from(src.w)).min(i64::from(frame.w)) as i32;
    let sy1 = (i64::from(src.y) + i64::from(src.h)).min(i64::from(frame.h)) as i32;
    if sx1 <= sx0 || sy1 <= sy0 || dst.w == 0 || dst.h == 0 {
        return Ok(None);
    }
    if dst.w > MAX_SIDE
        || dst.h > MAX_SIDE
        || dst.x.unsigned_abs() > MAX_SIDE
        || dst.y.unsigned_abs() > MAX_SIDE
    {
        return Err(Error::Other(format!(
            "sc_render: dst {dst:?} is outside the ±{MAX_SIDE} range"
        )));
    }
    let (sw, sh) = (sx1 - sx0, sy1 - sy0);
    let (dw, dh) = (dst.w as i32, dst.h as i32);
    let clip = [
        dst.x.max(0),
        dst.y.max(0),
        (dst.x + dw).min(canvas.w as i32),
        (dst.y + dh).min(canvas.h as i32),
    ];
    if clip[2] <= clip[0] || clip[3] <= clip[1] {
        return Ok(None);
    }
    let kx = (sw / dw).clamp(1, MAX_BOX as i32);
    let ky = (sh / dh).clamp(1, MAX_BOX as i32);
    Ok(Some(ScreenPlan {
        src: [sx0, sy0, sw, sh],
        dst: [dst.x, dst.y, dw, dh],
        clip,
        k: [kx, ky],
        p: [(sw + kx - 1) / kx, (sh + ky - 1) / ky],
        frame,
        full_range,
    }))
}

/// The camera layer as both renderers consume it: the screen-style sampling plan for the shape
/// rectangle, plus the region actually drawn (the shape, grown by the shadow) and the mask.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CameraPlan {
    pub sample: ScreenPlan,
    /// x0, y0, x1, y1 on the canvas.
    pub region: [i32; 4],
    pub radius: f32,
    pub mirror: bool,
    pub border_px: f32,
    pub border_rgb: Option<[i32; 3]>,
    pub shadow: i32,
    pub shadow_dy: i32,
    pub shadow_blur: f32,
}

pub(crate) fn plan_camera(
    canvas: SizePx,
    frame: SizePx,
    full_range: bool,
    layer: &crate::CameraLayer<'_>,
) -> Result<Option<CameraPlan>> {
    let Some(sample) = plan_screen(canvas, frame, full_range, layer.src, layer.dst)? else {
        return Ok(None);
    };
    let [dx, dy, dw, dh] = sample.dst;
    let short = dw.min(dh) as f32;
    let shadow = i32::from(layer.shadow);
    let (shadow_dy, shadow_blur) = if shadow > 0 {
        ((short * 0.02).round() as i32, (short * 0.06).max(1.0))
    } else {
        (0, 1.0)
    };
    let grow = if shadow > 0 {
        shadow_blur.ceil() as i32 + shadow_dy + 2
    } else {
        0
    };
    let region = [
        (dx - grow).max(0),
        (dy - grow).max(0),
        (dx + dw + grow).min(canvas.w as i32),
        (dy + dh + grow).min(canvas.h as i32),
    ];
    if region[2] <= region[0] || region[3] <= region[1] {
        return Ok(None);
    }
    Ok(Some(CameraPlan {
        sample,
        region,
        radius: (layer.radius_px as f32).min(short / 2.0),
        mirror: layer.mirror,
        border_px: layer.border.map_or(0.0, |b| b.0 as f32),
        border_rgb: layer
            .border
            .map(|b| [i32::from(b.1[0]), i32::from(b.1[1]), i32::from(b.1[2])]),
        shadow,
        shadow_dy,
        shadow_blur,
    }))
}

/// Signed distance from a point to a rounded rectangle centred on the origin with half extents
/// `hw`, `hh` and corner radius `r`; negative inside. Written as the WGSL in `gpu.rs` writes it.
#[inline]
pub(crate) fn sdf_rrect(px: f32, py: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = px.abs() - hw + r;
    let qy = py.abs() - hh + r;
    let mx = qx.max(0.0);
    let my = qy.max(0.0);
    (mx * mx + my * my).sqrt() + qx.max(qy).min(0.0) - r
}

/// Pixel coverage in 1/256 for a signed distance at the pixel centre.
#[inline]
pub(crate) fn coverage(d: f32) -> i32 {
    ((0.5 - d).clamp(0.0, 1.0) * 256.0 + 0.5).floor() as i32
}

/// Soft shadow opacity in 1/256 for a signed distance `ds` to the offset shape.
#[inline]
pub(crate) fn shadow_alpha(ds: f32, blur: f32, strength: i32) -> i32 {
    let t = ((blur - ds) / (2.0 * blur)).clamp(0.0, 1.0);
    let s = t * t * (3.0 - 2.0 * t);
    (s * strength as f32 * 0.45 / 255.0 * 256.0 + 0.5).floor() as i32
}

/// `a` over `b` with `w`/256 of `a`.
#[inline]
pub(crate) fn mix256(a: i32, b: i32, w: i32) -> i32 {
    (a * w + b * (256 - w) + 128) >> 8
}

/// A text layer as both renderers draw it: the canvas-to-image map, the image size, the opacity
/// in 256ths and the canvas pixels it can touch (`x0, y0, x1, y1`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TextPlan {
    pub inv: [f32; 6],
    pub size: [i32; 2],
    pub op256: i32,
    pub region: [i32; 4],
}

pub(crate) fn plan_text(canvas: SizePx, layer: &crate::TextLayer<'_>) -> Option<TextPlan> {
    let [a, b, c, d, e, f] = layer.to_canvas.map(f64::from);
    let det = a * d - b * c;
    let (w, h) = (layer.image.size.w, layer.image.size.h);
    if det.abs() < 1e-9 || w == 0 || h == 0 || layer.opacity <= 0.0 {
        return None;
    }
    let inv = [
        d / det,
        -b / det,
        -c / det,
        a / det,
        (c * f - d * e) / det,
        (b * e - a * f) / det,
    ];
    let corners = [
        (0.0, 0.0),
        (f64::from(w), 0.0),
        (0.0, f64::from(h)),
        (f64::from(w), f64::from(h)),
    ]
    .map(|(x, y)| (a * x + c * y + e, b * x + d * y + f));
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (x, y) in corners {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    let clamp_x = |v: f64| (v as i64).clamp(0, i64::from(canvas.w)) as i32;
    let clamp_y = |v: f64| (v as i64).clamp(0, i64::from(canvas.h)) as i32;
    let region = [
        clamp_x(x0.floor() - 1.0),
        clamp_y(y0.floor() - 1.0),
        clamp_x(x1.ceil() + 1.0),
        clamp_y(y1.ceil() + 1.0),
    ];
    if region[0] >= region[2] || region[1] >= region[3] {
        return None;
    }
    Some(TextPlan {
        inv: inv.map(|v| v as f32),
        size: [w as i32, h as i32],
        op256: (f64::from(layer.opacity.clamp(0.0, 1.0)) * 256.0).round() as i32,
        region,
    })
}

/// Where canvas pixel `(x, y)` samples the image: the top-left texel and the 8-bit weights.
#[inline]
pub(crate) fn text_sample(inv: &[f32; 6], x: i32, y: i32) -> (i32, i32, i32, i32) {
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
    let u = inv[0] * px + inv[2] * py + inv[4];
    let v = inv[1] * px + inv[3] * py + inv[5];
    let fx = (u * 256.0 + 0.5).floor() as i32 - 128;
    let fy = (v * 256.0 + 0.5).floor() as i32 - 128;
    (fx >> 8, fy >> 8, fx & 255, fy & 255)
}

/// Premultiplied `s` (after opacity) over `d`.
#[inline]
pub(crate) fn over(s: [i32; 4], d: [i32; 4]) -> [i32; 4] {
    let ia = 255 - s[3];
    [
        s[0] + (d[0] * ia + 127) / 255,
        s[1] + (d[1] * ia + 127) / 255,
        s[2] + (d[2] * ia + 127) / 255,
        s[3] + (d[3] * ia + 127) / 255,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limited_extremes_map_to_full_rgb() {
        let c = BT709_LIMITED;
        assert_eq!(yuv_to_rgb(&c, 16, 128, 128), [0, 0, 0]);
        assert_eq!(yuv_to_rgb(&c, 235, 128, 128), [255, 255, 255]);
        assert_eq!(rgb_to_y(0, 0, 0), 16);
        assert_eq!(rgb_to_y(255, 255, 255), 235);
        assert_eq!(rgb4_to_uv(1020, 1020, 1020), (128, 128));
        assert_eq!(rgb4_to_uv(0, 0, 0), (128, 128));
    }

    #[test]
    fn full_range_white_and_black() {
        let c = BT709_FULL;
        assert_eq!(yuv_to_rgb(&c, 0, 128, 128), [0, 0, 0]);
        assert_eq!(yuv_to_rgb(&c, 255, 128, 128), [255, 255, 255]);
    }

    #[test]
    fn axis_identity_hits_texel_centres() {
        for rel in 0..10 {
            let (i0, _, w) = axis(rel, 10, 10, 1, 10);
            assert_eq!((i0, w), (rel, 0));
        }
        for rel in 0..5 {
            let (i0, _, w) = axis(rel, 20, 5, 4, 5);
            assert_eq!((i0, w), (rel, 0));
        }
    }

    #[test]
    fn axis_clamps_at_edges_when_upscaling() {
        let (i0, i1, _) = axis(0, 2, 8, 1, 2);
        assert_eq!((i0, i1), (0, 0));
        let (i0, i1, _) = axis(7, 2, 8, 1, 2);
        assert_eq!((i0, i1), (1, 1));
    }

    #[test]
    fn plan_picks_box_factor_and_clips() {
        let p = plan_screen(
            SizePx { w: 1080, h: 1920 },
            SizePx { w: 7680, h: 2160 },
            false,
            RectPx {
                x: 0,
                y: 0,
                w: 7680,
                h: 2160,
            },
            RectPx {
                x: 0,
                y: 808,
                w: 1080,
                h: 304,
            },
        )
        .expect("plan")
        .expect("visible");
        assert_eq!(p.k, [7, 7]);
        assert_eq!(p.p, [1098, 309]);
        assert_eq!(p.clip, [0, 808, 1080, 1112]);
    }
}

/// A redaction on the prefiltered screen texels: x0, y0, x1, y1 and the mosaic cell side in
/// texels (0: black).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RedactRect {
    pub r: [i32; 4],
    pub cell: i32,
}

/// Mosaic cells are about a twelfth of the box's long side, and never smaller than a sixtieth
/// of the picture's short side, so text under a small box is not readable.
pub(crate) fn redact_rects(p: [i32; 2], boxes: &[crate::Redact]) -> Vec<RedactRect> {
    let [pw, ph] = p;
    boxes
        .iter()
        .filter_map(|b| {
            let [x, y, w, h] = b.rect;
            let at = |v: f64, n: i32, up: bool| {
                let t = v * f64::from(n);
                let t = if up { t.ceil() } else { t.floor() };
                (t.clamp(0.0, f64::from(n))) as i32
            };
            let (x0, y0) = (at(x, pw, false), at(y, ph, false));
            let (x1, y1) = (at(x + w, pw, true), at(y + h, ph, true));
            (x1 > x0 && y1 > y0).then(|| RedactRect {
                r: [x0, y0, x1, y1],
                cell: if b.black {
                    0
                } else {
                    ((x1 - x0).max(y1 - y0) / 12).max(pw.min(ph) / 60).max(4)
                },
            })
        })
        .take(crate::MAX_REDACTIONS)
        .collect()
}

/// The texel at `(x, y)` after the redactions (the last covering one wins), read from the
/// unredacted texels `pre` (`pw` wide); `None` outside every box. A mosaic cell is the mean of a
/// 4×4 grid of taps spread over it.
pub(crate) fn redact_texel(
    pre: &[[u8; 3]],
    pw: i32,
    x: i32,
    y: i32,
    rects: &[RedactRect],
) -> Option<[u8; 3]> {
    let mut out = None;
    for rr in rects {
        let [x0, y0, x1, y1] = rr.r;
        if x < x0 || x >= x1 || y < y0 || y >= y1 {
            continue;
        }
        let c = rr.cell;
        if c == 0 {
            out = Some([0, 0, 0]);
            continue;
        }
        let cx0 = x0 + (x - x0) / c * c;
        let cy0 = y0 + (y - y0) / c * c;
        let (cw, ch) = ((cx0 + c).min(x1) - cx0, (cy0 + c).min(y1) - cy0);
        let mut s = [0i32; 3];
        for j in 0..4 {
            let ty = cy0 + ((2 * j + 1) * ch) / 8;
            for i in 0..4 {
                let tx = cx0 + ((2 * i + 1) * cw) / 8;
                let px = pre[(ty * pw + tx) as usize];
                for k in 0..3 {
                    s[k] += i32::from(px[k]);
                }
            }
        }
        out = Some(s.map(|v| ((v + 8) / 16) as u8));
    }
    out
}
