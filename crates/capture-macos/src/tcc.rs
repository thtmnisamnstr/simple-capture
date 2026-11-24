//! TCC tripwire (plan §2 rule 2). In debug and test builds every TCC-touching call fails unless
//! the process runs under the dev host (`SC_DEV_HOST=1`, set by run.sh) or `SC_LIVE=1`, so plain
//! `cargo test` can never raise a permission prompt.

use sc_platform::{Error, Result};

#[cfg_attr(not(debug_assertions), allow(dead_code))]
fn check(what: &str, live: Option<&str>, dev_host: Option<&str>) -> Result<()> {
    if live == Some("1") || dev_host == Some("1") {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "TCC tripwire: {what}: run through tools/macos-dev-host/run.sh"
        )))
    }
}

/// Call before any TCC-touching API.
pub(crate) fn guard(what: &str) -> Result<()> {
    #[cfg(debug_assertions)]
    {
        let live = std::env::var("SC_LIVE").ok();
        let host = std::env::var("SC_DEV_HOST").ok();
        check(what, live.as_deref(), host.as_deref())
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = what;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tripwire_blocks_without_dev_host() {
        let e = check("SCShareableContent", None, None).expect_err("must refuse");
        assert!(e.to_string().contains("TCC tripwire"), "{e}");
        assert!(check("x", Some("0"), Some("")).is_err());
        assert!(check("x", Some("1"), None).is_ok());
        assert!(check("x", None, Some("1")).is_ok());
    }

    #[test]
    fn guard_blocks_plain_cargo_test() {
        let live = std::env::var("SC_LIVE").ok();
        let host = std::env::var("SC_DEV_HOST").ok();
        if live.as_deref() == Some("1") || host.as_deref() == Some("1") {
            eprintln!("SKIPPED: SC_LIVE or SC_DEV_HOST is set");
            return;
        }
        assert!(guard("SCStream start").is_err());
    }
}
