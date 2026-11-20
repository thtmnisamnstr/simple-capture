use sc_geometry::{
    Aspect, PointF, Ratio, RectPx, SizeClass, SizePx, contain_fit, cover_src_rect, even_round,
    natural_canvas, preset_size, reference_canvas_size, small_content,
};

struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn range(&mut self, lo: u32, hi: u32) -> u32 {
        lo + (self.next() % u64::from(hi - lo + 1)) as u32
    }
}

fn sz(w: u32, h: u32) -> SizePx {
    SizePx { w, h }
}

fn rect(x: i32, y: i32, w: u32, h: u32) -> RectPx {
    RectPx { x, y, w, h }
}

#[test]
fn even_round_rule() {
    assert_eq!(even_round(853.33), 854);
    assert_eq!(even_round(426.67), 426);
    assert_eq!(even_round(3491.56), 3492);
    assert_eq!(even_round(3.0), 4);
    assert_eq!(even_round(0.4), 2);
    assert_eq!(even_round(-5.0), 2);
    assert_eq!(even_round(f64::NAN), 2);
    assert_eq!(even_round(1e30) % 2, 0);
}

#[test]
fn contain_fit_worked_examples() {
    let cases = [
        (sz(1920, 1080), sz(7680, 2160), rect(0, 270, 1920, 540)),
        (sz(1080, 1920), sz(7680, 2160), rect(0, 808, 1080, 304)),
        (sz(1080, 1920), sz(1234, 567), rect(0, 712, 1080, 496)),
        (sz(1080, 1920), sz(3024, 1964), rect(0, 609, 1080, 702)),
        (sz(1080, 1080), sz(3024, 1964), rect(0, 189, 1080, 702)),
        (sz(1440, 1080), sz(3840, 2160), rect(0, 135, 1440, 810)),
        (sz(1920, 1080), sz(1080, 1920), rect(656, 0, 608, 1080)),
        (sz(1920, 1080), sz(1920, 1080), rect(0, 0, 1920, 1080)),
    ];
    for (canvas, content, want) in cases {
        assert_eq!(
            contain_fit(canvas, content),
            want,
            "{content:?} into {canvas:?}"
        );
    }
}

#[test]
fn contain_fit_degenerate_is_empty() {
    assert_eq!(contain_fit(sz(0, 1080), sz(10, 10)), RectPx::default());
    assert_eq!(contain_fit(sz(1920, 1080), sz(0, 10)), RectPx::default());
}

fn round_is_ambiguous(v: f64) -> bool {
    ((v - v.floor()) - 0.5).abs() < 1e-6
}

#[test]
fn contain_fit_properties_100k() {
    let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
    for _ in 0..100_000 {
        let canvas = sz(rng.range(1, 8192), rng.range(1, 8192));
        let content = sz(rng.range(1, 16384), rng.range(1, 16384));
        let d = contain_fit(canvas, content);
        let (wc, hc) = (f64::from(canvas.w), f64::from(canvas.h));
        let (w, h) = (f64::from(content.w), f64::from(content.h));
        let s = (wc / w).min(hc / h);
        let ctx = format!("{content:?} into {canvas:?} -> {d:?}");

        assert!(d.x >= 0 && d.y >= 0, "{ctx}");
        assert!(d.x as u64 + u64::from(d.w) <= u64::from(canvas.w), "{ctx}");
        assert!(d.y as u64 + u64::from(d.h) <= u64::from(canvas.h), "{ctx}");
        assert!(
            d.w == canvas.w || d.h == canvas.h,
            "limiting side spans: {ctx}"
        );

        assert!((f64::from(d.w) - w * s).abs() <= 1.0, "aspect w: {ctx}");
        assert!((f64::from(d.h) - h * s).abs() <= 1.0, "aspect h: {ctx}");

        let cx = f64::from(d.x) + f64::from(d.w) / 2.0;
        let cy = f64::from(d.y) + f64::from(d.h) / 2.0;
        assert!((cx - wc / 2.0).abs() <= 0.5, "centred x: {ctx}");
        assert!((cy - hc / 2.0).abs() <= 0.5, "centred y: {ctx}");

        let edges = [
            ((wc - w * s) / 2.0, f64::from(d.x)),
            ((wc + w * s) / 2.0, f64::from(d.x) + f64::from(d.w)),
            ((hc - h * s) / 2.0, f64::from(d.y)),
            ((hc + h * s) / 2.0, f64::from(d.y) + f64::from(d.h)),
        ];
        for (exact, got) in edges {
            if !round_is_ambiguous(exact) {
                assert_eq!(exact.round(), got, "edge snapped: {ctx}");
            }
        }
    }
}

#[test]
fn small_content_rule() {
    assert!(small_content(sz(1080, 1920), sz(7680, 2160)));
    assert!(!small_content(sz(1920, 1080), sz(7680, 2160)));
    assert!(!small_content(sz(1080, 1920), sz(3024, 1964)));
    assert!(small_content(sz(1920, 1080), sz(100, 4000)));
}

