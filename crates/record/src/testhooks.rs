//! Kill-test hold points (feature `test-hooks`; `SC_TEST_HOLD_AT=<point>`). Without the feature
//! `hold` does nothing.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldPoint {
    AfterStart,
    AfterSwitchBegin,
    AfterSegmentPrepare,
    AfterSegmentOpen,
}

impl HoldPoint {
    pub fn as_str(self) -> &'static str {
        match self {
            HoldPoint::AfterStart => "after_start",
            HoldPoint::AfterSegmentPrepare => "after_segment_prepare",
            HoldPoint::AfterSwitchBegin => "after_switch_begin",
            HoldPoint::AfterSegmentOpen => "after_segment_open",
        }
    }
}

/// Called AT the journaling site, on the thread that journals: when `SC_TEST_HOLD_AT` names
/// `p`, prints `SC_HOLD <point>` to stderr and blocks that thread for 30 s.
pub fn hold(p: HoldPoint) {
    #[cfg(feature = "test-hooks")]
    {
        use std::io::Write;
        if std::env::var("SC_TEST_HOLD_AT").ok().as_deref() == Some(p.as_str()) {
            let mut err = std::io::stderr().lock();
            let _ = writeln!(err, "SC_HOLD {}", p.as_str());
            let _ = err.flush();
            drop(err);
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
    }
    #[cfg(not(feature = "test-hooks"))]
    {
        let _ = p;
    }
}
