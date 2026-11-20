#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PixFmt {
    I420,
    Nv12,
}

pub const PATCHES: [(&str, [u8; 3]); 6] = [
    ("red75", [191, 0, 0]),
    ("green75", [0, 191, 0]),
    ("blue75", [0, 0, 191]),
    ("orange", [230, 120, 30]),
    ("gray50", [128, 128, 128]),
    ("skin", [200, 150, 120]),
];

// sRGB-encoded values are fed to the BT.709 matrix as-is (no re-linearization): the output is
// tagged bt709 primaries/transfer, and sRGB and BT.709 share primaries and white point.
pub fn rgb_to_ycbcr709(r: u8, g: u8, b: u8) -> [u8; 3] {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let cb = (b - y) / 1.8556;
    let cr = (r - y) / 1.5748;
    let q = |v: f32| v.round().clamp(0.0, 255.0) as u8;
    [q(16.0 + 219.0 * y), q(128.0 + 224.0 * cb), q(128.0 + 224.0 * cr)]
}

pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

pub struct Synth {
    pub w: usize,
    pub h: usize,
    pub fps: u32,
    bg_y: Vec<u8>,
    bg_u: Vec<u8>,
    bg_v: Vec<u8>,
}

const SEGMENTS: [u8; 10] = [
    0b0111111, 0b0000110, 0b1011011, 0b1001111, 0b1100110, 0b1101101, 0b1111101, 0b0000111,
    0b1111111, 0b1101111,
];

