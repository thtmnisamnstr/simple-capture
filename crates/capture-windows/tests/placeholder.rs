use std::path::Path;

use sc_platform::{Capabilities, Error, PermissionKind, PermissionStatus};

#[test]
fn placeholder_is_unsupported_everywhere() {
    let p = sc_capture_windows::platform();
    assert_eq!(p.name(), "windows");
    assert_eq!(p.capabilities(), Capabilities::none());
    assert!(matches!(p.screen().displays(), Err(Error::Unsupported(_))));
    assert!(matches!(p.screen().list(&[]), Err(Error::Unsupported(_))));
    assert!(matches!(p.mic().devices(), Err(Error::Unsupported(_))));
    assert!(matches!(p.camera().devices(), Err(Error::Unsupported(_))));
    assert!(matches!(
        p.default_projects_dir(),
        Err(Error::Unsupported(_))
    ));
    assert!(matches!(
        p.free_disk_bytes(Path::new(".")),
        Err(Error::Unsupported(_))
    ));
    assert!(matches!(
        p.decoder().probe(Path::new("x")),
        Err(Error::Unsupported(_))
    ));
    assert_eq!(
        p.permissions().status(PermissionKind::Screen),
        PermissionStatus::Unknown
    );
    assert!(p.overlays().registered().is_empty());
    assert_eq!(p.permission_host(), None);
}

#[test]
fn placeholder_clock_is_monotonic() {
    let p = sc_capture_windows::platform();
    let a = p.host_now();
    let b = p.host_now();
    assert!(b >= a);
}
