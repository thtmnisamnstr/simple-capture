use std::path::{Path, PathBuf};

use objc2_app_kit::{NSModalResponseOK, NSOpenPanel, NSWorkspace};
use objc2_foundation::{NSArray, NSString, NSURL};
use sc_platform::{Desktop, Error, Result};

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
}
