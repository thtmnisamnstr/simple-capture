use std::ffi::c_void;
use std::sync::Mutex;

use objc2::runtime::{AnyClass, AnyObject};
use objc2::{ClassType, MainThreadMarker, MainThreadOnly, define_class};
use objc2_app_kit::{
    NSPanel, NSResponder, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::NSObject;
use sc_platform::{Error, OverlayRole, OverlayWindows, Result, WindowId};

define_class!(
    #[unsafe(super(NSPanel, NSWindow, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SCCaptureOverlayPanel"]
    struct OverlayPanel;

    impl OverlayPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool {
            true
        }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main(&self) -> bool {
            false
        }

        #[unsafe(method(close))]
        fn close(&self) {
            let obj: &AnyObject = self.as_ref();
            if restore_class(obj) {
                let _: () = unsafe { objc2::msg_send![obj, close] };
            } else {
                let _: () = unsafe { objc2::msg_send![super(self), close] };
            }
        }
    }
);

/// isa before the swap, by object address. It may be AppKit's KVO subclass
/// (`NSKVONotifying_…`): KVO registrations are tied to it, so it must be back in place before
/// AppKit tears the window down, or removing its `_windowLayerContext` observer throws.
static ORIGINAL_CLASS: Mutex<Vec<(usize, &'static AnyClass)>> = Mutex::new(Vec::new());

fn restore_class(obj: &AnyObject) -> bool {
    let key = obj as *const AnyObject as usize;
    let orig = ORIGINAL_CLASS.lock().ok().and_then(|mut v| {
        let i = v.iter().position(|(k, _)| *k == key)?;
        Some(v.swap_remove(i).1)
    });
    match orig {
        Some(cls) => {
            unsafe { AnyObject::set_class(obj, cls) };
            true
        }
        None => false,
    }
}

fn is_kind(obj: &AnyObject, cls: &objc2::runtime::AnyClass) -> bool {
    unsafe { objc2::msg_send![obj, isKindOfClass: cls] }
}

static REGISTERED: Mutex<Vec<WindowId>> = Mutex::new(Vec::new());

pub(crate) fn registered_windows() -> Vec<WindowId> {
    REGISTERED.lock().map(|r| r.clone()).unwrap_or_default()
}

const LEVEL_NORMAL: isize = 0;
const LEVEL_FLOATING: isize = 3;
const LEVEL_STATUS: isize = 25;
const LEVEL_SCREEN_SAVER: isize = 1000;

pub(crate) fn level_for(role: OverlayRole) -> isize {
    match role {
        OverlayRole::FloatingBar | OverlayRole::Popover => LEVEL_STATUS,
        OverlayRole::RegionOverlay | OverlayRole::PickOverlay => LEVEL_SCREEN_SAVER,
        OverlayRole::Panel | OverlayRole::Saved | OverlayRole::Onboarding | OverlayRole::Stage => {
            LEVEL_FLOATING
        }
        OverlayRole::Editor => LEVEL_NORMAL,
    }
}

#[derive(Debug, Default)]
pub struct MacOverlays {
    _priv: (),
}

impl MacOverlays {
    pub fn new() -> MacOverlays {
        MacOverlays::default()
    }

    pub fn panel_class_name() -> &'static str {
        "SCCaptureOverlayPanel"
    }

    /// Puts back the window's original class (also done by `close`) and unregisters it.
    ///
    /// # Safety
    /// `raw_window` must be a live window passed to `configure`; main thread only.
    pub unsafe fn unconfigure(&self, raw_window: *mut c_void) {
        if let Some(obj) = unsafe { raw_window.cast::<AnyObject>().as_ref() } {
            restore_class(obj);
            let n: isize = unsafe { objc2::msg_send![obj, windowNumber] };
            if let Ok(n) = u32::try_from(n)
                && let Ok(mut r) = REGISTERED.lock()
            {
                r.retain(|w| w.0 != n);
            }
        }
    }
}

/// Without a window-server window an unshown NSWindow has `windowNumber <= 0` and cannot be
/// excluded; an invisible order-in/order-out creates it without activating anything.
fn force_window_device(win: &NSWindow) {
    if win.windowNumber() > 0 {
        return;
    }
    let alpha = win.alphaValue();
    win.setAlphaValue(0.0);
    win.orderFrontRegardless();
    win.orderOut(None);
    win.setAlphaValue(alpha);
}

fn register(win: &NSWindow) -> Result<WindowId> {
    let n = win.windowNumber();
    if n <= 0 {
        return Err(Error::Other(format!(
            "overlays: windowNumber {n} after forcing the device"
        )));
    }
    let id = WindowId(n as u32);
    if let Ok(mut r) = REGISTERED.lock()
        && !r.contains(&id)
    {
        r.push(id);
    }
    Ok(id)
}

impl OverlayWindows for MacOverlays {
    unsafe fn configure(&self, raw_window: *mut c_void, role: OverlayRole) -> Result<WindowId> {
        if MainThreadMarker::new().is_none() {
            return Err(Error::Other(
                "overlays: configure must run on the main thread".into(),
            ));
        }
        let obj = raw_window.cast::<AnyObject>();
        let Some(obj) = (unsafe { obj.as_ref() }) else {
            return Err(Error::Other("overlays: null window".into()));
        };
        if !is_kind(obj, NSWindow::class()) {
            return Err(Error::Other(format!(
                "overlays: {} is not an NSWindow",
                obj.class().name().to_string_lossy()
            )));
        }
        let win: &NSWindow = unsafe { &*(obj as *const AnyObject as *const NSWindow) };
        if role == OverlayRole::Editor {
            win.setLevel(level_for(role));
            force_window_device(win);
            return register(win);
        }
        if !is_kind(obj, NSPanel::class()) {
            let orig: &'static AnyClass =
                unsafe { AnyObject::set_class(obj, OverlayPanel::class()) };
            if let Ok(mut v) = ORIGINAL_CLASS.lock() {
                v.push((obj as *const AnyObject as usize, orig));
            }
        }
        win.setStyleMask(win.styleMask() | NSWindowStyleMask::NonactivatingPanel);
        // A private selector: App Store review (Guideline 2.5.1) rejects it, so that build relies
        // on the non-activating style alone.
        #[cfg(not(feature = "mas"))]
        if super::content::responds(obj, objc2::sel!(_setPreventsActivation:)) {
            let _: () = unsafe { objc2::msg_send![obj, _setPreventsActivation: true] };
        }
        if let Some(panel) = unsafe { (obj as *const AnyObject as *const NSPanel).as_ref() } {
            panel.setBecomesKeyOnlyIfNeeded(role != OverlayRole::Stage);
            panel.setFloatingPanel(true);
        }
        win.setHidesOnDeactivate(false);
        win.setCollectionBehavior(
            win.collectionBehavior()
                | NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        win.setLevel(level_for(role));
        force_window_device(win);
        register(win)
    }

    fn registered(&self) -> Vec<WindowId> {
        registered_windows()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_swap_keeps_instance_size() {
        let w = NSWindow::class().instance_size();
        let p = NSPanel::class().instance_size();
        let o = OverlayPanel::class().instance_size();
        assert_eq!(w, p, "NSWindow {w} vs NSPanel {p}");
        assert_eq!(p, o, "NSPanel {p} vs overlay {o}");
    }

    #[test]
    fn levels_by_role() {
        assert_eq!(level_for(OverlayRole::RegionOverlay), 1000);
        assert_eq!(level_for(OverlayRole::FloatingBar), 25);
        assert_eq!(level_for(OverlayRole::Panel), 3);
    }
}
