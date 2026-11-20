//! Live sck tests. Bodies run under the dev host via `just test-live simple-capture-capture-macos sck_`.

#[cfg(target_os = "macos")]
#[path = "../support/live_runner.rs"]
mod live_runner;

#[cfg(target_os = "macos")]
fn main() {
    live_runner::main(&[])
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("SKIPPED: macOS only");
    println!();
    println!(
        "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s"
    );
}
