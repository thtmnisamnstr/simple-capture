//! Opening files, web pages, System Settings panes and apps through NSWorkspace, the account's
//! real home folder and security-scoped bookmarks of folders the person picked. A spawned
//! `/usr/bin/open` would not be allowed inside the App Sandbox.

use std::ffi::{CStr, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSString, NSURL};
use sc_platform::{Error, Result};

/// Opens a web page or a system URL such as `x-apple.systempreferences:…` in its handler.
pub fn open_url(url: &str) -> Result<()> {
    let u = NSURL::URLWithString(&NSString::from_str(url))
        .ok_or_else(|| Error::Other(format!("not a URL: {url}")))?;
    open(&u, url)
}

/// Opens a file in its default app.
pub fn open_file(path: &Path) -> Result<()> {
    let u = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    open(&u, &path.display().to_string())
}

/// Launches the app with this bundle identifier; `NotFound` when it isn't installed.
pub fn open_app(bundle_id: &str) -> Result<()> {
    let u = NSWorkspace::sharedWorkspace()
        .URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle_id))
        .ok_or_else(|| Error::NotFound(bundle_id.to_string()))?;
    open(&u, bundle_id)
}

fn open(url: &NSURL, what: &str) -> Result<()> {
    if NSWorkspace::sharedWorkspace().openURL(url) {
        Ok(())
    } else {
        Err(Error::Other(format!("could not open {what}")))
    }
}

/// The home folder from the user database. Inside the App Sandbox `$HOME` is the app's
/// container, so paths shown to the person come from here.
pub fn real_home() -> Result<PathBuf> {
    let missing = || Error::NotFound("this account's home folder".into());
    // SAFETY: sysconf takes a plain constant.
    let hint = unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) };
    let mut len = usize::try_from(hint).unwrap_or(0).max(1024);
    loop {
        let mut buf = vec![0 as libc::c_char; len];
        // SAFETY: an all-zero passwd is a valid out parameter.
        let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
        let mut found: *mut libc::passwd = std::ptr::null_mut();
        // SAFETY: every pointer refers to a live local of the stated size; on success the
        // strings in `pwd` point into `buf`, which outlives their use below.
        let rc = unsafe {
            libc::getpwuid_r(
                libc::getuid(),
                &mut pwd,
                buf.as_mut_ptr(),
                buf.len(),
                &mut found,
            )
        };
        if rc == libc::ERANGE && len < 1 << 20 {
            len *= 4;
            continue;
        }
        if rc != 0 || found.is_null() || pwd.pw_dir.is_null() {
            return Err(missing());
        }
        // SAFETY: checked non-null; NUL-terminated inside `buf`.
        let dir = unsafe { CStr::from_ptr(pwd.pw_dir) };
        let home = PathBuf::from(OsStr::from_bytes(dir.to_bytes()));
        return Some(home).filter(|h| h.is_absolute()).ok_or_else(missing);
    }
}

pub use scoped::{bookmark, start_access};

pub(crate) use scoped::remember_pick;

mod scoped {
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, PoisonError};

    use objc2::runtime::Bool;
    use objc2_foundation::{
        NSData, NSString, NSURL, NSURLBookmarkCreationOptions, NSURLBookmarkResolutionOptions,
    };
    use sc_platform::{Error, Result};

    static PICKED: Mutex<Option<(PathBuf, Vec<u8>)>> = Mutex::new(None);

    /// Folders opened by [`start_access`]. Each stays open until the app quits: Finder, the
    /// pasteboard and the opening app use a revealed, copied or opened file after the call returns.
    static OPEN: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

    fn make(url: &NSURL) -> Result<Vec<u8>> {
        url.bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
            NSURLBookmarkCreationOptions::WithSecurityScope,
            None,
            None,
        )
        .map(|d| d.to_vec())
        .map_err(|e| Error::Other(format!("bookmark: {}", e.localizedDescription())))
    }

    pub(crate) fn remember_pick(url: &NSURL) {
        let Some(path) = url.path().map(|p| PathBuf::from(p.to_string())) else {
            return;
        };
        match make(url) {
            Ok(b) => *PICKED.lock().unwrap_or_else(PoisonError::into_inner) = Some((path, b)),
            Err(e) => tracing::warn!(folder = %path.display(), "{e}"),
        }
    }

    /// An app-scoped, security-scoped bookmark of `folder`, which this process can reach now
    /// (picked in the folder panel this session, or opened with [`start_access`]).
    pub fn bookmark(folder: &Path) -> Result<Vec<u8>> {
        let picked = PICKED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take_if(|(p, _)| p == folder)
            .map(|(_, b)| b);
        if let Some(b) = picked {
            return Ok(b);
        }
        make(&NSURL::fileURLWithPath_isDirectory(
            &NSString::from_str(&folder.to_string_lossy()),
            true,
        ))
    }

    /// Resolves a bookmark from [`bookmark`] and opens its folder to this process until it quits.
    /// Returns the folder's path now and whether the bookmark is stale (make a new one).
    pub fn start_access(bookmark: &[u8]) -> Result<(PathBuf, bool)> {
        let mut stale = Bool::NO;
        // SAFETY: `stale` outlives the call.
        let url = unsafe {
            NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
                &NSData::with_bytes(bookmark),
                NSURLBookmarkResolutionOptions::WithSecurityScope
                    | NSURLBookmarkResolutionOptions::WithoutUI,
                None,
                &mut stale,
            )
        }
        .map_err(|e| Error::Other(format!("bookmark: {}", e.localizedDescription())))?;
        let path = url
            .path()
            .map(|p| PathBuf::from(p.to_string()))
            .ok_or_else(|| Error::Other("bookmark: not a file URL".into()))?;
        let mut open = OPEN.lock().unwrap_or_else(PoisonError::into_inner);
        if !open.contains(&path) {
            // SAFETY: a URL resolved with a security scope; never stopped (see `OPEN`).
            if !unsafe { url.startAccessingSecurityScopedResource() } {
                return Err(Error::Other(format!(
                    "no access to {} any more",
                    path.display()
                )));
            }
            open.push(path.clone());
        }
        Ok((path, stale.as_bool()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_home_is_an_existing_folder() {
        let h = real_home().expect("home");
        assert!(h.is_absolute() && h.is_dir(), "{}", h.display());
    }

    #[test]
    fn a_missing_app_is_not_found() {
        assert!(matches!(
            open_app("com.simplecapture.no-such-app"),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn a_bookmark_resolves_to_its_folder() {
        let dir = std::env::temp_dir().join(format!("sc-bookmark-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let dir = dir.canonicalize().expect("canonical");
        let b = bookmark(&dir).expect("bookmark");
        let (path, stale) = start_access(&b).expect("resolve");
        assert_eq!(path, dir);
        assert!(!stale);
        let _ = std::fs::remove_dir(&dir);
    }
}
