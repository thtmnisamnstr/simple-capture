use std::ffi::c_void;
use std::num::NonZeroIsize;

use raw_window_handle::{RawDisplayHandle, RawWindowHandle, Win32WindowHandle, WindowsDisplayHandle};
use tao::platform::windows::WindowExtWindows;
use tao::window::Window;
use windows::core::{w, Interface, Result, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice2, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Gdi::{
    BitBlt, ClientToScreen, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
    SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, HDC, SRCCOPY,
};
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, RegisterClassExW, SetWindowPos, HWND_BOTTOM, HWND_TOP,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WINDOW_EX_STYLE, WM_ERASEBKGND, WNDCLASSEXW, WS_CHILD,
    WS_CLIPSIBLINGS, WS_VISIBLE,
};

use crate::{CssRect, Mode, PxRect};

pub enum Host {
    Dcomp {
        device: IDCompositionDevice,
        _target: IDCompositionTarget,
        _root: IDCompositionVisual,
        visual: IDCompositionVisual,
    },
    Child {
        hwnd: HWND,
        above: bool,
    },
    Parent {
        hwnd: HWND,
    },
}

fn parent_hwnd(window: &Window) -> HWND {
    HWND(window.hwnd() as *mut c_void)
}

impl Host {
    pub fn new(mode: Mode, window: &Window) -> Result<Self> {
        let parent = parent_hwnd(window);
        unsafe {
            match mode {
                Mode::Dcomp => {
                    let device: IDCompositionDevice = DCompositionCreateDevice2(None)?;
                    let target = device.CreateTargetForHwnd(parent, false)?;
                    let root = device.CreateVisual()?;
                    let visual = device.CreateVisual()?;
                    root.AddVisual(&visual, false, None)?;
                    target.SetRoot(&root)?;
                    device.Commit()?;
                    Ok(Self::Dcomp { device, _target: target, _root: root, visual })
                }
                Mode::ChildBelow | Mode::ChildAbove => {
                    let above = mode == Mode::ChildAbove;
                    Ok(Self::Child { hwnd: create_child(parent, above)?, above })
                }
                Mode::Parent => Ok(Self::Parent { hwnd: parent }),
            }
        }
    }

    pub fn surface_target(&self) -> wgpu::SurfaceTargetUnsafe {
        match self {
            Self::Dcomp { visual, .. } => wgpu::SurfaceTargetUnsafe::CompositionVisual(visual.as_raw()),
            Self::Child { hwnd, .. } | Self::Parent { hwnd } => wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(RawDisplayHandle::Windows(WindowsDisplayHandle::new())),
                raw_window_handle: RawWindowHandle::Win32(Win32WindowHandle::new(
                    NonZeroIsize::new(hwnd.0 as isize).expect("null HWND"),
                )),
            },
        }
    }

    pub fn place(&mut self, r: PxRect) -> Result<()> {
        unsafe {
            match self {
                Self::Dcomp { device, visual, .. } => {
                    visual.SetOffsetX2(r.x as f32)?;
                    visual.SetOffsetY2(r.y as f32)?;
                    device.Commit()
                }
                Self::Child { hwnd, above } => SetWindowPos(
                    *hwnd,
                    Some(if *above { HWND_TOP } else { HWND_BOTTOM }),
                    r.x,
                    r.y,
                    r.w as i32,
                    r.h as i32,
                    SWP_NOACTIVATE,
                ),
                Self::Parent { .. } => Ok(()),
            }
        }
    }
}

unsafe extern "system" fn child_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_ERASEBKGND {
        return LRESULT(1);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

unsafe fn create_child(parent: HWND, above: bool) -> Result<HWND> {
    unsafe {
        let hinst = GetModuleHandleW(None)?;
        let class = w!("SC_NATIVE_PREVIEW_CHILD");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(child_proc),
            hInstance: hinst.into(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let mut style = WS_CHILD | WS_VISIBLE;
        if above {
            style |= WS_CLIPSIBLINGS;
        }
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            PCWSTR::null(),
            style,
            0,
            0,
            1,
            1,
            Some(parent),
            None,
            Some(hinst.into()),
            None,
        )?;
        SetWindowPos(
            hwnd,
            Some(if above { HWND_TOP } else { HWND_BOTTOM }),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )?;
        Ok(hwnd)
    }
}

struct Capture {
    w: i32,
    h: i32,
    bgra: Vec<u8>,
}

impl Capture {
    fn rgb(&self, x: i32, y: i32) -> Option<[u8; 3]> {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return None;
        }
        let i = ((y * self.w + x) * 4) as usize;
        Some([self.bgra[i + 2], self.bgra[i + 1], self.bgra[i]])
    }

    fn write_bmp(&self, path: &std::path::Path) -> std::io::Result<()> {
        let data_len = (self.w * self.h * 4) as u32;
        let mut out = Vec::with_capacity(54 + data_len as usize);
        out.extend_from_slice(b"BM");
        out.extend_from_slice(&(54 + data_len).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&54u32.to_le_bytes());
        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&self.w.to_le_bytes());
        out.extend_from_slice(&(-self.h).to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&data_len.to_le_bytes());
        out.extend_from_slice(&[0u8; 16]);
        out.extend_from_slice(&self.bgra);
        std::fs::write(path, out)
    }
}

