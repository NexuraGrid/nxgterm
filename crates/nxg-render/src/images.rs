//! Image placements as draw rectangles, shared by both renderers, plus the
//! CPU blit.
//!
//! Scaling is nearest-neighbor with pixel-center sampling ([`sample`]):
//! exact integer math that the GPU shader reproduces bit for bit, so both
//! renderers agree and pixel art stays crisp. Images are clipped to the
//! grid area, never drawn over the padding.

use nxg_core::image::{PixelRect, SrcRect};
use nxg_core::{CellPixels, Terminal};

use crate::frame::Frame;
use crate::paint::Layout;
use crate::palette::rgb;

/// Largest destination side drawn, in pixels; keeps the shader's 32-bit
/// sampling math from overflowing on absurd sizes.
pub const MAX_DEST: u32 = 65_535;

/// One placement to draw, in window pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageDraw {
    /// `nxg_core::image::Image::key`.
    pub key: u64,
    pub dest: PixelRect,
    pub src: SrcRect,
    pub z: i32,
}

/// The grid area in window pixels as `(x, y, width, height)`.
pub fn grid_clip(term: &Terminal, layout: Layout) -> (u32, u32, u32, u32) {
    let size = term.size();
    (
        layout.padding,
        layout.padding,
        u32::from(size.cols()).saturating_mul(layout.cell.width),
        u32::from(size.rows()).saturating_mul(layout.cell.height),
    )
}

/// Placements below text (`z < 0`) or above it, in draw order (by z, then
/// creation), skipping those outside the grid area.
pub fn draws(term: &Terminal, layout: Layout, above: bool) -> Vec<ImageDraw> {
    let cell = CellPixels::new(layout.cell.width, layout.cell.height);
    let (cx, cy, cw, ch) = grid_clip(term, layout);
    let (cx, cy) = (i64::from(cx), i64::from(cy));
    let (cx1, cy1) = (cx + i64::from(cw), cy + i64::from(ch));
    let store = term.images();
    let mut out: Vec<ImageDraw> = store
        .placements()
        .iter()
        .filter(|p| (p.z >= 0) == above && store.image(p.image).is_some())
        .filter_map(|p| {
            let rect = p.pixel_rect(cell);
            let dest = PixelRect {
                x: rect.x + i64::from(layout.padding),
                y: rect.y + i64::from(layout.padding),
                width: rect.width.min(MAX_DEST),
                height: rect.height.min(MAX_DEST),
            };
            let visible = dest.width > 0
                && dest.height > 0
                && p.src.width > 0
                && p.src.height > 0
                && dest.x < cx1
                && dest.y < cy1
                && dest.x + i64::from(dest.width) > cx
                && dest.y + i64::from(dest.height) > cy;
            visible.then_some(ImageDraw {
                key: p.image,
                dest,
                src: p.src,
                z: p.z,
            })
        })
        .collect();
    out.sort_by_key(|d| d.z);
    out
}

/// Source coordinate for destination pixel `d` (0-based) when `src_len`
/// pixels are stretched over `dest_len`: the source pixel under the
/// destination pixel's center.
pub fn sample(d: u32, dest_len: u32, src_len: u32) -> u32 {
    if dest_len == 0 {
        return 0;
    }
    let s = (u64::from(d) * 2 + 1) * u64::from(src_len) / (u64::from(dest_len) * 2);
    (s as u32).min(src_len.saturating_sub(1))
}

