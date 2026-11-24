use std::path::{Path, PathBuf};

use sc_platform::{Desktop, Error, Pointer, Result};
use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, CoUninitialize,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalUnlock,
};
use windows::Win32::System::Ole::CF_HDROP;
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    DROPFILES, FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
    FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, SHFILEOPSTRUCTW,
    SHFileOperationW, SIGDN_FILESYSPATH,
};
use windows::core::{BOOL, PCWSTR};

use super::sys::wide;

fn pick(folders: bool) -> Option<PathBuf> {
    unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let r = (|| -> windows::core::Result<PathBuf> {
            let d: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
            let opts = d.GetOptions()?;
            d.SetOptions(
                opts | FOS_FORCEFILESYSTEM
                    | if folders {
                        FOS_PICKFOLDERS
                    } else {
                        Default::default()
                    },
            )?;
            if !folders {
                let name = wide("Images");
                let spec = wide("*.png;*.jpg;*.jpeg");
                d.SetFileTypes(&[COMDLG_FILTERSPEC {
                    pszName: PCWSTR(name.as_ptr()),
                    pszSpec: PCWSTR(spec.as_ptr()),
                }])?;
            }
            d.Show(None)?;
            let item = d.GetResult()?;
            let p = item.GetDisplayName(SIGDN_FILESYSPATH)?;
            let s = p.to_string();
            CoTaskMemFree(Some(p.0 as *const _));
            Ok(PathBuf::from(s.unwrap_or_default()))
        })();
        if init.is_ok() {
            CoUninitialize();
        }
        r.ok().filter(|p| !p.as_os_str().is_empty())
    }
}

fn on_sta(folders: bool, done: Box<dyn FnOnce(Option<PathBuf>) + Send>) {
    let spawned = std::thread::Builder::new()
        .name("file-dialog".into())
        .spawn(move || done(pick(folders)));
    if let Err(e) = spawned {
        tracing::warn!("file dialog thread: {e}");
    }
}

pub struct WinDesktop;

impl Desktop for WinDesktop {
    fn pointer(&self) -> Result<Pointer> {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromPoint};
        use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON,
        };
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut p = POINT::default();
        // SAFETY: plain Win32 queries with valid out-pointers; none needs a permission.
        unsafe {
            GetCursorPos(&mut p).map_err(|e| Error::Other(format!("GetCursorPos: {e}")))?;
            let mon = MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST);
            let (mut dx, mut dy) = (96u32, 96u32);
            let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
            let scale = f64::from(dx.max(1)) / 96.0;
            let down = |vk: i32| (GetAsyncKeyState(vk) as u16 & 0x8000) != 0;
            Ok(Pointer {
                at: sc_geometry::PointF {
                    x: f64::from(p.x) / scale,
                    y: f64::from(p.y) / scale,
                },
                buttons: u8::from(down(i32::from(VK_LBUTTON.0)))
                    | (u8::from(down(i32::from(VK_RBUTTON.0))) << 1),
            })
        }
    }

    fn reveal(&self, path: &Path) -> Result<()> {
        let mut cmd = std::process::Command::new("explorer.exe");
        if path.is_dir() {
            cmd.arg(path);
        } else {
            cmd.arg(format!("/select,{}", path.display()));
        }
        cmd.spawn()?;
        Ok(())
    }

    fn choose_image(&self, done: Box<dyn FnOnce(Option<PathBuf>) + Send>) {
        on_sta(false, done);
    }

    fn choose_folder(&self, done: Box<dyn FnOnce(Option<PathBuf>) + Send>) {
        on_sta(true, done);
    }

    fn trash(&self, path: &Path) -> Result<()> {
        let from: Vec<u16> = path
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain([0, 0])
            .collect();
        let mut op = SHFILEOPSTRUCTW {
            wFunc: FO_DELETE,
            pFrom: windows::core::PCWSTR(from.as_ptr()),
            fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT).0 as u16,
            ..Default::default()
        };
        let r = unsafe { SHFileOperationW(&mut op) };
        if r != 0 || op.fAnyOperationsAborted.as_bool() {
            return Err(Error::Other(format!(
                "move to the Recycle Bin failed ({r})"
            )));
        }
        Ok(())
    }

    fn copy_file(&self, path: &Path) -> Result<()> {
        let w: Vec<u16> = path
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain([0, 0])
            .collect();
        let head = std::mem::size_of::<DROPFILES>();
        unsafe {
            let mem: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, head + w.len() * 2)
                .map_err(|e| Error::Other(format!("clipboard memory: {e}")))?;
            let p = GlobalLock(mem) as *mut u8;
            if p.is_null() {
                return Err(Error::Other("clipboard memory lock".into()));
            }
            let df = p.cast::<DROPFILES>();
            (*df).pFiles = head as u32;
            (*df).fWide = BOOL(1);
            std::ptr::copy_nonoverlapping(w.as_ptr(), p.add(head).cast::<u16>(), w.len());
            let _ = GlobalUnlock(mem);
            OpenClipboard(None).map_err(|e| Error::Other(format!("open clipboard: {e}")))?;
            let _ = EmptyClipboard();
            let r = SetClipboardData(u32::from(CF_HDROP.0), Some(HANDLE(mem.0)));
            let _ = CloseClipboard();
            r.map_err(|e| Error::Other(format!("set clipboard: {e}")))?;
        }
        Ok(())
    }
}