#[test]
fn natural_canvas_rule() {
    let n = natural_canvas(Ratio::new(16, 9), sz(7680, 2160));
    assert_eq!(n, (7680.0, 4320.0));
    let n = natural_canvas(Ratio::new(16, 9), sz(3024, 1964));
    assert!((n.0 - 1964.0 * 16.0 / 9.0).abs() < 1e-9 && n.1 == 1964.0);
    let n = natural_canvas(Ratio::new(9, 16), sz(7680, 2160));
    assert!(n.0 == 7680.0 && (n.1 - 7680.0 * 16.0 / 9.0).abs() < 1e-9);
}

#[test]
fn reference_size_examples() {
    let any = Ratio::new(1, 1);
    let wide = [sz(7680, 2160)];
    assert_eq!(
        reference_canvas_size(Aspect::R16x9, any, &wide),
        sz(3840, 2160)
    );
    assert_eq!(
        reference_canvas_size(Aspect::R9x16, any, &wide),
        sz(2160, 3840)
    );
    assert_eq!(
        reference_canvas_size(Aspect::R4x3, any, &wide),
        sz(2880, 2160)
    );
    assert_eq!(
        reference_canvas_size(Aspect::R1x1, any, &wide),
        sz(2160, 2160)
    );
    assert_eq!(
        reference_canvas_size(Aspect::Original, Ratio::new(32, 9), &wide),
        sz(7680, 2160)
    );
    assert_eq!(
        reference_canvas_size(Aspect::R16x9, any, &[sz(3024, 1964)]),
        sz(3492, 1964)
    );
    assert_eq!(
        reference_canvas_size(Aspect::R16x9, any, &[sz(1920, 1080), sz(3024, 1964)]),
        sz(3492, 1964)
    );
    assert_eq!(
        reference_canvas_size(Aspect::Original, Ratio::new(605, 393), &[sz(3025, 1965)]),
        sz(3024, 1964)
    );
}

#[test]
fn reference_size_original_multi_segment() {
    let r169 = Ratio::new(16, 9);
    assert_eq!(
        reference_canvas_size(
            Aspect::Original,
            Ratio::new(32, 9),
            &[sz(7680, 2160), sz(3024, 1964)]
        ),
        sz(7680, 2160)
    );
    assert_eq!(
        reference_canvas_size(Aspect::Original, r169, &[sz(3840, 2160), sz(2160, 3840)]),
        sz(6826, 3840)
    );
    assert_eq!(
        reference_canvas_size(Aspect::Original, r169, &[sz(1920, 1080), sz(2880, 5120)]),
        sz(8192, 4608)
    );
    assert_eq!(
        reference_canvas_size(
            Aspect::Original,
            Ratio::new(9, 16),
            &[sz(1080, 1920), sz(7680, 2160)]
        ),
        sz(4608, 8192)
    );
}

#[test]
fn reference_size_without_segments() {
    let r = Ratio::new(16, 9);
    assert_eq!(reference_canvas_size(Aspect::R16x9, r, &[]), sz(1920, 1080));
    assert_eq!(reference_canvas_size(Aspect::R9x16, r, &[]), sz(1080, 1920));
    assert_eq!(
        reference_canvas_size(Aspect::Original, r, &[]),
        sz(1920, 1080)
    );
}

#[test]
fn reference_size_properties() {
    let mut rng = XorShift(0xD1B5_4A32_D192_ED03);
    for _ in 0..20_000 {
        let n = rng.range(1, 4) as usize;
        let segs: Vec<SizePx> = (0..n)
            .map(|_| sz(rng.range(2, 16384), rng.range(2, 16384)))
            .collect();
        for aspect in Aspect::ALL {
            let ratio = Ratio::of(segs[0]);
            let c = reference_canvas_size(aspect, ratio, &segs);
            let ctx = format!("{aspect:?} {segs:?} -> {c:?}");
            assert!(
                c.w.is_multiple_of(2) && c.h.is_multiple_of(2) && c.w >= 2 && c.h >= 2,
                "even: {ctx}"
            );
            match aspect {
                Aspect::Original if n == 1 => {
                    assert_eq!(c, sz(segs[0].w & !1, segs[0].h & !1), "{ctx}")
                }
                Aspect::Original => assert!(c.w.max(c.h) <= 8192, "{ctx}"),
                _ => {
                    assert!(c.w.min(c.h) <= 2160 && c.w.max(c.h) <= 3840, "{ctx}");
                    let Some(r) = aspect.ratio() else {
                        unreachable!()
                    };
                    let err = f64::from(c.w) - f64::from(c.h) * r.as_f64();
                    assert!(err.abs() <= 2.0, "ratio: {ctx}");
                }
            }
        }
    }
}

type PresetRow = (SizeClass, bool, [(u32, u32); 5]);

const RATIOS: [(u32, u32); 5] = [(16, 9), (4, 3), (1, 1), (9, 16), (4, 5)];