/// Blits the placements selected by `above` into `frame` (CPU renderer).
pub fn paint(term: &Terminal, frame: &mut Frame<'_>, layout: Layout, above: bool) {
    let (cx, cy, cw, ch) = grid_clip(term, layout);
    let x0 = i64::from(cx);
    let y0 = i64::from(cy);
    let x1 = (x0 + i64::from(cw)).min(i64::from(frame.width()));
    let y1 = (y0 + i64::from(ch)).min(i64::from(frame.height()));
    for draw in draws(term, layout, above) {
        let Some(image) = term.images().image(draw.key) else {
            continue;
        };
        let d = draw.dest;
        let (sx, sy) = (draw.src.x, draw.src.y);
        let left = d.x.max(x0);
        let right = (d.x + i64::from(d.width)).min(x1);
        let top = d.y.max(y0);
        let bottom = (d.y + i64::from(d.height)).min(y1);
        for y in top..bottom {
            let src_y = sy + sample((y - d.y) as u32, d.height, draw.src.height);
            for x in left..right {
                let src_x = sx + sample((x - d.x) as u32, d.width, draw.src.width);
                let [r, g, b, a] = image.pixel(src_x, src_y);
                if a > 0 {
                    frame.blend(x, y, rgb(r, g, b), a);
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::paint::CellSize;
    use nxg_core::TermSize;

    /// Standard padded base64, for building kitty commands in tests.
    pub(crate) fn encode_base64(data: &[u8]) -> String {
        const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in data.chunks(3) {
            let b = [
                chunk[0],
                *chunk.get(1).unwrap_or(&0),
                *chunk.get(2).unwrap_or(&0),
            ];
            let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
            for i in 0..4 {
                let c = if i <= chunk.len() {
                    ABC[(n >> (18 - 6 * i) & 63) as usize]
                } else {
                    b'='
                };
                out.push(c as char);
            }
        }
        out
    }

    const LAYOUT: Layout = Layout {
        cell: CellSize {
            width: 2,
            height: 2,
        },
        padding: 1,
    };

    /// A `w x h` image of `color` pixels displayed through kitty at the
    /// cursor, plus `extra` control keys.
    fn show(term: &mut Terminal, w: u32, h: u32, color: [u8; 4], extra: &str) {
        let data = encode_base64(&color.repeat((w * h) as usize));
        term.advance(format!("\x1b_Ga=T,s={w},v={h},C=1{extra};{data}\x1b\\").as_bytes());
    }

    fn term(cols: u16, rows: u16) -> Terminal {
        let mut term = Terminal::new(TermSize::new(cols, rows).unwrap());
        term.set_cell_pixels(2, 2);
        term.advance(b"\x1b[?25l");
        term
    }

    #[test]
    fn sample_maps_pixel_centers() {
        assert_eq!(
            (0..4).map(|d| sample(d, 4, 2)).collect::<Vec<_>>(),
            [0, 0, 1, 1]
        );
        assert_eq!((0..2).map(|d| sample(d, 2, 4)).collect::<Vec<_>>(), [1, 3]);
        assert_eq!(
            (0..3).map(|d| sample(d, 3, 3)).collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert_eq!(sample(5, 0, 3), 0);
        assert_eq!(sample(MAX_DEST - 1, MAX_DEST, 10_000), 9_999);
    }

    #[test]
    fn draws_split_by_z_and_sort_within_a_layer() {
        let mut t = term(4, 2);
        show(&mut t, 1, 1, [0; 4], ",z=5");
        show(&mut t, 1, 1, [0; 4], ",z=-1");
        show(&mut t, 1, 1, [0; 4], ",z=0");
        show(&mut t, 1, 1, [0; 4], ",z=-9");
        let z = |above| -> Vec<i32> { draws(&t, LAYOUT, above).iter().map(|d| d.z).collect() };
        assert_eq!(z(false), [-9, -1]);
        assert_eq!(z(true), [0, 5]);
    }

    #[test]
    fn draws_are_offset_by_padding_and_skip_invisible_ones() {
        let mut t = term(4, 2);
        t.advance(b"\x1b[2;2H");
        show(&mut t, 3, 1, [0; 4], "");
        show(&mut t, 1, 1, [0; 4], ",X=1,x=1"); // empty source
        let d = draws(&t, LAYOUT, true);
        assert_eq!(d.len(), 1);
        assert_eq!((d[0].dest.x, d[0].dest.y, d[0].dest.width), (3, 3, 3));
        assert_eq!(grid_clip(&t, LAYOUT), (1, 1, 8, 4));
    }

    fn render(t: &Terminal, above: bool) -> Vec<u32> {
        // Grid 4x2 cells of 2px plus 1px padding: 10x6 window.
        let mut pixels = vec![0; 10 * 6];
        let mut frame = Frame::new(&mut pixels, 10, 6).unwrap();
        paint(t, &mut frame, LAYOUT, above);
        pixels
    }

    #[test]
    fn blits_scaled_image_into_its_cells() {
        let mut t = term(4, 2);
        // 1x1 red stretched over 1x1 cells (c=1,r=1) = 2x2 pixels.
        show(&mut t, 1, 1, [255, 0, 0, 255], ",c=1,r=1");
        let pixels = render(&t, true);
        let red = rgb(255, 0, 0);
        let at = |x: usize, y: usize| pixels[y * 10 + x];
        assert_eq!([at(1, 1), at(2, 1), at(1, 2), at(2, 2)], [red; 4]);
        assert_eq!(at(0, 0), 0, "padding untouched");
        assert_eq!(at(3, 1), 0);
        assert!(render(&t, false).iter().all(|&p| p == 0), "no z<0 images");
    }

    #[test]
    fn clips_to_the_grid_area() {
        let mut t = term(4, 2);
        t.advance(b"\x1b[2;4H");
        show(&mut t, 20, 20, [0, 0, 255, 255], "");
        let pixels = render(&t, true);
        let blue = rgb(0, 0, 255);
        let inked: Vec<(usize, usize)> = (0..60)
            .filter(|&i| pixels[i] == blue)
            .map(|i| (i % 10, i / 10))
            .collect();
        assert_eq!(inked, [(7, 3), (8, 3), (7, 4), (8, 4)]);
    }

    #[test]
    fn blends_by_alpha_and_skips_transparent_pixels() {
        let mut t = term(4, 2);
        let data = encode_base64(&[255, 255, 255, 128, 9, 9, 9, 0]);
        t.advance(format!("\x1b_Ga=T,s=2,v=1;{data}\x1b\\").as_bytes());
        let pixels = render(&t, true);
        assert_eq!(pixels[11], rgb(128, 128, 128));
        assert_eq!(pixels[12], 0);
    }

    #[test]
    fn sixel_images_render_too() {
        let mut t = term(4, 2);
        t.advance(b"\x1b[?80h\x1bP0;1q#1;2;0;100;0#1!2~\x1b\\");
        let pixels = render(&t, true);
        let green = rgb(0, 255, 0);
        // 2x6 px image clipped to the 4px tall grid.
        assert_eq!(pixels.iter().filter(|&&p| p == green).count(), 2 * 4);
    }
}
