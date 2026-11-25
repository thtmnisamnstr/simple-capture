use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use sc_platform::{Desktop, Error, Pointer, Result};
use windows::Win32::Foundation::{ERROR_CANCELLED, HANDLE, HGLOBAL};
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
    DROPFILES, FOF_ALLOWUNDO, FOF_NO_UI, FOFX_RECYCLEONDELETE, FOS_FORCEFILESYSTEM,
    FOS_PICKFOLDERS, FileOpenDialog, FileOperation, IFileOpenDialog, IFileOperation,
    IFileOperationProgressSink, IFileOperationProgressSink_Impl, IShellItem,
    SHCreateItemFromParsingName, SIGDN_FILESYSPATH, TSF_DELETE_RECYCLE_IF_POSSIBLE,
};
use windows::core::{BOOL, ComObject, HRESULT, PCWSTR, Ref, implement};

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

#[implement(IFileOperationProgressSink)]
struct RecycleOnly {
    refused: AtomicBool,
    nuked: AtomicBool,
}

impl IFileOperationProgressSink_Impl for RecycleOnly_Impl {
    fn PreDeleteItem(&self, flags: u32, _: Ref<IShellItem>) -> windows::core::Result<()> {
        // Without this flag the shell is about to delete permanently (no Recycle Bin on the
        // drive, too large for it, or the bin set to delete at once), and FOF_NO_UI would let it.
        if flags & TSF_DELETE_RECYCLE_IF_POSSIBLE.0 as u32 != 0 {
            return Ok(());
        }
        self.refused.store(true, Ordering::SeqCst);
        Err(HRESULT::from_win32(ERROR_CANCELLED.0).into())
    }
    fn StartOperations(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn FinishOperations(&self, _: HRESULT) -> windows::core::Result<()> {
        Ok(())
    }
    fn PreRenameItem(&self, _: u32, _: Ref<IShellItem>, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostRenameItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn PreMoveItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostMoveItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn PreCopyItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostCopyItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostDeleteItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        hr: HRESULT,
        recycled: Ref<IShellItem>,
    ) -> windows::core::Result<()> {
        if hr.is_ok() && recycled.is_null() {
            self.nuked.store(true, Ordering::SeqCst);
        }
        Ok(())
    }
    fn PreNewItem(&self, _: u32, _: Ref<IShellItem>, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostNewItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        _: &PCWSTR,
        _: u32,
        _: HRESULT,
        _: Ref<IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn UpdateProgress(&self, _: u32, _: u32) -> windows::core::Result<()> {
        Ok(())
    }
    fn ResetTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn PauseTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn ResumeTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
}

fn recycle(path: &Path) -> Result<()> {
    let w: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let sink = ComObject::new(RecycleOnly {
        refused: AtomicBool::new(false),
        nuked: AtomicBool::new(false),
    });
    let r = unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let r = (|| -> windows::core::Result<bool> {
            let op: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER)?;
            op.SetOperationFlags(FOF_ALLOWUNDO | FOF_NO_UI | FOFX_RECYCLEONDELETE)?;
            let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(w.as_ptr()), None)?;
            let cookie = op.Advise(&sink.to_interface::<IFileOperationProgressSink>())?;
            let deleted = op
                .DeleteItem(&item, None)
                .and_then(|()| op.PerformOperations());
            let _ = op.Unadvise(cookie);
            deleted?;
            Ok(op.GetAnyOperationsAborted()?.as_bool())
        })();
        if init.is_ok() {
            CoUninitialize();
        }
        r
    };
    if sink.refused.load(Ordering::SeqCst) {
        return Err(Error::Other(
            "This recording can't go to the Recycle Bin (it is too large for it, or the Recycle \
             Bin is off for its drive), so it was not deleted."
                .into(),
        ));
    }
    if sink.nuked.load(Ordering::SeqCst) {
        return Err(Error::Other(
            "Windows deleted this recording permanently instead of moving it to the Recycle Bin."
                .into(),
        ));
    }
    match r {
        Ok(false) => Ok(()),
        Ok(true) => Err(Error::Other("move to the Recycle Bin was cancelled".into())),
        Err(e) => Err(Error::Other(format!("move to the Recycle Bin failed: {e}"))),
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
        let path = std::path::absolute(path)?;
        // IFileOperation needs a single-threaded apartment, and the caller's thread may already
        // be in the multithreaded one.
        std::thread::scope(|s| {
            std::thread::Builder::new()
                .name("recycle-bin".into())
                .spawn_scoped(s, || recycle(&path))
                .map_err(|e| Error::Other(format!("Recycle Bin thread: {e}")))?
                .join()
                .map_err(|_| Error::Other("the Recycle Bin thread panicked".into()))?
        })
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