unsafe fn capture(w: i32, h: i32, fill: impl FnOnce(HDC) -> bool) -> Option<Capture> {
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let result = match CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(bmp) => {
                let old = SelectObject(mem, bmp.into());
                let ok = fill(mem);
                let bgra = std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize).to_vec();
                SelectObject(mem, old);
                let _ = DeleteObject(bmp.into());
                ok.then_some(Capture { w, h, bgra })
            }
            Err(_) => None,
        };
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        result
    }
}

pub fn probe(window: &Window, mode: Mode, css: CssRect) {
    let hwnd = parent_hwnd(window);
    let mut rc = RECT::default();
    let mut origin = POINT { x: 0, y: 0 };
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
        let _ = ClientToScreen(hwnd, &mut origin);
    }
    let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
    let screen = unsafe {
        capture(w, h, |mem| {
            let src = GetDC(None);
            let ok = BitBlt(mem, 0, 0, w, h, Some(src), origin.x, origin.y, SRCCOPY | CAPTUREBLT).is_ok();
            ReleaseDC(None, src);
            ok
        })
    };
    let printed = unsafe { capture(w, h, |mem| PrintWindow(hwnd, mem, PRINT_WINDOW_FLAGS(1 | 2)).as_bool()) };

    let out_dir = std::env::var("SC_OUT_DIR").unwrap_or_else(|_| ".".into());
    for (name, cap) in [("screen", &screen), ("printwindow", &printed)] {
        if let Some(c) = cap {
            let path = std::path::Path::new(&out_dir).join(format!("probe-{}-{}.bmp", mode.name(), name));
            match c.write_bmp(&path) {
                Ok(()) => eprintln!("[probe] wrote {}", path.display()),
                Err(e) => eprintln!("[probe] could not write {}: {e}", path.display()),
            }
        } else {
            eprintln!("[probe] {name} capture failed");
        }
    }

    let magenta = [255, 0, 255];
    let over_video = mode != Mode::ChildAbove;
    let points: [(&str, f64, f64, [u8; 3]); 5] = [
        ("video, no HTML above", css.x + 0.25 * css.w, css.y + 0.5 * css.h, magenta),
        ("opaque HTML badge over video", css.x + 48.0, css.y + 48.0, if over_video { [0, 200, 0] } else { magenta }),
        (
            "50% blue HTML over video",
            css.x + 0.25 * css.w,
            css.y + 0.875 * css.h,
            if over_video { [128, 0, 255] } else { magenta },
        ),
        ("opaque sidebar", 110.0, css.y + 0.5 * css.h, [40, 40, 48]),
        ("stage margin", css.x - 12.0, css.y + 0.5 * css.h, [24, 24, 28]),
    ];
    let close = |a: [u8; 3], b: [u8; 3]| a.iter().zip(b).all(|(x, y)| (*x as i32 - y as i32).abs() <= 12);
    let mut all = true;
    for (label, x, y, want) in points {
        let (px, py) = ((x * css.dpr).round() as i32, (y * css.dpr).round() as i32);
        let s = screen.as_ref().and_then(|c| c.rgb(px, py));
        let p = printed.as_ref().and_then(|c| c.rgb(px, py));
        let pass = s.map(|v| close(v, want)).unwrap_or(false);
        all &= pass;
        eprintln!(
            "[probe] {:<30} at ({px},{py}) want {:?} screen {:?} printwindow {:?} -> {}",
            label,
            want,
            s,
            p,
            if pass { "PASS" } else { "FAIL" }
        );
    }
    eprintln!("[probe] mode={} verdict={}", mode.name(), if all { "PASS" } else { "FAIL" });
}
