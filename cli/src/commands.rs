//! Command bodies. Stubs until wp-cli; every one says so and exits 1.

use crate::args::{Command, Invocation, USAGE};

pub const EXIT_OK: u8 = 0;
pub const EXIT_ERROR: u8 = 1;
pub const EXIT_USAGE: u8 = 2;
pub const EXIT_UNSUPPORTED: u8 = 3;

fn not_yet(name: &str) -> u8 {
    eprintln!("simple-capture {name}: not implemented yet");
    EXIT_ERROR
}

pub fn run(inv: Invocation) -> u8 {
    match inv.command {
        Command::Help => {
            println!("{USAGE}");
            EXIT_OK
        }
        Command::SelfTest => {
            eprintln!("simple-capture --self-test: not implemented until M6");
            EXIT_UNSUPPORTED
        }
        Command::Sources => not_yet("sources"),
        Command::Displays => not_yet("displays"),
        Command::Devices => not_yet("devices"),
        Command::Perms => not_yet("perms"),
        Command::Record(_) => not_yet("record"),
        Command::Recover { .. } => not_yet("recover"),
        Command::Export(_) | Command::Validate(_) | Command::Probe { .. } => export_family(&inv),
    }
}

#[cfg(feature = "export")]
fn export_family(inv: &Invocation) -> u8 {
    match inv.command {
        Command::Export(_) => not_yet("export"),
        Command::Validate(_) => not_yet("validate"),
        _ => not_yet("probe"),
    }
}

#[cfg(not(feature = "export"))]
fn export_family(_inv: &Invocation) -> u8 {
    eprintln!("simple-capture: built without the `export` feature");
    EXIT_UNSUPPORTED
}
