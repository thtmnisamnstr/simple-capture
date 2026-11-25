use std::ffi::c_void;
use std::sync::Mutex;

use sc_platform::{Error, OverlayRole, OverlayWindows, Result, WindowId};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE};

use super::enumerate::id_of;

#[derive(Default)]
pub struct Overlays {
    ids: Mutex<Vec<WindowId>>,
}

impl OverlayWindows for Overlays {
    unsafe fn configure(&self, raw_window: *mut c_void, role: OverlayRole) -> Result<WindowId> {
        if raw_window.is_null() {
            return Err(Error::Other("no window handle".into()));
        }
        let hwnd = HWND(raw_window);
        if let Err(e) = unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) } {
            tracing::warn!(?role, "exclude from capture: {e}");
        }
        let id = id_of(hwnd);
        let mut ids = self
            .ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !ids.contains(&id) {
            ids.push(id);
        }
        Ok(id)
    }

    fn registered(&self) -> Vec<WindowId> {
        self.ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}
