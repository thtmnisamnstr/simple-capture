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
    let main = Collector::new();
    let second = p
        .camera()
        .start(CAMERA_DEVICE_ID, CameraLimit::default(), main.clone())
        .expect("the same camera as the main picture too");
    wait("frames to both", || main.camera().len() > 3);
    assert!(matches!(
        p.camera()
            .start("another", CameraLimit::default(), Collector::new()),
        Err(Error::Busy(_))
    ));
    second.stop().expect("stop the second");
    let seen = rec.camera().len();
    wait("the first keeps recording", || {
        rec.camera().len() > seen + 2
    });
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

#[test]
fn a_dropped_attachment_releases_the_camera() {
    let (p, cam) = platform();
    let rec = Collector::new();
    let att = p
        .camera()
        .start(CAMERA_DEVICE_ID, CameraLimit::default(), rec.clone())
        .expect("start");
    drop(att);
    let again = p
        .camera()
        .start(CAMERA_DEVICE_ID, CameraLimit::default(), rec.clone())
        .expect("not still recording");
    again.stop().expect("stop");
    cam.set_preview(None).expect("release");
    assert_eq!(cam.open_device(), None, "nothing holds the device");
}

#[test]
fn a_recording_camera_that_fails_to_open_gives_the_preview_back() {
    let (p, cam) = platform();
    cam.set_preview(Some(CAMERA_DEVICE_ID.into()))
        .expect("preview");
    assert!(
        p.camera()
            .start("no-such-camera", CameraLimit::default(), Collector::new())
            .is_err()
    );
    assert_eq!(cam.open_device().as_deref(), Some(CAMERA_DEVICE_ID));
    let seen = cam.frames_seen();
    wait("preview frames again", || cam.frames_seen() > seen + 3);
}

#[test]
fn a_lost_camera_is_closed_and_reopened_on_request() {
    let (p, cam) = platform();
    cam.set_preview(Some(CAMERA_DEVICE_ID.into()))
        .expect("preview");
    wait("preview frames", || cam.frames_seen() > 3);
    cam.device_lost(CAMERA_DEVICE_ID);
    assert_eq!(cam.open_device(), None);
    assert!(cam.latest().is_none(), "no frozen frame");
    cam.set_preview(Some(CAMERA_DEVICE_ID.into()))
        .expect("reopen");
    let seen = cam.frames_seen();
    wait("frames after reopening", || cam.frames_seen() > seen + 3);

    let rec = Collector::new();
    let att = p
        .camera()
        .start(CAMERA_DEVICE_ID, CameraLimit::default(), rec.clone())
        .expect("record");
    cam.device_lost(CAMERA_DEVICE_ID);
    assert_eq!(
        cam.open_device().as_deref(),
        Some(CAMERA_DEVICE_ID),
        "the recording decides when to let go"
    );
    att.stop().expect("stop");
    let seen = cam.frames_seen();
    wait("preview reopened after the recording", || {
        cam.frames_seen() > seen + 3
    });
}
