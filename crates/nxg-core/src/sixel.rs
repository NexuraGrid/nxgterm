//! Sixel image decoder (DCS `P1;P2;P3 q … ST`).
//!
//! Supports raster attributes (`"`), color registers (`#`, HLS and RGB),
//! repeats (`!`), graphics carriage return (`$`) and new line (`-`) with
//! the VT340 default palette. Pixels with no sixel drawn on them are
//! transparent when `P2 = 1`, otherwise they take color register 0 (the
//! VT340 background). The pixel aspect ratio (`P1`, `Pan;Pad`) is ignored:
//! pixels are square, as in modern terminals.

use crate::image::decode::Rgba;

/// Largest sixel image width or height; pixels beyond are dropped.
pub const MAX_SIZE: u32 = 4096;
/// Color registers; higher indexes wrap around.
const REGISTERS: usize = 256;

/// VT340 default colors, in percent.
const VT340: [[u8; 3]; 16] = [
    [0, 0, 0],
    [20, 20, 80],
    [80, 13, 13],
    [20, 80, 20],
    [80, 20, 80],
    [20, 80, 80],
    [80, 80, 20],
    [53, 53, 53],
    [26, 26, 26],
    [33, 33, 60],
    [60, 26, 26],
    [33, 60, 33],
    [60, 33, 60],
    [33, 60, 60],
    [60, 60, 33],
    [80, 80, 80],
];

fn percent(value: u16) -> u8 {
    ((u32::from(value.min(100)) * 255 + 50) / 100) as u8
}

/// DEC HLS (hue 0 = blue, 120 = red, 240 = green; lightness and
/// saturation in percent) to RGB8.
pub fn hls_to_rgb(hue: u16, lightness: u16, saturation: u16) -> [u8; 3] {
    // Rotate DEC hue to the usual HSL hue (0 = red).
    let h = f64::from((u32::from(hue) + 240) % 360);
    let l = f64::from(lightness.min(100)) / 100.0;
    let s = f64::from(saturation.min(100)) / 100.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    [to(r), to(g), to(b)]
}

/// Which multi-byte command is collecting numeric parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    None,
    Raster,
    Color,
    Repeat,
}

/// Streaming sixel decoder fed one byte at a time.
#[derive(Debug)]
pub struct SixelDecoder {
    transparent: bool,
    palette: Vec<[u8; 3]>,
    color: usize,
    /// Pixels as RGBA, `stride` wide, `rows` tall; grows on demand.
    pixels: Vec<u8>,
    stride: u32,
    rows: u32,
    /// Extent actually drawn plus the declared raster size.
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    pending: Pending,
    params: Vec<u16>,
}

impl SixelDecoder {
    /// Starts an image with the DCS parameters `P1;P2;P3`.
    pub fn new(params: &[u16]) -> Self {
        let mut palette = vec![[0; 3]; REGISTERS];
        for (slot, rgb) in palette.iter_mut().zip(VT340) {
            *slot = rgb.map(|v| percent(u16::from(v)));
        }
        Self {
            transparent: params.get(1) == Some(&1),
            palette,
            color: 0,
            pixels: Vec::new(),
            stride: 0,
            rows: 0,
            width: 0,
            height: 0,
            x: 0,
            y: 0,
            pending: Pending::None,
            params: Vec::new(),
        }
    }

    /// Feeds one byte of the DCS body.
    pub fn put(&mut self, byte: u8) {
        if self.pending != Pending::None {
            match byte {
                b'0'..=b'9' => {
                    if self.params.is_empty() {
                        self.params.push(0);
                    }
                    if let Some(last) = self.params.last_mut() {
                        *last = last
                            .saturating_mul(10)
                            .saturating_add(u16::from(byte - b'0'));
                    }
                    return;
                }
                b';' => {
                    if self.params.is_empty() {
                        self.params.push(0);
                    }
                    if self.params.len() < 8 {
                        self.params.push(0);
                    }
                    return;
                }
                _ => {
                    if self.dispatch(byte) {
                        return;
                    }
                }
            }
        }
        match byte {
            b'?'..=b'~' => self.sixel(byte - b'?', 1),
            b'"' => self.start(Pending::Raster),
            b'#' => self.start(Pending::Color),
            b'!' => self.start(Pending::Repeat),
            b'$' => self.x = 0,
            b'-' => {
                self.x = 0;
                self.y = self.y.saturating_add(6);
            }
            _ => {} // Whitespace and anything unknown.
        }
    }

    fn start(&mut self, pending: Pending) {
        self.pending = pending;
        self.params.clear();
    }

