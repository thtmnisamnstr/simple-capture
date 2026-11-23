use objc2_av_foundation::{
    AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio, AVMediaTypeVideo,
};
use objc2_core_graphics::{CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess};
use objc2_foundation::NSString;
use sc_platform::{PermissionKind, PermissionStatus, Permissions};

use super::glue::main_async;
use crate::tcc;

#[derive(Debug, Default)]
pub struct MacPermissions {
    _priv: (),
}

impl MacPermissions {
    pub fn new() -> MacPermissions {
        MacPermissions::default()
    }
}

fn media_type(k: PermissionKind) -> Option<&'static NSString> {
    match k {
        PermissionKind::Camera => unsafe { AVMediaTypeVideo },
        PermissionKind::Microphone => unsafe { AVMediaTypeAudio },
        PermissionKind::Screen => None,
    }
}

fn av_status(k: PermissionKind) -> PermissionStatus {
    let Some(mt) = media_type(k) else {
        return PermissionStatus::Unknown;
    };
    let s = unsafe { AVCaptureDevice::authorizationStatusForMediaType(mt) };
    map_av(s)
}

fn map_av(s: AVAuthorizationStatus) -> PermissionStatus {
    match s {
        AVAuthorizationStatus::Authorized => PermissionStatus::Granted,
        AVAuthorizationStatus::Denied => PermissionStatus::Denied,
        AVAuthorizationStatus::Restricted => PermissionStatus::Restricted,
        AVAuthorizationStatus::NotDetermined => PermissionStatus::NotDetermined,
        _ => PermissionStatus::Unknown,
    }
}

impl Permissions for MacPermissions {
    /// Preflight/status reads only: never prompts. Screen recording cannot tell "denied" from
    /// "never asked", so a missing grant reads as `NotDetermined`.
    fn status(&self, k: PermissionKind) -> PermissionStatus {
        match k {
            PermissionKind::Screen => {
                if CGPreflightScreenCaptureAccess() {
                    PermissionStatus::Granted
                } else {
                    PermissionStatus::NotDetermined
                }
            }
            _ => av_status(k),
        }
    }

    fn request(&self, k: PermissionKind, done: Box<dyn FnOnce(PermissionStatus) + Send>) {
        if tcc::guard("permission request").is_err() {
            done(PermissionStatus::Unknown);
            return;
        }
        match k {
            PermissionKind::Screen => main_async(move |_| {
                let ok = CGRequestScreenCaptureAccess();
                done(if ok {
                    PermissionStatus::Granted
                } else {
                    PermissionStatus::Denied
                });
            }),
            _ => {
                let Some(mt) = media_type(k) else {
                    done(PermissionStatus::Unknown);
                    return;
                };
                let cell = std::sync::Mutex::new(Some(done));
                let blk = block2::RcBlock::new(move |granted: objc2::runtime::Bool| {
                    if let Some(d) = cell.lock().ok().and_then(|mut c| c.take()) {
                        d(if granted.as_bool() {
                            PermissionStatus::Granted
                        } else {
                            PermissionStatus::Denied
                        });
                    }
                });
                unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(mt, &blk) };
            }
        }
    }

    fn settings_url(&self, k: PermissionKind) -> Option<String> {
        let pane = match k {
            PermissionKind::Screen => "Privacy_ScreenCapture",
            PermissionKind::Camera => "Privacy_Camera",
            PermissionKind::Microphone => "Privacy_Microphone",
        };
        Some(format!(
            "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?{pane}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_urls() {
        let p = MacPermissions::new();
        for k in [
            PermissionKind::Screen,
            PermissionKind::Camera,
            PermissionKind::Microphone,
        ] {
            let u = p.settings_url(k).unwrap_or_default();
            assert!(u.starts_with("x-apple.systempreferences:"), "{u}");
        }
    }

    #[test]
    fn status_never_panics_or_prompts() {
        let p = MacPermissions::new();
        let _ = p.status(PermissionKind::Screen);
        let _ = p.status(PermissionKind::Camera);
        let _ = p.status(PermissionKind::Microphone);
    }
}
