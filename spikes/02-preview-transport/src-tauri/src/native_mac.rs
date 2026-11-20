use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSView, NSWindowOrderingMode};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use objc2_quartz_core::CAMetalLayer;
use std::ffi::c_void;

pub struct Attached {
    pub layer_ptr: usize,
    pub description: String,
}

/// Inserts a layer-hosting NSView with a CAMetalLayer next to the WKWebView,
/// covering the given rect (CSS px in the webview's coordinate space).
/// `above == false`: the view goes *below* the webview, so the webview's
/// transparent regions reveal the Metal layer while its opaque HTML still draws
/// over it and receives all input. `above == true`: the view sits on top of the
/// webview, so HTML can only sit beside it, never over it.
pub fn attach_native_view(webview_ptr: *mut c_void, x: f64, y: f64, w: f64, h: f64, inner_h: f64, scale: f64, above: bool) -> Result<Attached, String> {
    let mtm = MainThreadMarker::new().ok_or("attach must run on the main thread")?;
    let wk: &NSView = unsafe { &*(webview_ptr as *const NSView) };
    let parent = unsafe { wk.superview() }.ok_or("webview has no superview")?;
    let b = wk.bounds();
    // The page viewport can be shorter than the WKWebView (top content inset
    // under the title bar), so measure from the bottom edge, which they share.
    let inset_top = (b.size.height - inner_h).max(0.0);
    let local_y = if wk.isFlipped() { inset_top + y } else { inner_h - y - h };
    let local = NSRect::new(NSPoint::new(x, local_y), NSSize::new(w, h));
    let frame = wk.convertRect_toView(local, Some(&parent));

    let view = NSView::initWithFrame(NSView::alloc(mtm), frame);
    let layer = CAMetalLayer::new();
    layer.setContentsScale(scale);
    layer.setOpaque(true);
    view.setLayer(Some(&layer));
    view.setWantsLayer(true);
    let order = if above { NSWindowOrderingMode::Above } else { NSWindowOrderingMode::Below };
    parent.addSubview_positioned_relativeTo(&view, order, Some(wk));

    let subviews = parent.subviews();
    let order: Vec<String> = subviews.iter().map(|v| v.class().name().to_str().unwrap_or("?").to_string()).collect();
    let description = format!(
        "placement={} webview_h={} viewport_h={} parent={} flipped={} frame=({:.0},{:.0} {:.0}x{:.0}) scale={} subview order (back->front)={:?}",
        if above { "above-webview" } else { "below-webview" },
        b.size.height,
        inner_h,
        parent.class().name().to_str().unwrap_or("?"),
        parent.isFlipped(),
        frame.origin.x,
        frame.origin.y,
        frame.size.width,
        frame.size.height,
        scale,
        order
    );
    let layer_ptr = Retained::as_ptr(&layer) as usize;
    std::mem::forget(view);
    std::mem::forget(layer);
    Ok(Attached { layer_ptr, description })
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
}

/// Never requests access: only reports whether this process may already capture.
pub fn screen_capture_preflight() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() }
}

pub fn window_number(ns_window_ptr: *mut c_void) -> isize {
    let w: &objc2_app_kit::NSWindow = unsafe { &*(ns_window_ptr as *const objc2_app_kit::NSWindow) };
    w.windowNumber()
}
