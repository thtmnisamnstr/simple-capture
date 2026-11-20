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
