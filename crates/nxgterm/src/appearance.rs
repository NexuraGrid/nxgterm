//! Config values turned into what renderers draw with: palette, pixel
//! sizes at the window's scale factor, and font zoom.

use nxg_config::{Colors, Rgb, clamp_font_size};
use nxg_render::Palette;
use nxg_render::palette::rgb;
use winit::window::Theme;

use crate::bindings::Action;

/// Font size change per zoom step, in points.
pub const ZOOM_STEP: f32 = 1.0;

/// The renderer palette for configured `colors`.
pub fn palette(colors: &Colors) -> Palette {
    Palette {
        foreground: pixel(colors.foreground),
        background: pixel(colors.background),
        cursor: pixel(colors.cursor),
        ansi: colors.ansi.map(pixel),
        selection_foreground: colors.selection_foreground.map(pixel),
        selection_background: colors.selection_background.map(pixel),
    }
}

/// The title bar theme to ask the system for: dark over a dark background.
pub fn window_theme(colors: &Colors) -> Theme {
    if colors.background.is_dark() {
        Theme::Dark
    } else {
        Theme::Light
    }
}

/// Whether to ask the system to blur behind the window: only a window
/// created transparent shows anything behind it.
pub fn blur(transparent_window: bool, blur: bool) -> bool {
    transparent_window && blur
}

/// Asks the system to blur what is behind the window, or stops. macOS and
/// KDE Plasma on Wayland do it through winit; Windows 11 draws its Acrylic
/// backdrop (older Windows ignores it); elsewhere it does nothing.
pub fn apply_blur(window: &winit::window::Window, blur: bool) {
    window.set_blur(blur);
    #[cfg(windows)]
    {
        use winit::platform::windows::WindowExtWindows;
        window.set_system_backdrop(backdrop(blur));
    }
}

/// The Windows 11 system backdrop: Acrylic (blurred) when `blur`, else the
/// default one winit sets.
#[cfg(windows)]
pub fn backdrop(blur: bool) -> winit::platform::windows::BackdropType {
    use winit::platform::windows::BackdropType;
    if blur {
        BackdropType::TransientWindow
    } else {
        BackdropType::Auto
    }
}

/// Why a background `opacity` below 1.0 does not show, if it does not: the
/// window was created opaque (only a restart makes it transparent) or the
/// `renderer` cannot blend with what is behind the window.
pub fn opacity_notice(
    opacity: f32,
    transparent_window: bool,
    renderer: &str,
    translucent: bool,
) -> Option<String> {
    if opacity >= 1.0 {
        None
    } else if !transparent_window {
        Some("window opacity changes apply on restart".into())
    } else if !translucent {
        Some(format!(
            "the {renderer} renderer cannot draw a translucent window here; the background stays opaque"
        ))
    } else {
        None
    }
}

/// Font size in physical pixels for `size` points at `scale`.
pub fn font_px(size: f32, scale: f64) -> f32 {
    (f64::from(size) * valid_scale(scale)) as f32
}

/// Padding in physical pixels for `padding` logical pixels at `scale`.
pub fn padding_px(padding: u16, scale: f64) -> u32 {
    (f64::from(padding) * valid_scale(scale)).round() as u32
}

/// The font size after a zoom `action`, from `current` (zoomed) and the
/// `configured` size that reset returns to; always clamped. `None` for
/// other actions.
pub fn zoom(action: Action, current: f32, configured: f32) -> Option<f32> {
    let size = match action {
        Action::ZoomIn => current + ZOOM_STEP,
        Action::ZoomOut => current - ZOOM_STEP,
        Action::ResetZoom => configured,
        _ => return None,
    };
    Some(clamp_font_size(size))
}

fn pixel(color: Rgb) -> u32 {
    rgb(color.r, color.g, color.b)
}

