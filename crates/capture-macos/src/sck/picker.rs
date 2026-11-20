use std::sync::{Arc, Mutex};
use std::time::Duration;

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, define_class, msg_send, sel};
use objc2_core_graphics::CGMainDisplayID;
use objc2_foundation::{NSArray, NSError, NSNumber};
use objc2_screen_capture_kit::{
    SCContentFilter, SCContentSharingPicker, SCContentSharingPickerConfiguration,
    SCContentSharingPickerMode, SCContentSharingPickerObserver, SCShareableContent,
    SCShareableContentStyle, SCStream,
};
use sc_core::project::SourceDescriptor;
use sc_geometry::RectPt;
use sc_platform::{Error, PickedSource, PickerRequest, Result, SourcePicker, WindowId};

use super::content::responds;
use super::glue::{SendBox, main_async, main_sync};
use super::overlays::registered_windows;
use crate::{tcc, util};

/// The `PickedSource::handle` of picks made here.
pub struct PickedFilter(SendBox<Retained<SCContentFilter>>);

impl PickedFilter {
    pub(crate) fn filter(&self) -> Retained<SCContentFilter> {
        self.0.0.clone()
    }
}

type Done = Box<dyn FnOnce(Result<Option<PickedSource>>) + Send>;
type OnPick = Box<dyn Fn(PickedSource) + Send + Sync>;

#[derive(Default)]
struct PickerShared {
    pending: Mutex<Option<Done>>,
    on_pick: Mutex<Option<OnPick>>,
}

impl PickerShared {
    fn finish(&self, r: Result<Option<PickedSource>>) {
        let pending = self.pending.lock().ok().and_then(|mut p| p.take());
        match (pending, r) {
            (Some(done), r) => done(r),
            (None, Ok(Some(ps))) => {
                if let Ok(cb) = self.on_pick.lock()
                    && let Some(cb) = cb.as_ref()
                {
                    cb(ps);
                }
            }
            (None, _) => {}
        }
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "SCCapturePickerObserver"]
    #[ivars = Arc<PickerShared>]
    struct PickerObserver;

    unsafe impl NSObjectProtocol for PickerObserver {}

    unsafe impl SCContentSharingPickerObserver for PickerObserver {
        #[unsafe(method(contentSharingPicker:didCancelForStream:))]
        fn did_cancel(&self, _p: &SCContentSharingPicker, _s: Option<&SCStream>) {
            self.ivars().finish(Ok(None));
        }

        #[unsafe(method(contentSharingPicker:didUpdateWithFilter:forStream:))]
        fn did_update(&self, _p: &SCContentSharingPicker, f: &SCContentFilter, _s: Option<&SCStream>) {
            self.ivars().finish(Ok(Some(picked_source(f))));
        }

        #[unsafe(method(contentSharingPickerStartDidFailWithError:))]
        fn did_fail(&self, e: &NSError) {
            self.ivars()
                .finish(Err(Error::Other(format!("picker: {}", util::ns_error_text(e)))));
        }
    }
);

static OBSERVER: Mutex<Option<SendBox<Retained<PickerObserver>>>> = Mutex::new(None);

fn modes() -> SCContentSharingPickerMode {
    SCContentSharingPickerMode::SingleWindow
        | SCContentSharingPickerMode::SingleApplication
        | SCContentSharingPickerMode::SingleDisplay
}

/// Modes are set once per launch: `allowedPickerModes` cannot be widened again once narrowed.
fn configure(_mtm: MainThreadMarker, shared: &Arc<PickerShared>, exclude: &[WindowId]) {
    let picker = unsafe { SCContentSharingPicker::sharedPicker() };
    let mut ids: Vec<WindowId> = registered_windows();
    for w in exclude {
        if !ids.contains(w) {
            ids.push(*w);
        }
    }
    let nums: Vec<Retained<NSNumber>> = ids
        .iter()
        .map(|w| NSNumber::new_isize(w.0 as isize))
        .collect();
    unsafe {
        let cfg = SCContentSharingPickerConfiguration::new();
        cfg.setAllowedPickerModes(modes());
        cfg.setAllowsChangingSelectedContent(true);
        cfg.setExcludedWindowIDs(&NSArray::from_retained_slice(&nums));
        picker.setDefaultConfiguration(&cfg);
        picker.setMaximumStreamCount(Some(&NSNumber::new_isize(0)));
    }
    if let Ok(mut o) = OBSERVER.lock()
        && o.is_none()
    {
        let obs = PickerObserver::alloc().set_ivars(shared.clone());
        let obs: Retained<PickerObserver> = unsafe { msg_send![super(obs), init] };
        let proto: &ProtocolObject<dyn SCContentSharingPickerObserver> =
            ProtocolObject::from_ref(&*obs);
        unsafe { picker.addObserver(proto) };
        *o = Some(SendBox(obs));
    }
}

#[derive(Debug, Clone)]
pub struct PickerConfigInfo {
    pub allowed_modes_raw: usize,
    pub allows_changing_selected_content: bool,
    pub excluded_window_ids: Vec<u32>,
    pub maximum_stream_count: Option<isize>,
    pub active: bool,
}

pub struct MacPicker {
    shared: Arc<PickerShared>,
}

impl Default for MacPicker {
    fn default() -> Self {
        MacPicker::new()
    }
}

