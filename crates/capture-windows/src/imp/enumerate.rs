//! Displays, windows and apps a person would record. Rects are in physical pixels (the process
//! is per-monitor DPI aware); points are pixels over the monitor's scale.

use std::collections::BTreeMap;

use sc_geometry::{RectPt, SizePx};
use sc_platform::{
    AppInfo, AppRef, DisplayId, DisplayInfo, Error, Result, SourceList, WindowId, WindowInfo,
};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MONITORINFOEXW, MonitorFromWindow,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GA_ROOT, GW_OWNER, GWL_EXSTYLE, GetAncestor, GetWindow, GetWindowLongW,
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, PWSTR};

#[derive(Debug, Clone)]
pub struct Monitor {
    pub hmon: HMONITOR,
    pub id: String,
    pub rect_px: RECT,
    pub scale: f64,
    pub primary: bool,
}

impl Monitor {
    pub fn size_px(&self) -> SizePx {
        SizePx {
            w: (self.rect_px.right - self.rect_px.left).max(0) as u32,
            h: (self.rect_px.bottom - self.rect_px.top).max(0) as u32,
        }
    }

    pub fn frame_pt(&self) -> RectPt {
        let s = self.scale;
        RectPt {
            x: f64::from(self.rect_px.left) / s,
            y: f64::from(self.rect_px.top) / s,
            w: f64::from(self.rect_px.right - self.rect_px.left) / s,
            h: f64::from(self.rect_px.bottom - self.rect_px.top) / s,
        }
    }

    pub fn info(&self, index: usize) -> DisplayInfo {
        DisplayInfo {
            id: DisplayId(self.id.clone()),
            name: if self.primary {
                format!("Display {} (main)", index + 1)
            } else {
                format!("Display {}", index + 1)
            },
            frame_pt: self.frame_pt(),
            scale: self.scale,
            size_px: self.size_px(),
            is_main: self.primary,
        }
    }
}

fn monitor_of(hmon: HMONITOR) -> Option<Monitor> {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    let ok = unsafe { GetMonitorInfoW(hmon, (&raw mut info).cast::<MONITORINFO>()) };
    if !ok.as_bool() {
        return None;
    }
    let (mut dx, mut dy) = (96u32, 96u32);
    let _ = unsafe { GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) };
    let n = info.szDevice.iter().position(|c| *c == 0).unwrap_or(32);
    Some(Monitor {
        hmon,
        id: String::from_utf16_lossy(&info.szDevice[..n]),
        rect_px: info.monitorInfo.rcMonitor,
        scale: f64::from(dx.max(48)) / 96.0,
        primary: info.monitorInfo.dwFlags & 1 == 1,
    })
}

unsafe extern "system" fn on_monitor(hmon: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
    let out = unsafe { &mut *(data.0 as *mut Vec<Monitor>) };
    if let Some(m) = monitor_of(hmon) {
        out.push(m);
    }
    BOOL(1)
}

pub fn monitors() -> Result<Vec<Monitor>> {
    let mut out: Vec<Monitor> = Vec::new();
    let ok = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(on_monitor),
            LPARAM((&raw mut out) as isize),
        )
    };
    if !ok.as_bool() {
        return Err(Error::Other("EnumDisplayMonitors failed".into()));
    }
    out.sort_by_key(|m| (!m.primary, m.rect_px.left, m.rect_px.top));
    Ok(out)
}

pub fn monitor(id: &DisplayId) -> Result<Monitor> {
    monitors()?
        .into_iter()
        .find(|m| m.id == id.0)
        .ok_or_else(|| Error::NotFound(format!("display {}", id.0)))
}

pub fn displays() -> Result<Vec<DisplayInfo>> {
    Ok(monitors()?
        .iter()
        .enumerate()
        .map(|(i, m)| m.info(i))
        .collect())
}

pub fn hwnd_of(id: WindowId) -> HWND {
    HWND(id.0 as i32 as isize as *mut std::ffi::c_void)
}

pub fn id_of(hwnd: HWND) -> WindowId {
    WindowId(hwnd.0 as isize as u32)
}

