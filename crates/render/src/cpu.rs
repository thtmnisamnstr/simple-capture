//! CPU reference renderer, the golden for GPU tests. Same stages and integer maths as the
//! shaders in `gpu.rs`.

use sc_core::image::RgbaImage;
use sc_geometry::SizePx;
use sc_platform::{Error, Nv12View, Result};

use crate::maths::{self, CameraPlan, ScreenPlan};
use crate::{CameraLayer, FrameJob, OutputFormat, RenderedFrame};

pub fn render_cpu(job: &FrameJob, out: OutputFormat) -> Result<RenderedFrame> {
    let canvas = job.canvas;
    check_job(job, out)?;
    let mut rgba = pack_background(job.background, canvas);
    if let Some(layer) = &job.screen {
        let mut result = Ok(());
        layer.frame.0.with_nv12(&mut |view| {
            result = draw_screen(&mut rgba, canvas, layer, &view);
        })?;
        result?;
    }
    if let Some(layer) = &job.camera {
        let mut result = Ok(());
        layer.frame.0.with_nv12(&mut |view| {
            result = draw_camera(&mut rgba, canvas, layer, &view);
        })?;
        result?;
    }
    for layer in job.text {
        if let Some(plan) = maths::plan_text(canvas, layer) {
            draw_text(&mut rgba, canvas, layer.image, &plan);
        }
    }
    let data = match out {
        OutputFormat::Rgba8 => rgba,
        OutputFormat::I420Bt709Limited => rgba_to_i420(&rgba, canvas),
    };
    Ok(RenderedFrame {
        size: canvas,
        format: out,
        data,
    })
}

pub(crate) fn check_job(job: &FrameJob, out: OutputFormat) -> Result<()> {
    let canvas = job.canvas;
    maths::check_size("canvas", canvas)?;
    if out == OutputFormat::I420Bt709Limited
        && (!canvas.w.is_multiple_of(2) || !canvas.h.is_multiple_of(2))
    {
        return Err(Error::Other(format!(
            "sc_render: I420 output needs an even canvas, got {}x{}",
            canvas.w, canvas.h
        )));
    }
    let bg = job.background;
    let row = canvas.w as usize * 4;
    if bg.size != canvas
        || bg.stride < row
        || bg.data.len() < bg.stride * (canvas.h as usize - 1) + row
    {
        return Err(Error::Other(format!(
            "sc_render: background {}x{} (stride {}, {} bytes) does not cover canvas {}x{}",
            bg.size.w,
            bg.size.h,
            bg.stride,
            bg.data.len(),
            canvas.w,
            canvas.h
        )));
    }
    Ok(())
}

pub(crate) fn check_view(view: &Nv12View<'_>) -> Result<()> {
    let (w, h) = (view.size.w as usize, view.size.h as usize);
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let y_ok = view.y_stride >= w && view.y.len() >= view.y_stride * (h - 1) + w;
    let uv_ok = view.uv_stride >= 2 * cw && view.uv.len() >= view.uv_stride * (ch - 1) + 2 * cw;
    if !(y_ok && uv_ok) {
        return Err(Error::Other(format!(
            "sc_render: NV12 view {w}x{h} has short planes (y {} bytes, stride {}; uv {} bytes, stride {})",
            view.y.len(),
            view.y_stride,
            view.uv.len(),
            view.uv_stride
        )));
    }
    Ok(())
}

fn pack_background(bg: &RgbaImage, canvas: SizePx) -> Vec<u8> {
    let row = canvas.w as usize * 4;
    let mut out = Vec::with_capacity(row * canvas.h as usize);
    for y in 0..canvas.h as usize {
        out.extend_from_slice(&bg.data[y * bg.stride..y * bg.stride + row]);
    }
    out
}

fn draw_screen(
    rgba: &mut [u8],
    canvas: SizePx,
    layer: &crate::ScreenLayer<'_>,
    view: &Nv12View<'_>,
) -> Result<()> {
    maths::check_size("frame", view.size)?;
    check_view(view)?;
    let Some(plan) = maths::plan_screen(canvas, view.size, view.full_range, layer.src, layer.dst)?
    else {
        return Ok(());
    };
    let pre = prefilter(&plan, view);
    let pre = redact(&plan, pre, &maths::redact_rects(plan.p, layer.redact));
    composite(rgba, canvas, &plan, &pre);
    Ok(())
}

fn redact(plan: &ScreenPlan, pre: Vec<[u8; 3]>, rects: &[maths::RedactRect]) -> Vec<[u8; 3]> {
    if rects.is_empty() {
        return pre;
    }
    let pw = plan.p[0];
    let mut out = pre.clone();
    for rr in rects {
        let [x0, y0, x1, y1] = rr.r;
        for y in y0..y1 {
            for x in x0..x1 {
                if let Some(v) = maths::redact_texel(&pre, pw, x, y, rects) {
                    out[(y * pw + x) as usize] = v;
                }
            }
        }
    }
    out
}

