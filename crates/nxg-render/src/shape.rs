//! Shapes drawn in window pixels over everything else, such as the window
//! buttons of an integrated title bar: solid rectangles and anti-aliased
//! coverage masks. Masks are rasterized once on the CPU, so both renderers
//! draw exactly the same pixels.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use crate::frame::Frame;
use crate::palette::Rgb;

/// Something drawn at window pixel `x`, `y` (its top-left corner).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shape {
    /// An opaque rectangle.
    Rect {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        color: Rgb,
    },
    /// `color` blended by the coverage of `mask`.
    Mask {
        x: i32,
        y: i32,
        mask: Arc<Mask>,
        color: Rgb,
    },
}

/// A straight line from `from` to `to`, in pixels from the top-left corner
/// of a mask: pixel `x`, `y` covers `x..x + 1`, `y..y + 1`, so a thin line
/// through pixel centers sits on `n + 0.5`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub from: (f32, f32),
    pub to: (f32, f32),
}

/// A `width x height` coverage bitmap (0-255, row-major), with a key that
/// identifies its contents so renderers can cache it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mask {
    width: u32,
    height: u32,
    coverage: Vec<u8>,
    key: u64,
}

impl Mask {
    /// A mask from its coverage; `coverage` is cut or padded with zeros to
    /// `width * height` bytes.
    pub fn new(width: u32, height: u32, mut coverage: Vec<u8>) -> Self {
        coverage.resize(width as usize * height as usize, 0);
        let mut hasher = DefaultHasher::new();
        (width, height, &coverage).hash(&mut hasher);
        Self {
            width,
            height,
            coverage,
            key: hasher.finish(),
        }
    }

