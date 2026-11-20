//! `sc-devtools killtest`: the dev-host kill test (macOS only in M1; see `live.rs`).

#[cfg(target_os = "macos")]
mod live;

#[cfg(target_os = "macos")]
pub fn run(args: &[String]) -> u8 {
    live::run(args)
}

#[cfg(not(target_os = "macos"))]
pub fn run(_args: &[String]) -> u8 {
    println!("SKIPPED: sc-devtools killtest needs macOS in M1");
    3
}
