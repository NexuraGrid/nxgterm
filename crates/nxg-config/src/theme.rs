//! Color themes: the built-in ones and theme files.
//!
//! A theme file, `<config dir>/themes/<name>.toml`, holds the same keys as
//! the `[colors]` overrides (`foreground`, `background`, `cursor`, `ansi`,
//! `selection_foreground`, `selection_background`), at the top level or
//! under a `[colors]` table. Keys it leaves out take the default theme's
//! values.

use std::fmt;
use std::path::{Path, PathBuf};

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

/// The theme `[colors] theme` names: a built-in one or one loaded from a
/// theme file. Parsing only reads the name (a built-in one is resolved
/// right away); [`crate::Config::load`] then resolves it against the theme
/// files, which need the config directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeName {
    name: String,
    colors: Colors,
    /// The theme file the colors come from; `None` for a built-in theme.
    file: Option<PathBuf>,
}

impl ThemeName {
    /// The built-in theme called `name` (case-insensitive).
    pub fn built_in(name: &str) -> Option<Self> {
        find(name).map(|theme| Self {
            name: theme.name.to_owned(),
            colors: theme.colors,
            file: None,
        })
    }

    /// The theme called `name` loaded from `file`.
    pub fn from_file(name: &str, colors: Colors, file: PathBuf) -> Self {
        Self {
            name: name.to_owned(),
            colors,
            file: Some(file),
        }
    }

    /// The name: as the built-in theme spells it, or as configured.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn colors(&self) -> Colors {
        self.colors
    }

    /// The theme file the colors come from; `None` for a built-in theme.
    pub fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }
}

impl Default for ThemeName {
    fn default() -> Self {
        Self::built_in(DEFAULT_THEME).expect("the default theme is built in")
    }
}

/// Unknown theme name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownTheme {
    pub name: String,
    /// The theme files found, by name, and the directory they are in.
    pub custom: Vec<String>,
    pub dir: Option<PathBuf>,
}

impl UnknownTheme {
    /// `name` is neither built in nor one of the `custom` theme files.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            custom: Vec::new(),
            dir: None,
        }
    }
}

impl fmt::Display for UnknownTheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown theme `{}`, available: {}", self.name, names())?;
        match &self.dir {
            Some(dir) if self.custom.is_empty() => {
                write!(f, "; no theme files in {}", dir.display())
            }
            Some(dir) => write!(
                f,
                "; theme files in {}: {}",
                dir.display(),
                self.custom.join(", ")
            ),
            None => Ok(()),
        }
    }
}

impl std::error::Error for UnknownTheme {}

impl std::str::FromStr for ThemeName {
    type Err = UnknownTheme;

    /// A built-in theme.
    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Self::built_in(name).ok_or_else(|| UnknownTheme::new(name))
    }
}

impl<'de> Deserialize<'de> for ThemeName {
    /// Any non-blank name: a built-in theme is resolved right away, any
    /// other keeps the default colors until it is resolved against the
    /// theme files.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        let name = name.trim();
        if name.is_empty() {
            return Err(serde::de::Error::custom("empty theme name"));
        }
        Ok(Self::built_in(name).unwrap_or_else(|| Self {
            name: name.to_owned(),
            ..Self::default()
        }))
    }
}

/// The keys of a theme file, all optional.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeFile {
    pub foreground: Option<Rgb>,
    pub background: Option<Rgb>,
    pub cursor: Option<Rgb>,
    pub ansi: Option<[Rgb; 16]>,
    pub selection_foreground: Option<Rgb>,
    pub selection_background: Option<Rgb>,
}

/// A theme file with its keys under `[colors]`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NestedThemeFile {
    colors: ThemeFile,
}

impl ThemeFile {
    /// Parses a theme file: its keys at the top level or under `[colors]`.
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        let table: toml::Table = toml::from_str(text)?;
        if table.contains_key("colors") {
            toml::from_str::<NestedThemeFile>(text).map(|file| file.colors)
        } else {
            toml::from_str(text)
        }
    }

    /// The colors of this file over those of the default theme.
    pub fn colors(&self) -> Colors {
        let base = THEMES[0].colors;
        Colors {
            foreground: self.foreground.unwrap_or(base.foreground),
            background: self.background.unwrap_or(base.background),
            cursor: self.cursor.unwrap_or(base.cursor),
            ansi: self.ansi.unwrap_or(base.ansi),
            selection_foreground: self.selection_foreground.or(base.selection_foreground),
            selection_background: self.selection_background.or(base.selection_background),
        }
    }
}

