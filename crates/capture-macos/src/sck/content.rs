use std::collections::HashMap;
use std::sync::{Mutex, mpsc};
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{ClassType, msg_send, sel};
use objc2_app_kit::{NSApplicationActivationPolicy, NSRunningApplication, NSScreen};
use objc2_core_foundation::{CFArray, CGRect};
use objc2_core_graphics::{
    CGDisplayBounds, CGDisplayCopyDisplayMode, CGDisplayIsBuiltin, CGDisplayMode,
    CGGetActiveDisplayList, CGMainDisplayID, CGWindowListCopyWindowInfo, CGWindowListOption,
    kCGWindowNumber,
};
use objc2_foundation::{NSArray, NSDictionary, NSError, NSNumber, NSString};
use objc2_screen_capture_kit::{SCDisplay, SCRunningApplication, SCShareableContent, SCWindow};
use sc_geometry::{RectPt, SizePx};
use sc_platform::{
    AppInfo, AppRef, DisplayId, DisplayInfo, Error, Result, SourceList, WindowId, WindowInfo,
};

use super::glue::{SendBox, main_sync};
use super::listable::{ActivationPolicy, WindowFacts, listable_window, listed_apps};
use crate::{tcc, util};

const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) fn our_pid() -> i32 {
    std::process::id() as i32
}

fn cgrect_pt(r: CGRect) -> RectPt {
    RectPt {
        x: r.origin.x,
        y: r.origin.y,
        w: r.size.width,
        h: r.size.height,
    }
}

type ContentResult = std::result::Result<SendBox<Retained<SCShareableContent>>, String>;

fn content_block(
    tx: mpsc::Sender<ContentResult>,
) -> RcBlock<dyn Fn(*mut SCShareableContent, *mut NSError)> {
    RcBlock::new(move |c: *mut SCShareableContent, e: *mut NSError| {
        let r = if let Some(c) = unsafe { Retained::retain(c) } {
            Ok(SendBox(c))
        } else if let Some(e) = unsafe { e.as_ref() } {
            Err(util::ns_error_text(e))
        } else {
            Err("no content and no error".to_string())
        };
        let _ = tx.send(r);
    })
}

/// Every window (`onScreenWindowsOnly = false` when `on_screen_only` is false), no desktop windows.
pub(crate) fn shareable(on_screen_only: bool) -> Result<Retained<SCShareableContent>> {
    tcc::guard("SCShareableContent")?;
    let (tx, rx) = mpsc::channel();
    let blk = content_block(tx);
    unsafe {
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
            true,
            on_screen_only,
            &blk,
        );
    }
    match util::wait_on(&rx, FETCH_TIMEOUT, "SCShareableContent")? {
        Ok(c) => Ok(c.0),
        Err(e) if e.contains("-3801") || e.to_lowercase().contains("declined") => {
            Err(Error::PermissionDenied(sc_platform::PermissionKind::Screen))
        }
        Err(e) => Err(Error::Other(format!("SCShareableContent: {e}"))),
    }
}

fn has_current_process_api() -> bool {
    let sel = sel!(getCurrentProcessShareableContentWithCompletionHandler:);
    let cls: &AnyClass = SCShareableContent::class();
    unsafe { msg_send![cls, respondsToSelector: sel] }
}

/// `SCShareableContent.currentProcess` (macOS 14.4+), or `Ok(None)` where it does not exist.
pub(crate) fn fetch_current_process() -> Result<Option<Retained<SCShareableContent>>> {
    tcc::guard("SCShareableContent.currentProcess")?;
    if !has_current_process_api() {
        return Ok(None);
    }
    let (tx, rx) = mpsc::channel();
    let blk = content_block(tx);
    unsafe { SCShareableContent::getCurrentProcessShareableContentWithCompletionHandler(&blk) };
    match util::wait_on(&rx, FETCH_TIMEOUT, "SCShareableContent.currentProcess")? {
        Ok(c) => Ok(Some(c.0)),
        Err(e) => Err(Error::Other(format!(
            "SCShareableContent.currentProcess: {e}"
        ))),
    }
}

#[derive(Debug, Clone)]
pub struct CurrentProcessReport {
    pub available: bool,
    pub displays: Vec<u32>,
    pub windows: Vec<(u32, String, i32, bool)>,
    pub apps: Vec<(i32, String)>,
}

