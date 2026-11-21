//! Text animations (SPEC Text overlays): in, out and emphasis loops, each a pure function of the
//! time into the item, so they look the same at every export frame rate.

use std::f32::consts::TAU;

use sc_core::project::{Anim, EmphasisKind, TextAnim};

use crate::item::Reveal;

pub const IN_KINDS: [&str; 9] = [
    "fade",
    "pop",
    "slide_left",
    "slide_right",
    "slide_up",
    "slide_down",
    "typewriter",
    "words",
    "bounce",
];
pub const OUT_KINDS: [&str; 6] = [
    "fade",
    "pop",
    "slide_left",
    "slide_right",
    "slide_up",
    "slide_down",
];
pub const EASINGS: [&str; 5] = ["linear", "ease_in", "ease_out", "ease_in_out", "back"];

const SLIDE_1080: f32 = 180.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimFrame {
    pub opacity: f32,
    /// In 1080 units.
    pub offset: (f32, f32),
    pub scale: f32,
    pub rotation_deg: f32,
    pub reveal: Reveal,
}

impl Default for AnimFrame {
    fn default() -> AnimFrame {
        AnimFrame {
            opacity: 1.0,
            offset: (0.0, 0.0),
            scale: 1.0,
            rotation_deg: 0.0,
            reveal: Reveal::All,
        }
    }
}

pub fn ease(name: &str, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    match name {
        "linear" => t,
        "ease_in" => t * t * t,
        "ease_in_out" => {
            if t < 0.5 {
                4.0 * t * t * t
            } else {
                1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
            }
        }
        "back" => {
            let c1 = 1.70158;
            let c3 = c1 + 1.0;
            1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
        }
        _ => 1.0 - (1.0 - t).powi(3),
    }
}

fn bounce_out(t: f32) -> f32 {
    let (n1, d1) = (7.5625, 2.75);
    if t < 1.0 / d1 {
        n1 * t * t
    } else if t < 2.0 / d1 {
        let t = t - 1.5 / d1;
        n1 * t * t + 0.75
    } else if t < 2.5 / d1 {
        let t = t - 2.25 / d1;
        n1 * t * t + 0.9375
    } else {
        let t = t - 2.625 / d1;
        n1 * t * t + 0.984375
    }
}

fn duration_ns(a: &Anim, len_ns: i64) -> i64 {
    ((a.duration_s.max(0.01) * 1e9) as i64)
        .min(len_ns / 2)
        .max(1)
}

fn slide(kind: &str, amount: f32) -> (f32, f32) {
    match kind {
        "slide_left" => (-amount, 0.0),
        "slide_right" => (amount, 0.0),
        "slide_up" => (0.0, amount),
        "slide_down" => (0.0, -amount),
        _ => (0.0, 0.0),
    }
}

