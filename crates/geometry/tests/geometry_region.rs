use sc_geometry::{
    Handle, PointF, Ratio, RectPt, RectPx, app_bounds_pt, aspect_locked_resize, flip_y, pt_to_px,
    snap_region_even_px,
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

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[(self.next() % items.len() as u64) as usize]
    }
}

fn rpt(x: f64, y: f64, w: f64, h: f64) -> RectPt {
    RectPt { x, y, w, h }
}

fn pt(x: f64, y: f64) -> PointF {
    PointF { x, y }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

#[test]
fn pt_to_px_snaps_both_edges() {
    assert_eq!(
        pt_to_px(rpt(10.25, 20.0, 100.5, 50.0), 2.0),
        RectPx {
            x: 21,
            y: 40,
            w: 201,
            h: 100
        }
    );
    assert_eq!(
        pt_to_px(rpt(-1512.0, 0.0, 1512.0, 982.0), 2.0),
        RectPx {
            x: -3024,
            y: 0,
            w: 3024,
            h: 1964
        }
    );
    assert_eq!(
        pt_to_px(rpt(1.0, 2.0, 3.0, 4.0), 0.0),
        pt_to_px(rpt(1.0, 2.0, 3.0, 4.0), 1.0)
    );
}

#[test]
fn flip_y_converts_origins_and_is_involutive() {
    let r = rpt(100.0, 50.0, 300.0, 200.0);
    let f = flip_y(r, 982.0);
    assert_eq!(f, rpt(100.0, 732.0, 300.0, 200.0));
    assert_eq!(flip_y(f, 982.0), r);
}

#[test]
fn snap_region_examples() {
    assert_eq!(
        snap_region_even_px(rpt(10.0, 10.0, 100.5, 51.0), 1.0),
        rpt(10.0, 10.0, 100.0, 50.0)
    );
    assert_eq!(
        snap_region_even_px(rpt(10.3, 10.0, 100.6, 50.0), 2.0),
        rpt(10.5, 10.0, 100.0, 50.0)
    );
    assert_eq!(
        snap_region_even_px(rpt(0.0, 0.0, 0.2, 0.2), 1.0),
        rpt(0.0, 0.0, 2.0, 2.0)
    );
}

#[test]
fn snap_region_properties() {
    let mut rng = XorShift(0x1234_5678_9ABC_DEF1);
    let scales = [1.0, 1.5, 2.0, 3.0, 1.25];
    for _ in 0..50_000 {
        let s = rng.pick(&scales);
        let r = rpt(
            rng.unit() * 4000.0 - 1000.0,
            rng.unit() * 3000.0,
            rng.unit() * 3000.0 + 2.0,
            rng.unit() * 2000.0 + 2.0,
        );
        let snapped = snap_region_even_px(r, s);
        let px = pt_to_px(snapped, s);
        let ctx = format!("{r:?} @{s} -> {snapped:?} {px:?}");
        assert!(
            px.w.is_multiple_of(2) && px.h.is_multiple_of(2),
            "even: {ctx}"
        );
        assert!(close(snapped.x * s, (snapped.x * s).round()), "{ctx}");
        let orig = pt_to_px(r, s);
        assert!(px.x == orig.x && px.y == orig.y, "origin: {ctx}");
        assert!(
            px.w <= orig.w.max(2) && px.h <= orig.h.max(2),
            "no growth: {ctx}"
        );
        assert!(orig.w - px.w.min(orig.w) <= 1, "{ctx}");
    }
}

#[test]
fn resize_corner_with_16x9_lock() {
    let bounds = rpt(0.0, 0.0, 1512.0, 982.0);
    let r = rpt(100.0, 100.0, 320.0, 180.0);
    let out = aspect_locked_resize(
        r,
        Some(Ratio::new(16, 9)),
        Handle::SE,
        pt(740.0, 300.0),
        bounds,
    );
    assert_eq!(out, rpt(100.0, 100.0, 640.0, 360.0));
    let out = aspect_locked_resize(
        r,
        Some(Ratio::new(16, 9)),
        Handle::SE,
        pt(5000.0, 300.0),
        bounds,
    );
    assert_eq!(out.x, 100.0);
    assert!(close(out.x + out.w, 1512.0) && close(out.w / out.h, 16.0 / 9.0));
    let out = aspect_locked_resize(
        r,
        Some(Ratio::new(16, 9)),
        Handle::NW,
        pt(-50.0, -50.0),
        bounds,
    );
    assert!(close(out.x + out.w, 420.0) && close(out.y + out.h, 280.0));
    assert!(close(out.x, 0.0) && close(out.w, 420.0) && close(out.w / out.h, 16.0 / 9.0));
}

#[test]
fn resize_edge_with_lock_grows_about_centre() {
    let bounds = rpt(0.0, 0.0, 1000.0, 1000.0);
    let r = rpt(100.0, 400.0, 200.0, 200.0);
    let out = aspect_locked_resize(r, Some(Ratio::new(1, 1)), Handle::E, pt(500.0, 0.0), bounds);
    assert_eq!(out, rpt(100.0, 300.0, 400.0, 400.0));
    let r = rpt(100.0, 0.0, 200.0, 200.0);
    let out = aspect_locked_resize(r, Some(Ratio::new(1, 1)), Handle::E, pt(500.0, 0.0), bounds);
    assert_eq!(out, rpt(100.0, 0.0, 400.0, 400.0));
}

#[test]
fn resize_free_and_move() {
    let bounds = rpt(0.0, 0.0, 1000.0, 800.0);
    let r = rpt(100.0, 100.0, 200.0, 100.0);
    assert_eq!(
        aspect_locked_resize(r, None, Handle::E, pt(450.0, 999.0), bounds),
        rpt(100.0, 100.0, 350.0, 100.0)
    );
    assert_eq!(
        aspect_locked_resize(r, None, Handle::SW, pt(50.0, 900.0), bounds),
        rpt(50.0, 100.0, 250.0, 700.0)
    );
    assert_eq!(
        aspect_locked_resize(r, None, Handle::W, pt(900.0, 0.0), bounds),
        rpt(299.0, 100.0, 1.0, 100.0)
    );
    assert_eq!(
        aspect_locked_resize(r, None, Handle::Move, pt(950.0, -20.0), bounds),
        rpt(800.0, 0.0, 200.0, 100.0)
    );
    assert_eq!(
        aspect_locked_resize(
            r,
            Some(Ratio::new(9, 16)),
            Handle::Move,
            pt(10.0, 20.0),
            bounds
        ),
        rpt(10.0, 20.0, 200.0, 100.0)
    );
}

#[test]
fn resize_match_frame_uses_canvas_ratio() {
    let canvas = Ratio::new(1080, 1920);
    let bounds = rpt(0.0, 0.0, 1512.0, 982.0);
    let r = rpt(0.0, 0.0, 90.0, 160.0);
    let out = aspect_locked_resize(r, Some(canvas), Handle::SE, pt(2000.0, 2000.0), bounds);
    assert!(
        close(out.h, 982.0) && close(out.w, 982.0 * 9.0 / 16.0),
        "{out:?}"
    );
}

const HANDLES: [Handle; 9] = [
    Handle::N,
    Handle::S,
    Handle::E,
    Handle::W,
    Handle::NE,
    Handle::NW,
    Handle::SE,
    Handle::SW,
    Handle::Move,
];

#[test]
fn resize_properties_keep_ratio_and_bounds() {
    let mut rng = XorShift(0xCAFE_F00D_DEAD_BEEF);
    let ratios = [
        None,
        Some(Ratio::new(16, 9)),
        Some(Ratio::new(9, 16)),
        Some(Ratio::new(4, 3)),
        Some(Ratio::new(1, 1)),
        Some(Ratio::new(1234, 567)),
    ];
    for _ in 0..100_000 {
        let b = rpt(
            rng.unit() * 2000.0 - 1000.0,
            rng.unit() * 2000.0 - 1000.0,
            rng.unit() * 3000.0 + 50.0,
            rng.unit() * 2000.0 + 50.0,
        );
        let w = rng.unit() * b.w;
        let h = rng.unit() * b.h;
        let r = rpt(
            b.x + rng.unit() * (b.w - w),
            b.y + rng.unit() * (b.h - h),
            w,
            h,
        );
        let ratio = rng.pick(&ratios);
        let handle = rng.pick(&HANDLES);
        let to = pt(
            b.x - 500.0 + rng.unit() * (b.w + 1000.0),
            b.y - 500.0 + rng.unit() * (b.h + 1000.0),
        );
        let out = aspect_locked_resize(r, ratio, handle, to, b);
        let ctx = format!("{r:?} {ratio:?} {handle:?} {to:?} in {b:?} -> {out:?}");
        let eps = 1e-9 * 4000.0;
        assert!(out.w >= 0.0 && out.h >= 0.0, "{ctx}");
        assert!(out.x >= b.x - eps && out.y >= b.y - eps, "bounds: {ctx}");
        assert!(out.x + out.w <= b.x + b.w + eps, "bounds: {ctx}");
        assert!(out.y + out.h <= b.y + b.h + eps, "bounds: {ctx}");
        if handle == Handle::Move {
            assert!(
                close(out.w, r.w) && close(out.h, r.h),
                "move keeps size: {ctx}"
            );
            continue;
        }
        if let Some(q) = ratio {
            assert!(close(out.w / out.h, q.as_f64()), "ratio: {ctx}");
        }
        let right = r.x + r.w;
        let bottom = r.y + r.h;
        match handle {
            Handle::E | Handle::NE | Handle::SE => assert!(close(out.x, r.x), "anchor: {ctx}"),
            Handle::W | Handle::NW | Handle::SW => {
                assert!(close(out.x + out.w, right), "anchor: {ctx}")
            }
            _ => {}
        }
        match handle {
            Handle::S | Handle::SE | Handle::SW => assert!(close(out.y, r.y), "anchor: {ctx}"),
            Handle::N | Handle::NE | Handle::NW => {
                assert!(close(out.y + out.h, bottom), "anchor: {ctx}")
            }
            _ => {}
        }
    }
}

#[test]
fn app_bounds_table() {
    let d = rpt(0.0, 0.0, 1512.0, 982.0);
    let right = rpt(1512.0, -100.0, 1920.0, 1080.0);
    let cases: &[(&str, Vec<RectPt>, RectPt, Option<RectPt>)] = &[
        ("no windows", vec![], d, None),
        (
            "one window",
            vec![rpt(100.0, 50.0, 800.0, 600.0)],
            d,
            Some(rpt(100.0, 50.0, 800.0, 600.0)),
        ),
        (
            "union of two",
            vec![
                rpt(100.0, 50.0, 400.0, 300.0),
                rpt(600.0, 200.0, 300.0, 500.0),
            ],
            d,
            Some(rpt(100.0, 50.0, 800.0, 650.0)),
        ),
        (
            "clipped to the display",
            vec![rpt(-50.0, 900.0, 300.0, 200.0)],
            d,
            Some(rpt(0.0, 900.0, 250.0, 82.0)),
        ),
        (
            "window on another display is ignored",
            vec![
                rpt(100.0, 50.0, 200.0, 100.0),
                rpt(1600.0, 0.0, 800.0, 600.0),
            ],
            d,
            Some(rpt(100.0, 50.0, 200.0, 100.0)),
        ),
        (
            "display-local on a secondary display",
            vec![rpt(1612.0, 0.0, 400.0, 300.0)],
            right,
            Some(rpt(100.0, 100.0, 400.0, 300.0)),
        ),
        (
            "sliver under 2 pt",
            vec![rpt(1511.0, 10.0, 100.0, 100.0)],
            d,
            None,
        ),
        ("empty window", vec![rpt(10.0, 10.0, 0.0, 0.0)], d, None),
        (
            "non-finite window skipped",
            vec![rpt(f64::NAN, 0.0, 10.0, 10.0), rpt(10.0, 10.0, 20.0, 20.0)],
            d,
            Some(rpt(10.0, 10.0, 20.0, 20.0)),
        ),
        (
            "covers the whole display",
            vec![rpt(-10.0, -10.0, 2000.0, 2000.0)],
            d,
            Some(d),
        ),
    ];
    for (name, windows, display, want) in cases {
        assert_eq!(app_bounds_pt(windows, *display), *want, "{name}");
    }
}