/// k×k box average of the converted source, one RGB triple per prefiltered texel.
fn prefilter(plan: &ScreenPlan, view: &Nv12View<'_>) -> Vec<[u8; 3]> {
    let c = maths::coeffs(plan.full_range);
    let [sx, sy, sw, sh] = plan.src;
    let [kx, ky] = plan.k;
    let [pw, ph] = plan.p;
    let mut row_rgb = vec![[0i32; 3]; sw as usize];
    let mut sums = vec![[0i32; 3]; pw as usize];
    let mut out = Vec::with_capacity((pw * ph) as usize);
    for j in 0..ph {
        let y0 = j * ky;
        let y1 = (y0 + ky).min(sh);
        sums.iter_mut().for_each(|s| *s = [0; 3]);
        for yy in y0..y1 {
            let fy = (sy + yy) as usize;
            let yrow = &view.y[fy * view.y_stride..];
            let uvrow = &view.uv[(fy >> 1) * view.uv_stride..];
            for (x, px) in row_rgb.iter_mut().enumerate() {
                let fx = sx as usize + x;
                let ci = (fx >> 1) * 2;
                *px = maths::yuv_to_rgb(
                    &c,
                    i32::from(yrow[fx]),
                    i32::from(uvrow[ci]),
                    i32::from(uvrow[ci + 1]),
                );
            }
            for (i, s) in sums.iter_mut().enumerate() {
                let x0 = i as i32 * kx;
                let x1 = (x0 + kx).min(sw);
                for px in &row_rgb[x0 as usize..x1 as usize] {
                    s[0] += px[0];
                    s[1] += px[1];
                    s[2] += px[2];
                }
            }
        }
        for (i, s) in sums.iter().enumerate() {
            let x0 = i as i32 * kx;
            let n = ((x0 + kx).min(sw) - x0) * (y1 - y0);
            out.push([
                ((s[0] + n / 2) / n) as u8,
                ((s[1] + n / 2) / n) as u8,
                ((s[2] + n / 2) / n) as u8,
            ]);
        }
    }
    out
}

fn composite(rgba: &mut [u8], canvas: SizePx, plan: &ScreenPlan, pre: &[[u8; 3]]) {
    let [_, _, sw, sh] = plan.src;
    let [dx, dy, dw, dh] = plan.dst;
    let [cx0, cy0, cx1, cy1] = plan.clip;
    let [kx, ky] = plan.k;
    let [pw, ph] = plan.p;
    let xs: Vec<(i32, i32, i32)> = (cx0..cx1)
        .map(|x| maths::axis(x - dx, sw, dw, kx, pw))
        .collect();
    for y in cy0..cy1 {
        let (y0, y1, wy) = maths::axis(y - dy, sh, dh, ky, ph);
        let (r0, r1) = ((y0 * pw) as usize, (y1 * pw) as usize);
        let row = (y as usize * canvas.w as usize + cx0 as usize) * 4;
        for (i, &(x0, x1, wx)) in xs.iter().enumerate() {
            let (p00, p10) = (pre[r0 + x0 as usize], pre[r0 + x1 as usize]);
            let (p01, p11) = (pre[r1 + x0 as usize], pre[r1 + x1 as usize]);
            let o = row + i * 4;
            for ch in 0..3 {
                rgba[o + ch] = maths::bilerp(
                    i32::from(p00[ch]),
                    i32::from(p10[ch]),
                    i32::from(p01[ch]),
                    i32::from(p11[ch]),
                    wx,
                    wy,
                ) as u8;
            }
            rgba[o + 3] = 255;
        }
    }
}

fn draw_camera(
    rgba: &mut [u8],
    canvas: SizePx,
    layer: &CameraLayer<'_>,
    view: &Nv12View<'_>,
) -> Result<()> {
    maths::check_size("camera frame", view.size)?;
    check_view(view)?;
    let Some(plan) = maths::plan_camera(canvas, view.size, view.full_range, layer)? else {
        return Ok(());
    };
    let pre = prefilter(&plan.sample, view);
    composite_camera(rgba, canvas, &plan, &pre);
    Ok(())
}

