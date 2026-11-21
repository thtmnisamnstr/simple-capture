//! The quick export draws the camera from its track where the keyframes put it.

mod common;

use std::sync::Arc;

use common::*;
use sc_core::project::{CameraKey, CameraShape, Easing};
use sc_export::recipe::FrameRenderer;
use sc_geometry::{PointF, SizePx};
use sc_platform::CpuNv12Frame;
use sc_platform::synthetic::SctrkDecoder;
use sc_render::{Backend, OutputFormat, RenderedFrame};

fn red(size: SizePx) -> CpuNv12Frame {
    let (w, h) = (size.w as usize, size.h as usize);
    CpuNv12Frame {
        size,
        y: vec![63; w * h],
        uv: [102u8, 240].repeat(w.div_ceil(2) * h.div_ceil(2)),
        full_range: false,
    }
}

fn px(f: &RenderedFrame, x: u32, y: u32) -> [u8; 4] {
    let o = ((y * f.size.w + x) * 4) as usize;
    [f.data[o], f.data[o + 1], f.data[o + 2], f.data[o + 3]]
}

#[test]
fn camera_follows_its_keyframes_in_the_export() {
    let tmp = TempDir::new("export_camera");
    let mut shown = CameraKey::new(0, CameraShape::Circle, PointF { x: 0.75, y: 0.5 }, 0.3);
    shown.easing = Easing::Step;
    let hidden = CameraKey {
        t_rec_ns: 1_000 * MS,
        visible: false,
        easing: Easing::Step,
        ..shown.clone()
    };
    let mut proj = new_project(tmp.path(), "cam", 2_000);
    proj.segment(Seg::new(0, 2_000, 1280, 720)).camera(
        red(SizePx { w: 640, h: 480 }),
        2_000,
        vec![shown, hidden],
    );
    for t in &mut proj.project.recording.tracks {
        if t.id == "camera" {
            t.first_sample_offset_ns = 30 * MS;
        }
    }
    let dir = proj.save();
    let mut plain = proj.project.clone();
    plain.camera_layout.overlays.clear();
    let canvas = SizePx { w: 1280, h: 720 };
    let render = |p: &sc_core::project::ProjectV1, t: i64| {
        FrameRenderer::new(
            p,
            &dir,
            Arc::new(SctrkDecoder),
            canvas,
            Backend::CpuOnly,
            OutputFormat::Rgba8,
        )
        .expect("renderer")
        .render(t)
        .expect("render")
        .0
    };
    let first = px(&render(&proj.project, 0), 960, 360);
    assert!(
        first[0] > 240 && first[1] < 20 && first[2] < 20,
        "frame 0, before the camera's first sample: its first frame is held, {first:?}"
    );
    let with = render(&proj.project, 500 * MS);
    let without = render(&plain, 500 * MS);
    let centre = px(&with, 960, 360);
    assert!(
        centre[0] > 240 && centre[1] < 20 && centre[2] < 20,
        "camera in the circle: {centre:?}"
    );
    assert_eq!(
        px(&with, 320, 360),
        px(&without, 320, 360),
        "screen untouched elsewhere"
    );
    let r = 720.0 * 0.3 / 2.0;
    let corner = (960.0 - r * 0.9) as u32;
    assert_eq!(
        px(&with, corner, (360.0 - r * 0.9) as u32),
        px(&without, corner, (360.0 - r * 0.9) as u32),
        "outside the circle"
    );
    assert_eq!(
        render(&proj.project, 1_500 * MS).data,
        render(&plain, 1_500 * MS).data,
        "hidden from 1 s"
    );
}
