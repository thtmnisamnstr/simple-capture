//! `simple-capture`: the headless CLI (M1 plan §5.1). Exit codes: 0 ok, 1 error, 2 usage,
//! 3 unsupported on this OS.

mod args;
mod commands;

use std::process::ExitCode;

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_env("SC_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}

fn main() -> ExitCode {
    init_tracing();
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match args::parse(&argv) {
        Ok(inv) => ExitCode::from(commands::run(inv)),
        Err(e) => {
            eprintln!("simple-capture: {e}");
            eprintln!();
            eprintln!("{}", args::USAGE);
            ExitCode::from(commands::EXIT_USAGE)
        }
    }
}
