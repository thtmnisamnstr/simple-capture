use sc_geometry::{
    CameraPose, Corner, PointF, Ratio, RectF, SizePx, camera_rects, corner_center, snap_center,
};
use serde_json::Value;

const EPS: f64 = 1e-9;

fn cases() -> Value {
    serde_json::from_str(include_str!("camera_cases.json")).expect("camera_cases.json")
}

fn num(v: &Value) -> f64 {
    v.as_f64().expect("number")
}

fn size(v: &Value) -> SizePx {
    SizePx {
        w: num(&v[0]) as u32,
        h: num(&v[1]) as u32,
    }
}

fn ratio(v: &Value) -> Ratio {
    Ratio::new(num(&v[0]) as u32, num(&v[1]) as u32)
}

fn point(v: &Value) -> PointF {
    PointF {
        x: num(&v[0]),
        y: num(&v[1]),
    }
}

fn rect(v: &Value) -> RectF {
    RectF {
        x: num(&v[0]),
        y: num(&v[1]),
        w: num(&v[2]),
        h: num(&v[3]),
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= EPS * b.abs().max(1.0)
}

fn close_rect(a: RectF, b: RectF) -> bool {
    close(a.x, b.x) && close(a.y, b.y) && close(a.w, b.w) && close(a.h, b.h)
}

#[test]
fn camera_rects_match_the_shared_cases() {
    for c in cases()["rects"].as_array().expect("rects") {
        let p = &c["pose"];
        let pose = CameraPose {
            center: point(&p["center"]),
            size: num(&p["size"]),
            zoom: num(&p["zoom"]),
            pan: point(&p["pan"]),
            radius: num(&p["radius"]),
        };
        let r = camera_rects(
            size(&c["canvas"]),
            size(&c["camera"]),
            ratio(&c["aspect"]),
            &pose,
        );
        let name = &c["name"];
        assert!(
            close_rect(r.dst, rect(&c["dst"])),
            "{name}: dst {:?}",
            r.dst
        );
        assert!(
            close_rect(r.src, rect(&c["src"])),
            "{name}: src {:?}",
            r.src
        );
        assert!(
            close(r.radius_px, num(&c["radius_px"])),
            "{name}: radius {}",
            r.radius_px
        );
    }
}

#[test]
fn corner_presets_match_the_shared_cases() {
    for c in cases()["corners"].as_array().expect("corners") {
        let corner = match c["corner"].as_str().expect("corner") {
            "top_left" => Corner::TopLeft,
            "top_right" => Corner::TopRight,
            "bottom_left" => Corner::BottomLeft,
            _ => Corner::BottomRight,
        };
        let got = corner_center(
            size(&c["canvas"]),
            ratio(&c["aspect"]),
            num(&c["size"]),
            corner,
        );
        let want = point(&c["center"]);
        assert!(close(got.x, want.x) && close(got.y, want.y), "{c}: {got:?}");
    }
}

#[test]
fn snapping_matches_the_shared_cases() {
    for c in cases()["snaps"].as_array().expect("snaps") {
        let got = snap_center(
            size(&c["canvas"]),
            ratio(&c["aspect"]),
            num(&c["size"]),
            point(&c["center"]),
            num(&c["threshold_px"]),
        );
        let want = point(&c["snapped"]);
        assert!(
            close(got.x, want.x) && close(got.y, want.y),
            "{}: {got:?}",
            c["name"]
        );
    }
}
