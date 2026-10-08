//! The window icon, embedded from the packaged PNG.

use nxg_core::image::decode;
use winit::window::Icon;

/// The 64x64 application icon shipped with the Linux packages.
const PNG: &[u8] = include_bytes!("../../../assets/icons/nxgterm-64.png");

/// The icon for the title bar and task switcher. Shown on Windows and X11;
/// Wayland and macOS take the icon from the desktop entry or the bundle.
pub fn window_icon() -> Option<Icon> {
    let image = decode::png(PNG).ok()?;
    Icon::from_rgba(image.pixels, image.width, image.height).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_icon_decodes_to_64_pixels_square() {
        let image = decode::png(PNG).unwrap();
        assert_eq!((image.width, image.height), (64, 64));
        assert_eq!(image.pixels.len(), 64 * 64 * 4);
        assert!(window_icon().is_some());
    }
}
