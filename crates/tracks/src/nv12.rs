//! CPU pixel conversions for backends whose capture APIs hand over packed RGB: BT.709 limited
//! NV12, the format every writer and the compositor take.

use sc_geometry::SizePx;
use sc_platform::{CpuNv12Frame, NativeFrame, Nv12View, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackedOrder {
    Bgra,
    Rgba,
}

/// `src` rows of `stride` bytes, 4 bytes per pixel; alpha is ignored.
pub fn packed_to_nv12(src: &[u8], stride: usize, size: SizePx, order: PackedOrder) -> CpuNv12Frame {
    let (w, h) = (size.w as usize, size.h as usize);
    let mut out = CpuNv12Frame::black(size, false);
    let uv_stride = out.uv_stride();
    let (ri, bi) = match order {
        PackedOrder::Bgra => (2, 0),
        PackedOrder::Rgba => (0, 2),
    };
    let px = |x: usize, y: usize| -> (i32, i32, i32) {
        let o = y * stride + x * 4;
        (
            i32::from(src[o + ri]),
            i32::from(src[o + 1]),
            i32::from(src[o + bi]),
        )
    };
    for y in 0..h {
        let row = &mut out.y[y * w..(y + 1) * w];
        for (x, v) in row.iter_mut().enumerate() {
            let (r, g, b) = px(x, y);
            *v = (16 + ((47 * r + 157 * g + 16 * b + 128) >> 8)) as u8;
        }
    }
    for cy in 0..h.div_ceil(2) {
        for cx in 0..w.div_ceil(2) {
            let (mut r, mut g, mut b, mut n) = (0, 0, 0, 0);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (x, y) = (cx * 2 + dx, cy * 2 + dy);
                if x < w && y < h {
                    let p = px(x, y);
                    r += p.0;
                    g += p.1;
                    b += p.2;
                    n += 1;
                }
            }
            let (r, g, b) = (r / n, g / n, b / n);
            let o = cy * uv_stride + cx * 2;
            out.uv[o] = (128 + ((-26 * r - 86 * g + 112 * b + 128) >> 8)).clamp(16, 240) as u8;
            out.uv[o + 1] = (128 + ((112 * r - 102 * g - 10 * b + 128) >> 8)).clamp(16, 240) as u8;
        }
    }
    out
}

/// Tightly packed limited-range NV12 at `size` (even), nearest-neighbour resampled when the
/// frame is another size, as rawvideo `nv12` expects it.
pub fn pack_nv12(frame: &dyn NativeFrame, size: SizePx, out: &mut Vec<u8>) -> Result<()> {
    let (w, h) = (size.w as usize, size.h as usize);
    out.clear();
    out.resize(w * h + w * h / 2, 0);
    let (luma, chroma) = out.split_at_mut(w * h);
    frame.with_nv12(&mut |v: Nv12View<'_>| {
        let (sw, sh) = (v.size.w as usize, v.size.h as usize);
        let same = sw == w && sh == h;
        let ys: Vec<usize> = (0..h).map(|y| if same { y } else { y * sh / h }).collect();
        let xs: Vec<usize> = (0..w).map(|x| if same { x } else { x * sw / w }).collect();
        let y_map = |c: u8| -> u8 {
            if v.full_range {
                (16 + (u32::from(c) * 219 + 127) / 255) as u8
            } else {
                c
            }
        };
        let c_map = |c: u8| -> u8 {
            if v.full_range {
                (128 + ((i32::from(c) - 128) * 224 + if c >= 128 { 127 } else { -127 }) / 255) as u8
            } else {
                c
            }
        };
        for (y, sy) in ys.iter().enumerate() {
            let src = &v.y[sy * v.y_stride..];
            let dst = &mut luma[y * w..(y + 1) * w];
            if same && !v.full_range {
                dst.copy_from_slice(&src[..w]);
            } else {
                for (d, sx) in dst.iter_mut().zip(&xs) {
                    *d = y_map(src[*sx]);
                }
            }
        }
        let (cw, ch) = (w / 2, h / 2);
        let (scw, sch) = (sw.div_ceil(2), sh.div_ceil(2));
        for cy in 0..ch {
            let sy = if same {
                cy
            } else {
                (cy * sch / ch).min(sch - 1)
            };
            let src = &v.uv[sy * v.uv_stride..];
            let dst = &mut chroma[cy * w..cy * w + cw * 2];
            for cx in 0..cw {
                let sx = if same {
                    cx
                } else {
                    (cx * scw / cw).min(scw - 1)
                };
                dst[cx * 2] = c_map(src[sx * 2]);
                dst[cx * 2 + 1] = c_map(src[sx * 2 + 1]);
            }
        }
    })
}

/// The largest even size not above `size` (at least 2×2).
pub fn even(size: SizePx) -> SizePx {
    SizePx {
        w: (size.w & !1).max(2),
        h: (size.h & !1).max(2),
    }
}

/// A packed frame back as a [`CpuNv12Frame`] (the reader's output).
pub fn unpack_nv12(bytes: Vec<u8>, size: SizePx) -> CpuNv12Frame {
    let n = size.w as usize * size.h as usize;
    let mut y = bytes;
    let uv = y.split_off(n);
    CpuNv12Frame {
        size,
        y,
        uv,
        full_range: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_black_and_red_land_on_bt709_limited_codes() {
        let size = SizePx { w: 4, h: 2 };
        let mut px = Vec::new();
        for c in [
            [255u8, 255, 255, 255],
            [0, 0, 0, 255],
            [0, 0, 255, 255],
            [0, 0, 255, 255],
        ] {
            px.extend_from_slice(&c);
        }
        let row = px.clone();
        px.extend_from_slice(&row);
        let f = packed_to_nv12(&px, 16, size, PackedOrder::Bgra);
        assert_eq!(f.y[0], 235);
        assert_eq!(f.y[1], 16);
        assert_eq!(f.y[2], 63);
        assert_eq!((f.uv[2], f.uv[3]), (102, 240));
    }

    #[test]
    fn pack_resamples_and_limits_full_range() {
        let src = CpuNv12Frame::filled(SizePx { w: 8, h: 8 }, 255, 255, 0, true);
        let mut out = Vec::new();
        pack_nv12(&src, SizePx { w: 4, h: 4 }, &mut out).expect("pack");
        assert_eq!(out.len(), 24);
        assert!(out[..16].iter().all(|v| *v == 235));
        assert_eq!((out[16], out[17]), (240, 16));
    }
}
