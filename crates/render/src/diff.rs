use crate::RenderedFrame;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DiffStats {
    pub max: u8,
    pub mean: f64,
}

/// Per-byte absolute difference over every byte of the packed data. Frames of different size,
/// format or length compare as entirely different (`max` 255, `mean` 255).
pub fn diff(a: &RenderedFrame, b: &RenderedFrame) -> DiffStats {
    if a.size != b.size || a.format != b.format || a.data.len() != b.data.len() {
        return DiffStats {
            max: 255,
            mean: 255.0,
        };
    }
    if a.data.is_empty() {
        return DiffStats::default();
    }
    let mut max = 0u8;
    let mut sum = 0u64;
    for (x, y) in a.data.iter().zip(&b.data) {
        let d = x.abs_diff(*y);
        max = max.max(d);
        sum += u64::from(d);
    }
    DiffStats {
        max,
        mean: sum as f64 / a.data.len() as f64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OutputFormat;
    use sc_geometry::SizePx;

    fn frame(data: Vec<u8>) -> RenderedFrame {
        RenderedFrame {
            size: SizePx { w: 1, h: 1 },
            format: OutputFormat::Rgba8,
            data,
        }
    }

    #[test]
    fn diff_counts_every_byte() {
        let s = diff(&frame(vec![0, 10, 20, 255]), &frame(vec![2, 10, 20, 251]));
        assert_eq!(s.max, 4);
        assert!((s.mean - 1.5).abs() < 1e-12);
    }

    #[test]
    fn mismatched_frames_are_fully_different() {
        let s = diff(&frame(vec![0; 4]), &frame(vec![0; 8]));
        assert_eq!(s.max, 255);
    }
}
