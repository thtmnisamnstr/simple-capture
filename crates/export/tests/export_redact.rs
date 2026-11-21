//! Blur boxes hide the screen picture in exports while they are on the timeline.

mod common;

use std::sync::Arc;

use common::*;
use sc_core::project::{RectNorm, RedactStyle, Redaction};
use sc_export::recipe::FrameRenderer;
use sc_geometry::SizePx;
use sc_platform::synthetic::SctrkDecoder;
use sc_render::{Backend, OutputFormat, RenderedFrame};

fn px(f: &RenderedFrame, x: u32, y: u32) -> [u8; 3] {
    let o = ((y * f.size.w + x) * 4) as usize;
    [f.data[o], f.data[o + 1], f.data[o + 2]]
}

#[test]
fn a_black_box_hides_the_screen_only_while_it_is_on_the_timeline() {
    let tmp = TempDir::new("export_redact");
    let mut proj = new_project(tmp.path(), "redact", 2_000);
    proj.segment(Seg::new(0, 2_000, 1280, 720));
    proj.project.redactions.push(Redaction {
        id: "r1".into(),
        start_tl_ns: 500 * MS,
        end_tl_ns: 1_500 * MS,
        rect_norm: RectNorm {
            x: 0.25,
            y: 0.25,
            w: 0.5,
            h: 0.5,
        },
        style: RedactStyle::Black,
        extra: Default::default(),
    });
    let dir = proj.save();
    let canvas = SizePx { w: 1280, h: 720 };
    let mut r = FrameRenderer::new(
        &proj.project,
        &dir,
        Arc::new(SctrkDecoder),
        canvas,
        Backend::CpuOnly,
        OutputFormat::Rgba8,
    )
    .expect("renderer");
    let before = r.render(100 * MS).expect("before").0;
    let during = r.render(1_000 * MS).expect("during").0;
    let after = r.render(1_800 * MS).expect("after").0;
    assert_eq!(px(&during, 640, 360), [0, 0, 0], "inside the box");
    assert_ne!(px(&before, 640, 360), [0, 0, 0], "before it starts");
    assert_ne!(px(&after, 640, 360), [0, 0, 0], "after it ends");
    assert_eq!(
        px(&during, 100, 100),
        px(&before, 100, 100),
        "outside the box"
    );
}
