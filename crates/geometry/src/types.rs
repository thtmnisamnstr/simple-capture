#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// A size in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SizePx {
    pub w: u32,
    pub h: u32,
}

/// A rectangle in pixels, top-left origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct RectPx {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// A point (units given by context).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PointF {
    pub x: f64,
    pub y: f64,
}

/// A rectangle in points, top-left origin.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct RectPt {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// A rectangle normalised to 0..1.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct RectNorm {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// An exact ratio, always reduced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Ratio {
    pub num: u32,
    pub den: u32,
}

fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

impl Ratio {
    /// Reduced ratio; a zero term yields 1:1.
    pub fn new(num: u32, den: u32) -> Ratio {
        if num == 0 || den == 0 {
            return Ratio { num: 1, den: 1 };
        }
        let g = gcd(num, den);
        Ratio {
            num: num / g,
            den: den / g,
        }
    }

    pub fn of(size: SizePx) -> Ratio {
        Ratio::new(size.w, size.h)
    }

    pub fn as_f64(self) -> f64 {
        f64::from(self.num) / f64::from(self.den)
    }
}

/// Output canvas aspect (ADR-033).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Aspect {
    #[cfg_attr(feature = "serde", serde(rename = "original"))]
    Original,
    #[cfg_attr(feature = "serde", serde(rename = "16:9"))]
    R16x9,
    #[cfg_attr(feature = "serde", serde(rename = "4:3"))]
    R4x3,
    #[cfg_attr(feature = "serde", serde(rename = "1:1"))]
    R1x1,
    #[cfg_attr(feature = "serde", serde(rename = "9:16"))]
    R9x16,
    #[cfg_attr(feature = "serde", serde(rename = "4:5"))]
    R4x5,
}

impl Aspect {
    pub const ALL: [Aspect; 6] = [
        Aspect::Original,
        Aspect::R16x9,
        Aspect::R4x3,
        Aspect::R1x1,
        Aspect::R9x16,
        Aspect::R4x5,
    ];

    /// `None` for `Original` (resolved from the first segment).
    pub fn ratio(self) -> Option<Ratio> {
        match self {
            Aspect::Original => None,
            Aspect::R16x9 => Some(Ratio::new(16, 9)),
            Aspect::R4x3 => Some(Ratio::new(4, 3)),
            Aspect::R1x1 => Some(Ratio::new(1, 1)),
            Aspect::R9x16 => Some(Ratio::new(9, 16)),
            Aspect::R4x5 => Some(Ratio::new(4, 5)),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Aspect::Original => "original",
            Aspect::R16x9 => "16:9",
            Aspect::R4x3 => "4:3",
            Aspect::R1x1 => "1:1",
            Aspect::R9x16 => "9:16",
            Aspect::R4x5 => "4:5",
        }
    }

    pub fn parse(s: &str) -> Option<Aspect> {
        Aspect::ALL.into_iter().find(|a| a.as_str() == s)
    }
}

/// Export size classes by short side (ARCHITECTURE §10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum SizeClass {
    S2160,
    S1440,
    S1080,
    S720,
    S480,
    S360,
    S240,
}

/// Region-selection drag handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Handle {
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
    Move,
}
