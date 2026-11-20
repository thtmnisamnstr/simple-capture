//! Everything OS-specific: the native platform, the main-thread run loop, the data folder and
//! signal handling. Kept in one module so `--no-default-features` (no Skia) checks it per target.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sc_platform::Platform;

use crate::args::PlatformChoice;
use crate::commands::CliError;

pub fn platform(choice: PlatformChoice) -> Result<Arc<dyn Platform>, CliError> {
    match choice {
        PlatformChoice::Synthetic => Ok(sc_platform::synthetic::SyntheticPlatform::new(
            sc_platform::synthetic::Scenario::new(),
        )),
        PlatformChoice::Native => native(),
    }
}

#[cfg(target_os = "macos")]
fn native() -> Result<Arc<dyn Platform>, CliError> {
    Ok(sc_capture_macos::platform())
}

#[cfg(target_os = "windows")]
fn native() -> Result<Arc<dyn Platform>, CliError> {
    Ok(sc_capture_windows::platform())
}

#[cfg(target_os = "linux")]
fn native() -> Result<Arc<dyn Platform>, CliError> {
    Ok(sc_capture_linux::platform())
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn native() -> Result<Arc<dyn Platform>, CliError> {
    Err(CliError::unsupported("no native platform on this OS"))
}

pub fn needs_main_loop(choice: PlatformChoice) -> bool {
    cfg!(target_os = "macos") && choice == PlatformChoice::Native
}

/// Runs `work` on a worker thread while the main thread services the run loop (macOS native:
/// picker, overlays and permission requests hop to the main queue), calling `tick` between pumps.
/// Elsewhere `work` runs inline and `tick` is never called.
pub fn run_main<T: Send + 'static>(
    pump: bool,
    work: impl FnOnce() -> T + Send + 'static,
    #[allow(unused_mut, unused_variables)] mut tick: impl FnMut(),
) -> T {
    #[cfg(target_os = "macos")]
    if pump {
        appkit::init_accessory_app();
        let worker = std::thread::Builder::new()
            .name("sc-cli-worker".to_string())
            .spawn(work)
            .expect("spawn the CLI worker thread");
        while !worker.is_finished() {
            sc_capture_macos::mainloop::pump(std::time::Duration::from_millis(10));
            tick();
        }
        return match worker.join() {
            Ok(v) => v,
            Err(panic) => std::panic::resume_unwind(panic),
        };
    }
    let _ = pump;
    work()
}

/// SCK content filters for windows and applications query the window server, which aborts the
/// process (`CGS_REQUIRE_INIT`) unless this process has an AppKit connection. Accessory policy:
/// no Dock icon and never focus.
#[cfg(target_os = "macos")]
mod appkit {
    use std::ffi::{CStr, c_char, c_void};

    type Id = *mut c_void;
    type Sel = *mut c_void;

    #[link(name = "objc")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> Id;
        fn sel_registerName(name: *const c_char) -> Sel;
        fn objc_msgSend();
    }

    #[link(name = "AppKit", kind = "framework")]
    unsafe extern "C" {}

    const NS_APP_ACTIVATION_ACCESSORY: isize = 1;

    fn sel(name: &CStr) -> Sel {
        // SAFETY: registering a selector name is always valid.
        unsafe { sel_registerName(name.as_ptr()) }
    }

    /// Main thread only.
    pub fn init_accessory_app() {
        // SAFETY: `+[NSApplication sharedApplication]` and `-setActivationPolicy:` with their
        // documented signatures, on the main thread; nil is checked before use.
        unsafe {
            let cls = objc_getClass(c"NSApplication".as_ptr());
            if cls.is_null() {
                tracing::warn!("NSApplication class not found");
                return;
            }
            let shared: unsafe extern "C" fn(Id, Sel) -> Id =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            let app = shared(cls, sel(c"sharedApplication"));
            if app.is_null() {
                tracing::warn!("NSApplication sharedApplication returned nil");
                return;
            }
            let set_policy: unsafe extern "C" fn(Id, Sel, isize) -> bool =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            set_policy(
                app,
                sel(c"setActivationPolicy:"),
                NS_APP_ACTIVATION_ACCESSORY,
            );
        }
    }
}

/// The app data folder, shared with the app (Tauri keys it by bundle identifier).
/// `SC_DATA_DIR` overrides it.
#[cfg_attr(not(feature = "export"), allow(dead_code))]
pub fn data_dir() -> Result<PathBuf, CliError> {
    const ID: &str = "com.simplecapture.desktop";
    if let Some(d) = std::env::var_os("SC_DATA_DIR") {
        return Ok(PathBuf::from(d));
    }
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    let base = if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library/Application Support"))
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home().map(|h| h.join(".local/share")))
    };
    base.map(|b| b.join(ID))
        .ok_or_else(|| CliError::error("cannot find the app data folder; set SC_DATA_DIR"))
}

static SIGNALLED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(sig: libc::c_int) {
    SIGNALLED.store(true, Ordering::SeqCst);
    // A second signal gets the default action, so a stuck stop can still be killed.
    // SAFETY: signal(2) is async-signal-safe.
    unsafe {
        libc::signal(sig, libc::SIG_DFL);
    }
}

/// The first SIGINT/SIGTERM only sets the flag `signalled` reports, for a graceful stop.
pub fn install_signal_handlers() {
    let handler = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
    // SAFETY: the handler only stores an atomic and calls signal(2).
    unsafe {
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
    }
}

pub fn signalled() -> bool {
    SIGNALLED.load(Ordering::SeqCst)
}
