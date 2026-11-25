//! System materials behind a transparent webview (ADR-044): the dark HUD of the control bar and
//! the popover of the source switcher, with rounded corners and the window's shadow following
//! them.

use std::ffi::c_void;
use std::ptr::NonNull;

use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameDarkAqua, NSAutoresizingMaskOptions,
    NSColor, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
    NSVisualEffectView, NSWindowOrderingMode,
};
use sc_platform::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Material {
    /// Always dark, like the system's own recording controls.
    Hud,
    Popover,
}

/// Makes the window of `webview` transparent and puts `material` under the webview, clipped to
/// `corner_radius` points.
///
/// # Safety
/// `webview` points to a live NSView whose page draws no background; main thread only.
pub unsafe fn material_under(
    webview: NonNull<c_void>,
    material: Material,
    corner_radius: f64,
) -> Result<()> {
    let fail = |what: &str| Error::Other(format!("material_under: {what}"));
    let mtm = MainThreadMarker::new().ok_or_else(|| fail("not on the main thread"))?;
    // SAFETY: the caller passes a live NSView.
    let wk: &NSView = unsafe { webview.cast().as_ref() };
    // SAFETY: read on the main thread.
    let parent = unsafe { wk.superview() }.ok_or_else(|| fail("the webview has no superview"))?;
    let window = wk
        .window()
        .ok_or_else(|| fail("the webview has no window"))?;
    window.setOpaque(false);
    window.setBackgroundColor(Some(&NSColor::clearColor()));
    let fx = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), wk.frame());
    fx.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    fx.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    fx.setState(NSVisualEffectState::Active);
    match material {
        Material::Hud => {
            fx.setMaterial(NSVisualEffectMaterial::HUDWindow);
            // SAFETY: a framework constant.
            let dark = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua });
            window.setAppearance(dark.as_deref());
        }
        Material::Popover => fx.setMaterial(NSVisualEffectMaterial::Popover),
    }
    fx.setWantsLayer(true);
    if let Some(layer) = fx.layer() {
        layer.setCornerRadius(corner_radius);
        layer.setMasksToBounds(true);
    }
    parent.addSubview_positioned_relativeTo(&fx, NSWindowOrderingMode::Below, Some(wk));
    window.invalidateShadow();
    Ok(())
}
