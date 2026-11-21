//! Text items are drawn on the exported frames while they are on the timeline, with their
//! animation, the same at every frame rate.

mod common;

use std::sync::Arc;

use common::*;
use sc_core::project::{Anim, Extra, Rgba, TextFill, TextItem};
use sc_export::recipe::FrameRenderer;
use sc_geometry::{PointF, SizePx};
use sc_platform::synthetic::SctrkDecoder;
use sc_render::{Backend, OutputFormat, RenderedFrame};

fn lit(f: &RenderedFrame, x0: u32, y0: u32, x1: u32, y1: u32) -> usize {
    let mut n = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            let o = ((y * f.size.w + x) * 4) as usize;
            if f.data[o] > 200 && f.data[o + 1] > 200 && f.data[o + 2] < 80 {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn text_shows_while_on_the_timeline_and_fades_in() {
    let tmp = TempDir::new("export_text");
    let mut proj = new_project(tmp.path(), "text", 3_000);
    proj.segment(Seg::new(0, 3_000, 1280, 720));
    let mut item = TextItem::new(1_000 * MS, 2_000 * MS, "HELLO");
    item.style.fill = TextFill::Solid(Rgba([1.0, 0.95, 0.0, 1.0]));
    item.style.size_1080 = 120.0;
    item.transform.center_norm = PointF { x: 0.5, y: 0.3 };
    item.anim.in_ = Some(Anim {
        kind: "fade".into(),
        duration_s: 0.4,
        easing: "linear".into(),
        extra: Extra::new(),
    });
    proj.project.text.push(item);
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
    let mut at = |ms: i64| r.render(ms * MS).expect("render").0;
    let region = |f: &RenderedFrame| lit(f, 400, 120, 880, 320);
    assert_eq!(region(&at(500)), 0, "before the item");
    let half = region(&at(1_200));
    let full = region(&at(1_600));
    assert!(full > 3_000, "the text is drawn: {full} yellow pixels");
    assert!(half < full, "fading in: {half} < {full}");
    assert_eq!(region(&at(2_500)), 0, "after the item");
    let a = at(1_500);
    let b = at(1_500);
    assert_eq!(a.data, b.data, "the same time draws the same frame");
}