impl Synth {
    pub fn new(w: usize, h: usize, fps: u32) -> Self {
        assert!(w % 2 == 0 && h % 2 == 0, "4:2:0 needs even dimensions");
        let bw = 2 * w;
        let rgb = |x: usize, y: usize| -> [u8; 3] {
            let x = x % w;
            let mut r = (x * 255 / w) as i32;
            let mut g = (y * 255 / h) as i32;
            let mut b = 255 - ((x + y) * 255 / (w + h)) as i32;
            if ((x + y) / 24) % 2 == 0 {
                r -= 40;
                g -= 40;
                b -= 40;
            }
            if x % 40 == 0 || y % 40 == 0 {
                r = 250;
                g = 250;
                b = 250;
            }
            if (x / 4 + y / 4) % 2 == 0 && x % 320 < 64 && y % 200 < 32 {
                r = 20;
                g = 20;
                b = 20;
            }
            [r.clamp(0, 255) as u8, g.clamp(0, 255) as u8, b.clamp(0, 255) as u8]
        };
        let mut bg_y = vec![0u8; bw * h];
        let mut bg_u = vec![0u8; (bw / 2) * (h / 2)];
        let mut bg_v = vec![0u8; (bw / 2) * (h / 2)];
        for y in 0..h {
            for x in 0..bw {
                let [r, g, b] = rgb(x, y);
                bg_y[y * bw + x] = rgb_to_ycbcr709(r, g, b)[0];
            }
        }
        for cy in 0..h / 2 {
            for cx in 0..bw / 2 {
                let mut acc = [0u32; 3];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let p = rgb(cx * 2 + dx, cy * 2 + dy);
                    for c in 0..3 {
                        acc[c] += p[c] as u32;
                    }
                }
                let avg = acc.map(|v| ((v + 2) / 4) as u8);
                let [_, u, v] = rgb_to_ycbcr709(avg[0], avg[1], avg[2]);
                bg_u[cy * (bw / 2) + cx] = u;
                bg_v[cy * (bw / 2) + cx] = v;
            }
        }
        Synth { w, h, fps, bg_y, bg_u, bg_v }
    }

    pub fn frame_bytes(&self) -> usize {
        self.w * self.h * 3 / 2
    }

    fn unit(&self) -> usize {
        (self.h / 540).max(1) * 2
    }

    pub fn patch_rects(&self) -> Vec<(&'static str, [u8; 3], Rect)> {
        let s = self.unit() * 24;
        PATCHES
            .iter()
            .enumerate()
            .map(|(i, (name, rgb))| {
                let x = self.unit() * 8 + i * (s + self.unit() * 4);
                let y = self.h - s - self.unit() * 8;
                (*name, *rgb, Rect { x, y, w: s, h: s })
            })
            .collect()
    }

    pub fn render_i420(&self, n: u64, out: &mut [u8]) {
        let (w, h) = (self.w, self.h);
        let bw = 2 * w;
        let off = ((n as usize * 4) % w) & !1;
        let (yp, rest) = out.split_at_mut(w * h);
        let (up, vp) = rest.split_at_mut(w * h / 4);
        for row in 0..h {
            yp[row * w..row * w + w].copy_from_slice(&self.bg_y[row * bw + off..row * bw + off + w]);
        }
        let (cw, coff) = (w / 2, off / 2);
        for row in 0..h / 2 {
            let s = row * (bw / 2) + coff;
            up[row * cw..row * cw + cw].copy_from_slice(&self.bg_u[s..s + cw]);
            vp[row * cw..row * cw + cw].copy_from_slice(&self.bg_v[s..s + cw]);
        }
        let mut planes = Planes { y: yp, u: up, v: vp, w, h };
        let u = self.unit();
        self.draw_camera(n, &mut planes);

        let bs = u * 40;
        let span_x = w - bs;
        let bx = (n as usize * u * 4) % span_x;
        let span_y = h / 3;
        let t = (n as usize * u * 2) % (2 * span_y);
        let by = h / 3 + if t < span_y { t } else { 2 * span_y - t };
        planes.fill(Rect { x: bx, y: by, w: bs, h: bs }, rgb_to_ycbcr709(240, 200, 40));

        for (_, rgb, r) in self.patch_rects() {
            planes.fill(r, rgb_to_ycbcr709(rgb[0], rgb[1], rgb[2]));
        }

        if n % self.fps as u64 == 0 {
            planes.fill(Rect { x: w - u * 48, y: u * 8, w: u * 40, h: u * 40 }, [235, 128, 128]);
        }

        let digits = 6;
        let (dw, dh, th, gap) = (u * 9, u * 16, u * 2, u * 3);
        let px = u * 8;
        let py = u * 8;
        let panel = Rect { x: px, y: py, w: digits * (dw + gap) + gap, h: dh + 2 * gap };
        planes.fill(panel, [16, 128, 128]);
        let mut v = n;
        for d in (0..digits).rev() {
            let digit = (v % 10) as usize;
            v /= 10;
            let x0 = px + gap + d * (dw + gap);
            let y0 = py + gap;
            let half = dh / 2;
            let segs = [
                Rect { x: x0, y: y0, w: dw, h: th },
                Rect { x: x0 + dw - th, y: y0, w: th, h: half },
                Rect { x: x0 + dw - th, y: y0 + half, w: th, h: half },
                Rect { x: x0, y: y0 + dh - th, w: dw, h: th },
                Rect { x: x0, y: y0 + half, w: th, h: half },
                Rect { x: x0, y: y0, w: th, h: half },
                Rect { x: x0, y: y0 + half - th / 2, w: dw, h: th },
            ];
            for (i, r) in segs.into_iter().enumerate() {
                if SEGMENTS[digit] & (1 << i) != 0 {
                    planes.fill(r, [235, 128, 128]);
                }
            }
        }
    }

    pub fn camera_rect(&self) -> Rect {
        let u = self.unit();
        let (cw, ch) = ((self.w / 4) & !1, (self.h / 4) & !1);
        Rect { x: self.w - cw - u * 8, y: self.h - ch - u * 8, w: cw, h: ch }
    }

    // Stand-in for a camera overlay: smooth value noise whose lattice drifts and re-randomizes
    // continuously, so unlike the scrolling background it cannot be predicted by motion search.
    fn draw_camera(&self, n: u64, p: &mut Planes) {
        let r = self.camera_rect();
        let cell = self.unit() * 8;
        let sec = n / self.fps as u64;
        let f = (n % self.fps as u64) as u32 * 256 / self.fps;
        let drift = n as usize * self.unit();
        let lattice = |seed: u64, i: usize, j: usize, salt: u64| -> u32 {
            let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (i as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9) ^ (j as u64).wrapping_mul(0x94D0_49BB_1331_11EB) ^ salt;
            x ^= x >> 31;
            x = x.wrapping_mul(0xD6E8_FEB8_6659_FD93);
            x ^= x >> 32;
            (x & 0xff) as u32
        };
        let sample = |px: usize, py: usize, cell: usize, salt: u64| -> u32 {
            let (i, j) = (px / cell, py / cell);
            let (fx, fy) = ((px % cell) as u32 * 256 / cell as u32, (py % cell) as u32 * 256 / cell as u32);
            let at = |seed: u64| {
                let a = lattice(seed, i, j, salt) * (256 - fx) + lattice(seed, i + 1, j, salt) * fx;
                let b = lattice(seed, i, j + 1, salt) * (256 - fx) + lattice(seed, i + 1, j + 1, salt) * fx;
                (a * (256 - fy) + b * fy) >> 16
            };
            (at(sec) * (256 - f) + at(sec + 1) * f) >> 8
        };
        for y in 0..r.h {
            let row = &mut p.y[(r.y + y) * p.w + r.x..(r.y + y) * p.w + r.x + r.w];
            for (x, px) in row.iter_mut().enumerate() {
                *px = (40 + sample(x + drift, y, cell, 0) * 180 / 255) as u8;
            }
        }
        let cw = p.w / 2;
        for y in 0..r.h / 2 {
            let o = (r.y / 2 + y) * cw + r.x / 2;
            for x in 0..r.w / 2 {
                p.u[o + x] = (108 + sample(x + drift / 2, y, cell / 2, 1) * 40 / 255) as u8;
                p.v[o + x] = (108 + sample(x + drift / 2, y, cell / 2, 2) * 40 / 255) as u8;
            }
        }
    }

    pub fn render(&self, n: u64, fmt: PixFmt, out: &mut [u8], scratch: &mut Vec<u8>) {
        match fmt {
            PixFmt::I420 => self.render_i420(n, out),
            PixFmt::Nv12 => {
                scratch.resize(self.frame_bytes(), 0);
                self.render_i420(n, scratch);
                let ys = self.w * self.h;
                let cs = ys / 4;
                out[..ys].copy_from_slice(&scratch[..ys]);
                let (u, v) = scratch[ys..].split_at(cs);
                for (i, pair) in out[ys..].chunks_exact_mut(2).enumerate() {
                    pair[0] = u[i];
                    pair[1] = v[i];
                }
            }
        }
    }

    pub fn y4m_header(&self) -> String {
        format!(
            "YUV4MPEG2 W{} H{} F{}:1 Ip A1:1 C420mpeg2 XYSCSS=420MPEG2 XCOLORRANGE=LIMITED\n",
            self.w, self.h, self.fps
        )
    }
}

