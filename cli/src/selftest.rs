use std::sync::Arc;

use sc_platform::Platform;

use crate::commands::{CliResult, EXIT_ERROR, EXIT_OK, print_json};

pub fn run(p: Arc<dyn Platform>, json: bool) -> CliResult {
    let o = sc_diagnostics::run(p.clone(), &mut |l| println!("{l}"));
    let dir = crate::host::data_dir()
        .map(|d| d.join("self-test"))
        .unwrap_or_else(|_| std::env::temp_dir());
    match sc_diagnostics::write_report(&o, &dir, &*p) {
        Ok(path) => println!(
            "Report: {} (and .json). Send it with a bug report; it holds no recordings.",
            path.display()
        ),
        Err(e) => eprintln!("could not write the report: {e}"),
    }
    if json {
        print_json(&o.json);
    }
    Ok(if o.failed == 0 { EXIT_OK } else { EXIT_ERROR })
}