/// What `SCShareableContent.currentProcess` lists: (window id, title, owner pid, on screen).
pub fn current_process_report() -> Result<CurrentProcessReport> {
    let Some(c) = fetch_current_process()? else {
        return Ok(CurrentProcessReport {
            available: false,
            displays: Vec::new(),
            windows: Vec::new(),
            apps: Vec::new(),
        });
    };
    unsafe {
        Ok(CurrentProcessReport {
            available: true,
            displays: c.displays().iter().map(|d| d.displayID()).collect(),
            windows: c
                .windows()
                .iter()
                .map(|w| {
                    (
                        w.windowID(),
                        w.title().map(|t| t.to_string()).unwrap_or_default(),
                        w.owningApplication().map(|a| a.processID()).unwrap_or(-1),
                        w.isOnScreen(),
                    )
                })
                .collect(),
            apps: c
                .applications()
                .iter()
                .map(|a| (a.processID(), a.applicationName().to_string()))
                .collect(),
        })
    }
}

pub(crate) fn window_pid(w: &SCWindow) -> Option<i32> {
    unsafe { w.owningApplication() }.map(|a| unsafe { a.processID() })
}

/// Windows our process owns: the full content plus anything only `currentProcess` lists.
pub(crate) fn own_windows(content: &SCShareableContent) -> Vec<Retained<SCWindow>> {
    let pid = our_pid();
    let mut out: Vec<Retained<SCWindow>> = unsafe { content.windows() }
        .iter()
        .filter(|w| window_pid(w) == Some(pid))
        .collect();
    if let Ok(Some(cp)) = fetch_current_process() {
        for w in unsafe { cp.windows() }.iter() {
            let id = unsafe { w.windowID() };
            let ours = window_pid(&w).is_none_or(|p| p == pid);
            if ours && !out.iter().any(|o| unsafe { o.windowID() } == id) {
                out.push(w);
            }
        }
    }
    out
}

pub(crate) fn windows_by_id(
    content: &SCShareableContent,
    ids: &[WindowId],
) -> Vec<Retained<SCWindow>> {
    unsafe { content.windows() }
        .iter()
        .filter(|w| ids.iter().any(|i| i.0 == unsafe { w.windowID() }))
        .collect()
}

/// `exclude` ∪ every window this process owns, deduplicated.
pub(crate) fn exclusion_windows(
    content: &SCShareableContent,
    exclude: &[WindowId],
) -> Vec<Retained<SCWindow>> {
    let mut out = own_windows(content);
    for w in windows_by_id(content, exclude) {
        let id = unsafe { w.windowID() };
        if !out.iter().any(|o| unsafe { o.windowID() } == id) {
            out.push(w);
        }
    }
    out
}

/// Whether a window at `layer` is part of an app's content for the app crop: normal windows and
/// floating panels count; the menu bar level (24) and above (status items, pop-up menus, overlays)
/// do not.
pub(crate) fn app_content_layer(layer: isize) -> bool {
    (0..24).contains(&layer)
}

/// Frames (global points) of `pid`'s on-screen windows on an `app_content_layer` that are at least
/// 2 pt on a side, not in `exclude`, and intersect `display_frame_pt`: the input of
/// `app_bounds_pt` (ADR-039).
pub(crate) fn app_window_frames(
    content: &SCShareableContent,
    pid: i32,
    display_frame_pt: RectPt,
    exclude: &[WindowId],
) -> Vec<RectPt> {
    let d = display_frame_pt;
    unsafe { content.windows() }
        .iter()
        .filter(|w| window_pid(w) == Some(pid))
        .filter(|w| unsafe { w.isOnScreen() && app_content_layer(w.windowLayer()) })
        .filter(|w| !exclude.iter().any(|e| e.0 == unsafe { w.windowID() }))
        .map(|w| cgrect_pt(unsafe { w.frame() }))
        .filter(|f| {
            f.w >= 2.0
                && f.h >= 2.0
                && f.x < d.x + d.w
                && f.x + f.w > d.x
                && f.y < d.y + d.h
                && f.y + f.h > d.y
        })
        .collect()
}

pub(crate) fn find_window(
    content: &SCShareableContent,
    id: WindowId,
) -> Option<Retained<SCWindow>> {
    unsafe { content.windows() }
        .iter()
        .find(|w| unsafe { w.windowID() } == id.0)
}

pub(crate) fn find_display(content: &SCShareableContent, id: u32) -> Option<Retained<SCDisplay>> {
    unsafe { content.displays() }
        .iter()
        .find(|d| unsafe { d.displayID() } == id)
}

pub(crate) fn find_app(
    content: &SCShareableContent,
    pid: i32,
) -> Option<Retained<SCRunningApplication>> {
    unsafe { content.applications() }
        .iter()
        .find(|a| unsafe { a.processID() } == pid)
}

