use std::path::{Path, PathBuf};

use objc2::runtime::ProtocolObject;
use objc2_app_kit::{
    NSColor, NSColorSampler, NSColorSpace, NSModalResponseOK, NSOpenPanel, NSPasteboard,
    NSPasteboardWriting, NSWorkspace,
};
use objc2_foundation::{NSArray, NSString, NSURL};
use sc_platform::{Desktop, Error, Pointer, Result};

#[repr(C)]
#[derive(Clone, Copy)]
struct CgPoint {
    x: f64,
    y: f64,
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventCreate(source: *const std::ffi::c_void) -> *const std::ffi::c_void;
    fn CGEventGetLocation(event: *const std::ffi::c_void) -> CgPoint;
    fn CGEventSourceButtonState(state: i32, button: u32) -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const std::ffi::c_void);
}

/// kCGEventSourceStateCombinedSessionState.
const COMBINED_SESSION: i32 = 0;

use super::glue::main_async;

#[derive(Debug, Default)]
pub struct MacDesktop {
    _priv: (),
}

impl MacDesktop {
    pub fn new() -> MacDesktop {
        MacDesktop::default()
    }
}

impl Desktop for MacDesktop {
    fn pointer(&self) -> Result<Pointer> {
        // SAFETY: CGEventCreate(NULL) returns a fresh event we release; the state queries take
        // plain values. None of these need a permission (no event tap).
        unsafe {
            let e = CGEventCreate(std::ptr::null());
            if e.is_null() {
                return Err(Error::Other("pointer: no event".into()));
            }
            let at = CGEventGetLocation(e);
            CFRelease(e);
            let left = CGEventSourceButtonState(COMBINED_SESSION, 0);
            let right = CGEventSourceButtonState(COMBINED_SESSION, 1);
            Ok(Pointer {
                at: sc_geometry::PointF { x: at.x, y: at.y },
                buttons: u8::from(left) | (u8::from(right) << 1),
            })
        }
    }

    fn trash(&self, path: &Path) -> Result<()> {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        objc2_foundation::NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, None)
            .map_err(|e| Error::Other(format!("move to the Trash: {}", e.localizedDescription())))
    }

    fn reveal(&self, path: &Path) -> Result<()> {
        if !path.exists() {
            return Err(Error::NotFound(path.display().to_string()));
        }
        let p = path.to_string_lossy().into_owned();
        main_async(move |_| {
            let url = NSURL::fileURLWithPath(&NSString::from_str(&p));
            NSWorkspace::sharedWorkspace()
                .activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[url]));
        });
        Ok(())
    }

    fn choose_image(&self, done: Box<dyn FnOnce(Option<PathBuf>) + Send>) {
        main_async(move |mtm| {
            let panel = NSOpenPanel::openPanel(mtm);
            panel.setCanChooseFiles(true);
            panel.setCanChooseDirectories(false);
            panel.setAllowsMultipleSelection(false);
            let types: Vec<_> = ["png", "jpg", "jpeg"]
                .iter()
                .map(|t| NSString::from_str(t))
                .collect();
            #[allow(deprecated)]
            panel.setAllowedFileTypes(Some(&NSArray::from_retained_slice(&types)));
            let cell = std::sync::Mutex::new(Some(done));
            let p2 = panel.clone();
            let blk = block2::RcBlock::new(move |resp: isize| {
                let path = (resp == NSModalResponseOK)
                    .then(|| {
                        p2.URL()
                            .and_then(|u| u.path())
                            .map(|s| PathBuf::from(s.to_string()))
                    })
                    .flatten();
                if let Some(d) = cell.lock().ok().and_then(|mut c| c.take()) {
                    d(path);
                }
            });
            panel.beginWithCompletionHandler(&blk);
        });
    }

    fn choose_folder(&self, done: Box<dyn FnOnce(Option<PathBuf>) + Send>) {
        main_async(move |mtm| {
            let panel = NSOpenPanel::openPanel(mtm);
            panel.setCanChooseFiles(false);
            panel.setCanChooseDirectories(true);
            panel.setCanCreateDirectories(true);
            panel.setAllowsMultipleSelection(false);
            let cell = std::sync::Mutex::new(Some(done));
            let p2 = panel.clone();
            let blk = block2::RcBlock::new(move |resp: isize| {
                let url = (resp == NSModalResponseOK).then(|| p2.URL()).flatten();
                #[cfg(feature = "mas")]
                if let Some(u) = &url {
                    crate::workspace::remember_pick(u);
                }
                let path = url
                    .and_then(|u| u.path())
                    .map(|s| PathBuf::from(s.to_string()));
                if let Some(d) = cell.lock().ok().and_then(|mut c| c.take()) {
                    d(path);
                }
            });
            panel.beginWithCompletionHandler(&blk);
        });
    }

    fn copy_file(&self, path: &Path) -> Result<()> {
        if !path.exists() {
            return Err(Error::NotFound(path.display().to_string()));
        }
        let p = path.to_string_lossy().into_owned();
        main_async(move |_| {
            let url = NSURL::fileURLWithPath(&NSString::from_str(&p));
            let pb = NSPasteboard::generalPasteboard();
            pb.clearContents();
            let item: &ProtocolObject<dyn NSPasteboardWriting> = ProtocolObject::from_ref(&*url);
            pb.writeObjects(&NSArray::from_slice(&[item]));
        });
        Ok(())
    }

    fn pick_color(&self, done: Box<dyn FnOnce(Option<[f64; 4]>) + Send>) {
        main_async(move |_| {
            let sampler = NSColorSampler::new();
            let cell = std::sync::Mutex::new(Some(done));
            let blk = block2::RcBlock::new(move |picked: *mut NSColor| {
                // SAFETY: AppKit passes a valid colour or nil.
                let rgba = unsafe { picked.as_ref() }
                    .and_then(|c| c.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()))
                    .map(|c| {
                        [
                            c.redComponent(),
                            c.greenComponent(),
                            c.blueComponent(),
                            c.alphaComponent(),
                        ]
                    });
                if let Some(d) = cell.lock().ok().and_then(|mut c| c.take()) {
                    d(rgba);
                }
            });
            // SAFETY: the block lives until AppKit calls it once.
            unsafe { sampler.showSamplerWithSelectionHandler(&blk) };
        });
    }
}
