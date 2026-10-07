//! Caller-owned 0RGB framebuffer and pixel primitives.

use crate::palette::Rgb;

/// A `width x height` row-major view over a pixel buffer.
#[derive(Debug)]
pub struct Frame<'a> {
    pixels: &'a mut [u32],
    width: u32,
    height: u32,
}

impl<'a> Frame<'a> {
    /// Returns `None` if `pixels` is smaller than `width * height`.
    pub fn new(pixels: &'a mut [u32], width: u32, height: u32) -> Option<Self> {
        let len = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
        (pixels.len() >= len).then_some(Self {
            pixels,
            width,
            height,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<Rgb> {
        self.index(x, y).map(|i| self.pixels[i])
    }

    pub fn clear(&mut self, color: Rgb) {
        let len = self.width as usize * self.height as usize;
        self.pixels[..len].fill(color);
    }

    /// Fills a rectangle, clipped to the frame.
    pub fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: Rgb) {
        let x_end = x.saturating_add(w).min(self.width);
        let y_end = y.saturating_add(h).min(self.height);
        if x >= x_end {
            return;
        }
        for row in y..y_end {
            let start = row as usize * self.width as usize;
            self.pixels[start + x as usize..start + x_end as usize].fill(color);
        }
    }

    /// Blends `color` over the pixel at (x, y) with `alpha` coverage (0-255).
    /// Coordinates outside the frame are ignored.
    pub fn blend(&mut self, x: i64, y: i64, color: Rgb, alpha: u8) {
        let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
            return;
        };
        let Some(i) = self.index(x, y) else {
            return;
        };
        let dst = self.pixels[i];
        let a = u32::from(alpha);
        let mix = |shift: u32| {
            let s = (color >> shift) & 0xff;
            let d = (dst >> shift) & 0xff;
            ((s * a + d * (255 - a) + 127) / 255) << shift
        };
        self.pixels[i] = mix(16) | mix(8) | mix(0);
    }

    fn index(&self, x: u32, y: u32) -> Option<usize> {
        (x < self.width && y < self.height).then(|| y as usize * self.width as usize + x as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::rgb;

    #[test]
    fn rejects_undersized_buffers() {
        let mut pixels = vec![0; 5];
        assert!(Frame::new(&mut pixels, 3, 2).is_none());
        assert!(Frame::new(&mut pixels, 5, 1).is_some());
    }

    #[test]
    fn fill_rect_is_clipped() {
        let mut pixels = vec![0; 9];
        let mut frame = Frame::new(&mut pixels, 3, 3).unwrap();
        frame.fill_rect(1, 1, 10, 10, 7);
        assert_eq!(pixels, [0, 0, 0, 0, 7, 7, 0, 7, 7]);
    }

    #[test]
    fn blend_mixes_by_alpha_and_ignores_outside() {
        let mut pixels = vec![rgb(0, 0, 0); 2];
        let mut frame = Frame::new(&mut pixels, 2, 1).unwrap();
        frame.blend(0, 0, rgb(255, 255, 255), 255);
        frame.blend(1, 0, rgb(200, 100, 0), 128);
        frame.blend(-1, 0, rgb(255, 255, 255), 255);
        frame.blend(2, 0, rgb(255, 255, 255), 255);
        assert_eq!(frame.pixel(0, 0), Some(rgb(255, 255, 255)));
        assert_eq!(frame.pixel(1, 0), Some(rgb(100, 50, 0)));
        assert_eq!(frame.pixel(2, 0), None);
    }
}