pub fn process_path(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let r =
            QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        r.ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

fn app_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

pub fn window_rect_px(hwnd: HWND) -> Option<RECT> {
    let mut r = RECT::default();
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut r).cast(),
            std::mem::size_of::<RECT>() as u32,
        )
        .ok()?;
    }
    Some(r)
}

fn cloaked(hwnd: HWND) -> bool {
    let mut c = 0u32;
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            (&raw mut c).cast(),
            std::mem::size_of::<u32>() as u32,
        )
        .is_ok()
            && c != 0
    }
}

pub fn window_pid(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

fn title(hwnd: HWND) -> String {
    let n = unsafe { GetWindowTextLengthW(hwnd) };
    if n <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; n as usize + 1];
    let got = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..got.max(0) as usize])
}

fn recordable(hwnd: HWND, own_pid: u32) -> bool {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return false;
        }
        if GetAncestor(hwnd, GA_ROOT) != hwnd {
            return false;
        }
        if GetWindow(hwnd, GW_OWNER).is_ok_and(|o| !o.is_invalid()) {
            return false;
        }
        if GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
            return false;
        }
    }
    if cloaked(hwnd) || window_pid(hwnd) == own_pid {
        return false;
    }
    window_rect_px(hwnd).is_some_and(|r| r.right - r.left >= 40 && r.bottom - r.top >= 40)
}

unsafe extern "system" fn on_window(hwnd: HWND, data: LPARAM) -> BOOL {
    let out = unsafe { &mut *(data.0 as *mut Vec<HWND>) };
    out.push(hwnd);
    BOOL(1)
}

pub fn top_level_windows() -> Vec<HWND> {
    let mut all: Vec<HWND> = Vec::new();
    let _ = unsafe { EnumWindows(Some(on_window), LPARAM((&raw mut all) as isize)) };
    all
}

pub fn window_info(hwnd: HWND, monitors: &[Monitor], z: u32) -> Option<WindowInfo> {
    let r = window_rect_px(hwnd)?;
    let hmon = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let mon = monitors.iter().find(|m| m.hmon == hmon);
    let scale = mon.map_or(1.0, |m| m.scale);
    let pid = window_pid(hwnd);
    let path = process_path(pid).unwrap_or_default();
    Some(WindowInfo {
        id: id_of(hwnd),
        title: title(hwnd),
        app_name: app_name(&path),
        app: AppRef {
            pid: pid as i32,
            bundle_id: (!path.is_empty()).then(|| path.to_lowercase()),
        },
        frame_pt: RectPt {
            x: f64::from(r.left) / scale,
            y: f64::from(r.top) / scale,
            w: f64::from(r.right - r.left) / scale,
            h: f64::from(r.bottom - r.top) / scale,
        },
        display: mon.map(|m| DisplayId(m.id.clone())),
        on_screen: true,
        layer: 0,
        z_index: z,
    })
}

pub fn list(hide: &[WindowId]) -> Result<SourceList> {
    let monitors = monitors()?;
    let own = unsafe { GetCurrentProcessId() };
    let mut windows = Vec::new();
    for (z, hwnd) in top_level_windows().into_iter().enumerate() {
        if hide.contains(&id_of(hwnd)) || !recordable(hwnd, own) {
            continue;
        }
        if let Some(w) = window_info(hwnd, &monitors, z as u32)
            && !w.title.trim().is_empty()
        {
            windows.push(w);
        }
    }
    let mut apps: BTreeMap<i32, AppInfo> = BTreeMap::new();
    for w in &windows {
        apps.entry(w.app.pid).or_insert_with(|| AppInfo {
            app: w.app.clone(),
            name: w.app_name.clone(),
        });
    }
    Ok(SourceList {
        displays: monitors
            .iter()
            .enumerate()
            .map(|(i, m)| m.info(i))
            .collect(),
        windows,
        apps: apps.into_values().collect(),
    })
}

/// Pixel rects (global) of an app's recordable windows.
pub fn app_window_rects(pid: i32, hide: &[WindowId]) -> Vec<RECT> {
    let own = unsafe { GetCurrentProcessId() };
    top_level_windows()
        .into_iter()
        .filter(|h| {
            window_pid(*h) as i32 == pid && !hide.contains(&id_of(*h)) && recordable(*h, own)
        })
        .filter_map(window_rect_px)
        .collect()
}
