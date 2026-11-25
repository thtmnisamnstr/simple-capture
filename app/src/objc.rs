//! The few AppKit calls the shell makes itself, through the Objective-C runtime (the app crate
//! has no objc2 dependency).
// Most of this module is window and menu introspection for the debug driver.
#![cfg_attr(not(debug_assertions), allow(dead_code))]

use std::ffi::{CStr, c_void};

type Id = *mut c_void;
type Sel = *mut c_void;

#[link(name = "objc")]
unsafe extern "C" {
    fn sel_registerName(name: *const std::ffi::c_char) -> Sel;
    fn objc_msgSend();
}

unsafe extern "C" {
    static _dispatch_main_q: c_void;
    fn dispatch_async_f(queue: *const c_void, ctx: *mut c_void, work: extern "C" fn(*mut c_void));
}

fn sel(name: &CStr) -> Sel {
    unsafe { sel_registerName(name.as_ptr()) }
}

unsafe fn send0(obj: Id, name: &CStr) -> Id {
    let f: unsafe extern "C" fn(Id, Sel) -> Id =
        unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(obj, sel(name)) }
}

/// Shows a window without making it key or activating the app.
///
/// # Safety
/// `ns_window` is a live NSWindow; main thread only.
pub unsafe fn order_front_regardless(ns_window: Id) {
    if !ns_window.is_null() {
        unsafe { send0(ns_window, c"orderFrontRegardless") };
    }
}

/// Closes the status item's menu if it is open. Runs on the main dispatch queue, which AppKit
/// drains during menu tracking.
pub fn cancel_status_item_menu(status_item: usize) {
    extern "C" fn work(ctx: *mut c_void) {
        unsafe {
            let menu = send0(ctx, c"menu");
            if !menu.is_null() {
                send0(menu, c"cancelTracking");
            }
        }
    }
    if status_item == 0 {
        return;
    }
    unsafe {
        dispatch_async_f(
            &raw const _dispatch_main_q,
            status_item as *mut c_void,
            work,
        )
    };
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFArrayGetCount(a: *const c_void) -> isize;
    fn CFArrayGetValueAtIndex(a: *const c_void, i: isize) -> *const c_void;
    fn CFDictionaryGetValue(d: *const c_void, key: *const c_void) -> *const c_void;
    fn CFNumberGetValue(n: *const c_void, kind: isize, out: *mut c_void) -> bool;
    fn CFRelease(cf: *const c_void);
}

#[repr(C)]
#[derive(Default)]
struct CgRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> *const c_void;
    fn CGRectMakeWithDictionaryRepresentation(d: *const c_void, rect: *mut CgRect) -> bool;
    static kCGWindowNumber: *const c_void;
    static kCGWindowOwnerPID: *const c_void;
    static kCGWindowLayer: *const c_void;
    static kCGWindowBounds: *const c_void;
}

/// An on-screen window of this process, from the window server's list (no permission needed
/// for numbers, layers and bounds).
#[derive(Debug, Clone, serde::Serialize)]
pub struct OwnWindow {
    pub id: u32,
    pub layer: i64,
    /// Global top-left points.
    pub rect_pt: [f64; 4],
}

pub fn own_windows_on_screen() -> Vec<OwnWindow> {
    const ON_SCREEN_ONLY: u32 = 1;
    const SINT64: isize = 4;
    let me = i64::from(std::process::id());
    let mut out = Vec::new();
    unsafe {
        let list = CGWindowListCopyWindowInfo(ON_SCREEN_ONLY, 0);
        if list.is_null() {
            return out;
        }
        let num = |d: *const c_void, key: *const c_void| -> Option<i64> {
            let v = CFDictionaryGetValue(d, key);
            let mut n: i64 = 0;
            (!v.is_null() && CFNumberGetValue(v, SINT64, (&raw mut n).cast())).then_some(n)
        };
        for i in 0..CFArrayGetCount(list) {
            let d = CFArrayGetValueAtIndex(list, i);
            if d.is_null() || num(d, kCGWindowOwnerPID) != Some(me) {
                continue;
            }
            let mut r = CgRect::default();
            let b = CFDictionaryGetValue(d, kCGWindowBounds);
            if b.is_null() || !CGRectMakeWithDictionaryRepresentation(b, &mut r) {
                continue;
            }
            out.push(OwnWindow {
                id: num(d, kCGWindowNumber)
                    .and_then(|n| u32::try_from(n).ok())
                    .unwrap_or(0),
                layer: num(d, kCGWindowLayer).unwrap_or(0),
                rect_pt: [r.x, r.y, r.w, r.h],
            });
        }
        CFRelease(list);
    }
    out
}