impl MacPicker {
    pub fn new() -> MacPicker {
        MacPicker {
            shared: Arc::new(PickerShared::default()),
        }
    }

    /// Configures the shared picker (never presents it) and reads the configuration back.
    pub fn configuration_readback(&self, exclude: &[WindowId]) -> Result<PickerConfigInfo> {
        tcc::guard("SCContentSharingPicker")?;
        let shared = self.shared.clone();
        let exclude = exclude.to_vec();
        main_sync("picker configuration", Duration::from_secs(5), move |mtm| {
            configure(mtm, &shared, &exclude);
            let picker = unsafe { SCContentSharingPicker::sharedPicker() };
            unsafe {
                let c = picker.defaultConfiguration();
                PickerConfigInfo {
                    allowed_modes_raw: c.allowedPickerModes().bits(),
                    allows_changing_selected_content: c.allowsChangingSelectedContent(),
                    excluded_window_ids: c
                        .excludedWindowIDs()
                        .iter()
                        .map(|n| n.unsignedIntValue())
                        .collect(),
                    maximum_stream_count: picker.maximumStreamCount().map(|n| n.integerValue()),
                    active: picker.isActive(),
                }
            }
        })
    }

    pub fn expected_modes_raw() -> usize {
        modes().bits()
    }
}

impl SourcePicker for MacPicker {
    fn present(
        &self,
        req: PickerRequest,
        done: Box<dyn FnOnce(Result<Option<PickedSource>>) + Send>,
    ) {
        if let Err(e) = tcc::guard("SCContentSharingPicker present") {
            done(Err(e));
            return;
        }
        let shared = self.shared.clone();
        main_async(move |mtm| {
            configure(mtm, &shared, &req.exclude);
            let previous = shared.pending.lock().ok().and_then(|mut p| p.replace(done));
            if let Some(prev) = previous {
                prev(Ok(None));
            }
            let picker = unsafe { SCContentSharingPicker::sharedPicker() };
            unsafe {
                picker.setActive(true);
                picker.present();
            }
        });
    }

    fn set_observer(&self, on_pick: Option<Box<dyn Fn(PickedSource) + Send + Sync>>) {
        if let Ok(mut o) = self.shared.on_pick.lock() {
            *o = on_pick;
        }
    }

    fn set_active(&self, active: bool) -> Result<()> {
        tcc::guard("SCContentSharingPicker setActive")?;
        let shared = self.shared.clone();
        main_async(move |mtm| {
            if active {
                configure(mtm, &shared, &[]);
            }
            unsafe { SCContentSharingPicker::sharedPicker().setActive(active) };
        });
        Ok(())
    }
}

fn picked_source(f: &SCContentFilter) -> PickedSource {
    let info = unsafe { SCShareableContent::infoForFilter(f) };
    let scale = f64::from(unsafe { info.pointPixelScale() });
    let r = unsafe { info.contentRect() };
    let style = unsafe { info.style() };
    let obj: &objc2::runtime::AnyObject = util::as_obj(f);
    let display_id = if responds(obj, sel!(includedDisplays)) {
        unsafe { f.includedDisplays() }
            .firstObject()
            .map(|d| unsafe { d.displayID() })
    } else {
        None
    }
    .unwrap_or_else(|| CGMainDisplayID())
    .to_string();
    let descriptor = if style == SCShareableContentStyle::Window {
        let w = if responds(obj, sel!(includedWindows)) {
            unsafe { f.includedWindows() }.firstObject()
        } else {
            None
        };
        let app = w.as_ref().and_then(|w| unsafe { w.owningApplication() });
        SourceDescriptor::Window {
            window_id: w.as_ref().map(|w| unsafe { w.windowID() }).unwrap_or(0),
            app_bundle_id: app
                .as_ref()
                .map(|a| unsafe { a.bundleIdentifier() }.to_string())
                .filter(|s| !s.is_empty()),
            app_name: app
                .as_ref()
                .map(|a| unsafe { a.applicationName() }.to_string()),
            window_title: w
                .as_ref()
                .and_then(|w| unsafe { w.title() })
                .map(|t| t.to_string()),
            scale,
        }
    } else if style == SCShareableContentStyle::Application {
        let app = if responds(obj, sel!(includedApplications)) {
            unsafe { f.includedApplications() }.firstObject()
        } else {
            None
        };
        SourceDescriptor::Application {
            display_id,
            app_bundle_id: app
                .as_ref()
                .map(|a| unsafe { a.bundleIdentifier() }.to_string())
                .filter(|s| !s.is_empty()),
            app_pid: app.as_ref().map(|a| unsafe { a.processID() }),
            app_name: app
                .as_ref()
                .map(|a| unsafe { a.applicationName() }.to_string()),
            scale,
            crop_pt: None,
        }
    } else {
        SourceDescriptor::Display {
            display_id,
            display_name: None,
            scale,
            frame_pt: Some(RectPt {
                x: r.origin.x,
                y: r.origin.y,
                w: r.size.width,
                h: r.size.height,
            }),
        }
    };
    let filter: Retained<SCContentFilter> =
        unsafe { Retained::retain(f as *const SCContentFilter as *mut SCContentFilter) }
            .unwrap_or_else(|| unsafe { SCContentFilter::new() });
    PickedSource {
        descriptor,
        handle: Arc::new(PickedFilter(SendBox(filter))),
        region_pt: None,
    }
}
