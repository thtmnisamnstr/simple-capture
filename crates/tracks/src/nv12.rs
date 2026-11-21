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

/// [`packed_to_nv12`] over horizontal bands on up to four threads (bands are whole chroma rows).
pub fn packed_to_nv12_par(
    src: &[u8],
    stride: usize,
    size: SizePx,
    order: PackedOrder,
) -> CpuNv12Frame {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get().min(4))
        .unwrap_or(1);
    if threads < 2 || size.h < 64 {
        return packed_to_nv12(src, stride, size, order);
    }
    let rows = (size.h as usize).div_ceil(threads).div_ceil(2) * 2;
    let bands: Vec<(usize, usize)> = (0..size.h as usize)
        .step_by(rows)
        .map(|y0| (y0, (y0 + rows).min(size.h as usize)))
        .collect();
    let parts: Vec<CpuNv12Frame> = std::thread::scope(|s| {
        let handles: Vec<_> = bands
            .iter()
            .map(|(y0, y1)| {
                s.spawn(move || {
                    packed_to_nv12(
                        &src[y0 * stride..],
                        stride,
                        SizePx {
                            w: size.w,
                            h: (y1 - y0) as u32,
                        },
                        order,
                    )
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| CpuNv12Frame::black(size, false))
            })
            .collect()
    });
    let mut out = CpuNv12Frame::black(size, false);
    let (w, uv_stride) = (size.w as usize, out.uv_stride());
    for ((y0, _), part) in bands.iter().zip(parts) {
        out.y[y0 * w..y0 * w + part.y.len()].copy_from_slice(&part.y);
        let c0 = y0 / 2 * uv_stride;
        out.uv[c0..c0 + part.uv.len()].copy_from_slice(&part.uv);
    }
    out
}

/// Nearest-neighbour fit of packed pixels into `out`, aspect kept, centred, black around.
/// Returns the packed BGRA/RGBA buffer (`out.w * 4` stride) and where the content landed.
pub fn fit_packed(
    src: &[u8],
    stride: usize,
    size: SizePx,
    out: SizePx,
) -> (Vec<u8>, sc_geometry::RectPx) {
    let sx = f64::from(out.w) / f64::from(size.w.max(1));
    let sy = f64::from(out.h) / f64::from(size.h.max(1));
    let s = sx.min(sy);
    let dw = ((f64::from(size.w) * s).round() as u32).clamp(1, out.w);
    let dh = ((f64::from(size.h) * s).round() as u32).clamp(1, out.h);
    let dx = (out.w - dw) / 2;
    let dy = (out.h - dh) / 2;
    let mut buf = vec![0u8; out.w as usize * out.h as usize * 4];
    for alpha in buf.chunks_exact_mut(4) {
        alpha[3] = 255;
    }
    let xs: Vec<usize> = (0..dw)
        .map(|x| {
            ((u64::from(x) * u64::from(size.w) / u64::from(dw)) as usize).min(size.w as usize - 1)
        })
        .collect();
    for y in 0..dh {
        let sy =
            ((u64::from(y) * u64::from(size.h) / u64::from(dh)) as usize).min(size.h as usize - 1);
        let srow = &src[sy * stride..];
        let o = ((y + dy) * out.w + dx) as usize * 4;
        let drow = &mut buf[o..o + dw as usize * 4];
        if dw == size.w {
            drow.copy_from_slice(&srow[..dw as usize * 4]);
        } else {
            for (d, x) in drow.chunks_exact_mut(4).zip(&xs) {
                d.copy_from_slice(&srow[x * 4..x * 4 + 4]);
            }
        }
    }
    (
        buf,
        sc_geometry::RectPx {
            x: dx as i32,
            y: dy as i32,
            w: dw,
            h: dh,
        },
    )
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
    fn banded_conversion_matches_the_single_pass() {
        let size = SizePx { w: 96, h: 130 };
        let px: Vec<u8> = (0..size.w * size.h * 4)
            .map(|i| (i * 37 % 251) as u8)
            .collect();
        let a = packed_to_nv12(&px, 96 * 4, size, PackedOrder::Bgra);
        let b = packed_to_nv12_par(&px, 96 * 4, size, PackedOrder::Bgra);
        assert_eq!(a, b);
    }

    #[test]
    fn fit_letterboxes_a_wide_source() {
        let src = vec![200u8; 8 * 2 * 4];
        let (buf, r) = fit_packed(&src, 32, SizePx { w: 8, h: 2 }, SizePx { w: 4, h: 4 });
        assert_eq!(
            r,
            sc_geometry::RectPx {
                x: 0,
                y: 1,
                w: 4,
                h: 1
            }
        );
        assert_eq!(buf[0], 0);
        assert_eq!(buf[4 * 4], 200);
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