pub(crate) fn parse_display_id(id: &DisplayId) -> Result<u32> {
    id.0.parse::<u32>()
        .map_err(|_| Error::NotFound(format!("display {:?}", id.0)))
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CgDisplay {
    pub id: u32,
    pub frame_pt: RectPt,
    pub size_px: SizePx,
    pub scale: f64,
    pub is_main: bool,
    pub builtin: bool,
}

pub(crate) fn cg_displays() -> Result<Vec<CgDisplay>> {
    let mut ids = [0u32; 32];
    let mut n = 0u32;
    let err = unsafe { CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut n) };
    if err.0 != 0 {
        return Err(Error::Os {
            code: i64::from(err.0),
            message: "CGGetActiveDisplayList".into(),
        });
    }
    let main = CGMainDisplayID();
    Ok(ids[..n as usize]
        .iter()
        .map(|&id| cg_display(id, main))
        .collect())
}

pub(crate) fn cg_display_by_id(id: u32) -> Result<CgDisplay> {
    cg_displays()?
        .into_iter()
        .find(|d| d.id == id)
        .ok_or_else(|| Error::NotFound(format!("display {id}")))
}

fn cg_display(id: u32, main: u32) -> CgDisplay {
    let frame_pt = cgrect_pt(CGDisplayBounds(id));
    let mode = CGDisplayCopyDisplayMode(id);
    let (pw, ph) = match mode.as_deref() {
        Some(m) => (
            CGDisplayMode::pixel_width(Some(m)),
            CGDisplayMode::pixel_height(Some(m)),
        ),
        None => (frame_pt.w as usize, frame_pt.h as usize),
    };
    let scale = if frame_pt.w > 0.0 {
        pw as f64 / frame_pt.w
    } else {
        1.0
    };
    CgDisplay {
        id,
        frame_pt,
        size_px: SizePx {
            w: pw as u32,
            h: ph as u32,
        },
        scale,
        is_main: id == main,
        builtin: CGDisplayIsBuiltin(id),
    }
}

static SCREEN_NAMES: Mutex<Option<HashMap<u32, String>>> = Mutex::new(None);

fn screen_names_main(mtm: objc2::MainThreadMarker) -> HashMap<u32, String> {
    let mut out = HashMap::new();
    let key = NSString::from_str("NSScreenNumber");
    for s in NSScreen::screens(mtm).iter() {
        let desc = s.deviceDescription();
        let id = desc
            .objectForKey(&key)
            .and_then(|o: Retained<AnyObject>| o.downcast::<NSNumber>().ok())
            .map(|n| n.unsignedIntValue());
        if let Some(id) = id {
            out.insert(id, s.localizedName().to_string());
        }
    }
    out
}

fn screen_names() -> HashMap<u32, String> {
    if let Ok(names) = main_sync(
        "NSScreen names",
        Duration::from_millis(250),
        screen_names_main,
    ) {
        if let Ok(mut g) = SCREEN_NAMES.lock() {
            *g = Some(names.clone());
        }
        return names;
    }
    SCREEN_NAMES
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_default()
}

/// CoreGraphics and NSScreen only: never TCC.
pub(crate) fn displays() -> Result<Vec<DisplayInfo>> {
    let names = screen_names();
    Ok(cg_displays()?
        .into_iter()
        .map(|d| DisplayInfo {
            id: DisplayId(d.id.to_string()),
            name: names.get(&d.id).cloned().unwrap_or_else(|| {
                if d.builtin {
                    "Built-in Display".to_string()
                } else {
                    format!("Display {}", d.id)
                }
            }),
            frame_pt: d.frame_pt,
            scale: d.scale,
            size_px: d.size_px,
            is_main: d.is_main,
        })
        .collect())
}

/// Window number → front-to-back index of the on-screen window list (no TCC).
pub(crate) fn z_order() -> HashMap<u32, u32> {
    let mut out = HashMap::new();
    let Some(arr) = CGWindowListCopyWindowInfo(
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
        0,
    ) else {
        return out;
    };
    let arr: &NSArray<NSDictionary<NSString, AnyObject>> =
        unsafe { &*(&*arr as *const CFArray as *const NSArray<NSDictionary<NSString, AnyObject>>) };
    let key = util::cfstr_as_ns(unsafe { kCGWindowNumber });
    for (i, d) in arr.iter().enumerate() {
        if let Some(n) = d
            .objectForKey(key)
            .and_then(|o| o.downcast::<NSNumber>().ok())
        {
            out.insert(n.unsignedIntValue(), i as u32);
        }
    }
    out
}

