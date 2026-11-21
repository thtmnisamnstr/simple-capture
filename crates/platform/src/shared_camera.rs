//! One camera session shared by the live preview (bubble, stage) and a recording (M2_PLAN §3.1,
//! ADR-041). `SharedCamera` is a `CameraSource`, so a recording session that asks for the camera
//! attaches to the session the preview already holds instead of opening the device again.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::Duration;

use sc_core::ClockKind;
use sc_core::folder::LocalDateTime;
use sc_core::time::HostTime;

use crate::{
    AudioBuffer, AudioSource, AudioTrackKind, CameraLimit, CameraSource, Capabilities,
    CaptureDevice, Decoder, Desktop, DeviceStream, Error, FrameSink, OverlayWindows, Permissions,
    Platform, Result, ScreenSource, SourceEvent, SourcePicker, SystemAudioSource, VideoFrame,
    WriterFactory,
};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

#[derive(Default)]
struct Fanout {
    latest: Mutex<Option<VideoFrame>>,
    fresh: Condvar,
    recorder: Mutex<Option<Arc<dyn FrameSink>>>,
    frames: AtomicU64,
}

impl FrameSink for Fanout {
    fn on_screen(&self, _f: VideoFrame) {}

    fn on_camera(&self, f: VideoFrame) {
        let recorder = lock(&self.recorder).clone();
        if let Some(r) = recorder {
            r.on_camera(f.clone());
        }
        {
            let mut latest = lock(&self.latest);
            *latest = Some(f);
            self.frames.fetch_add(1, Ordering::Relaxed);
        }
        self.fresh.notify_all();
    }

    fn on_audio(&self, _t: AudioTrackKind, _a: AudioBuffer) {}

    fn on_event(&self, e: SourceEvent) {
        let recorder = lock(&self.recorder).clone();
        if let Some(r) = recorder {
            r.on_event(e);
        }
    }
}

struct Open {
    device: String,
    stream: Box<dyn DeviceStream>,
    format: String,
}

#[derive(Default)]
struct State {
    open: Option<Open>,
    preview: Option<String>,
    recording: bool,
}

pub struct SharedCamera {
    platform: Arc<dyn Platform>,
    me: Weak<SharedCamera>,
    state: Mutex<State>,
    fan: Arc<Fanout>,
}

impl SharedCamera {
    pub fn new(platform: Arc<dyn Platform>) -> Arc<SharedCamera> {
        Arc::new_cyclic(|me| SharedCamera {
            platform,
            me: me.clone(),
            state: Mutex::new(State::default()),
            fan: Arc::new(Fanout::default()),
        })
    }

    /// The device the preview wants open, or `None` to release it. While a recording holds the
    /// camera, the device cannot change.
    pub fn set_preview(&self, device: Option<String>) -> Result<()> {
        let mut st = lock(&self.state);
        if st.recording
            && device.is_some()
            && st.open.as_ref().map(|o| &o.device) != device.as_ref()
        {
            return Err(Error::Busy("the recording is using another camera".into()));
        }
        st.preview = device;
        self.settle(&mut st)
    }

    pub fn latest(&self) -> Option<VideoFrame> {
        lock(&self.fan.latest).clone()
    }

    pub fn frames_seen(&self) -> u64 {
        self.fan.frames.load(Ordering::Relaxed)
    }

    /// Returns once more than `seen` frames have arrived, or after `timeout`.
    pub fn wait_newer(&self, seen: u64, timeout: Duration) {
        let latest = lock(&self.fan.latest);
        if self.fan.frames.load(Ordering::Relaxed) == seen {
            let _ = self.fan.fresh.wait_timeout(latest, timeout);
        }
    }

    pub fn open_device(&self) -> Option<String> {
        lock(&self.state).open.as_ref().map(|o| o.device.clone())
    }

    fn settle(&self, st: &mut State) -> Result<()> {
        let want = if st.recording {
            st.open.as_ref().map(|o| o.device.clone())
        } else {
            st.preview.clone()
        };
        if st.open.as_ref().map(|o| &o.device) == want.as_ref() {
            return Ok(());
        }
        self.close(st);
        if let Some(device) = want {
            st.open = Some(self.open(&device, CameraLimit::default())?);
        }
        Ok(())
    }

    fn close(&self, st: &mut State) {
        if let Some(old) = st.open.take() {
            *lock(&self.fan.latest) = None;
            if let Err(e) = old.stream.stop() {
                tracing::warn!(device = old.device, "camera stop: {e}");
            }
        }
    }

    fn open(&self, device: &str, limit: CameraLimit) -> Result<Open> {
        let sink: Arc<dyn FrameSink> = self.fan.clone();
        let stream = self.platform.camera().start(device, limit, sink)?;
        let format = stream.format();
        tracing::info!(device, format, "camera opened");
        Ok(Open {
            device: device.to_string(),
            stream,
            format,
        })
    }

    fn detach(&self) {
        let mut st = lock(&self.state);
        *lock(&self.fan.recorder) = None;
        st.recording = false;
        if let Err(e) = self.settle(&mut st) {
            tracing::warn!("camera after recording: {e}");
        }
    }
}

struct Attachment {
    cam: Arc<SharedCamera>,
    format: String,
}

impl DeviceStream for Attachment {
    fn format(&self) -> String {
        self.format.clone()
    }