/// `scale`, or 1.0 when it is not a positive finite number.
fn valid_scale(scale: f64) -> f64 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nxg_config::{Config, MAX_FONT_SIZE, MIN_FONT_SIZE, THEMES};

    #[test]
    fn nxg_dark_keeps_the_original_palette() {
        let colors = THEMES.iter().find(|t| t.name == "nxg-dark").unwrap().colors;
        assert_eq!(palette(&colors), Palette::default());
    }

    #[test]
    fn default_config_draws_catppuccin_mocha() {
        let palette = palette(&Config::default().colors.resolve());
        assert_eq!(palette.background, rgb(0x1e, 0x1e, 0x2e));
        assert_eq!(palette.selection_background, Some(rgb(0x58, 0x5b, 0x70)));
    }

    #[test]
    fn converts_every_theme_color() {
        let colors = THEMES
            .iter()
            .find(|t| t.name == "tokyo-night")
            .unwrap()
            .colors;
        let palette = palette(&colors);
        assert_eq!(palette.background, rgb(0x1a, 0x1b, 0x26));
        assert_eq!(palette.foreground, rgb(0xc0, 0xca, 0xf5));
        assert_eq!(palette.cursor, rgb(0xc0, 0xca, 0xf5));
        assert_eq!(palette.ansi[1], rgb(0xf7, 0x76, 0x8e));
        assert_eq!(palette.ansi[15], rgb(0xc0, 0xca, 0xf5));
    }

    #[test]
    fn title_bar_follows_the_background() {
        let theme = |name: &str| THEMES.iter().find(|t| t.name == name).unwrap().colors;
        assert_eq!(window_theme(&theme("catppuccin-mocha")), Theme::Dark);
        assert_eq!(window_theme(&theme("nxg-dark")), Theme::Dark);
        assert_eq!(window_theme(&theme("nxg-light")), Theme::Light);
        let mut colors = theme("nxg-light");
        colors.background = Rgb::hex(0x202020);
        assert_eq!(window_theme(&colors), Theme::Dark, "background override");
    }

    #[test]
    fn blur_needs_a_transparent_window() {
        assert!(blur(true, true));
        assert!(!blur(false, true), "nothing shows behind an opaque window");
        assert!(!blur(true, false));
    }

    #[cfg(windows)]
    #[test]
    fn blur_on_windows_is_the_acrylic_backdrop() {
        use winit::platform::windows::BackdropType;
        assert_eq!(backdrop(true), BackdropType::TransientWindow);
        assert_eq!(backdrop(false), BackdropType::default());
    }

    #[test]
    fn opacity_notice_says_why_the_background_stays_opaque() {
        assert_eq!(opacity_notice(1.0, false, "cpu", false), None, "opaque");
        assert_eq!(opacity_notice(0.8, true, "gpu", true), None, "translucent");
        assert_eq!(
            opacity_notice(0.8, false, "gpu", false).as_deref(),
            Some("window opacity changes apply on restart")
        );
        let cpu = opacity_notice(0.8, true, "cpu", false).unwrap();
        assert!(
            cpu.contains("cpu renderer") && cpu.contains("opaque"),
            "{cpu}"
        );
    }

    #[test]
    fn scales_sizes_by_the_display_scale() {
        assert_eq!(font_px(14.0, 1.0), 14.0);
        assert_eq!(font_px(14.0, 2.0), 28.0);
        assert_eq!(font_px(14.0, 1.25), 17.5);
        assert_eq!(padding_px(4, 1.0), 4);
        assert_eq!(padding_px(4, 1.5), 6);
        assert_eq!(padding_px(3, 1.25), 4, "rounded");
        assert_eq!(padding_px(0, 2.0), 0);
    }

    #[test]
    fn invalid_scale_factors_count_as_one() {
        assert_eq!(font_px(14.0, 0.0), 14.0);
        assert_eq!(font_px(14.0, f64::NAN), 14.0);
        assert_eq!(padding_px(4, -2.0), 4);
    }

    #[test]
    fn zoom_steps_and_resets() {
        assert_eq!(zoom(Action::ZoomIn, 14.0, 14.0), Some(15.0));
        assert_eq!(zoom(Action::ZoomOut, 14.0, 14.0), Some(13.0));
        assert_eq!(zoom(Action::ResetZoom, 20.0, 12.0), Some(12.0));
        assert_eq!(zoom(Action::NewTab, 20.0, 12.0), None, "not a zoom action");
    }

    #[test]
    fn zoom_is_clamped() {
        assert_eq!(
            zoom(Action::ZoomIn, MAX_FONT_SIZE, 14.0),
            Some(MAX_FONT_SIZE)
        );
        assert_eq!(
            zoom(Action::ZoomOut, MIN_FONT_SIZE, 14.0),
            Some(MIN_FONT_SIZE)
        );
    }
}
