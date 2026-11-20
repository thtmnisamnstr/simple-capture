#[derive(Clone, Copy, Debug)]
pub struct Affine {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Affine {
    pub fn identity() -> Self {
        Self { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 }
    }
    pub fn translate(x: f32, y: f32) -> Self {
        Self { tx: x, ty: y, ..Self::identity() }
    }
    pub fn scale(s: f32) -> Self {
        Self { a: s, d: s, ..Self::identity() }
    }
    pub fn rotate_deg(deg: f32) -> Self {
        let (s, c) = deg.to_radians().sin_cos();
        Self { a: c, b: s, c: -s, d: c, tx: 0.0, ty: 0.0 }
    }
    pub fn then(self, o: Affine) -> Affine {
        Affine {
            a: o.a * self.a + o.c * self.b,
            b: o.b * self.a + o.d * self.b,
            c: o.a * self.c + o.c * self.d,
            d: o.b * self.c + o.d * self.d,
            tx: o.a * self.tx + o.c * self.ty + o.tx,
            ty: o.b * self.tx + o.d * self.ty + o.ty,
        }
    }
    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (self.a * x + self.c * y + self.tx, self.b * x + self.d * y + self.ty)
    }
    pub fn invert(&self) -> Affine {
        let det = self.a * self.d - self.b * self.c;
        let ia = self.d / det;
        let ib = -self.b / det;
        let ic = -self.c / det;
        let id = self.a / det;
        Affine { a: ia, b: ib, c: ic, d: id, tx: -(ia * self.tx + ic * self.ty), ty: -(ib * self.tx + id * self.ty) }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Draw {
    pub tex: usize,
    pub src: [f32; 4],
    pub xf: Affine,
    pub opacity: f32,
}

pub struct Image {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

fn texel(img: &Image, x: i32, y: i32) -> [f32; 4] {
    let x = x.clamp(0, img.w as i32 - 1) as usize;
    let y = y.clamp(0, img.h as i32 - 1) as usize;
    let i = (y * img.w as usize + x) * 4;
    let p = &img.px[i..i + 4];
    [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0, p[3] as f32 / 255.0]
}

fn bilinear(img: &Image, u: f32, v: f32) -> [f32; 4] {
    let x = u - 0.5;
    let y = v - 0.5;
    let x0 = x.floor();
    let y0 = y.floor();
    let fx = x - x0;
    let fy = y - y0;
    let (x0, y0) = (x0 as i32, y0 as i32);
    let t00 = texel(img, x0, y0);
    let t10 = texel(img, x0 + 1, y0);
    let t01 = texel(img, x0, y0 + 1);
    let t11 = texel(img, x0 + 1, y0 + 1);
    let mut o = [0.0; 4];
    for k in 0..4 {
        let top = t00[k] + (t10[k] - t00[k]) * fx;
        let bot = t01[k] + (t11[k] - t01[k]) * fx;
        o[k] = top + (bot - top) * fy;
    }
    o
}

pub fn quad(d: &Draw) -> [(f32, f32); 4] {
    let [l, t, r, b] = d.src;
    [d.xf.apply(l, t), d.xf.apply(r, t), d.xf.apply(r, b), d.xf.apply(l, b)]
}

pub fn cpu_composite(bg: &Image, textures: &[Image], draws: &[Draw]) -> Image {
    let mut out = bg.px.clone();
    let (w, h) = (bg.w as i32, bg.h as i32);
    for d in draws {
        let tex = &textures[d.tex];
        let q = quad(d);
        let minx = q.iter().map(|p| p.0).fold(f32::MAX, f32::min).floor().max(0.0) as i32;
        let maxx = q.iter().map(|p| p.0).fold(f32::MIN, f32::max).ceil().min(w as f32) as i32;
        let miny = q.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor().max(0.0) as i32;
        let maxy = q.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil().min(h as f32) as i32;
        let inv = d.xf.invert();
        let [l, t, r, b] = d.src;
        for y in miny..maxy {
            for x in minx..maxx {
                let (u, v) = inv.apply(x as f32 + 0.5, y as f32 + 0.5);
                if u < l || u >= r || v < t || v >= b {
                    continue;
                }
                let s = bilinear(tex, u, v);
                let i = ((y * w + x) * 4) as usize;
                let sa = s[3] * d.opacity;
                for k in 0..4 {
                    let dst = out[i + k] as f32 / 255.0;
                    let o = s[k] * d.opacity + dst * (1.0 - sa);
                    out[i + k] = (o.clamp(0.0, 1.0) * 255.0).round() as u8;
                }
            }
        }
    }
    Image { w: bg.w, h: bg.h, px: out }
}

pub struct DiffStats {
    pub max: u8,
    pub mean: f64,
    pub over1: usize,
    pub over2: usize,
    pub over4: usize,
    pub channels: usize,
}

pub fn diff(a: &Image, b: &Image) -> (DiffStats, Image) {
    let mut max = 0u8;
    let mut sum = 0u64;
    let (mut o1, mut o2, mut o4) = (0, 0, 0);
    let mut vis = vec![0u8; a.px.len()];
    for (i, (x, y)) in a.px.iter().zip(&b.px).enumerate() {
        let d = x.abs_diff(*y);
        if i % 4 == 3 {
            vis[i] = 255;
            continue;
        }
        max = max.max(d);
        sum += d as u64;
        if d > 1 {
            o1 += 1;
        }
        if d > 2 {
            o2 += 1;
        }
        if d > 4 {
            o4 += 1;
        }
        vis[i] = d.saturating_mul(32);
    }
    let channels = a.px.len() / 4 * 3;
    (
        DiffStats { max, mean: sum as f64 / channels as f64, over1: o1, over2: o2, over4: o4, channels },
        Image { w: a.w, h: a.h, px: vis },
    )
}
