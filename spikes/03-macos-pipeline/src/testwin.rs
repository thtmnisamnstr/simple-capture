use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor, NSScreen, NSWindow, NSWindowStyleMask};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

pub fn init_app(mtm: MainThreadMarker) -> Retained<NSApplication> {
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    unsafe { app.finishLaunching() };
    app
}

pub struct TestWindow {
    pub window: Retained<NSWindow>,
}

impl TestWindow {
    pub fn new(mtm: MainThreadMarker, x: f64, y_from_top: f64, w: f64, h: f64, rgb: (f64, f64, f64)) -> Self {
        let screen_h = NSScreen::mainScreen(mtm).map(|s| s.frame().size.height).unwrap_or(1080.0);
        let rect = NSRect::new(NSPoint::new(x, screen_h - y_from_top - h), NSSize::new(w, h));
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                rect,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setOpaque(true);
        window.setHasShadow(false);
        window.setLevel(25);
        window.setIgnoresMouseEvents(true);
        let t = Self { window };
        t.set_color(rgb);
        t.window.orderFrontRegardless();
        t
    }

    pub fn set_color(&self, rgb: (f64, f64, f64)) {
        let c = NSColor::colorWithSRGBRed_green_blue_alpha(rgb.0, rgb.1, rgb.2, 1.0);
        self.window.setBackgroundColor(Some(&c));
    }

    pub fn rect_top_left_points(&self, mtm: MainThreadMarker) -> (f64, f64, f64, f64) {
        let screen_h = NSScreen::mainScreen(mtm).map(|s| s.frame().size.height).unwrap_or(1080.0);
        let f = self.window.frame();
        (f.origin.x, screen_h - f.origin.y - f.size.height, f.size.width, f.size.height)
    }

    pub fn window_number(&self) -> isize {
        self.window.windowNumber()
    }
}

pub fn play_sound_in_process(name: &str) -> Option<Retained<objc2_avf_audio::AVAudioPlayer>> {
    use objc2::AnyThread;
    let path = format!("/System/Library/Sounds/{name}.aiff");
    let url = objc2_foundation::NSURL::fileURLWithPath(&NSString::from_str(&path));
    let p = unsafe { objc2_avf_audio::AVAudioPlayer::initWithContentsOfURL_error(objc2_avf_audio::AVAudioPlayer::alloc(), &url) }.ok()?;
    unsafe {
        p.setVolume(1.0);
        p.play();
    }
    Some(p)
}

pub fn main_screen_scale(mtm: MainThreadMarker) -> f64 {
    NSScreen::mainScreen(mtm).map(|s| s.backingScaleFactor()).unwrap_or(1.0)
}
