//! Surface format, alpha mode and clear color choice.
//!
//! The CPU renderer blends 8-bit sRGB-encoded values directly (no
//! linearization). To match it, the GPU renderer prefers a non-sRGB
//! (`*Unorm`) surface, so palette values are written and blended as-is.
//! When only sRGB formats exist, the shader converts colors to linear so
//! solid colors still match; glyph edges then blend in linear space and
//! look slightly different.
//!
//! A translucent background is written premultiplied: the clear color is
//! scaled by its alpha, and quads and glyphs blend over it with
//! [`wgpu::BlendState::ALPHA_BLENDING`], which keeps the target
//! premultiplied (their own alpha is 1.0 or the glyph coverage).

use wgpu::{CompositeAlphaMode, TextureFormat};

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

/// Picks how the system composites the surface from `available`.
///
/// When `translucent`, a mode that blends the window with what is behind
/// it: `PreMultiplied`, else `PostMultiplied` (the only one Metal offers;
/// Core Animation still composites premultiplied colors, which is what
/// the renderer writes). Otherwise, or when neither exists, `Opaque`, else
/// the first one. Check the result with [`blends`].
pub fn alpha_mode(available: &[CompositeAlphaMode], translucent: bool) -> CompositeAlphaMode {
    let blending = [
        CompositeAlphaMode::PreMultiplied,
        CompositeAlphaMode::PostMultiplied,
    ];
    let opaque = || {
        if available.contains(&CompositeAlphaMode::Opaque) {
            CompositeAlphaMode::Opaque
        } else {
            available
                .first()
                .copied()
                .unwrap_or(CompositeAlphaMode::Auto)
        }
    };
    blending
        .into_iter()
        .find(|mode| translucent && available.contains(mode))
        .unwrap_or_else(opaque)
}

/// Whether the system blends a surface in `mode` with what is behind it,
/// so a background alpha below 1.0 shows.
pub fn blends(mode: CompositeAlphaMode) -> bool {
    matches!(
        mode,
        CompositeAlphaMode::PreMultiplied | CompositeAlphaMode::PostMultiplied
    )
}

/// The alpha of the default background: `opacity` when the surface
/// `blends` with what is behind the window, 1.0 otherwise.
pub fn background_alpha(blends: bool, opacity: f32) -> f32 {
    if blends { opacity } else { 1.0 }
}

/// The clear color for `color` at `alpha` (clamped to 0.0-1.0),
/// linearized when the target is sRGB (the shader does the same for
/// instance colors) and premultiplied by the alpha.
pub fn clear_color(color: Rgb, srgb: bool, alpha: f32) -> wgpu::Color {
    let alpha = if alpha.is_finite() {
        f64::from(alpha.clamp(0.0, 1.0))
    } else {
        1.0
    };
    let channel = |shift: u32| {
        let c = f64::from((color >> shift) & 0xff) / 255.0;
        match srgb {
            false => c,
            true if c <= 0.04045 => c / 12.92,
            true => ((c + 0.055) / 1.055).powf(2.4),
        }
    };
    wgpu::Color {
        r: channel(16) * alpha,
        g: channel(8) * alpha,
        b: channel(0) * alpha,
        a: alpha,
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
        let c = clear_color(crate::palette::rgb(0xff, 0x80, 0x00), false, 1.0);
        assert!(approx(c.r, 1.0) && approx(c.g, 128.0 / 255.0) && approx(c.b, 0.0));
        assert!(approx(c.a, 1.0));
    }

    #[test]
    fn clear_color_is_linearized_for_srgb_targets() {
        let c = clear_color(crate::palette::rgb(0xff, 0x80, 0x0a), true, 1.0);
        assert!(approx(c.r, 1.0));
        assert!(approx(c.g, 0.215861), "mid gray: {}", c.g);
        assert!(approx(c.b, 10.0 / 255.0 / 12.92), "linear segment: {}", c.b);
    }

    #[test]
    fn clear_color_is_premultiplied_by_the_opacity() {
        let c = clear_color(crate::palette::rgb(0xff, 0x80, 0x00), false, 0.5);
        assert!(approx(c.r, 0.5) && approx(c.g, 64.0 / 255.0) && approx(c.b, 0.0));
        assert!(approx(c.a, 0.5));
        let srgb = clear_color(crate::palette::rgb(0xff, 0x80, 0x0a), true, 0.25);
        assert!(approx(srgb.g, 0.215861 * 0.25), "linearized, then scaled");
        assert!(approx(srgb.a, 0.25));
    }

    #[test]
    fn clear_color_clamps_the_opacity() {
        let white = crate::palette::rgb(0xff, 0xff, 0xff);
        assert!(approx(clear_color(white, false, 2.0).a, 1.0));
        assert!(approx(clear_color(white, false, -1.0).r, 0.0));
        assert!(approx(clear_color(white, false, f32::NAN).a, 1.0));
    }

    use CompositeAlphaMode::{Auto, Inherit, Opaque, PostMultiplied, PreMultiplied};

    #[test]
    fn opacity_needs_a_blending_surface() {
        assert_eq!(background_alpha(true, 0.8), 0.8);
        assert_eq!(background_alpha(false, 0.8), 1.0);
        assert_eq!(background_alpha(true, 1.0), 1.0);
    }

    #[test]
    fn opaque_windows_keep_the_opaque_mode() {
        assert_eq!(alpha_mode(&[PreMultiplied, Opaque], false), Opaque);
        assert_eq!(alpha_mode(&[Inherit, PreMultiplied], false), Inherit);
        assert_eq!(alpha_mode(&[], false), Auto);
    }

    #[test]
    fn translucent_windows_prefer_premultiplied_then_postmultiplied() {
        let modes = [Opaque, PostMultiplied, PreMultiplied, Inherit];
        assert_eq!(alpha_mode(&modes, true), PreMultiplied);
        assert_eq!(alpha_mode(&[Opaque, PostMultiplied], true), PostMultiplied);
    }

    #[test]
    fn translucent_windows_fall_back_to_opaque_surfaces() {
        // DX12 window handles and OpenGL offer only Opaque in wgpu 26.
        assert_eq!(alpha_mode(&[Opaque], true), Opaque);
        assert_eq!(alpha_mode(&[Inherit, Opaque], true), Opaque);
        assert!(!blends(alpha_mode(&[Opaque, Inherit], true)));
        assert!(blends(PreMultiplied) && blends(PostMultiplied));
        assert!(!blends(Auto) && !blends(Inherit));
    }
}