fn display_of(frame: RectPt, displays: &[CgDisplay]) -> Option<DisplayId> {
    let cx = frame.x + frame.w / 2.0;
    let cy = frame.y + frame.h / 2.0;
    displays
        .iter()
        .find(|d| {
            cx >= d.frame_pt.x
                && cx < d.frame_pt.x + d.frame_pt.w
                && cy >= d.frame_pt.y
                && cy < d.frame_pt.y + d.frame_pt.h
        })
        .map(|d| DisplayId(d.id.to_string()))
}

fn app_ref(a: &SCRunningApplication) -> AppRef {
    let bid = unsafe { a.bundleIdentifier() }.to_string();
    AppRef {
        pid: unsafe { a.processID() },
        bundle_id: (!bid.is_empty()).then_some(bid),
    }
}

pub(crate) fn window_app(id: WindowId) -> Result<Option<AppRef>> {
    let content = shareable(true)?;
    Ok(find_window(&content, id)
        .and_then(|w| unsafe { w.owningApplication() })
        .map(|a| app_ref(&a)))
}

fn activation_policy(pid: i32) -> Option<ActivationPolicy> {
    let app: Option<Retained<NSRunningApplication>> = unsafe {
        msg_send![NSRunningApplication::class(), runningApplicationWithProcessIdentifier: pid]
    };
    let p = app?.activationPolicy();
    Some(if p == NSApplicationActivationPolicy::Regular {
        ActivationPolicy::Regular
    } else if p == NSApplicationActivationPolicy::Accessory {
        ActivationPolicy::Accessory
    } else {
        ActivationPolicy::Prohibited
    })
}

/// On-screen windows a person would record (`listable::listable_window`) and the apps that own one.
pub(crate) fn list(hide: &[WindowId]) -> Result<SourceList> {
    let content = shareable(true)?;
    let cg = cg_displays()?;
    let infos = displays()?;
    let z = z_order();
    let pid = our_pid();
    let sc_displays: Vec<u32> = unsafe { content.displays() }
        .iter()
        .map(|d| unsafe { d.displayID() })
        .collect();
    let displays: Vec<DisplayInfo> = infos
        .into_iter()
        .filter(|d| parse_display_id(&d.id).is_ok_and(|id| sc_displays.contains(&id)))
        .collect();
    let display_frames: Vec<RectPt> = cg.iter().map(|d| d.frame_pt).collect();
    let mut policies: HashMap<i32, Option<ActivationPolicy>> = HashMap::new();
    let mut windows = Vec::new();
    for w in unsafe { content.windows() }.iter() {
        let Some(owner) = (unsafe { w.owningApplication() }) else {
            continue;
        };
        let app = app_ref(&owner);
        let id = unsafe { w.windowID() };
        let title = unsafe { w.title() }
            .map(|t| t.to_string())
            .unwrap_or_default();
        let frame_pt = cgrect_pt(unsafe { w.frame() });
        let on_screen = unsafe { w.isOnScreen() };
        let layer = unsafe { w.windowLayer() };
        let facts = WindowFacts {
            id,
            pid: app.pid,
            bundled: app.bundle_id.is_some(),
            policy: *policies
                .entry(app.pid)
                .or_insert_with(|| activation_policy(app.pid)),
            layer,
            on_screen,
            frame_pt,
            title: &title,
        };
        if !listable_window(&facts, pid, hide, &display_frames) {
            continue;
        }
        windows.push(WindowInfo {
            id: WindowId(id),
            title,
            app_name: unsafe { owner.applicationName() }.to_string(),
            app,
            frame_pt,
            display: display_of(frame_pt, &cg),
            on_screen,
            layer: layer as i32,
            z_index: z.get(&id).copied().unwrap_or(u32::MAX),
        });
    }
    windows.sort_by_key(|w| w.z_index);
    let apps: Vec<AppInfo> = unsafe { content.applications() }
        .iter()
        .map(|a| AppInfo {
            app: app_ref(&a),
            name: unsafe { a.applicationName() }.to_string(),
        })
        .collect();
    let apps = listed_apps(apps, &windows);
    Ok(SourceList {
        displays,
        windows,
        apps,
    })
}

pub(crate) fn responds(obj: &AnyObject, sel: Sel) -> bool {
    unsafe { msg_send![obj, respondsToSelector: sel] }
}

#[cfg(test)]
mod app_layer_tests {
    use super::app_content_layer;

    #[test]
    fn floating_panels_count_menu_bar_and_above_do_not() {
        for layer in [0, 3, 8, 19, 23] {
            assert!(app_content_layer(layer), "{layer}");
        }
        for layer in [-1, 24, 25, 101, 1000, isize::MAX] {
            assert!(!app_content_layer(layer), "{layer}");
        }
    }
}
