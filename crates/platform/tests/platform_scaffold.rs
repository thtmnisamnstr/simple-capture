use sc_core::ClockKind;
use sc_platform::synthetic::{Scenario, SyntheticPlatform, display_spec};
use sc_platform::unsupported::UnsupportedPlatform;
use sc_platform::{Error, Platform, SourceSpec};

#[test]
fn unsupported_platform_refuses_everything() {
    let p = UnsupportedPlatform::new("test", ClockKind::ClockMonotonic, "test: unsupported");
    assert!(matches!(p.screen().displays(), Err(Error::Unsupported(_))));
    assert!(!p.capabilities().switch_source);
}

#[test]
fn synthetic_platform_builds() {
    let p = SyntheticPlatform::new(Scenario::new());
    assert_eq!(p.name(), "synthetic");
    assert_eq!(p.writers().extension(), "sctrk");
    match display_spec("a", None) {
        SourceSpec::Display { display } => assert_eq!(display.0, "synthetic:a"),
        other => panic!("unexpected {other:?}"),
    }
}
