//! Surface format choice.
//!
//! The CPU renderer blends 8-bit sRGB-encoded values directly (no
//! linearization). To match it, the GPU renderer prefers a non-sRGB
//! (`*Unorm`) surface, so palette values are written and blended as-is.
//! When only sRGB formats exist, the shader converts colors to linear so
//! solid colors still match; glyph edges then blend in linear space and
//! look slightly different.

use wgpu::TextureFormat;

use crate::palette::Rgb;

/// Picks a surface format from `available`.
///
/// Prefers 8-bit non-sRGB BGRA/RGBA, then any non-sRGB format, then the
/// first one. Returns `None` when `available` is empty.
pub fn choose(available: &[TextureFormat]) -> Option<TextureFormat> {
    let preferred = [TextureFormat::Bgra8Unorm, TextureFormat::Rgba8Unorm];
    preferred
        .into_iter()
        .find(|format| available.contains(format))
        .or_else(|| available.iter().copied().find(|format| !format.is_srgb()))
        .or_else(|| available.first().copied())
}

/// The clear color for `color`, linearized when the target is sRGB
/// (the shader does the same for instance colors).
pub fn clear_color(color: Rgb, srgb: bool) -> wgpu::Color {
    let channel = |shift: u32| {
        let c = f64::from((color >> shift) & 0xff) / 255.0;
        match srgb {
            false => c,
            true if c <= 0.04045 => c / 12.92,
            true => ((c + 0.055) / 1.055).powf(2.4),
        }
    };
    wgpu::Color {
        r: channel(16),
        g: channel(8),
        b: channel(0),
        a: 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use TextureFormat::*;

    #[test]
    fn prefers_8bit_unorm_over_srgb_listed_first() {
        assert_eq!(
            choose(&[Bgra8UnormSrgb, Rgba16Float, Bgra8Unorm]),
            Some(Bgra8Unorm)
        );
        assert_eq!(choose(&[Rgba8UnormSrgb, Rgba8Unorm]), Some(Rgba8Unorm));
    }

    #[test]
    fn falls_back_to_any_non_srgb_format() {
        assert_eq!(choose(&[Bgra8UnormSrgb, Rgb10a2Unorm]), Some(Rgb10a2Unorm));
    }

    #[test]
    fn uses_srgb_only_when_nothing_else_exists() {
        assert_eq!(choose(&[Bgra8UnormSrgb]), Some(Bgra8UnormSrgb));
        assert_eq!(choose(&[]), None);
    }

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn clear_color_unpacks_0rgb_as_is_for_unorm_targets() {
        let c = clear_color(crate::palette::rgb(0xff, 0x80, 0x00), false);
        assert!(approx(c.r, 1.0) && approx(c.g, 128.0 / 255.0) && approx(c.b, 0.0));
        assert!(approx(c.a, 1.0));
    }

    #[test]
    fn clear_color_is_linearized_for_srgb_targets() {
        let c = clear_color(crate::palette::rgb(0xff, 0x80, 0x0a), true);
        assert!(approx(c.r, 1.0));
        assert!(approx(c.g, 0.215861), "mid gray: {}", c.g);
        assert!(approx(c.b, 10.0 / 255.0 / 12.92), "linear segment: {}", c.b);
    }
}
