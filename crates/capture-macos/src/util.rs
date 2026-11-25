//! Time, CMTime and Foundation helpers. Waiting helpers never pump a run loop (plan §4.3
//! threading contract); `mainloop` does that.

use std::sync::OnceLock;
use std::sync::mpsc;
use std::time::Duration;

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_core_foundation::CFString;
use objc2_core_media::{CMTime, CMTimeFlags};
use objc2_foundation::{NSDictionary, NSError, NSNumber, NSString};
use sc_core::time::HostTime;
use sc_platform::{Error, Result};

/// mach timebase (numer, denom); 125/3 on Apple silicon.
#[allow(deprecated)]
pub(crate) fn timebase() -> (u32, u32) {
    static TB: OnceLock<(u32, u32)> = OnceLock::new();
    *TB.get_or_init(|| {
        let mut tb = libc::mach_timebase_info { numer: 0, denom: 0 };
        // SAFETY: mach_timebase_info writes into the struct we pass.
        let rc = unsafe { libc::mach_timebase_info(&mut tb) };
        if rc != 0 || tb.denom == 0 {
            (1, 1)
        } else {
            (tb.numer, tb.denom)
        }
    })
}

#[allow(deprecated)]
pub(crate) fn mach_now() -> u64 {
    // SAFETY: no preconditions.
    unsafe { libc::mach_absolute_time() }
}

pub(crate) fn mach_to_ns(t: u64) -> i64 {
    let (n, d) = timebase();
    let ns = u128::from(t) * u128::from(n) / u128::from(d);
    i64::try_from(ns).unwrap_or(i64::MAX)
}

#[cfg(test)]
pub(crate) fn ns_to_mach(ns: i64) -> u64 {
    let (n, d) = timebase();
    let t = u128::from(ns.max(0) as u64) * u128::from(d) / u128::from(n);
    u64::try_from(t).unwrap_or(u64::MAX)
}

pub(crate) fn host_now() -> HostTime {
    HostTime(mach_to_ns(mach_now()))
}

/// CMClock host time clock now (same timeline as `mach_now`).
#[cfg(test)]
pub(crate) fn host_clock_now() -> CMTime {
    // SAFETY: the host time clock is a process-wide singleton.
    unsafe { objc2_core_media::CMClock::host_time_clock().time() }
}

pub(crate) fn cmtime_valid(t: &CMTime) -> bool {
    t.flags.contains(CMTimeFlags::Valid)
}

/// Nanoseconds of a valid, numeric CMTime.
pub(crate) fn cmtime_to_ns(t: CMTime) -> Option<i64> {
    if !cmtime_valid(&t) || t.timescale <= 0 {
        return None;
    }
    let ns = i128::from(t.value) * 1_000_000_000 / i128::from(t.timescale);
    i64::try_from(ns).ok()
}

pub(crate) fn ns_to_cmtime(ns: i64) -> CMTime {
    // SAFETY: CMTimeMake has no preconditions.
    unsafe { CMTime::new(ns, 1_000_000_000) }
}

pub(crate) fn nsstr(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

pub(crate) fn cfstr_as_ns(s: &CFString) -> &NSString {
    // SAFETY: CFString and NSString are toll-free bridged.
    unsafe { &*(s as *const CFString as *const NSString) }
}

pub(crate) fn dict(
    pairs: &[(&NSString, &AnyObject)],
) -> Retained<NSDictionary<NSString, AnyObject>> {
    let keys: Vec<&NSString> = pairs.iter().map(|p| p.0).collect();
    let vals: Vec<&AnyObject> = pairs.iter().map(|p| p.1).collect();
    NSDictionary::from_slices(&keys, &vals)
}

pub(crate) fn num_i(v: i64) -> Retained<NSNumber> {
    NSNumber::new_i64(v)
}

pub(crate) fn num_f(v: f64) -> Retained<NSNumber> {
    NSNumber::new_f64(v)
}

pub(crate) fn num_b(v: bool) -> Retained<NSNumber> {
    NSNumber::new_bool(v)
}

pub(crate) fn as_obj<T: objc2::Message>(r: &T) -> &AnyObject {
    // SAFETY: every objc2 Message type is an Objective-C object.
    unsafe { &*(r as *const T as *const AnyObject) }
}

pub(crate) fn fourcc(v: u32) -> String {
    v.to_be_bytes()
        .iter()
        .map(|c| {
            if c.is_ascii_graphic() {
                *c as char
            } else {
                '?'
            }
        })
        .collect()
}

pub(crate) fn ns_error_text(e: &NSError) -> String {
    format!("{} (code {})", e.localizedDescription(), e.code())
}

/// Runs `f`, turning an Objective-C exception into an error. An exception that reaches Rust
/// aborts the process.
pub(crate) fn objc_try<R>(what: &str, f: impl FnOnce() -> R) -> Result<R> {
    objc2::exception::catch(std::panic::AssertUnwindSafe(f)).map_err(|e| {
        let text = e.map_or_else(|| "nil exception".to_string(), |e| format!("{e:?}"));
        tracing::warn!(what, exception = %text, "Objective-C exception");
        Error::Other(format!("{what}: {text}"))
    })
}

/// Waits on a channel WITHOUT pumping a run loop.
pub(crate) fn wait_on<T>(rx: &mpsc::Receiver<T>, timeout: Duration, what: &str) -> Result<T> {
    match rx.recv_timeout(timeout) {
        Ok(v) => Ok(v),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(Error::Timeout(what.to_string())),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(Error::Other(format!("{what}: completion handler dropped")))
        }
    }
}

/// Calls `f` with an `NSError*` completion block and waits (no pumping) for it.
pub(crate) fn wait_err(
    f: impl FnOnce(&DynBlock<dyn Fn(*mut NSError)>),
    what: &str,
    timeout: Duration,
) -> Result<()> {
    let (tx, rx) = mpsc::channel::<Option<String>>();
    let blk = RcBlock::new(move |e: *mut NSError| {
        // SAFETY: the framework passes either null or a valid NSError.
        let m = unsafe { e.as_ref() }.map(ns_error_text);
        let _ = tx.send(m);
    });
    f(&blk);
    match wait_on(&rx, timeout, what)? {
        None => Ok(()),
        Some(e) => Err(Error::Other(format!("{what}: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objc_try_turns_an_exception_into_an_error() {
        let empty = objc2_foundation::NSArray::<NSString>::new();
        let r = objc_try("reading past the end", || {
            // SAFETY: deliberately out of range; the exception is caught.
            let _: *mut AnyObject = unsafe { objc2::msg_send![&*empty, objectAtIndex: 3usize] };
        });
        let e = r
            .expect_err("the exception must become an error")
            .to_string();
        assert!(e.contains("NSRangeException"), "{e}");
        assert_eq!(objc_try("no exception", || 7).ok(), Some(7));
    }

    #[test]
    fn mach_ns_round_trip() {
        let t = mach_now();
        let ns = mach_to_ns(t);
        let back = ns_to_mach(ns);
        assert!(back.abs_diff(t) <= 1);
    }

    #[test]
    fn cmtime_ns_round_trip() {
        assert_eq!(
            cmtime_to_ns(ns_to_cmtime(1_234_567_890)),
            Some(1_234_567_890)
        );
    }

    #[test]
    fn host_clock_matches_mach() {
        let a = host_now().0;
        let c = cmtime_to_ns(host_clock_now()).expect("valid host time");
        let b = host_now().0;
        assert!(c >= a - 1_000_000 && c <= b + 1_000_000, "{a} {c} {b}");
    }
}
