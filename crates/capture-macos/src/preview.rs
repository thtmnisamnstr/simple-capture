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
    pub view: NonNull<c_void>,
    pub page: Page,
}

/// The page's part of the window (its content layout rectangle: below the title bar, all of it
/// in full screen), in points, and the window's backing scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Page {
    pub w: f64,
    pub h: f64,
    pub scale: f64,
}

/// Adds the layer in a view just below `webview`, fitted with [`fit_to_page`]. The view holds
/// the layer, so both pointers stay valid while the window lives.
///
/// # Safety
/// `webview` points to a live NSView; main thread only.
pub unsafe fn metal_layer_under(webview: NonNull<c_void>) -> Result<MetalLayer> {
    let fail = |what: &str| Error::Other(format!("metal_layer_under: {what}"));
    let mtm = MainThreadMarker::new().ok_or_else(|| fail("not on the main thread"))?;
    // SAFETY: the caller passes a live NSView.
    let wk: &NSView = unsafe { webview.cast().as_ref() };
    // SAFETY: read on the main thread.
    let parent = unsafe { wk.superview() }.ok_or_else(|| fail("the webview has no superview"))?;
    let view = NSView::initWithFrame(NSView::alloc(mtm), wk.frame());
    view.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    let layer = CAMetalLayer::new();
    layer.setOpaque(true);
    view.setLayer(Some(&layer));
    view.setWantsLayer(true);
    parent.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Below, Some(wk));
    let view_ptr = NonNull::from(&*view).cast();
    // SAFETY: the view was just added to a window, on the main thread.
    let page = unsafe { fit_to_page(view_ptr) }.ok_or_else(|| fail("the webview has no window"))?;
    Ok(MetalLayer {
        layer: NonNull::new(Retained::as_ptr(&layer).cast_mut().cast())
            .ok_or_else(|| fail("null layer"))?,
        view: view_ptr,
        page,
    })
}

/// Puts the view from [`metal_layer_under`] over the window's content layout rectangle and its
/// layer at the window's scale. `None` once the view has left its window.
///
/// # Safety
/// `view` comes from [`metal_layer_under`]; main thread only.
pub unsafe fn fit_to_page(view: NonNull<c_void>) -> Option<Page> {
    // SAFETY: the caller passes the live view.
    let view: &NSView = unsafe { view.cast().as_ref() };
    // SAFETY: read on the main thread.
    let parent = unsafe { view.superview() }?;
    let window = view.window()?;
    let page = parent.convertRect_fromView(window.contentLayoutRect(), None);
    let scale = window.backingScaleFactor();
    if view.frame() != page {
        view.setFrame(page);
    }
    if let Some(layer) = view.layer()
        && layer.contentsScale() != scale
    {
        layer.setContentsScale(scale);
    }
    Some(Page {
        w: page.size.width,
        h: page.size.height,
        scale,
    })
}
