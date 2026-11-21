//! The native preview surface (ADR-007): a `CAMetalLayer` in a view just below a window's
//! WKWebView. Where the page is transparent the layer shows through; the HTML still draws on top
//! and gets every click.

use std::ffi::c_void;
use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView, NSWindowOrderingMode};
use objc2_quartz_core::CAMetalLayer;
use sc_platform::{Error, Result};

pub struct MetalLayer {
    pub layer: NonNull<c_void>,
    /// Points at the top of the webview that the page does not use (under the title bar); the
    /// layer leaves them out.
    pub top_inset: f64,
}

/// Adds the layer under `webview`, covering the page's part of it (the window's content layout
/// rectangle) and following the window's size. The view holds the layer, so the pointer stays
/// valid while the window lives.
///
/// # Safety
/// `webview` points to a live NSView; main thread only.
pub unsafe fn metal_layer_under(webview: NonNull<c_void>, scale: f64) -> Result<MetalLayer> {
    let fail = |what: &str| Error::Other(format!("metal_layer_under: {what}"));
    let mtm = MainThreadMarker::new().ok_or_else(|| fail("not on the main thread"))?;
    // SAFETY: the caller passes a live NSView.
    let wk: &NSView = unsafe { webview.cast().as_ref() };
    // SAFETY: read on the main thread.
    let parent = unsafe { wk.superview() }.ok_or_else(|| fail("the webview has no superview"))?;
    let window = wk
        .window()
        .ok_or_else(|| fail("the webview has no window"))?;
    let page = parent.convertRect_fromView(window.contentLayoutRect(), None);
    let web = wk.frame();
    let top_inset = if parent.isFlipped() {
        page.origin.y - web.origin.y
    } else {
        (web.origin.y + web.size.height) - (page.origin.y + page.size.height)
    };
    let view = NSView::initWithFrame(NSView::alloc(mtm), page);
    view.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    let layer = CAMetalLayer::new();
    layer.setContentsScale(scale);
    layer.setOpaque(true);
    view.setLayer(Some(&layer));
    view.setWantsLayer(true);
    parent.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Below, Some(wk));
    tracing::debug!(
        ?web,
        ?page,
        top_inset,
        scale,
        "preview: metal layer under the webview"
    );
    Ok(MetalLayer {
        layer: NonNull::new(Retained::as_ptr(&layer).cast_mut().cast())
            .ok_or_else(|| fail("null layer"))?,
        top_inset: top_inset.max(0.0),
    })
}
