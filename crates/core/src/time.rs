//! Clock model (ARCHITECTURE §5). All values are i64 nanoseconds.

use serde::{Deserialize, Serialize};

/// The OS host clock a recording is stamped with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockKind {
    MachHost,
    Qpc,
    ClockMonotonic,
}

/// Host clock time.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct HostTime(pub i64);

/// Recording time: host time minus start minus pauses.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct RecTime(pub i64);

/// Edited-timeline time.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct TlTime(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PauseSpan {
    pub start: HostTime,
    pub end: Option<HostTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecClock {
    pub start: HostTime,
    pub pauses: Vec<PauseSpan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToRec {
    BeforeStart,
    Paused,
    At(RecTime),
}

impl RecClock {
    /// `host − start − (pause time before host)`. A pause covers `[start, end)`; an open pause
    /// (`end: None`) covers everything from its start on.
    pub fn to_rec(&self, h: HostTime) -> ToRec {
        if h < self.start {
            return ToRec::BeforeStart;
        }
        let paused = self
            .pauses
            .iter()
            .any(|p| p.start <= h && p.end.is_none_or(|e| h < e));
        if paused {
            return ToRec::Paused;
        }
        ToRec::At(RecTime(
            h.0.saturating_sub(self.start.0)
                .saturating_sub(self.pause_total_before(h)),
        ))
    }

    /// Total paused nanoseconds in `[start, h)`. Pause parts before `start` are not counted.
    pub fn pause_total_before(&self, h: HostTime) -> i64 {
        self.pauses
            .iter()
            .map(|p| {
                let from = p.start.max(self.start);
                let to = p.end.map_or(h, |e| e.min(h));
                to.0.saturating_sub(from.0).max(0)
            })
            .fold(0i64, i64::saturating_add)
    }
}

/// Host clock now in nanoseconds, on the same timeline as the capture backends
/// (`mach_absolute_time` on macOS, `CLOCK_MONOTONIC` on Linux, QPC on Windows).
pub(crate) fn host_now_ns() -> i64 {
    #[cfg(target_vendor = "apple")]
    {
        mach::now_ns()
    }
    #[cfg(all(unix, not(target_vendor = "apple")))]
    #[allow(clippy::useless_conversion)] // time_t / c_long are i32 on 32-bit targets
    {
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: `ts` is a valid, writable timespec.
        let rc = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
        if rc != 0 {
            return 0;
        }
        i64::from(ts.tv_sec)
            .saturating_mul(1_000_000_000)
            .saturating_add(i64::from(ts.tv_nsec))
    }
    #[cfg(windows)]
    {
        win::qpc_ns()
    }
    #[cfg(not(any(unix, windows)))]
    {
        0
    }
}

#[cfg(target_vendor = "apple")]
#[allow(deprecated)]
mod mach {
    use std::sync::OnceLock;

    fn timebase() -> (u32, u32) {
        static TB: OnceLock<(u32, u32)> = OnceLock::new();
        *TB.get_or_init(|| {
            let mut tb = libc::mach_timebase_info { numer: 0, denom: 0 };
            // SAFETY: `tb` is a valid, writable mach_timebase_info.
            let rc = unsafe { libc::mach_timebase_info(&mut tb) };
            if rc != 0 || tb.denom == 0 {
                (1, 1)
            } else {
                (tb.numer, tb.denom)
            }
        })
    }

    pub(super) fn now_ns() -> i64 {
        // SAFETY: no preconditions.
        let t = unsafe { libc::mach_absolute_time() };
        let (n, d) = timebase();
        i64::try_from(u128::from(t) * u128::from(n) / u128::from(d)).unwrap_or(i64::MAX)
    }
}

#[cfg(windows)]
pub(crate) mod win {
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub(crate) struct SystemTime {
        pub year: u16,
        pub month: u16,
        pub day_of_week: u16,
        pub day: u16,
        pub hour: u16,
        pub minute: u16,
        pub second: u16,
        pub milliseconds: u16,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn QueryPerformanceCounter(count: *mut i64) -> i32;
        fn QueryPerformanceFrequency(freq: *mut i64) -> i32;
        fn GetLocalTime(st: *mut SystemTime);
        fn GetSystemTime(st: *mut SystemTime);
    }

    pub(crate) fn qpc_ns() -> i64 {
        let (mut c, mut f) = (0i64, 0i64);
        // SAFETY: both pointers are valid, writable i64s.
        let ok = unsafe {
            QueryPerformanceCounter(&mut c) != 0 && QueryPerformanceFrequency(&mut f) != 0
        };
        if !ok || f <= 0 {
            return 0;
        }
        let ns = i128::from(c) * 1_000_000_000 / i128::from(f);
        i64::try_from(ns).unwrap_or(i64::MAX)
    }

    pub(crate) fn local_and_utc() -> (SystemTime, SystemTime) {
        let (mut l, mut u) = (SystemTime::default(), SystemTime::default());
        // SAFETY: both pointers are valid, writable SYSTEMTIME structs.
        unsafe {
            GetLocalTime(&mut l);
            GetSystemTime(&mut u);
        }
        (l, u)
    }
}
