//! `sc-devtools testapps`: fixture windows and tone players for live tests.
//!
//! ```text
//! testapps window --color C --title T [--size WxH] [--pos X,Y] [--floating] [--animate]
//!                 [--close-on-signal USR1] [--second-window-on-signal USR2] [--minimized]
//! testapps tone --hz F [--window] [--title T] [--pos X,Y]
//! testapps fixture [--out FILE]
//! ```
//! Positions are top-left points of the primary display. Each app prints one JSON `ready` line
//! (pid, title, window id, rect) and runs until SIGTERM, or until its parent exits when started
//! by `fixture`. Windows are non-activating panels shown with `orderFrontRegardless`, in an
//! Accessory app: they never take focus.

#[cfg(target_os = "macos")]
mod mac;

#[cfg(target_os = "macos")]
pub fn run(args: &[String]) -> u8 {
    match mac::run(args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("sc-devtools testapps: {e:#}");
            1
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn run(_args: &[String]) -> u8 {
    eprintln!("sc-devtools testapps: macOS only in M1");
    3
}
