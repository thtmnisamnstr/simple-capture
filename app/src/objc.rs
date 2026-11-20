//! The few AppKit calls the shell makes itself, through the Objective-C runtime (the app crate
//! has no objc2 dependency).

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
