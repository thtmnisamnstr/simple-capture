//! The parent for portal dialogs: the app window that has the focus (the one whose button opened
//! the dialog), so the desktop attaches the share, shortcut and file dialogs to it instead of
//! floating them on their own. Wayland needs an xdg-foreign handle, which GTK exports; X11 takes the
//! window's XID.

use std::ffi::{CStr, c_char, c_ulong, c_void};
use std::sync::mpsc;
use std::time::Duration;

use ashpd::WindowIdentifier;
use gtk::gdk::ffi::GdkWindow;
use gtk::glib::translate::ToGlibPtr;
use gtk::prelude::*;

const EXPORTED_KEY: &CStr = c"sc-xdg-foreign-handle";

unsafe extern "C" {
    fn gdk_x11_window_get_xid(window: *mut GdkWindow) -> c_ulong;
    fn gdk_wayland_window_export_handle(
        window: *mut GdkWindow,
        callback: unsafe extern "C" fn(*mut GdkWindow, *const c_char, *mut c_void),
        user_data: *mut c_void,
        destroy: Option<unsafe extern "C" fn(*mut c_void)>,
    ) -> gtk::glib::ffi::gboolean;
}

enum Parent {
    X11(c_ulong),
    Wayland(String),
}

type Reply = mpsc::Sender<Option<Parent>>;

/// The focused app window as a portal parent, or None (no focused window, or GTK didn't answer
/// within half a second). Never call it on the GTK main thread: the Wayland handle arrives there.
pub fn focused() -> Option<WindowIdentifier> {
    let context = gtk::glib::MainContext::default();
    if context.is_owner() {
        return None;
    }
    let (tx, rx) = mpsc::channel::<Option<Parent>>();
    context.invoke(move || export_focused(tx));
    match rx.recv_timeout(Duration::from_millis(500)).ok().flatten()? {
        Parent::X11(xid) => Some(WindowIdentifier::from_xid(xid)),
        Parent::Wayland(handle) => Some(WindowIdentifier::from_xdg_foreign_exported(handle)),
    }
}

fn export_focused(tx: Reply) {
    let focused = gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .find(|w| w.is_active() && w.is_visible())
        .and_then(|w| w.window());
    let Some(gdk) = focused else {
        let _ = tx.send(None);
        return;
    };
    let raw: *mut GdkWindow = gdk.to_glib_none().0;
    match gdk.type_().name() {
        "GdkX11Window" => {
            let _ = tx.send(Some(Parent::X11(unsafe { gdk_x11_window_get_xid(raw) })));
        }
        "GdkWaylandWindow" => export_wayland(raw, tx),
        _ => {
            let _ = tx.send(None);
        }
    }
}

/// GTK refuses a second export of the same window, so the handle is kept on the window.
fn export_wayland(raw: *mut GdkWindow, tx: Reply) {
    let kept = unsafe { gtk::glib::gobject_ffi::g_object_get_data(raw.cast(), EXPORTED_KEY.as_ptr()) };
    if !kept.is_null() {
        let handle = unsafe { CStr::from_ptr(kept.cast()) }.to_string_lossy().into_owned();
        let _ = tx.send(Some(Parent::Wayland(handle)));
        return;
    }
    let data = Box::into_raw(Box::new(tx));
    let requested =
        unsafe { gdk_wayland_window_export_handle(raw, exported, data.cast(), Some(free_reply)) };
    if requested == gtk::glib::ffi::GFALSE {
        let tx = unsafe { Box::from_raw(data) };
        let _ = tx.send(None);
    }
}

unsafe extern "C" fn exported(window: *mut GdkWindow, handle: *const c_char, data: *mut c_void) {
    let tx = unsafe { &*data.cast::<Reply>() };
    if handle.is_null() {
        let _ = tx.send(None);
        return;
    }
    unsafe {
        gtk::glib::gobject_ffi::g_object_set_data_full(
            window.cast(),
            EXPORTED_KEY.as_ptr(),
            gtk::glib::ffi::g_strdup(handle).cast(),
            Some(gtk::glib::ffi::g_free),
        );
    }
    let handle = unsafe { CStr::from_ptr(handle) }.to_string_lossy().into_owned();
    let _ = tx.send(Some(Parent::Wayland(handle)));
}

unsafe extern "C" fn free_reply(data: *mut c_void) {
    drop(unsafe { Box::from_raw(data.cast::<Reply>()) });
}
