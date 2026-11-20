//! `record --own-test-window` (macOS debug builds): an in-process magenta panel that must never
//! appear in the recording. The CLI has no objc2 dependency, so this talks to the Objective-C
//! runtime directly. Main thread only.

use std::ffi::{CStr, c_char, c_void};

use sc_platform::{OverlayRole, Platform, WindowId};

type Id = *mut c_void;
type Sel = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> Id;
    fn sel_registerName(name: *const c_char) -> Sel;
    fn objc_msgSend();
    fn objc_autoreleasePoolPush() -> *mut c_void;
    fn objc_autoreleasePoolPop(pool: *mut c_void);
}

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

const NS_APP_ACTIVATION_ACCESSORY: isize = 1;
const NS_BORDERLESS_NONACTIVATING_PANEL: usize = 1 << 7;
const NS_BACKING_BUFFERED: usize = 2;
const NS_STATUS_WINDOW_LEVEL: isize = 25;

/// Content rect in Cocoa screen points (bottom-left origin of the main display).
pub const FRAME_PT: (f64, f64, f64, f64) = (160.0, 160.0, 360.0, 240.0);

fn sel(name: &CStr) -> Sel {
    // SAFETY: registering a selector name is always valid.
    unsafe { sel_registerName(name.as_ptr()) }
}

macro_rules! send {
    ($obj:expr, $sel:literal $(, $arg:expr => $ty:ty)* ; -> $ret:ty) => {{
        let f: unsafe extern "C" fn(Id, Sel $(, $ty)*) -> $ret =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        f($obj, sel($sel) $(, $arg)*)
    }};
}

fn class(name: &CStr) -> Result<Id, String> {
    // SAFETY: objc_getClass takes a NUL-terminated name and returns nil when unknown.
    let c = unsafe { objc_getClass(name.as_ptr()) };
    if c.is_null() {
        Err(format!("Objective-C class {name:?} not found"))
    } else {
        Ok(c)
    }
}

pub struct OwnWindow {
    panel: Id,
    pub id: WindowId,
}

impl OwnWindow {
    /// Creates the panel transparent and ordered in (so it has a window number), then registers it
    /// with the platform's overlays (falling back to its window number). `reveal` makes it visible
    /// once the exclusion is in place.
    pub fn create(platform: &dyn Platform, title: &str) -> Result<OwnWindow, String> {
        let app_cls = class(c"NSApplication")?;
        let panel_cls = class(c"NSPanel")?;
        let color_cls = class(c"NSColor")?;
        let string_cls = class(c"NSString")?;
        let title = std::ffi::CString::new(title).map_err(|e| e.to_string())?;
        // SAFETY: plain AppKit messages with their documented signatures, on the main thread;
        // every receiver is checked for nil before use.
        unsafe {
            let pool = objc_autoreleasePoolPush();
            let app = send!(app_cls, c"sharedApplication"; -> Id);
            if !app.is_null() {
                send!(app, c"setActivationPolicy:", NS_APP_ACTIVATION_ACCESSORY => isize; -> bool);
            }
            let alloc = send!(panel_cls, c"alloc"; -> Id);
            let (x, y, w, h) = FRAME_PT;
            let panel = send!(alloc, c"initWithContentRect:styleMask:backing:defer:",
                CGRect { x, y, w, h } => CGRect,
                NS_BORDERLESS_NONACTIVATING_PANEL => usize,
                NS_BACKING_BUFFERED => usize,
                false => bool; -> Id);
            if panel.is_null() {
                objc_autoreleasePoolPop(pool);
                return Err("NSPanel init returned nil".to_string());
            }
            send!(panel, c"setReleasedWhenClosed:", false => bool; -> ());
            send!(panel, c"setOpaque:", true => bool; -> ());
            send!(panel, c"setHasShadow:", false => bool; -> ());
            send!(panel, c"setIgnoresMouseEvents:", true => bool; -> ());
            send!(panel, c"setLevel:", NS_STATUS_WINDOW_LEVEL => isize; -> ());
            let color = send!(color_cls, c"colorWithSRGBRed:green:blue:alpha:",
                1.0 => f64, 0.0 => f64, 1.0 => f64, 1.0 => f64; -> Id);
            send!(panel, c"setBackgroundColor:", color => Id; -> ());
            let ns_title =
                send!(string_cls, c"stringWithUTF8String:", title.as_ptr() => *const c_char; -> Id);
            send!(panel, c"setTitle:", ns_title => Id; -> ());
            send!(panel, c"setAlphaValue:", 0.0 => f64; -> ());
            send!(panel, c"orderFrontRegardless"; -> ());
            let number = send!(panel, c"windowNumber"; -> isize);
            objc_autoreleasePoolPop(pool);
            let id =
                match platform.overlays().configure(panel, OverlayRole::Panel) {
                    Ok(id) => id,
                    Err(e) => {
                        tracing::warn!("overlays().configure failed ({e}); using windowNumber");
                        WindowId(u32::try_from(number).map_err(|_| {
                            format!("own test window has no window number ({number})")
                        })?)
                    }
                };
            Ok(OwnWindow { panel, id })
        }
    }

    pub fn reveal(&self) {
        // SAFETY: `panel` is a live NSPanel we own; main thread.
        unsafe {
            send!(self.panel, c"setAlphaValue:", 1.0 => f64; -> ());
            send!(self.panel, c"orderFrontRegardless"; -> ());
        }
    }

    pub fn close(self) {
        // SAFETY: `panel` is a live NSPanel we own (releasedWhenClosed = NO); main thread.
        unsafe {
            send!(self.panel, c"orderOut:", std::ptr::null_mut::<c_void>() => Id; -> ());
            send!(self.panel, c"close"; -> ());
            send!(self.panel, c"release"; -> ());
        }
    }
}