/// The item's look `t_ns` after it starts; it lasts `len_ns`. `clusters` and `words` are its
/// glyph-cluster and word counts, for the reveals.
pub fn evaluate(
    anim: &TextAnim,
    t_ns: i64,
    len_ns: i64,
    clusters: usize,
    words: usize,
) -> AnimFrame {
    let mut f = AnimFrame::default();
    if let Some(a) = &anim.in_ {
        let d = duration_ns(a, len_ns);
        if t_ns < d {
            let p = t_ns.max(0) as f32 / d as f32;
            let e = ease(&a.easing, p);
            match a.kind.as_str() {
                "fade" => f.opacity *= e,
                "pop" => {
                    f.scale *= 0.4 + 0.6 * ease("back", p);
                    f.opacity *= (p * 4.0).min(1.0);
                }
                "typewriter" => {
                    f.reveal = Reveal::Clusters((p * clusters as f32).floor() as usize);
                }
                "words" => f.reveal = Reveal::Words((p * words as f32).floor() as usize + 1),
                "bounce" => {
                    f.offset.1 -= (1.0 - bounce_out(p)) * SLIDE_1080;
                    f.opacity *= (p * 5.0).min(1.0);
                }
                k => {
                    let (dx, dy) = slide(k, (1.0 - e) * SLIDE_1080);
                    f.offset.0 += dx;
                    f.offset.1 += dy;
                    f.opacity *= e;
                }
            }
        }
    }
    if let Some(a) = &anim.out {
        let d = duration_ns(a, len_ns);
        let left = len_ns - t_ns;
        if left < d {
            let q = 1.0 - left.max(0) as f32 / d as f32;
            let e = ease(&a.easing, q);
            match a.kind.as_str() {
                "fade" => f.opacity *= 1.0 - e,
                "pop" => {
                    f.scale *= 1.0 - 0.6 * e;
                    f.opacity *= 1.0 - e;
                }
                k => {
                    let (dx, dy) = slide(k, -e * SLIDE_1080);
                    f.offset.0 -= dx;
                    f.offset.1 -= dy;
                    f.opacity *= 1.0 - e;
                }
            }
        }
    }
    if let Some(em) = &anim.emphasis {
        let phase = (t_ns as f64 / 1e9 / em.period_s.max(0.05)) as f32;
        let s = (TAU * phase).sin();
        match em.kind {
            EmphasisKind::Pulse => f.scale *= 1.0 + 0.07 * s,
            EmphasisKind::Wiggle => f.rotation_deg += 5.0 * s,
            EmphasisKind::Float => f.offset.1 -= 12.0 * s,
        }
    }
    f.opacity = f.opacity.clamp(0.0, 1.0);
    f
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::project::{Emphasis, Extra};

    const S: i64 = 1_000_000_000;

    fn anim(kind: &str, d: f64) -> Anim {
        Anim {
            kind: kind.into(),
            duration_s: d,
            easing: "linear".into(),
            extra: Extra::new(),
        }
    }

    fn with(in_: Option<Anim>, out: Option<Anim>) -> TextAnim {
        TextAnim {
            in_,
            out,
            emphasis: None,
            extra: Extra::new(),
        }
    }

    #[test]
    fn fades_in_and_out_and_is_steady_between() {
        let a = with(Some(anim("fade", 0.5)), Some(anim("fade", 0.5)));
        assert_eq!(evaluate(&a, 0, 3 * S, 5, 1).opacity, 0.0);
        assert!((evaluate(&a, S / 4, 3 * S, 5, 1).opacity - 0.5).abs() < 1e-3);
        assert_eq!(evaluate(&a, S, 3 * S, 5, 1), AnimFrame::default());
        assert!((evaluate(&a, 3 * S - S / 4, 3 * S, 5, 1).opacity - 0.5).abs() < 1e-3);
        assert_eq!(evaluate(&a, 3 * S, 3 * S, 5, 1).opacity, 0.0);
    }

    #[test]
    fn typewriter_and_words_reveal_in_order() {
        let a = with(Some(anim("typewriter", 1.0)), None);
        assert_eq!(evaluate(&a, 0, 3 * S, 10, 2).reveal, Reveal::Clusters(0));
        assert_eq!(
            evaluate(&a, S / 2, 3 * S, 10, 2).reveal,
            Reveal::Clusters(5)
        );
        assert_eq!(evaluate(&a, S, 3 * S, 10, 2).reveal, Reveal::All);
        let w = with(Some(anim("words", 1.0)), None);
        assert_eq!(evaluate(&w, 0, 3 * S, 10, 4).reveal, Reveal::Words(1));
        assert_eq!(
            evaluate(&w, S * 3 / 4, 3 * S, 10, 4).reveal,
            Reveal::Words(4)
        );
    }

    #[test]
    fn slides_start_offset_and_end_in_place() {
        for k in ["slide_left", "slide_right", "slide_up", "slide_down"] {
            let a = with(Some(anim(k, 0.5)), Some(anim(k, 0.5)));
            let start = evaluate(&a, 0, 2 * S, 1, 1);
            assert!(start.offset.0.abs() + start.offset.1.abs() > 100.0, "{k}");
            assert_eq!(evaluate(&a, S, 2 * S, 1, 1).offset, (0.0, 0.0), "{k}");
        }
        let left = with(Some(anim("slide_left", 0.5)), None);
        assert!(evaluate(&left, 0, 2 * S, 1, 1).offset.0 < 0.0);
    }

    #[test]
    fn durations_are_capped_by_the_item_and_emphasis_loops() {
        let a = with(Some(anim("fade", 10.0)), None);
        assert_eq!(evaluate(&a, S, 2 * S, 1, 1).opacity, 1.0);
        let mut e = with(None, None);
        e.emphasis = Some(Emphasis {
            kind: EmphasisKind::Pulse,
            period_s: 1.0,
            extra: Extra::new(),
        });
        let quarter = evaluate(&e, S / 4, 5 * S, 1, 1).scale;
        assert!((quarter - 1.07).abs() < 1e-3);
        assert!((evaluate(&e, S + S / 4, 5 * S, 1, 1).scale - quarter).abs() < 1e-4);
    }

    #[test]
    fn same_time_same_frame_at_any_frame_rate() {
        let a = with(Some(anim("pop", 0.4)), Some(anim("slide_up", 0.3)));
        for fps in [15i64, 24, 30, 50, 60] {
            for n in 0..(2 * fps) {
                let t = n * S / fps;
                assert_eq!(evaluate(&a, t, 2 * S, 3, 1), evaluate(&a, t, 2 * S, 3, 1));
            }
        }
        assert_eq!(
            evaluate(&a, S / 5, 2 * S, 3, 1),
            evaluate(&a, 6 * S / 30, 2 * S, 3, 1)
        );
    }
}
