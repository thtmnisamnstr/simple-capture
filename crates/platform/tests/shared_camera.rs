use std::sync::Arc;
use std::time::{Duration, Instant};

use sc_geometry::SizePx;
use sc_platform::synthetic::{
    CAMERA_DEVICE_ID, Collector, Scenario, SyntheticPlatform, VideoScript,
};
use sc_platform::{CameraLimit, Error, Platform, WithSharedCamera};

fn platform() -> (Arc<WithSharedCamera>, Arc<sc_platform::SharedCamera>) {
    let p: Arc<dyn Platform> = SyntheticPlatform::new(Scenario::new().camera(Some(VideoScript {
        size: SizePx { w: 320, h: 180 },
        fps: 30,
    })));
    WithSharedCamera::new(p)
}

fn wait(what: &str, f: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn recording_attaches_to_the_preview_session() {
    let (p, cam) = platform();
    cam.set_preview(Some(CAMERA_DEVICE_ID.into()))
        .expect("preview");
    wait("preview frames", || cam.frames_seen() > 3);
    let preview_tag = cam.latest().expect("latest").stream;

    let rec = Collector::new();
    let att = p
        .camera()
        .start(CAMERA_DEVICE_ID, CameraLimit::default(), rec.clone())
        .expect("attach");
    assert!(!att.format().is_empty());
    wait("recorded frames", || rec.camera().len() > 3);
    assert!(
        rec.camera().iter().all(|f| f.stream == preview_tag),
        "one device session"
    );

    att.stop().expect("detach");
    let n = rec.camera().len();
    let seen = cam.frames_seen();
    wait("preview keeps running", || cam.frames_seen() > seen + 3);
    assert_eq!(rec.camera().len(), n, "detached sink gets nothing more");
    assert_eq!(cam.open_device().as_deref(), Some(CAMERA_DEVICE_ID));

    cam.set_preview(None).expect("release");
    assert_eq!(cam.open_device(), None);
    assert!(cam.latest().is_none());
}

#[test]
fn recording_without_a_preview_opens_and_closes_the_device() {
    let (p, cam) = platform();
    let rec = Collector::new();
    let att = p
        .camera()
        .start(CAMERA_DEVICE_ID, CameraLimit::default(), rec.clone())
        .expect("start");
    assert_eq!(cam.open_device().as_deref(), Some(CAMERA_DEVICE_ID));
    wait("recorded frames", || rec.camera().len() > 3);
    assert!(matches!(
        p.camera()
            .start(CAMERA_DEVICE_ID, CameraLimit::default(), rec.clone()),
        Err(Error::Busy(_))
    ));
    assert!(matches!(
        cam.set_preview(Some("another".into())),
        Err(Error::Busy(_))
    ));
    cam.set_preview(Some(CAMERA_DEVICE_ID.into()))
        .expect("same device is fine");
    att.stop().expect("stop");
    assert_eq!(
        cam.open_device().as_deref(),
        Some(CAMERA_DEVICE_ID),
        "preview keeps it"
    );
    cam.set_preview(None).expect("release");
    assert_eq!(cam.open_device(), None);
}