    /// `segments` stroked `thickness` pixels wide with square caps (each
    /// end reaches `thickness / 2` past its point) on a `width x height`
    /// mask. Edges are anti-aliased with a one-pixel ramp, so axis-aligned
    /// strokes placed on the pixel grid (see [`Segment`]) come out crisp.
    pub fn stroke(width: u32, height: u32, segments: &[Segment], thickness: f32) -> Self {
        let half = thickness.max(0.0) / 2.0;
        let mut coverage = vec![0u8; width as usize * height as usize];
        for y in 0..height {
            for x in 0..width {
                let center = (x as f32 + 0.5, y as f32 + 0.5);
                let distance = segments
                    .iter()
                    .map(|segment| segment.distance(center, half))
                    .fold(f32::INFINITY, f32::min);
                let alpha = (0.5 - distance).clamp(0.0, 1.0);
                coverage[(y * width + x) as usize] = (alpha * 255.0).round() as u8;
            }
        }
        Self::new(width, height, coverage)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn coverage(&self) -> &[u8] {
        &self.coverage
    }

    /// Equal for masks with equal contents.
    pub fn key(&self) -> u64 {
        self.key
    }
}

impl Segment {
    /// Signed distance from `point` to the edge of this segment stroked
    /// `half` pixels to each side with square caps: negative inside.
    fn distance(self, (px, py): (f32, f32), half: f32) -> f32 {
        let (dx, dy) = (self.to.0 - self.from.0, self.to.1 - self.from.1);
        let length = (dx * dx + dy * dy).sqrt();
        let (mx, my) = (
            (self.from.0 + self.to.0) / 2.0,
            (self.from.1 + self.to.1) / 2.0,
        );
        let (rx, ry) = (px - mx, py - my);
        if length == 0.0 {
            // A dot: a square `2 * half` wide.
            return rx.abs().max(ry.abs()) - half;
        }
        let (ux, uy) = (dx / length, dy / length);
        let along = (rx * ux + ry * uy).abs() - length / 2.0 - half;
        let across = (rx * uy - ry * ux).abs() - half;
        along.max(across)
    }
}

/// Paints `shapes` over `frame`, in order.
pub fn paint(shapes: &[Shape], frame: &mut Frame<'_>) {
    for shape in shapes {
        match shape {
            &Shape::Rect {
                x,
                y,
                width,
                height,
                color,
            } => {
                // Clip the part left of or above the frame.
                let (left, top) = (x.max(0), y.max(0));
                let width = width.saturating_sub(left.abs_diff(x));
                let height = height.saturating_sub(top.abs_diff(y));
                frame.fill_rect(left as u32, top as u32, width, height, color);
            }
            Shape::Mask { x, y, mask, color } => {
                let width = mask.width as usize;
                for (i, &alpha) in mask.coverage.iter().enumerate() {
                    if alpha > 0 {
                        let (mx, my) = ((i % width) as i64, (i / width) as i64);
                        frame.blend(i64::from(*x) + mx, i64::from(*y) + my, *color, alpha);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(mask: &Mask) -> Vec<Vec<u8>> {
        mask.coverage()
            .chunks(mask.width() as usize)
            .map(<[u8]>::to_vec)
            .collect()
    }

    #[test]
    fn a_thin_line_on_pixel_centers_is_crisp() {
        let line = Segment {
            from: (1.5, 2.5),
            to: (3.5, 2.5),
        };
        let mask = Mask::stroke(5, 4, &[line], 1.0);
        assert_eq!(
            rows(&mask),
            [
                [0, 0, 0, 0, 0],
                [0, 0, 0, 0, 0],
                [0, 255, 255, 255, 0],
                [0, 0, 0, 0, 0],
            ]
        );
    }

    #[test]
    fn a_two_pixel_line_on_pixel_edges_is_crisp_and_square_capped() {
        let line = Segment {
            from: (1.0, 2.0),
            to: (1.0, 4.0),
        };
        let mask = Mask::stroke(3, 6, &[line], 2.0);
        assert_eq!(
            rows(&mask),
            [
                [0, 0, 0],
                [255, 255, 0],
                [255, 255, 0],
                [255, 255, 0],
                [255, 255, 0],
                [0, 0, 0],
            ]
        );
    }

    #[test]
    fn diagonal_lines_are_anti_aliased() {
        let line = Segment {
            from: (0.0, 0.0),
            to: (8.0, 8.0),
        };
        let mask = Mask::stroke(8, 8, &[line], 1.0);
        let partial = mask
            .coverage()
            .iter()
            .filter(|&&a| a > 0 && a < 255)
            .count();
        assert!(partial > 0, "{:?}", rows(&mask));
        assert_eq!(mask.coverage()[0], 255, "on the line");
        assert_eq!(mask.coverage()[7], 0, "far corner");
    }

    #[test]
    fn overlapping_segments_take_the_highest_coverage() {
        let across = Segment {
            from: (0.5, 1.5),
            to: (2.5, 1.5),
        };
        let down = Segment {
            from: (1.5, 0.5),
            to: (1.5, 2.5),
        };
        let mask = Mask::stroke(3, 3, &[across, down], 1.0);
        assert_eq!(rows(&mask), [[0, 255, 0], [255, 255, 255], [0, 255, 0]]);
    }

    #[test]
    fn keys_follow_the_contents() {
        let a = Mask::new(2, 1, vec![1, 2]);
        assert_eq!(a.key(), Mask::new(2, 1, vec![1, 2]).key());
        assert_ne!(a.key(), Mask::new(2, 1, vec![2, 1]).key());
        assert_ne!(a.key(), Mask::new(1, 2, vec![1, 2]).key());
        assert_eq!(Mask::new(2, 2, vec![9]).coverage(), [9, 0, 0, 0]);
    }

    #[test]
    fn shapes_paint_in_order_and_clip_to_the_frame() {
        let mut pixels = vec![0; 16];
        let mut frame = Frame::new(&mut pixels, 4, 4).unwrap();
        let mask = Arc::new(Mask::new(2, 1, vec![255, 128]));
        paint(
            &[
                Shape::Rect {
                    x: -1,
                    y: -1,
                    width: 3,
                    height: 3,
                    color: 0x00ff_0000,
                },
                Shape::Mask {
                    x: 1,
                    y: 1,
                    mask,
                    color: 0x0000_00ff,
                },
            ],
            &mut frame,
        );
        assert_eq!(frame.pixel(0, 0), Some(0x00ff_0000));
        assert_eq!(frame.pixel(2, 0), Some(0), "clipped width");
        assert_eq!(frame.pixel(1, 1), Some(0x0000_00ff), "full coverage");
        assert_eq!(frame.pixel(2, 1), Some(0x0000_0080), "half over black");
    }
}