#[test]
fn preset_table_matches_architecture_10() {
    let table: [PresetRow; 7] = [
        (
            SizeClass::S2160,
            false,
            [
                (3840, 2160),
                (2880, 2160),
                (2160, 2160),
                (2160, 3840),
                (2160, 2700),
            ],
        ),
        (
            SizeClass::S1440,
            false,
            [
                (2560, 1440),
                (1920, 1440),
                (1440, 1440),
                (1440, 2560),
                (1440, 1800),
            ],
        ),
        (
            SizeClass::S1080,
            false,
            [
                (1920, 1080),
                (1440, 1080),
                (1080, 1080),
                (1080, 1920),
                (1080, 1350),
            ],
        ),
        (
            SizeClass::S720,
            false,
            [(1280, 720), (960, 720), (720, 720), (720, 1280), (720, 900)],
        ),
        (
            SizeClass::S480,
            false,
            [(854, 480), (640, 480), (480, 480), (480, 854), (480, 600)],
        ),
        (
            SizeClass::S360,
            true,
            [(640, 360), (480, 360), (360, 360), (360, 640), (360, 450)],
        ),
        (
            SizeClass::S240,
            true,
            [(427, 240), (320, 240), (240, 240), (240, 427), (240, 300)],
        ),
    ];
    for (class, gif, row) in table {
        for ((num, den), (w, h)) in RATIOS.into_iter().zip(row) {
            assert_eq!(
                preset_size(class, Ratio::new(num, den), gif),
                sz(w, h),
                "{class:?} {num}:{den} gif={gif}"
            );
        }
    }
}

#[test]
fn preset_video_is_even_gif_is_not() {
    let r = Ratio::new(16, 9);
    assert_eq!(preset_size(SizeClass::S240, r, false), sz(426, 240));
    assert_eq!(preset_size(SizeClass::S240, r, true), sz(427, 240));
    assert_eq!(preset_size(SizeClass::S480, r, true), sz(853, 480));
    assert_eq!(
        preset_size(SizeClass::S240, Ratio::new(9, 16), false),
        sz(240, 426)
    );
    assert_eq!(
        preset_size(SizeClass::S1080, Ratio::new(1920, 1080), false),
        sz(1920, 1080)
    );
    let original = Ratio::of(sz(3024, 1964));
    assert_eq!(
        preset_size(SizeClass::S1080, original, false),
        sz(1662, 1080)
    );
    assert_eq!(preset_size(SizeClass::S240, original, true), sz(370, 240));
}

#[test]
fn cover_src_rect_examples() {
    let img = sz(4000, 1000);
    let dst = sz(1920, 1080);
    let vw = 1000.0 * 1920.0 / 1080.0;
    let c = cover_src_rect(img, dst, PointF { x: 0.5, y: 0.5 });
    assert!((c.x - (2000.0 - vw / 2.0)).abs() < 1e-9 && c.y == 0.0);
    assert!((c.w - vw).abs() < 1e-9 && c.h == 1000.0);
    let c = cover_src_rect(img, dst, PointF { x: 0.0, y: 0.0 });
    assert!(c.x == 0.0 && c.y == 0.0);
    let c = cover_src_rect(img, dst, PointF { x: 1.0, y: 1.0 });
    assert!((c.x + c.w - 4000.0).abs() < 1e-9 && c.y == 0.0);
    let c = cover_src_rect(sz(1000, 4000), dst, PointF { x: 0.5, y: 0.25 });
    assert!(c.x == 0.0 && c.w == 1000.0);
    assert!((c.h - 562.5).abs() < 1e-9 && (c.y - (1000.0 - 281.25)).abs() < 1e-9);
    let c = cover_src_rect(img, sz(0, 1), PointF { x: 0.5, y: 0.5 });
    assert!(c.x == 0.0 && c.w == 4000.0 && c.h == 1000.0);
}

#[test]
fn cover_src_rect_properties() {
    let mut rng = XorShift(0x2545_F491_4F6C_DD1D);
    for _ in 0..50_000 {
        let img = sz(rng.range(1, 10000), rng.range(1, 10000));
        let dst = sz(rng.range(1, 8192), rng.range(1, 8192));
        let focus = PointF {
            x: f64::from(rng.range(0, 1400)) / 1000.0 - 0.2,
            y: f64::from(rng.range(0, 1400)) / 1000.0 - 0.2,
        };
        let c = cover_src_rect(img, dst, focus);
        let ctx = format!("{img:?} {dst:?} {focus:?} -> {c:?}");
        let eps = 1e-6;
        assert!(c.x >= 0.0 && c.y >= 0.0, "{ctx}");
        assert!(c.x + c.w <= f64::from(img.w) + eps, "{ctx}");
        assert!(c.y + c.h <= f64::from(img.h) + eps, "{ctx}");
        assert!(
            c.w == f64::from(img.w) || c.h == f64::from(img.h),
            "fills: {ctx}"
        );
        let want = f64::from(dst.w) / f64::from(dst.h);
        let got = c.w / c.h;
        assert!(
            (got - want).abs() / want < 1e-9 || c.w.min(c.h) < 1.0,
            "ratio: {ctx}"
        );
    }
}
