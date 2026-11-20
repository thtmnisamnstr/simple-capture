//! `sc-devtools`: developer tools for live tests. Never shipped.
//! Exit codes: 0 ok, 1 error, 2 usage, 3 environment not suitable.

mod envcheck;
mod killtest;
mod soak;
mod sync;
mod testapps;

use std::process::ExitCode;

const USAGE: &str = "usage: sc-devtools <envcheck|testapps|sync|soak|killtest> [args...]";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(cmd) = args.next() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let rest: Vec<String> = args.collect();
    let code = match cmd.as_str() {
        "envcheck" => envcheck::run(&rest),
        "testapps" => testapps::run(&rest),
        "sync" => sync::run(&rest),
        "soak" => soak::run(&rest),
        "killtest" => killtest::run(&rest),
        "-h" | "--help" => {
            println!("{USAGE}");
            0
        }
        other => {
            eprintln!("sc-devtools: unknown command {other:?}\n{USAGE}");
            2
        }
    };
    ExitCode::from(code)
}
