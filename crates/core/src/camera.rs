//! Camera overlay placement over time (M2_PLAN §3.2): which `CameraKey` applies at a recording
//! time, interpolated per the next key's easing.

use sc_geometry::{CameraPose, PointF, Ratio, ease_in_out, lerp_pose};

use crate::project::{CameraBorder, CameraKey, CameraShadow, CameraShape, Easing, Extra};

impl CameraShape {
    /// Width:height of the shape.
    pub fn aspect(self) -> Ratio {
        match self {
            CameraShape::Circle | CameraShape::Square | CameraShape::Rounded => Ratio::new(1, 1),
            CameraShape::Rect4x3 => Ratio::new(4, 3),
            CameraShape::Rect16x9 => Ratio::new(16, 9),
            CameraShape::Rect3x4 => Ratio::new(3, 4),
            CameraShape::Rect9x16 => Ratio::new(9, 16),
        }
    }
}

/// Where and how the camera is drawn at one instant.
#[derive(Debug, Clone, PartialEq)]
pub struct CameraPlacement {
    pub shape: CameraShape,
    pub pose: CameraPose,
    pub mirror: bool,
    pub border: Option<CameraBorder>,
    pub shadow: Option<CameraShadow>,
}

impl CameraKey {
    /// `corner_radius` is a fraction of half the shape's short side; a circle is always 1.
    pub fn pose(&self) -> CameraPose {
        CameraPose {
            center: self.center_norm,
            size: self.size_norm,
            zoom: self.zoom,
            pan: self.pan_norm,
            radius: if self.shape == CameraShape::Circle {
                1.0
            } else {
                self.corner_radius
            },
        }
    }

    /// A medium circle in the bottom-right corner of a 16:9 frame: the bubble's first placement.
    pub fn default_placement() -> CameraKey {
        CameraKey::new(
            0,
            CameraShape::Circle,
            PointF { x: 0.91, y: 0.84 },
            sc_geometry::CAMERA_SIZE_PRESETS[1],
        )
    }

    pub fn new(t_rec_ns: i64, shape: CameraShape, center: PointF, size: f64) -> CameraKey {
        CameraKey {
            t_rec_ns,
            visible: true,
            shape,
            corner_radius: if shape == CameraShape::Rounded {
                0.35
            } else {
                0.0
            },
            center_norm: center,
            size_norm: size,
            zoom: 1.0,
            pan_norm: PointF { x: 0.0, y: 0.0 },
            mirror: false,
            border: None,
            shadow: None,
            easing: Easing::Linear,
            extra: Extra::default(),
        }
    }
}

/// The placement at `t_rec_ns`, or `None` when there are no keys or the camera is hidden. Before
/// the first key the first key applies. Discrete fields (visibility, shape, mirror, border,
/// shadow) come from the key in effect; the pose moves towards the next key unless that key's
/// easing is `Step`.
pub fn camera_at(keys: &[CameraKey], t_rec_ns: i64) -> Option<CameraPlacement> {
    let i = keys
        .iter()
        .rposition(|k| k.t_rec_ns <= t_rec_ns)
        .unwrap_or(0);
    let k0 = keys.get(i)?;
    if !k0.visible {
        return None;
    }
    let pose = match keys.get(i + 1) {
        Some(k1)
            if k1.easing != Easing::Step && k1.t_rec_ns > k0.t_rec_ns && t_rec_ns > k0.t_rec_ns =>
        {
            let f = (t_rec_ns - k0.t_rec_ns) as f64 / (k1.t_rec_ns - k0.t_rec_ns) as f64;
            let f = if k1.easing == Easing::EaseInOut {
                ease_in_out(f)
            } else {
                f.clamp(0.0, 1.0)
            };
            let (a, b) = (k0.pose(), k1.pose());
            if k1.shape == k0.shape {
                lerp_pose(&a, &b, f)
            } else {
                a
            }
        }
        _ => k0.pose(),
    };
    Some(CameraPlacement {
        shape: k0.shape,
        pose,
        mirror: k0.mirror,
        border: k0.border.clone(),
        shadow: k0.shadow.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(t: i64, x: f64, easing: Easing) -> CameraKey {
        CameraKey {
            easing,
            ..CameraKey::new(t, CameraShape::Circle, PointF { x, y: 0.5 }, 0.26)
        }
    }

    #[test]
    fn no_keys_or_hidden_means_no_camera() {
        assert_eq!(camera_at(&[], 0), None);
        let hidden = CameraKey {
            visible: false,
            ..key(0, 0.5, Easing::Step)
        };
        assert_eq!(camera_at(&[hidden], 10), None);
    }

    #[test]
    fn before_the_first_key_the_first_key_applies() {
        let p = camera_at(&[key(1_000, 0.2, Easing::Step)], 0).expect("placement");
        assert_eq!(p.pose.center.x, 0.2);
    }

    #[test]
    fn linear_and_eased_moves_interpolate_step_jumps() {
        let lin = [key(0, 0.2, Easing::Step), key(1_000, 0.6, Easing::Linear)];
        assert!((camera_at(&lin, 250).expect("p").pose.center.x - 0.3).abs() < 1e-12);
        assert_eq!(camera_at(&lin, 1_000).expect("p").pose.center.x, 0.6);
        assert_eq!(camera_at(&lin, 5_000).expect("p").pose.center.x, 0.6);
        let ease = [
            key(0, 0.2, Easing::Step),
            key(1_000, 0.6, Easing::EaseInOut),
        ];
        assert!((camera_at(&ease, 500).expect("p").pose.center.x - 0.4).abs() < 1e-12);
        assert!(camera_at(&ease, 250).expect("p").pose.center.x < 0.3);
        let step = [key(0, 0.2, Easing::Step), key(1_000, 0.6, Easing::Step)];
        assert_eq!(camera_at(&step, 999).expect("p").pose.center.x, 0.2);
        assert_eq!(camera_at(&step, 1_000).expect("p").pose.center.x, 0.6);
    }

    #[test]
    fn a_circle_is_always_fully_round_and_shape_changes_do_not_blend() {
        let mut a = key(0, 0.2, Easing::Step);
        a.corner_radius = 0.1;
        assert_eq!(camera_at(&[a.clone()], 0).expect("p").pose.radius, 1.0);
        let b = CameraKey {
            shape: CameraShape::Rect16x9,
            ..key(1_000, 0.8, Easing::Linear)
        };
        let p = camera_at(&[a, b], 500).expect("p");
        assert_eq!(p.shape, CameraShape::Circle);
        assert_eq!(p.pose.center.x, 0.2);
    }
}
