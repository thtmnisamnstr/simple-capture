use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::{Arc, OnceLock};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2_app_kit::{
    NSMenuDidBeginTrackingNotification, NSWorkspace, NSWorkspaceDidWakeNotification,
    NSWorkspaceWillSleepNotification,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSNotificationName, NSProcessInfo};
use sc_core::folder::LocalDateTime;
use sc_platform::{
    Capabilities, DeviceStream, Error, ExclusionKind, FrameSink, Result, SourceEvent,
};

use super::glue::SendBox;

/// (major, minor, patch) of the running macOS.
pub fn macos_version() -> (isize, isize, isize) {
    static V: OnceLock<(isize, isize, isize)> = OnceLock::new();
    *V.get_or_init(|| {
        let v = NSProcessInfo::processInfo().operatingSystemVersion();
        (v.majorVersion, v.minorVersion, v.patchVersion)
    })
}

pub fn capabilities() -> Capabilities {
    let (major, _, _) = macos_version();
    Capabilities {
        system_picker: major >= 14,
        application_source: true,
        app_audio: true,
        show_clicks: major >= 15,
        mic_modes: major >= 12,
        switch_source: true,
        exclusion: ExclusionKind::PerWindow,
    }
}

type Token = Retained<ProtocolObject<dyn NSObjectProtocol>>;

struct SystemWatcher {
    tokens: SendBox<Vec<(Retained<NSNotificationCenter>, Token)>>,
}

impl DeviceStream for SystemWatcher {
    fn format(&self) -> String {
        "system: sleep/wake, own menus".to_string()
    }

    fn stop(self: Box<Self>) -> Result<()> {
        for (center, token) in self.tokens.0.iter() {
            unsafe { center.removeObserver(crate::util::as_obj(&**token)) };
        }
        Ok(())
    }
}

fn observe(
    center: &Retained<NSNotificationCenter>,
    name: &NSNotificationName,
    sink: Arc<dyn FrameSink>,
    ev: SourceEvent,
) -> (Retained<NSNotificationCenter>, Token) {
    let blk = RcBlock::new(move |_n: NonNull<NSNotification>| sink.on_event(ev.clone()));
    let token =
        unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &blk) };
    (center.clone(), token)
}

/// Sleep/wake (NSWorkspace) and our own menus opening (`OwnWindowsChanged`). Delivered on the
/// thread that posts them, normally main, so the host must service the main run loop.
pub fn watch_system(sink: Arc<dyn FrameSink>) -> Result<Box<dyn DeviceStream>> {
    let ws = NSWorkspace::sharedWorkspace().notificationCenter();
    let dc = NSNotificationCenter::defaultCenter();
    let tokens = unsafe {
        vec![
            observe(
                &ws,
                NSWorkspaceWillSleepNotification,
                sink.clone(),
                SourceEvent::SystemWillSleep,
            ),
            observe(
                &ws,
                NSWorkspaceDidWakeNotification,
                sink.clone(),
                SourceEvent::SystemDidWake,
            ),
            observe(
                &dc,
                NSMenuDidBeginTrackingNotification,
                sink,
                SourceEvent::OwnWindowsChanged,
            ),
        ]
    };
    Ok(Box::new(SystemWatcher {
        tokens: SendBox(tokens),
    }))
}

/// Bytes available to an unprivileged user on the volume holding `path` (or its nearest
/// existing ancestor).
pub fn free_disk_bytes(path: &Path) -> Result<u64> {
    let mut p = path;
    while !p.exists() {
        p = p
            .parent()
            .ok_or_else(|| Error::NotFound(path.display().to_string()))?;
    }
    let c = std::ffi::CString::new(p.as_os_str().as_encoded_bytes())
        .map_err(|e| Error::Other(format!("free_disk_bytes: {e}")))?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return Err(Error::Io(std::io::Error::last_os_error()));
    }
    Ok(u64::from(st.f_bavail) * st.f_frsize)
}

/// `~/Movies/Simple Capture` (not created here), under the real home folder rather than the
/// sandbox container that `$HOME` names.
pub fn default_projects_dir() -> Result<PathBuf> {
    Ok(crate::workspace::real_home()?
        .join("Movies")
        .join("Simple Capture"))
}

pub fn local_now() -> LocalDateTime {
    let t = unsafe { libc::time(std::ptr::null_mut()) };
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return LocalDateTime::default();
    }
    LocalDateTime {
        y: tm.tm_year + 1900,
        mo: (tm.tm_mon + 1) as u8,
        d: tm.tm_mday as u8,
        h: tm.tm_hour as u8,
        mi: tm.tm_min as u8,
        s: tm.tm_sec.min(59) as u8,
        offset_min: (tm.tm_gmtoff / 60) as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_disk_of_missing_path_uses_ancestor() {
        let p = std::env::temp_dir().join("sc-no-such-dir/a/b");
        let n = free_disk_bytes(&p).expect("statvfs");
        assert!(n > 0);
    }

    #[test]
    fn projects_dir_is_under_movies() {
        let d = default_projects_dir().expect("HOME");
        assert!(d.ends_with("Movies/Simple Capture"), "{}", d.display());
    }

    #[test]
    fn local_now_is_sane() {
        let t = local_now();
        assert!(t.y >= 2024 && (1..=12).contains(&t.mo) && (1..=31).contains(&t.d));
        assert!(t.offset_min.abs() <= 14 * 60);
    }

    #[test]
    fn capabilities_on_this_mac() {
        let c = capabilities();
        assert!(macos_version().0 >= 14);
        assert!(c.system_picker && c.application_source && c.switch_source);
        assert_eq!(c.show_clicks, macos_version().0 >= 15);
    }

    #[test]
    fn watch_system_starts_and_stops() {
        struct Null;
        impl FrameSink for Null {
            fn on_screen(&self, _f: sc_platform::VideoFrame) {}
            fn on_camera(&self, _f: sc_platform::VideoFrame) {}
            fn on_audio(&self, _t: sc_platform::AudioTrackKind, _a: sc_platform::AudioBuffer) {}
            fn on_event(&self, _e: SourceEvent) {}
        }
        let w = watch_system(Arc::new(Null)).expect("watch");
        w.stop().expect("stop");
    }
}