    fn stop(self: Box<Self>) -> Result<()> {
        self.cam.detach();
        Ok(())
    }
}

impl CameraSource for SharedCamera {
    fn devices(&self) -> Result<Vec<CaptureDevice>> {
        self.platform.camera().devices()
    }

    fn show_video_effects(&self) -> Result<()> {
        self.platform.camera().show_video_effects()
    }

    fn start(
        &self,
        device_id: &str,
        limit: CameraLimit,
        sink: Arc<dyn FrameSink>,
    ) -> Result<Box<dyn DeviceStream>> {
        let cam = self
            .me
            .upgrade()
            .ok_or_else(|| Error::Other("shared camera dropped".into()))?;
        let mut st = lock(&self.state);
        if st.recording {
            return Err(Error::Busy("the camera is already recording".into()));
        }
        if st.open.as_ref().map(|o| o.device.as_str()) != Some(device_id) {
            self.close(&mut st);
            st.open = Some(self.open(device_id, limit)?);
        }
        *lock(&self.fan.recorder) = Some(sink);
        st.recording = true;
        let format = st
            .open
            .as_ref()
            .map(|o| o.format.clone())
            .unwrap_or_default();
        Ok(Box::new(Attachment { cam, format }))
    }
}

pub struct WithSharedCamera {
    inner: Arc<dyn Platform>,
    camera: Arc<SharedCamera>,
}

impl WithSharedCamera {
    pub fn new(inner: Arc<dyn Platform>) -> (Arc<WithSharedCamera>, Arc<SharedCamera>) {
        let camera = SharedCamera::new(inner.clone());
        (
            Arc::new(WithSharedCamera {
                inner,
                camera: camera.clone(),
            }),
            camera,
        )
    }
}

impl Platform for WithSharedCamera {
    fn name(&self) -> &'static str {
        self.inner.name()
    }
    fn capabilities(&self) -> Capabilities {
        self.inner.capabilities()
    }
    fn clock_kind(&self) -> ClockKind {
        self.inner.clock_kind()
    }
    fn host_now(&self) -> HostTime {
        self.inner.host_now()
    }
    fn host_duration(&self, ns: i64) -> std::time::Duration {
        self.inner.host_duration(ns)
    }
    fn local_now(&self) -> LocalDateTime {
        self.inner.local_now()
    }
    fn screen(&self) -> &dyn ScreenSource {
        self.inner.screen()
    }
    fn system_audio(&self) -> &dyn SystemAudioSource {
        self.inner.system_audio()
    }
    fn mic(&self) -> &dyn AudioSource {
        self.inner.mic()
    }
    fn camera(&self) -> &dyn CameraSource {
        &*self.camera
    }
    fn picker(&self) -> &dyn SourcePicker {
        self.inner.picker()
    }
    fn writers(&self) -> Arc<dyn WriterFactory> {
        self.inner.writers()
    }
    fn decoder(&self) -> Arc<dyn Decoder> {
        self.inner.decoder()
    }
    fn permissions(&self) -> &dyn Permissions {
        self.inner.permissions()
    }
    fn overlays(&self) -> &dyn OverlayWindows {
        self.inner.overlays()
    }
    fn desktop(&self) -> &dyn Desktop {
        self.inner.desktop()
    }
    fn watch_system(&self, sink: Arc<dyn FrameSink>) -> Result<Box<dyn DeviceStream>> {
        self.inner.watch_system(sink)
    }
    fn free_disk_bytes(&self, path: &Path) -> Result<u64> {
        self.inner.free_disk_bytes(path)
    }
    fn default_projects_dir(&self) -> Result<PathBuf> {
        self.inner.default_projects_dir()
    }
    fn permission_host(&self) -> Option<String> {
        self.inner.permission_host()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CpuNv12Frame, FrameBuffer, StreamTag};
    use sc_geometry::SizePx;
    use std::time::Instant;

    fn frame() -> VideoFrame {
        let size = SizePx { w: 4, h: 4 };
        VideoFrame {
            stream: StreamTag(1),
            pts: HostTime(1),
            size,
            content_rect_pt: None,
            content_scale: None,
            buffer: FrameBuffer(Arc::new(CpuNv12Frame::black(size, false))),
        }
    }

    #[test]
    fn wait_newer_wakes_on_a_frame_and_times_out_without_one() {
        let fan = Arc::new(Fanout::default());
        let cam = SharedCamera {
            platform: crate::synthetic::SyntheticPlatform::new(crate::synthetic::Scenario::new()),
            me: Weak::new(),
            state: Mutex::new(State::default()),
            fan: fan.clone(),
        };
        let t = Instant::now();
        cam.wait_newer(0, Duration::from_millis(50));
        assert!(t.elapsed() >= Duration::from_millis(45));

        let seen = cam.frames_seen();
        let producer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            fan.on_camera(frame());
        });
        let t = Instant::now();
        cam.wait_newer(seen, Duration::from_secs(5));
        assert!(t.elapsed() < Duration::from_secs(2));
        assert_eq!(cam.frames_seen(), seen + 1);
        producer.join().unwrap();

        let t = Instant::now();
        cam.wait_newer(0, Duration::from_secs(5));
        assert!(t.elapsed() < Duration::from_millis(100));
    }
}