    /// Ends the pending command at `byte`; returns true when `byte` was
    /// consumed (a repeated sixel).
    fn dispatch(&mut self, byte: u8) -> bool {
        let pending = std::mem::replace(&mut self.pending, Pending::None);
        let p = |i: usize| self.params.get(i).copied().unwrap_or(0);
        match pending {
            Pending::None => false,
            Pending::Raster => {
                self.width = self.width.max(u32::from(p(2)).min(MAX_SIZE));
                self.height = self.height.max(u32::from(p(3)).min(MAX_SIZE));
                false
            }
            Pending::Color => {
                let index = usize::from(p(0)) % REGISTERS;
                if self.params.len() >= 5 {
                    self.palette[index] = match p(1) {
                        1 => hls_to_rgb(p(2), p(3), p(4)),
                        _ => [percent(p(2)), percent(p(3)), percent(p(4))],
                    };
                }
                self.color = index;
                false
            }
            Pending::Repeat => {
                if matches!(byte, b'?'..=b'~') {
                    self.sixel(byte - b'?', u32::from(p(0)).max(1));
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Draws `count` columns of the 6-pixel pattern `bits` at the cursor.
    fn sixel(&mut self, bits: u8, count: u32) {
        let start = self.x;
        self.x = self.x.saturating_add(count);
        if bits == 0 || start >= MAX_SIZE || self.y >= MAX_SIZE {
            self.width = self.width.max(self.x.min(MAX_SIZE));
            return;
        }
        let end = self.x.min(MAX_SIZE);
        let bottom = (0..6u32).rev().find(|b| bits & (1 << b) != 0).unwrap_or(0);
        let rows_end = (self.y + bottom + 1).min(MAX_SIZE);
        self.grow(end, rows_end);
        let [r, g, b] = self.palette[self.color];
        for bit in 0..6u32 {
            let y = self.y + bit;
            if bits & (1 << bit) == 0 || y >= MAX_SIZE {
                continue;
            }
            let row = y as usize * self.stride as usize;
            for x in start..end {
                let i = (row + x as usize) * 4;
                self.pixels[i..i + 4].copy_from_slice(&[r, g, b, 255]);
            }
        }
        self.width = self.width.max(end);
        self.height = self.height.max(rows_end);
    }

    /// Makes the buffer at least `width x height`, doubling as it grows.
    fn grow(&mut self, width: u32, height: u32) {
        if width <= self.stride && height <= self.rows {
            return;
        }
        let stride = if width > self.stride {
            width.max(self.stride * 2).min(MAX_SIZE)
        } else {
            self.stride
        };
        let rows = if height > self.rows {
            height.max(self.rows * 2).min(MAX_SIZE)
        } else {
            self.rows
        };
        let mut pixels = vec![0; stride as usize * rows as usize * 4];
        let old = self.stride as usize * 4;
        for y in 0..self.rows as usize {
            let dst = y * stride as usize * 4;
            pixels[dst..dst + old].copy_from_slice(&self.pixels[y * old..(y + 1) * old]);
        }
        self.pixels = pixels;
        self.stride = stride;
        self.rows = rows;
    }

    /// The finished image, or `None` when nothing was drawn or declared.
    pub fn finish(mut self) -> Option<Rgba> {
        if self.pending != Pending::None {
            self.dispatch(0);
        }
        let (width, height) = (self.width, self.height);
        if width == 0 || height == 0 {
            return None;
        }
        let background = self.palette[0];
        let mut out = vec![0; width as usize * height as usize * 4];
        for y in 0..height as usize {
            for x in 0..width as usize {
                let dst = (y * width as usize + x) * 4;
                let inside = x < self.stride as usize && y < self.rows as usize;
                let src = (y * self.stride as usize + x) * 4;
                if inside && self.pixels[src + 3] != 0 {
                    out[dst..dst + 4].copy_from_slice(&self.pixels[src..src + 4]);
                } else if !self.transparent {
                    let [r, g, b] = background;
                    out[dst..dst + 4].copy_from_slice(&[r, g, b, 255]);
                }
            }
        }
        Some(Rgba {
            width,
            height,
            pixels: out,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(params: &[u16], body: &[u8]) -> Option<Rgba> {
        let mut decoder = SixelDecoder::new(params);
        body.iter().for_each(|&b| decoder.put(b));
        decoder.finish()
    }

    fn pixel(image: &Rgba, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * image.width + x) * 4) as usize;
        image.pixels[i..i + 4].try_into().unwrap()
    }

    const RED: [u8; 4] = [255, 0, 0, 255];
    const NONE: [u8; 4] = [0; 4];

    #[test]
    fn single_full_sixel_is_one_by_six() {
        // `~` sets all six bits.
        let image = decode(&[0, 1], b"#1;2;100;0;0#1~").unwrap();
        assert_eq!((image.width, image.height), (1, 6));
        assert!((0..6).all(|y| pixel(&image, 0, y) == RED));
    }

    #[test]
    fn bits_map_top_to_bottom() {
        // `@` = 0b000001 (top pixel), `_` = 0b100000 (bottom pixel).
        let image = decode(&[0, 1], b"#1;2;100;0;0@_").unwrap();
        assert_eq!((image.width, image.height), (2, 6));
        assert_eq!(pixel(&image, 0, 0), RED);
        assert_eq!(pixel(&image, 0, 1), NONE);
        assert_eq!(pixel(&image, 1, 5), RED);
        assert_eq!(pixel(&image, 1, 0), NONE);
    }

    #[test]
    fn height_stops_at_lowest_drawn_pixel() {
        let image = decode(&[0, 1], b"#1;2;100;0;0F").unwrap(); // 0b000111
        assert_eq!((image.width, image.height), (1, 3));
    }

    #[test]
    fn repeat_draws_many_columns() {
        let image = decode(&[0, 1], b"#1;2;100;0;0!5~").unwrap();
        assert_eq!(image.width, 5);
        assert_eq!(pixel(&image, 4, 5), RED);
    }

    #[test]
    fn carriage_return_overlays_and_newline_moves_down_six() {
        let image = decode(&[0, 1], b"#1;2;100;0;0@$#2;2;0;100;0A-#1@").unwrap();
        assert_eq!((image.width, image.height), (1, 7));
        // `A` = 0b000010; overlay keeps the red top pixel.
        assert_eq!(pixel(&image, 0, 0), RED);
        assert_eq!(pixel(&image, 0, 1), [0, 255, 0, 255]);
        assert_eq!(pixel(&image, 0, 6), RED);
    }

    #[test]
    fn raster_attributes_set_minimum_size() {
        let image = decode(&[0, 1], b"\"1;1;4;8#1;2;100;0;0@").unwrap();
        assert_eq!((image.width, image.height), (4, 8));
        assert_eq!(pixel(&image, 3, 7), NONE);
    }

    #[test]
    fn default_palette_and_opaque_background() {
        // Register 2 is VT340 red (80%, 13%, 13%); P2=0 fills with color 0.
        let image = decode(&[0, 0], b"#2@").unwrap();
        assert_eq!(pixel(&image, 0, 0), [204, 33, 33, 255]);
        assert_eq!((image.width, image.height), (1, 1));
        let image = decode(&[], b"\"1;1;2;1#2@").unwrap();
        assert_eq!(pixel(&image, 1, 0), [0, 0, 0, 255], "background is color 0");
    }

    #[test]
    fn hls_uses_dec_hue_origin() {
        assert_eq!(hls_to_rgb(0, 50, 100), [0, 0, 255], "0 is blue");
        assert_eq!(hls_to_rgb(120, 50, 100), [255, 0, 0], "120 is red");
        assert_eq!(hls_to_rgb(240, 50, 100), [0, 255, 0], "240 is green");
        assert_eq!(hls_to_rgb(0, 100, 0), [255, 255, 255]);
        assert_eq!(hls_to_rgb(0, 0, 0), [0, 0, 0]);
        let image = decode(&[0, 1], b"#5;1;120;50;100#5@").unwrap();
        assert_eq!(pixel(&image, 0, 0), RED);
    }

    #[test]
    fn empty_image_is_none() {
        assert!(decode(&[0, 1], b"").is_none());
        assert!(decode(&[0, 1], b"$-").is_none());
    }

    #[test]
    fn huge_sizes_are_capped() {
        let image = decode(&[0, 1], b"#1!65535~\"1;1;65535;65535").unwrap();
        assert_eq!((image.width, image.height), (MAX_SIZE, MAX_SIZE));
        let mut tall = b"#1~".to_vec();
        for _ in 0..1000 {
            tall.extend_from_slice(b"-~");
        }
        let image = decode(&[0, 1], &tall).unwrap();
        assert_eq!(image.height, MAX_SIZE);
    }

    #[test]
    fn color_registers_wrap_and_bad_params_do_not_panic() {
        let image = decode(&[0, 1], b"#65535;2;100;0;0#65535~#;;;;#9;9@!@\"").unwrap();
        assert_eq!(pixel(&image, 0, 0), RED);
    }
}
