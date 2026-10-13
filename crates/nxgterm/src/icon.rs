//! The window icon, embedded from the packaged PNG: for the system, and
//! drawn on the integrated title bar.

use std::sync::{Arc, OnceLock};

use nxg_core::image::decode::{self, Rgba};
use nxg_render::palette::rgb;
use nxg_render::{Mask, Shape};
use winit::window::Icon;

/// The 64x64 application icon shipped with the Linux packages.
const PNG: &[u8] = include_bytes!("../../../assets/icons/nxgterm-64.png");

/// The icon for the title bar and task switcher. Shown on Windows and X11;
/// Wayland and macOS take the icon from the desktop entry or the bundle.
pub fn window_icon() -> Option<Icon> {
    let image = decode::png(PNG).ok()?;
    Icon::from_rgba(image.pixels, image.width, image.height).ok()
}

/// The icon `side` pixels square with its top-left corner at window pixel
/// `x`, `y`, as one shape per visible pixel: a one-pixel mask whose
/// coverage is the pixel's alpha, in its color. Both renderers draw shapes
/// the same way, so the icon needs no image pipeline of its own.
pub fn shapes(x: i32, y: i32, side: u32) -> Vec<Shape> {
    static DECODED: OnceLock<Option<Rgba>> = OnceLock::new();
    let Some(image) = DECODED.get_or_init(|| decode::png(PNG).ok()) else {
        return Vec::new();
    };
    let mut masks: [Option<Arc<Mask>>; 256] = std::array::from_fn(|_| None);
    let mut shapes = Vec::new();
    for (i, [r, g, b, a]) in downscale(image, side).into_iter().enumerate() {
        if a == 0 {
            continue;
        }
        let mask = masks[usize::from(a)].get_or_insert_with(|| Arc::new(Mask::new(1, 1, vec![a])));
        let (dx, dy) = ((i as u32 % side) as i32, (i as u32 / side) as i32);
        shapes.push(Shape::Mask {
            x: x + dx,
            y: y + dy,
            mask: Arc::clone(mask),
            color: rgb(r, g, b),
        });
    }
    shapes
}

/// `image` scaled to `side` pixels square, row by row: each pixel averages
/// the block of the image it covers, weighting colors by their alpha so
/// transparent pixels do not darken the edges.
fn downscale(image: &Rgba, side: u32) -> Vec<[u8; 4]> {
    let (width, height) = (u64::from(image.width), u64::from(image.height));
    let side64 = u64::from(side);
    let mut out = Vec::with_capacity(side as usize * side as usize);
    for ty in 0..side64 {
        let rows =
            (ty * height / side64)..((ty + 1) * height / side64).max(ty * height / side64 + 1);
        for tx in 0..side64 {
            let cols =
                (tx * width / side64)..((tx + 1) * width / side64).max(tx * width / side64 + 1);
            let (mut sum, mut alpha, mut count) = ([0u64; 3], 0u64, 0u64);
            for sy in rows.clone() {
                for sx in cols.clone() {
                    let at = ((sy * width + sx) * 4) as usize;
                    let Some(&[r, g, b, a]) = image
                        .pixels
                        .get(at..at + 4)
                        .and_then(|p| p.first_chunk::<4>())
                    else {
                        continue;
                    };
                    let a = u64::from(a);
                    for (total, channel) in sum.iter_mut().zip([r, g, b]) {
                        *total += u64::from(channel) * a;
                    }
                    alpha += a;
                    count += 1;
                }
            }
            let pixel = if alpha == 0 {
                [0; 4]
            } else {
                let [r, g, b] = sum.map(|total| ((total + alpha / 2) / alpha) as u8);
                [r, g, b, (alpha / count) as u8]
            };
            out.push(pixel);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use nxg_core::image::decode::Rgba;

    #[test]
    fn downscaling_averages_blocks_by_their_alpha() {
        // 2x2: opaque red, transparent green, opaque blue, half white.
        let image = Rgba {
            width: 2,
            height: 2,
            pixels: vec![
                255, 0, 0, 255, 0, 255, 0, 0, //
                0, 0, 255, 255, 255, 255, 255, 128,
            ],
        };
        assert_eq!(
            downscale(&image, 2),
            [
                [255, 0, 0, 255],
                [0, 0, 0, 0],
                [0, 0, 255, 255],
                [255, 255, 255, 128]
            ]
        );
        // One pixel: the transparent green adds nothing to the color.
        let [r, g, b, a] = downscale(&image, 1)[0];
        assert_eq!(g, 51, "white only");
        assert!(r > 100 && b > 100, "{r} {b}");
        assert_eq!(a, ((255 + 255 + 128) / 4) as u8);
        assert!(downscale(&image, 0).is_empty());
    }

    #[test]
    fn the_icon_is_drawn_pixel_by_pixel_in_its_rect() {
        let shapes = shapes(10, 9, 16);
        assert!(!shapes.is_empty() && shapes.len() <= 16 * 16);
        for shape in &shapes {
            let nxg_render::Shape::Mask { x, y, mask, .. } = shape else {
                panic!("{shape:?}");
            };
            assert!((10..26).contains(x) && (9..25).contains(y));
            assert_eq!((mask.width(), mask.height()), (1, 1));
            assert!(mask.coverage()[0] > 0, "transparent pixels are skipped");
        }
        assert!(super::shapes(10, 9, 0).is_empty());
    }

    #[test]
    fn embedded_icon_decodes_to_64_pixels_square() {
        let image = decode::png(PNG).unwrap();
        assert_eq!((image.width, image.height), (64, 64));
        assert_eq!(image.pixels.len(), 64 * 64 * 4);
        assert!(window_icon().is_some());
    }
}
