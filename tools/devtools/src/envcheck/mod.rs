//! `sc-devtools envcheck`: fails with exit 3 unless the screen is unlocked and awake (plan
//! §0.4). Needs no permission: reads the CGSession dictionary and `CGDisplayIsAsleep` only.

pub const NEEDS: &str = "needs an unlocked, awake screen";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenState {
    pub has_session: bool,
    pub locked: bool,
    pub asleep: bool,
}

impl ScreenState {
    pub fn usable(self) -> bool {
        self.has_session && !self.locked && !self.asleep
    }
}

pub fn run(args: &[String]) -> u8 {
    let json = args.iter().any(|a| a == "--json");
    let s = match read() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("sc-devtools envcheck: {e}");
            return 1;
        }
    };
    if json {
        println!(
            "{{\"has_session\":{},\"locked\":{},\"asleep\":{},\"ok\":{}}}",
            s.has_session,
            s.locked,
            s.asleep,
            s.usable()
        );
    }
    if s.usable() {
        if !json {
            println!("envcheck: ok (screen unlocked and awake)");
        }
        0
    } else {
        eprintln!(
            "envcheck: {NEEDS} (session: {}, locked: {}, display asleep: {})",
            s.has_session, s.locked, s.asleep
        );
        3
    }
}

#[cfg(target_os = "macos")]
fn read() -> anyhow::Result<ScreenState> {
    use objc2_core_foundation::{CFBoolean, CFDictionary, CFString, CFType};
    use objc2_core_graphics::{CGDisplayIsAsleep, CGMainDisplayID, CGSessionCopyCurrentDictionary};

    let Some(dict) = CGSessionCopyCurrentDictionary() else {
        return Ok(ScreenState {
            has_session: false,
            locked: true,
            asleep: false,
        });
    };
    // SAFETY: the session dictionary maps CFString keys to CFType values.
    let dict: &CFDictionary<CFString, CFType> = unsafe { dict.cast_unchecked() };
    let key = CFString::from_static_str("CGSSessionScreenIsLocked");
    let locked = dict
        .get(&key)
        .and_then(|v| v.downcast_ref::<CFBoolean>().map(|b| b.as_bool()))
        .unwrap_or(false);
    let asleep = CGDisplayIsAsleep(CGMainDisplayID());
    Ok(ScreenState {
        has_session: true,
        locked,
        asleep,
    })
}

#[cfg(not(target_os = "macos"))]
fn read() -> anyhow::Result<ScreenState> {
    Ok(ScreenState {
        has_session: true,
        locked: false,
        asleep: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usable_needs_all_three() {
        let ok = ScreenState {
            has_session: true,
            locked: false,
            asleep: false,
        };
        assert!(ok.usable());
        assert!(!ScreenState { locked: true, ..ok }.usable());
        assert!(!ScreenState { asleep: true, ..ok }.usable());
        assert!(
            !ScreenState {
                has_session: false,
                ..ok
            }
            .usable()
        );
    }

    #[test]
    fn read_does_not_fail() {
        assert!(read().is_ok());
    }
}