/// The theme file for `name` in `themes_dir`, or `None` when `name` cannot
/// be a file name (empty, `.`/`..` or with a path separator).
pub fn file_path(themes_dir: &Path, name: &str) -> Option<PathBuf> {
    let valid = !matches!(name, "" | "." | "..") && !name.contains(['/', '\\']);
    valid.then(|| themes_dir.join(format!("{name}.toml")))
}

/// Names of the theme files (`*.toml`) in `themes_dir`, sorted; empty when
/// it cannot be read.
pub fn file_names(themes_dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(themes_dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml") && path.is_file())
        .filter_map(|path| Some(path.file_stem()?.to_str()?.to_owned()))
        .collect();
    names.sort();
    names
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
        assert_eq!(ThemeName::default().name(), DEFAULT_THEME);
        assert_eq!(ThemeName::default().file(), None);
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

    #[test]
    fn unknown_theme_error_lists_theme_files_too() {
        let error = UnknownTheme {
            name: "nope".into(),
            custom: vec!["mine".into(), "work".into()],
            dir: Some(PathBuf::from("/cfg/themes")),
        };
        let text = error.to_string();
        assert!(text.contains("available: catppuccin-mocha"), "{text}");
        assert!(
            text.ends_with("theme files in /cfg/themes: mine, work"),
            "{text}"
        );
        let none = UnknownTheme {
            custom: Vec::new(),
            ..error
        };
        assert!(none.to_string().ends_with("no theme files in /cfg/themes"));
    }

    #[test]
    fn theme_names_parse_without_resolving_unknown_ones() {
        let name: ThemeName = toml::Value::String(" Nord ".into()).try_into().unwrap();
        assert_eq!(name.name(), "nord");
        assert_eq!(name.colors(), find("nord").unwrap().colors);
        let custom: ThemeName = toml::Value::String("mine".into()).try_into().unwrap();
        assert_eq!(custom.name(), "mine");
        assert_eq!(custom.colors(), THEMES[0].colors, "until resolved");
        let blank = toml::Value::String("  ".into()).try_into::<ThemeName>();
        assert!(blank.unwrap_err().to_string().contains("empty theme name"));
    }

    #[test]
    fn theme_files_take_top_level_or_nested_keys() {
        let top = ThemeFile::parse("background = \"#000000\"\ncursor = \"#fff\"\n").unwrap();
        let nested =
            ThemeFile::parse("[colors]\nbackground = \"#000000\"\ncursor = \"#fff\"\n").unwrap();
        assert_eq!(top, nested);
        assert_eq!(top.background, Some(Rgb::hex(0)));
        assert!(ThemeFile::parse("theme = \"nord\"").is_err(), "unknown key");
        assert!(ThemeFile::parse("[colors]\nforeground = 1").is_err());
    }

    #[test]
    fn theme_file_keys_left_out_take_the_default_theme() {
        let colors = ThemeFile::parse("foreground = \"#010203\"")
            .unwrap()
            .colors();
        let base = THEMES[0].colors;
        assert_eq!(colors.foreground, Rgb::new(1, 2, 3));
        assert_eq!(colors.background, base.background);
        assert_eq!(colors.ansi, base.ansi);
        assert_eq!(colors.selection_background, base.selection_background);
    }

    #[test]
    fn theme_file_paths_reject_path_like_names() {
        let dir = Path::new("/cfg/themes");
        assert_eq!(
            file_path(dir, "mine"),
            Some(PathBuf::from("/cfg/themes/mine.toml"))
        );
        assert_eq!(file_path(dir, "../secret"), None);
        assert_eq!(file_path(dir, "a\\b"), None);
        assert_eq!(file_path(dir, ".."), None);
    }
}