struct Planes<'a> {
    y: &'a mut [u8],
    u: &'a mut [u8],
    v: &'a mut [u8],
    w: usize,
    h: usize,
}

impl Planes<'_> {
    fn fill(&mut self, r: Rect, c: [u8; 3]) {
        let x0 = r.x & !1;
        let y0 = r.y & !1;
        let x1 = (r.x + r.w).min(self.w) & !1;
        let y1 = (r.y + r.h).min(self.h) & !1;
        for row in y0..y1 {
            self.y[row * self.w + x0..row * self.w + x1].fill(c[0]);
        }
        let cw = self.w / 2;
        for row in y0 / 2..y1 / 2 {
            self.u[row * cw + x0 / 2..row * cw + x1 / 2].fill(c[1]);
            self.v[row * cw + x0 / 2..row * cw + x1 / 2].fill(c[2]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bt709_reference_values() {
        assert_eq!(rgb_to_ycbcr709(0, 0, 0), [16, 128, 128]);
        assert_eq!(rgb_to_ycbcr709(255, 255, 255), [235, 128, 128]);
        assert_eq!(rgb_to_ycbcr709(255, 0, 0), [63, 102, 240]);
        assert_eq!(rgb_to_ycbcr709(0, 255, 0), [173, 42, 26]);
        assert_eq!(rgb_to_ycbcr709(0, 0, 255), [32, 240, 118]);
    }

    #[test]
    fn nv12_matches_i420() {
        let s = Synth::new(640, 360, 30);
        let mut a = vec![0; s.frame_bytes()];
        let mut b = vec![0; s.frame_bytes()];
        let mut scratch = Vec::new();
        s.render(7, PixFmt::I420, &mut a, &mut scratch);
        s.render(7, PixFmt::Nv12, &mut b, &mut scratch);
        let ys = 640 * 360;
        assert_eq!(a[..ys], b[..ys]);
        assert_eq!(a[ys], b[ys]);
        assert_eq!(a[ys + ys / 4], b[ys + 1]);
    }
}
