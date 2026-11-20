//! SPEC (`--source`, `--switch`) and BG (`--background`) syntax of plan §5.1, and resolving a
//! SPEC against a platform.

use std::path::PathBuf;

use sc_core::project::{Background, GradientStop, Rgba};
use sc_geometry::{PointF, RectPt, SizePx};
use sc_platform::{AppRef, DisplayId, DisplayInfo, Platform, SourceSpec, WindowId, synthetic};

/// `Key` is tried as a display ID first, then as a 1-based index into `displays()` order.
#[derive(Debug, Clone, PartialEq)]
pub enum DisplaySel {
    Main,
    Key(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SpecText {
    Display(DisplaySel),
    Window(u32),
    WindowTitle(String),
    App {
        bundle_id: String,
        display: DisplaySel,
    },
    AppPid {
        pid: i32,
        display: DisplaySel,
    },
    Region {
        display: DisplaySel,
        rect: RectPt,
    },
    Synthetic {
        name: String,
        size: Option<SizePx>,
    },
}

fn display_sel(s: &str) -> Result<DisplaySel, String> {
    match s {
        "" => Err("empty display".to_string()),
        "main" => Ok(DisplaySel::Main),
        other => Ok(DisplaySel::Key(other.to_string())),
    }
}

fn with_display(rest: &str) -> Result<(&str, DisplaySel), String> {
    match rest.rsplit_once('@') {
        Some((head, d)) => Ok((head, display_sel(d)?)),
        None => Ok((rest, DisplaySel::Main)),
    }
}

fn parse_size(s: &str) -> Option<SizePx> {
    let (w, h) = s.split_once('x')?;
    let size = SizePx {
        w: w.parse().ok()?,
        h: h.parse().ok()?,
    };
    (size.w > 0 && size.h > 0).then_some(size)
}

pub fn parse_spec(s: &str) -> Result<SpecText, String> {
    let bad = |why: &str| format!("SPEC {s:?}: {why}");
    let (kind, rest) = match s.split_once(':') {
        Some((k, r)) => (k, Some(r)),
        None => (s, None),
    };
    match (kind, rest) {
        ("display", None) => Ok(SpecText::Display(DisplaySel::Main)),
        ("display", Some(r)) => display_sel(r).map(SpecText::Display).map_err(|e| bad(&e)),
        ("window", Some(r)) => r
            .parse()
            .map(SpecText::Window)
            .map_err(|_| bad("window ID must be a number")),
        ("window-title", Some(r)) if !r.is_empty() => Ok(SpecText::WindowTitle(r.to_string())),
        ("app", Some(r)) => {
            let (bundle, display) = with_display(r).map_err(|e| bad(&e))?;
            if bundle.is_empty() {
                return Err(bad("missing BUNDLE_ID"));
            }
            Ok(SpecText::App {
                bundle_id: bundle.to_string(),
                display,
            })
        }
        ("app-pid", Some(r)) => {
            let (pid, display) = with_display(r).map_err(|e| bad(&e))?;
            let pid = pid.parse().map_err(|_| bad("PID must be a number"))?;
            Ok(SpecText::AppPid { pid, display })
        }
        ("region", Some(r)) => {
            let (display, rect) = r
                .rsplit_once(':')
                .ok_or_else(|| bad("expected region:DISPLAY:X,Y,W,H"))?;
            let [x, y, w, h] = nums::<4>(rect, "X,Y,W,H").map_err(|e| bad(&e))?;
            if !(w > 0.0 && h > 0.0 && x.is_finite() && y.is_finite()) {
                return Err(bad("region needs a positive W and H"));
            }
            Ok(SpecText::Region {
                display: display_sel(display).map_err(|e| bad(&e))?,
                rect: RectPt { x, y, w, h },
            })
        }
        ("synthetic", Some(r)) => {
            let (name, size) = match r.split_once(':') {
                Some((n, sz)) => (
                    n,
                    Some(parse_size(sz).ok_or_else(|| bad("size must be WxH"))?),
                ),
                None => (r, None),
            };
            if name.is_empty() {
                return Err(bad("missing NAME"));
            }
            Ok(SpecText::Synthetic {
                name: name.to_string(),
                size,
            })
        }
        _ => Err(bad(
            "expected display[:ID|main|N], window:ID, window-title:TEXT, app:BUNDLE_ID[@DISPLAY], \
             app-pid:PID[@DISPLAY], region:DISPLAY:X,Y,W,H or synthetic:NAME[:WxH]",
        )),
    }
}

#[derive(Debug)]
pub enum ResolveError {
    Usage(String),
    NotFound(String),
    Platform(sc_platform::Error),
}

impl From<sc_platform::Error> for ResolveError {
    fn from(e: sc_platform::Error) -> Self {
        ResolveError::Platform(e)
    }
}

pub fn pick_display(displays: &[DisplayInfo], sel: &DisplaySel) -> Option<DisplayId> {
    match sel {
        DisplaySel::Main => displays
            .iter()
            .find(|d| d.is_main)
            .or_else(|| displays.first())
            .map(|d| d.id.clone()),
        DisplaySel::Key(k) => displays
            .iter()
            .find(|d| d.id.0 == *k)
            .or_else(|| {
                let n: usize = k.parse().ok()?;
                displays.get(n.checked_sub(1)?)
            })
            .map(|d| d.id.clone()),
    }
}

fn resolve_display(
    platform: &dyn Platform,
    sel: &DisplaySel,
    synthetic: bool,
) -> Result<DisplayId, ResolveError> {
    let displays = platform.screen().displays()?;
    if let Some(id) = pick_display(&displays, sel) {
        return Ok(id);
    }
    match sel {
        // The synthetic platform creates unknown displays on demand.
        DisplaySel::Key(k) if synthetic && k.starts_with("synthetic:") => Ok(DisplayId(k.clone())),
        DisplaySel::Main if synthetic => Ok(DisplayId("synthetic:main".to_string())),
        _ => Err(ResolveError::NotFound(format!(
            "no display {sel:?} (have: {})",
            displays
                .iter()
                .map(|d| d.id.0.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// `window-title:` and `app:` read the shareable content, which needs the Screen Recording grant.
pub fn resolve(
    platform: &dyn Platform,
    spec: &SpecText,
    synthetic: bool,
) -> Result<SourceSpec, ResolveError> {
    Ok(match spec {
        SpecText::Synthetic { name, size } => {
            if !synthetic {
                return Err(ResolveError::Usage(
                    "synthetic:NAME needs --platform synthetic".to_string(),
                ));
            }
            synthetic::display_spec(name, *size)
        }
        SpecText::Display(sel) => SourceSpec::Display {
            display: resolve_display(platform, sel, synthetic)?,
        },
        SpecText::Window(id) => SourceSpec::Window {
            window: WindowId(*id),
        },
        SpecText::WindowTitle(text) => {
            let list = platform.screen().list(&[])?;
            let mut hits: Vec<_> = list
                .windows
                .iter()
                .filter(|w| w.title.contains(text.as_str()))
                .collect();
            hits.sort_by_key(|w| (!w.on_screen, w.layer != 0, w.z_index));
            let w = hits.first().ok_or_else(|| {
                ResolveError::NotFound(format!("no window whose title contains {text:?}"))
            })?;
            SourceSpec::Window { window: w.id }
        }
        SpecText::App { bundle_id, display } => {
            let list = platform.screen().list(&[])?;
            let app = list
                .apps
                .iter()
                .find(|a| a.app.bundle_id.as_deref() == Some(bundle_id.as_str()))
                .ok_or_else(|| {
                    ResolveError::NotFound(format!("no running application {bundle_id:?}"))
                })?;
            SourceSpec::Application {
                display: resolve_display(platform, display, synthetic)?,
                app: app.app.clone(),
            }
        }
        SpecText::AppPid { pid, display } => {
            // Test apps have no bundle id, so a failed lookup is not an error.
            let bundle_id = platform.screen().list(&[]).ok().and_then(|l| {
                l.apps
                    .into_iter()
                    .find(|a| a.app.pid == *pid)
                    .and_then(|a| a.app.bundle_id)
            });
            SourceSpec::Application {
                display: resolve_display(platform, display, synthetic)?,
                app: AppRef {
                    pid: *pid,
                    bundle_id,
                },
            }
        }
        SpecText::Region { display, rect } => SourceSpec::Region {
            display: resolve_display(platform, display, synthetic)?,
            region_pt: *rect,
        },
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum BgChoice {
    Canvas(Background),
    Image(PathBuf),
}

fn parse_color(s: &str) -> Result<Rgba, String> {
    let hex = s
        .strip_prefix('#')
        .ok_or_else(|| format!("colour {s:?} must start with #"))?;
    if !(hex.len() == 6 || hex.len() == 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("colour {s:?} must be #RRGGBB or #RRGGBBAA"));
    }
    let byte = |i: usize| -> Result<f64, String> {
        u8::from_str_radix(&hex[i..i + 2], 16)
            .map(|v| f64::from(v) / 255.0)
            .map_err(|_| format!("colour {s:?}: bad hex"))
    };
    let a = if hex.len() == 8 { byte(6)? } else { 1.0 };
    Ok(Rgba([byte(0)?, byte(2)?, byte(4)?, a]))
}

fn parse_stops(s: &str) -> Result<Vec<GradientStop>, String> {
    let stops = s
        .split(',')
        .map(|stop| {
            let (c, at) = stop
                .split_once('@')
                .ok_or_else(|| format!("gradient stop {stop:?} must be #RRGGBB@POS"))?;
            let at: f64 = at
                .parse()
                .map_err(|_| format!("gradient stop {stop:?}: bad position"))?;
            if !(0.0..=1.0).contains(&at) {
                return Err(format!("gradient stop {stop:?}: position must be 0..1"));
            }
            Ok(GradientStop {
                at,
                color: parse_color(c)?,
                extra: Default::default(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    if stops.len() < 2 {
        return Err("a gradient needs at least two stops".to_string());
    }
    if stops.windows(2).any(|w| w[1].at < w[0].at) {
        return Err("gradient stop positions must not decrease".to_string());
    }
    Ok(stops)
}

fn nums<const N: usize>(s: &str, what: &str) -> Result<[f64; N], String> {
    let err = || format!("{what}: expected {N} numbers, got {s:?}");
    let v: Vec<f64> = s
        .split(',')
        .map(|n| n.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .map_err(|_| err())?;
    v.try_into().map_err(|_| err())
}

pub fn parse_bg(s: &str) -> Result<BgChoice, String> {
    let wrap = |e: String| format!("BG {s:?}: {e}");
    let (kind, rest) = s
        .split_once(':')
        .ok_or_else(|| wrap("expected solid:, linear:, radial: or image:".to_string()))?;
    match kind {
        "solid" => Ok(BgChoice::Canvas(Background::Solid {
            color: parse_color(rest).map_err(wrap)?,
        })),
        "linear" => {
            let (deg, stops) = rest
                .split_once(':')
                .ok_or_else(|| wrap("expected linear:DEG:STOPS".to_string()))?;
            let angle_deg: f64 = deg
                .parse()
                .map_err(|_| wrap(format!("bad angle {deg:?}")))?;
            Ok(BgChoice::Canvas(Background::Linear {
                stops: parse_stops(stops).map_err(wrap)?,
                angle_deg,
            }))
        }
        "radial" => {
            let (geom, stops) = rest
                .split_once(':')
                .ok_or_else(|| wrap("expected radial:CX,CY,R:STOPS".to_string()))?;
            let [cx, cy, r] = nums::<3>(geom, "radial CX,CY,R").map_err(wrap)?;
            if r <= 0.0 {
                return Err(wrap("radius must be positive".to_string()));
            }
            Ok(BgChoice::Canvas(Background::Radial {
                stops: parse_stops(stops).map_err(wrap)?,
                center_norm: PointF { x: cx, y: cy },
                radius_norm: r,
            }))
        }
        "image" if !rest.is_empty() => Ok(BgChoice::Image(PathBuf::from(rest))),
        _ => Err(wrap(
            "expected solid:, linear:, radial: or image:PATH".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_platform::synthetic::{Scenario, SyntheticPlatform};

    fn rect(x: f64, y: f64, w: f64, h: f64) -> RectPt {
        RectPt { x, y, w, h }
    }

    #[test]
    fn specs() {
        let main = DisplaySel::Main;
        let key = |k: &str| DisplaySel::Key(k.into());
        let cases = [
            ("display", SpecText::Display(main.clone())),
            ("display:main", SpecText::Display(main.clone())),
            ("display:69733382", SpecText::Display(key("69733382"))),
            ("window:42", SpecText::Window(42)),
            (
                "window-title:SC-TEST: RED",
                SpecText::WindowTitle("SC-TEST: RED".into()),
            ),
            (
                "app:com.apple.Safari@2",
                SpecText::App {
                    bundle_id: "com.apple.Safari".into(),
                    display: key("2"),
                },
            ),
            (
                "app:com.apple.Safari",
                SpecText::App {
                    bundle_id: "com.apple.Safari".into(),
                    display: main.clone(),
                },
            ),
            (
                "app-pid:123",
                SpecText::AppPid {
                    pid: 123,
                    display: main.clone(),
                },
            ),
            (
                "region:main:10,20.5,617,283.5",
                SpecText::Region {
                    display: main.clone(),
                    rect: rect(10.0, 20.5, 617.0, 283.5),
                },
            ),
            // Synthetic display IDs contain ':'.
            (
                "region:synthetic:a:0,0,100,50",
                SpecText::Region {
                    display: key("synthetic:a"),
                    rect: rect(0.0, 0.0, 100.0, 50.0),
                },
            ),
            (
                "synthetic:a:640x360",
                SpecText::Synthetic {
                    name: "a".into(),
                    size: Some(SizePx { w: 640, h: 360 }),
                },
            ),
            (
                "synthetic:b",
                SpecText::Synthetic {
                    name: "b".into(),
                    size: None,
                },
            ),
        ];
        for (text, want) in cases {
            assert_eq!(parse_spec(text), Ok(want), "{text}");
        }
        for bad in [
            "",
            "screen",
            "window:abc",
            "window-title:",
            "app:",
            "app-pid:x",
            "region:main:1,2,3",
            "region:main:1,2,0,4",
            "region:1,2,3,4",
            "synthetic:",
            "synthetic:a:640",
            "synthetic:a:0x10",
        ] {
            assert!(parse_spec(bad).is_err(), "{bad:?} should fail");
        }
    }

    #[test]
    fn backgrounds() {
        assert_eq!(
            parse_bg("solid:#ff0080"),
            Ok(BgChoice::Canvas(Background::Solid {
                color: Rgba([1.0, 0.0, 128.0 / 255.0, 1.0])
            }))
        );
        let Ok(BgChoice::Canvas(Background::Linear { stops, angle_deg })) =
            parse_bg("linear:135:#000000@0,#FFFFFF@0.5,#ff0000@1")
        else {
            panic!("linear")
        };
        assert_eq!(angle_deg, 135.0);
        assert_eq!(stops.len(), 3);
        assert_eq!(stops[1].at, 0.5);
        assert_eq!(stops[1].color, Rgba([1.0, 1.0, 1.0, 1.0]));
        let Ok(BgChoice::Canvas(Background::Radial {
            center_norm,
            radius_norm,
            stops,
        })) = parse_bg("radial:0.5,0.4,0.8:#112233@0,#445566@1")
        else {
            panic!("radial")
        };
        assert_eq!(center_norm, PointF { x: 0.5, y: 0.4 });
        assert_eq!(radius_norm, 0.8);
        assert_eq!(stops.len(), 2);
        assert_eq!(
            parse_bg("image:/tmp/bg.png"),
            Ok(BgChoice::Image(PathBuf::from("/tmp/bg.png")))
        );
        for bad in [
            "solid",
            "solid:red",
            "solid:#12345",
            "linear:45:#000000@0",
            "linear:x:#000000@0,#ffffff@1",
            "linear:45:#000000@0.6,#ffffff@0.2",
            "linear:45:#000000@0,#ffffff@2",
            "radial:0.5,0.5:#000000@0,#ffffff@1",
            "radial:0.5,0.5,0:#000000@0,#ffffff@1",
            "image:",
            "webp:x",
        ] {
            assert!(parse_bg(bad).is_err(), "{bad:?} should fail");
        }
    }

    fn display(id: &str, main: bool) -> DisplayInfo {
        DisplayInfo {
            id: DisplayId(id.into()),
            name: id.into(),
            frame_pt: RectPt::default(),
            scale: 2.0,
            size_px: SizePx { w: 100, h: 100 },
            is_main: main,
        }
    }

    #[test]
    fn display_selection() {
        let ds = [display("7", false), display("1", true), display("3", false)];
        let id = |s: &str| Some(DisplayId(s.into()));
        let key = |k: &str| DisplaySel::Key(k.into());
        assert_eq!(pick_display(&ds, &DisplaySel::Main), id("1"));
        assert_eq!(pick_display(&ds, &key("3")), id("3"));
        assert_eq!(pick_display(&ds, &key("2")), id("1"));
        assert_eq!(pick_display(&ds, &key("0")), None);
        assert_eq!(pick_display(&ds, &key("9")), None);
        assert_eq!(pick_display(&[], &DisplaySel::Main), None);
    }

    #[test]
    fn resolve_against_synthetic() {
        let p = SyntheticPlatform::new(Scenario::new());
        let parsed = |s: &str| parse_spec(s).expect("parses");
        let SourceSpec::Display { display } =
            resolve(&*p, &parsed("synthetic:a"), true).expect("resolves")
        else {
            panic!("display spec")
        };
        assert_eq!(display.0, "synthetic:a");
        assert!(matches!(
            resolve(&*p, &parsed("region:synthetic:a:0,0,64,32"), true),
            Ok(SourceSpec::Region { .. })
        ));
        assert!(matches!(
            resolve(&*p, &parsed("synthetic:a"), false),
            Err(ResolveError::Usage(_))
        ));
        assert!(matches!(
            resolve(&*p, &parsed("window-title:no such window"), true),
            Err(ResolveError::NotFound(_))
        ));
    }
}