fn composite_camera(rgba: &mut [u8], canvas: SizePx, plan: &CameraPlan, pre: &[[u8; 3]]) {
    let s = &plan.sample;
    let [_, _, sw, sh] = s.src;
    let [dx, dy, dw, dh] = s.dst;
    let [kx, ky] = s.k;
    let [pw, ph] = s.p;
    let [x0, y0, x1, y1] = plan.region;
    let (hw, hh) = (dw as f32 * 0.5, dh as f32 * 0.5);
    let (cx, cy) = (dx as f32 + hw, dy as f32 + hh);
    for y in y0..y1 {
        let py = y as f32 + 0.5 - cy;
        for x in x0..x1 {
            let px = x as f32 + 0.5 - cx;
            let o = (y as usize * canvas.w as usize + x as usize) * 4;
            let mut c = [
                i32::from(rgba[o]),
                i32::from(rgba[o + 1]),
                i32::from(rgba[o + 2]),
                i32::from(rgba[o + 3]),
            ];
            if plan.shadow > 0 {
                let ds = maths::sdf_rrect(px, py - plan.shadow_dy as f32, hw, hh, plan.radius);
                let sa = maths::shadow_alpha(ds, plan.shadow_blur, plan.shadow);
                for v in &mut c[..3] {
                    *v = maths::mix256(0, *v, sa);
                }
                c[3] = maths::mix256(255, c[3], sa);
            }
            let d = maths::sdf_rrect(px, py, hw, hh, plan.radius);
            let cov = maths::coverage(d);
            if cov > 0 {
                let rx = if plan.mirror {
                    dw - 1 - (x - dx)
                } else {
                    x - dx
                };
                let (ax0, ax1, wx) = maths::axis(rx, sw, dw, kx, pw);
                let (ay0, ay1, wy) = maths::axis(y - dy, sh, dh, ky, ph);
                let (r0, r1) = ((ay0 * pw) as usize, (ay1 * pw) as usize);
                let (p00, p10) = (pre[r0 + ax0 as usize], pre[r0 + ax1 as usize]);
                let (p01, p11) = (pre[r1 + ax0 as usize], pre[r1 + ax1 as usize]);
                let inner = plan.border_rgb.map(|_| maths::coverage(d + plan.border_px));
                for ch in 0..3 {
                    let mut v = maths::bilerp(
                        i32::from(p00[ch]),
                        i32::from(p10[ch]),
                        i32::from(p01[ch]),
                        i32::from(p11[ch]),
                        wx,
                        wy,
                    );
                    if let (Some(b), Some(ci)) = (plan.border_rgb, inner) {
                        v = maths::mix256(v, b[ch], ci);
                    }
                    c[ch] = maths::mix256(v, c[ch], cov);
                }
                c[3] = maths::mix256(255, c[3], cov);
            }
            for ch in 0..4 {
                rgba[o + ch] = c[ch] as u8;
            }
        }
    }
}

fn draw_text(rgba: &mut [u8], canvas: SizePx, img: &RgbaImage, plan: &maths::TextPlan) {
    let [w, h] = plan.size;
    let texel = |x: i32, y: i32| -> [i32; 4] {
        if x < 0 || y < 0 || x >= w || y >= h {
            return [0; 4];
        }
        let o = y as usize * img.stride + x as usize * 4;
        [
            i32::from(img.data[o]),
            i32::from(img.data[o + 1]),
            i32::from(img.data[o + 2]),
            i32::from(img.data[o + 3]),
        ]
    };
    let [x0, y0, x1, y1] = plan.region;
    for y in y0..y1 {
        for x in x0..x1 {
            let (tx, ty, wx, wy) = maths::text_sample(&plan.inv, x, y);
            let (p00, p10, p01, p11) = (
                texel(tx, ty),
                texel(tx + 1, ty),
                texel(tx, ty + 1),
                texel(tx + 1, ty + 1),
            );
            let mut s = [0; 4];
            for ch in 0..4 {
                let v = maths::bilerp(p00[ch], p10[ch], p01[ch], p11[ch], wx, wy);
                s[ch] = (v * plan.op256 + 128) >> 8;
            }
            if s[3] == 0 && s[0] == 0 && s[1] == 0 && s[2] == 0 {
                continue;
            }
            let o = (y as usize * canvas.w as usize + x as usize) * 4;
            let d = [
                i32::from(rgba[o]),
                i32::from(rgba[o + 1]),
                i32::from(rgba[o + 2]),
                i32::from(rgba[o + 3]),
            ];
            let c = maths::over(s, d);
            for ch in 0..4 {
                rgba[o + ch] = c[ch].clamp(0, 255) as u8;
            }
        }
    }
}

/// Y plane, then U, then V; chroma from 2×2 block sums.
pub(crate) fn rgba_to_i420(rgba: &[u8], canvas: SizePx) -> Vec<u8> {
    let (w, h) = (canvas.w as usize, canvas.h as usize);
    let (cw, ch) = (w / 2, h / 2);
    let mut out = vec![0u8; w * h + 2 * cw * ch];
    let (yp, rest) = out.split_at_mut(w * h);
    let (up, vp) = rest.split_at_mut(cw * ch);
    for (i, px) in rgba.chunks_exact(4).enumerate() {
        yp[i] = maths::rgb_to_y(i32::from(px[0]), i32::from(px[1]), i32::from(px[2])) as u8;
    }
    for cy in 0..ch {
        for cx in 0..cw {
            let mut s = [0i32; 3];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let o = ((2 * cy + dy) * w + 2 * cx + dx) * 4;
                for c in 0..3 {
                    s[c] += i32::from(rgba[o + c]);
                }
            }
            let (u, v) = maths::rgb4_to_uv(s[0], s[1], s[2]);
            up[cy * cw + cx] = u as u8;
            vp[cy * cw + cx] = v as u8;
        }
    }
    out
}
