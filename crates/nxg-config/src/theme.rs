//! Built-in color themes.

use std::fmt;

use serde::{Deserialize, Deserializer};

use crate::color::Rgb;

/// Default foreground, background, cursor and the 16 ANSI colors
/// (0-7 normal, 8-15 bright). Indices 16-255 follow the xterm layout and
/// are not themable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Colors {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    pub ansi: [Rgb; 16],
    /// Selected text; `None` swaps the colors of selected cells.
    pub selection_foreground: Option<Rgb>,
    pub selection_background: Option<Rgb>,
}

/// A named built-in theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub name: &'static str,
    pub colors: Colors,
}

/// Name of the default theme.
pub const DEFAULT_THEME: &str = "catppuccin-mocha";

/// Builds a [`Theme`] from `0xRRGGBB` literals.
const fn theme(name: &'static str, fg: u32, bg: u32, cursor: u32, ansi: [u32; 16]) -> Theme {
    let mut colors = [Rgb::new(0, 0, 0); 16];
    let mut i = 0;
    while i < 16 {
        colors[i] = Rgb::hex(ansi[i]);
        i += 1;
    }
    Theme {
        name,
        colors: Colors {
            foreground: Rgb::hex(fg),
            background: Rgb::hex(bg),
            cursor: Rgb::hex(cursor),
            ansi: colors,
            selection_foreground: None,
            selection_background: None,
        },
    }
}

impl Theme {
    /// Draws selected text in `fg` on `bg` instead of swapping the colors.
    const fn with_selection(mut self, fg: u32, bg: u32) -> Self {
        self.colors.selection_foreground = Some(Rgb::hex(fg));
        self.colors.selection_background = Some(Rgb::hex(bg));
        self
    }
}

/// Every built-in theme; the first one is the default.
pub const THEMES: &[Theme] = &[
    // Catppuccin's official Mocha palette: text on base, rosewater cursor,
    // selection on surface2.
    theme(
        "catppuccin-mocha",
        0xcdd6f4,
        0x1e1e2e,
        0xf5e0dc,
        [
            0x45475a, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xbac2de,
            0x585b70, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xa6adc8,
        ],
    )
    .with_selection(0xcdd6f4, 0x585b70),
    // xterm's colors on a near-black background.
    theme(
        "nxg-dark",
        0xe5e5e5,
        0x101010,
        0xc0c0c0,
        [
            0x000000, 0xcd0000, 0x00cd00, 0xcdcd00, 0x0000ee, 0xcd00cd, 0x00cdcd, 0xe5e5e5,
            0x7f7f7f, 0xff0000, 0x00ff00, 0xffff00, 0x5c5cff, 0xff00ff, 0x00ffff, 0xffffff,
        ],
    ),
    theme(
        "nxg-light",
        0x1f2328,
        0xfafafa,
        0x1f2328,
        [
            0x24292f, 0xcf222e, 0x116329, 0x4d2d00, 0x0969da, 0x8250df, 0x1b7c83, 0x6e7781,
            0x57606a, 0xa40e26, 0x1a7f37, 0x633c01, 0x218bff, 0xa475f9, 0x3192aa, 0x8c959f,
        ],
    ),
    theme(
        "tokyo-night",
        0xc0caf5,
        0x1a1b26,
        0xc0caf5,
        [
            0x15161e, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xa9b1d6,
            0x414868, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xc0caf5,
        ],
    ),
    theme(
        "gruvbox-dark",
        0xebdbb2,
        0x282828,
        0xebdbb2,
        [
            0x282828, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0xa89984,
            0x928374, 0xfb4934, 0xb8bb26, 0xfabd2f, 0x83a598, 0xd3869b, 0x8ec07c, 0xebdbb2,
        ],
    ),
    theme(
        "dracula",
        0xf8f8f2,
        0x282a36,
        0xf8f8f2,
        [
            0x21222c, 0xff5555, 0x50fa7b, 0xf1fa8c, 0xbd93f9, 0xff79c6, 0x8be9fd, 0xf8f8f2,
            0x6272a4, 0xff6e6e, 0x69ff94, 0xffffa5, 0xd6acff, 0xff92df, 0xa4ffff, 0xffffff,
        ],
    ),
    theme(
        "nord",
        0xd8dee9,
        0x2e3440,
        0xd8dee9,
        [
            0x3b4252, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x88c0d0, 0xe5e9f0,
            0x4c566a, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x8fbcbb, 0xeceff4,
        ],
    ),
    theme(
        "one-dark",
        0xabb2bf,
        0x282c34,
        0x528bff,
        [
            0x282c34, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xabb2bf,
            0x5c6370, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xffffff,
        ],
    ),
];

/// The built-in theme called `name` (case-insensitive).
pub fn find(name: &str) -> Option<&'static Theme> {
    THEMES
        .iter()
        .find(|theme| theme.name.eq_ignore_ascii_case(name.trim()))
}

/// Names of every built-in theme, comma separated, for error messages.
pub fn names() -> String {
    let names: Vec<_> = THEMES.iter().map(|theme| theme.name).collect();
    names.join(", ")
}

/// A theme name validated against [`THEMES`] while parsing, so a typo is
/// reported with its location in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeName(&'static Theme);

impl ThemeName {
    pub fn theme(self) -> &'static Theme {
        self.0
    }
}

impl Default for ThemeName {
    fn default() -> Self {
        Self(&THEMES[0])
    }
}

/// Unknown theme name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownTheme(pub String);

impl fmt::Display for UnknownTheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown theme `{}`, available: {}", self.0, names())
    }
}

impl std::error::Error for UnknownTheme {}

impl std::str::FromStr for ThemeName {
    type Err = UnknownTheme;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        find(name)
            .map(Self)
            .ok_or_else(|| UnknownTheme(name.to_owned()))
    }
}

impl<'de> Deserialize<'de> for ThemeName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        name.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ships_the_documented_themes_with_default_first() {
        let names: Vec<_> = THEMES.iter().map(|t| t.name).collect();
        assert_eq!(
            names,
            [
                "catppuccin-mocha",
                "nxg-dark",
                "nxg-light",
                "tokyo-night",
                "gruvbox-dark",
                "dracula",
                "nord",
                "one-dark",
            ]
        );
        assert_eq!(THEMES[0].name, DEFAULT_THEME);
        assert_eq!(ThemeName::default().theme().name, DEFAULT_THEME);
    }

    #[test]
    fn default_theme_is_catppuccin_mocha() {
        let colors = THEMES[0].colors;
        assert_eq!(colors.foreground, Rgb::hex(0xcdd6f4));
        assert_eq!(colors.background, Rgb::hex(0x1e1e2e));
        assert_eq!(colors.cursor, Rgb::hex(0xf5e0dc));
        assert_eq!(colors.ansi[0], Rgb::hex(0x45475a));
        assert_eq!(colors.ansi[4], Rgb::hex(0x89b4fa));
        assert_eq!(colors.ansi[15], Rgb::hex(0xa6adc8));
        assert_eq!(colors.selection_foreground, Some(Rgb::hex(0xcdd6f4)));
        assert_eq!(colors.selection_background, Some(Rgb::hex(0x585b70)));
    }

    #[test]
    fn nxg_dark_keeps_the_original_xterm_colors() {
        let colors = find("nxg-dark").unwrap().colors;
        assert_eq!(colors.foreground, Rgb::hex(0xe5e5e5));
        assert_eq!(colors.background, Rgb::hex(0x101010));
        assert_eq!(colors.cursor, Rgb::hex(0xc0c0c0));
        assert_eq!(colors.ansi[1], Rgb::hex(0xcd0000));
        assert_eq!(colors.ansi[12], Rgb::hex(0x5c5cff));
        assert_eq!(colors.selection_background, None);
    }

    #[test]
    fn themes_have_readable_contrast() {
        for theme in THEMES {
            let c = theme.colors;
            assert_ne!(c.foreground, c.background, "{}", theme.name);
            assert_ne!(c.cursor, c.background, "{}", theme.name);
        }
    }

    #[test]
    fn finds_themes_ignoring_case() {
        assert_eq!(find("dracula").map(|t| t.name), Some("dracula"));
        assert_eq!(find("Tokyo-Night").map(|t| t.name), Some("tokyo-night"));
        assert_eq!(find("solarized"), None);
    }

    #[test]
    fn unknown_theme_error_lists_available_names() {
        let error = "solarized".parse::<ThemeName>().unwrap_err().to_string();
        assert!(error.contains("`solarized`"), "{error}");
        assert!(
            error.contains("catppuccin-mocha, nxg-dark, nxg-light"),
            "{error}"
        );
        assert!(error.ends_with("one-dark"), "{error}");
    }
}
