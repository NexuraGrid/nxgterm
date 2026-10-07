//! Configuration: the TOML file, its defaults and the built-in themes.
//!
//! Pure: no window, GPU or OS APIs. Every key is optional; a missing file
//! or section means the defaults, and unknown keys are errors so typos do
//! not go unnoticed. See [`DEFAULT_CONFIG_TOML`] for the documented format.

pub mod color;
pub mod path;
pub mod theme;

use std::fmt;
use std::io;
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer};

pub use color::Rgb;
pub use path::{Platform, config_path};
pub use theme::{Colors, THEMES, Theme, ThemeName};

/// Smallest and largest font size accepted, in points at scale 1.0.
pub const MIN_FONT_SIZE: f32 = 6.0;
pub const MAX_FONT_SIZE: f32 = 72.0;
/// Font size used when none is configured.
pub const DEFAULT_FONT_SIZE: f32 = 14.0;

/// A documented sample config that parses to [`Config::default`]; printed
/// by `nxgterm --print-config`.
pub const DEFAULT_CONFIG_TOML: &str = r##"# nxgterm configuration.
#
# Location: $XDG_CONFIG_HOME/nxgterm/nxgterm.toml or ~/.config/nxgterm/nxgterm.toml
# (Linux and macOS), %APPDATA%\nxgterm\nxgterm.toml (Windows). Override it with
# NXGTERM_CONFIG=<path> or `nxgterm --config <path>`.
#
# Every key is optional; the values below are the defaults. Unknown keys are
# reported as errors. Changes are applied live when the file is saved, except
# [shell] and [renderer], which apply on the next start.

[font]
# Font family. When unset or not installed, the system monospace font is used.
# family = "JetBrains Mono"
# Size in points at 100% display scale (clamped to 6-72).
size = 14.0

[window]
# Space around the grid, in pixels at 100% display scale.
padding = 4
# Initial size in character cells.
columns = 100
rows = 30

[colors]
# Built-in theme: nxg-dark, nxg-light, tokyo-night, catppuccin-mocha,
# gruvbox-dark, dracula, nord, one-dark.
theme = "nxg-dark"
# Optional overrides on top of the theme, as "#rrggbb" (or "#rgb"):
# foreground = "#c0caf5"
# background = "#1a1b26"
# cursor = "#c0caf5"
# The 16 ANSI colors: black, red, green, yellow, blue, magenta, cyan, white,
# then their bright variants.
# ansi = [
#   "#15161e", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff", "#a9b1d6",
#   "#414868", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff", "#c0caf5",
# ]

[shell]
# Program to run instead of the platform default ($SHELL on Unix; PowerShell 7,
# then Windows PowerShell, then cmd.exe on Windows), and its arguments.
# program = "pwsh.exe"
# args = ["-NoLogo"]

[renderer]
# auto (GPU, falling back to CPU), gpu or cpu. NXGTERM_RENDERER overrides it.
backend = "auto"

[scrollback]
# Lines kept after they scroll off the top of the screen; 0 disables it.
lines = 10000
"##;

/// The whole configuration file.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub font: FontConfig,
    pub window: WindowConfig,
    pub colors: ColorsConfig,
    pub shell: ShellConfig,
    pub renderer: RendererConfig,
    pub scrollback: ScrollbackConfig,
}

/// `[font]`
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FontConfig {
    /// Preferred family; `None` means the system monospace font.
    #[serde(deserialize_with = "non_empty")]
    pub family: Option<String>,
    /// Points at scale 1.0, already clamped with [`clamp_font_size`].
    #[serde(deserialize_with = "font_size")]
    pub size: f32,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: None,
            size: DEFAULT_FONT_SIZE,
        }
    }
}

/// `[window]`
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WindowConfig {
    /// Pixels around the grid at scale 1.0.
    pub padding: u16,
    /// Initial grid size in cells.
    pub columns: NonZeroU16,
    pub rows: NonZeroU16,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            padding: 4,
            columns: NonZeroU16::new(100).expect("non-zero"),
            rows: NonZeroU16::new(30).expect("non-zero"),
        }
    }
}

/// `[colors]`: a built-in theme plus optional overrides.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ColorsConfig {
    pub theme: ThemeName,
    pub foreground: Option<Rgb>,
    pub background: Option<Rgb>,
    pub cursor: Option<Rgb>,
    pub ansi: Option<[Rgb; 16]>,
}

impl ColorsConfig {
    /// The theme's colors with the overrides applied.
    pub fn resolve(&self) -> Colors {
        let theme = self.theme.theme().colors;
        Colors {
            foreground: self.foreground.unwrap_or(theme.foreground),
            background: self.background.unwrap_or(theme.background),
            cursor: self.cursor.unwrap_or(theme.cursor),
            ansi: self.ansi.unwrap_or(theme.ansi),
        }
    }
}

/// `[shell]`
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShellConfig {
    /// Program to run; `None` keeps the platform default.
    #[serde(deserialize_with = "non_empty")]
    pub program: Option<String>,
    /// Arguments, used only with `program`.
    pub args: Vec<String>,
}

/// `[renderer]`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RendererConfig {
    pub backend: Backend,
}

/// `[scrollback]`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScrollbackConfig {
    /// History lines kept; 0 disables the scrollback.
    pub lines: usize,
}

impl Default for ScrollbackConfig {
    fn default() -> Self {
        Self { lines: 10_000 }
    }
}

/// Which renderer to use.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    /// GPU, falling back to CPU.
    #[default]
    Auto,
    Gpu,
    Cpu,
}

/// Clamps a font size to [`MIN_FONT_SIZE`]..=[`MAX_FONT_SIZE`]; a
/// non-finite size becomes [`DEFAULT_FONT_SIZE`].
pub fn clamp_font_size(size: f32) -> f32 {
    if size.is_finite() {
        size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)
    } else {
        DEFAULT_FONT_SIZE
    }
}

/// Why a config file could not be used.
#[derive(Debug)]
pub enum ConfigError {
    /// The file exists but could not be read.
    Io { path: PathBuf, source: io::Error },
    /// The file is not valid TOML or does not match the schema. `message`
    /// carries the line, column and offending snippet.
    Parse { path: PathBuf, message: String },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "cannot read {}: {source}", path.display()),
            Self::Parse { path, message } => {
                write!(
                    f,
                    "invalid config {}: {}",
                    path.display(),
                    message.trim_end()
                )
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Parse { .. } => None,
        }
    }
}

impl Config {
    /// Parses `text`; `path` only labels errors.
    pub fn parse(text: &str, path: &Path) -> Result<Self, ConfigError> {
        toml::from_str(text).map_err(|error| ConfigError::Parse {
            path: path.to_owned(),
            message: error.to_string(),
        })
    }

    /// Reads and parses the file at `path`. A missing file yields
    /// `Ok(None)` so callers can fall back to the defaults.
    pub fn load(path: &Path) -> Result<Option<Self>, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text, path).map(Some),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(ConfigError::Io {
                path: path.to_owned(),
                source,
            }),
        }
    }
}

/// Treats an empty or blank string as absent.
fn non_empty<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value.filter(|s| !s.trim().is_empty()))
}

/// Accepts integers or floats and clamps them.
fn font_size<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Number {
        Int(i64),
        Float(f64),
    }
    let size = match Number::deserialize(deserializer)? {
        Number::Int(n) => n as f32,
        Number::Float(n) => n as f32,
    };
    Ok(clamp_font_size(size))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Config, ConfigError> {
        Config::parse(text, Path::new("/cfg/nxgterm.toml"))
    }

    fn parse_error(text: &str) -> String {
        parse(text).unwrap_err().to_string()
    }

    #[test]
    fn empty_file_means_defaults() {
        assert_eq!(parse("").unwrap(), Config::default());
    }

    #[test]
    fn documented_sample_parses_to_the_defaults() {
        assert_eq!(parse(DEFAULT_CONFIG_TOML).unwrap(), Config::default());
    }

    #[test]
    fn defaults_match_the_documentation() {
        let config = Config::default();
        assert_eq!(config.font.family, None);
        assert_eq!(config.font.size, 14.0);
        assert_eq!(config.window.padding, 4);
        assert_eq!(
            (config.window.columns.get(), config.window.rows.get()),
            (100, 30)
        );
        assert_eq!(config.colors.theme.theme().name, "nxg-dark");
        assert_eq!(config.shell, ShellConfig::default());
        assert_eq!(config.renderer.backend, Backend::Auto);
        assert_eq!(config.scrollback.lines, 10_000);
    }

    #[test]
    fn parses_every_section() {
        let config = parse(
            r##"
            [font]
            family = "JetBrains Mono"
            size = 12
            [window]
            padding = 0
            columns = 80
            rows = 24
            [colors]
            theme = "dracula"
            background = "#000000"
            [shell]
            program = "pwsh.exe"
            args = ["-NoLogo"]
            [renderer]
            backend = "cpu"
            [scrollback]
            lines = 0
            "##,
        )
        .unwrap();
        assert_eq!(config.font.family.as_deref(), Some("JetBrains Mono"));
        assert_eq!(config.font.size, 12.0);
        assert_eq!(config.window.padding, 0);
        assert_eq!(
            (config.window.columns.get(), config.window.rows.get()),
            (80, 24)
        );
        assert_eq!(config.colors.theme.theme().name, "dracula");
        assert_eq!(config.colors.background, Some(Rgb::hex(0)));
        assert_eq!(config.shell.program.as_deref(), Some("pwsh.exe"));
        assert_eq!(config.shell.args, ["-NoLogo"]);
        assert_eq!(config.renderer.backend, Backend::Cpu);
        assert_eq!(config.scrollback.lines, 0);
    }

    #[test]
    fn partial_sections_keep_other_defaults() {
        let config = parse("[window]\npadding = 10\n").unwrap();
        assert_eq!(config.window.padding, 10);
        assert_eq!(config.window.columns.get(), 100);
        assert_eq!(config.font, FontConfig::default());
    }

    #[test]
    fn blank_strings_mean_unset() {
        let config = parse("[font]\nfamily = \"  \"\n[shell]\nprogram = \"\"\n").unwrap();
        assert_eq!(config.font.family, None);
        assert_eq!(config.shell.program, None);
    }

    #[test]
    fn font_size_is_clamped() {
        assert_eq!(parse("[font]\nsize = 2").unwrap().font.size, MIN_FONT_SIZE);
        assert_eq!(
            parse("[font]\nsize = 500.5").unwrap().font.size,
            MAX_FONT_SIZE
        );
        assert_eq!(parse("[font]\nsize = 13.5").unwrap().font.size, 13.5);
    }

    #[test]
    fn clamp_font_size_handles_bounds_and_non_finite() {
        assert_eq!(clamp_font_size(5.9), MIN_FONT_SIZE);
        assert_eq!(clamp_font_size(72.1), MAX_FONT_SIZE);
        assert_eq!(clamp_font_size(20.0), 20.0);
        assert_eq!(clamp_font_size(f32::NAN), DEFAULT_FONT_SIZE);
        assert_eq!(clamp_font_size(f32::INFINITY), DEFAULT_FONT_SIZE);
    }

    #[test]
    fn theme_alone_resolves_to_its_colors() {
        let config = parse("[colors]\ntheme = \"nord\"").unwrap();
        assert_eq!(config.colors.resolve(), theme::find("nord").unwrap().colors);
    }

    #[test]
    fn overrides_apply_on_top_of_the_theme() {
        let ansi: Vec<String> = (0..16).map(|i| format!("\"#0000{i:02x}\"")).collect();
        let text = format!(
            "[colors]\ntheme = \"nord\"\nforeground = \"#010203\"\ncursor = \"#fff\"\nansi = [{}]",
            ansi.join(", ")
        );
        let colors = parse(&text).unwrap().colors.resolve();
        let nord = theme::find("nord").unwrap().colors;
        assert_eq!(colors.foreground, Rgb::new(1, 2, 3));
        assert_eq!(colors.background, nord.background, "not overridden");
        assert_eq!(colors.cursor, Rgb::new(255, 255, 255));
        assert_eq!(colors.ansi[0], Rgb::hex(0));
        assert_eq!(colors.ansi[15], Rgb::hex(0x0f));
    }

    #[test]
    fn errors_carry_path_line_and_reason() {
        let error = parse_error("[colors]\ntheme = \"solarized\"\n");
        assert!(error.contains("/cfg/nxgterm.toml"), "{error}");
        assert!(error.contains("line 2"), "{error}");
        assert!(error.contains("unknown theme `solarized`"), "{error}");
        assert!(error.contains("available: nxg-dark"), "{error}");
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let error = parse_error("[font]\nfamilly = \"x\"\n");
        assert!(
            error.contains("familly") && error.contains("line 2"),
            "{error}"
        );
        let error = parse_error("[fonts]\n");
        assert!(error.contains("fonts"), "{error}");
        let error = parse_error("[scrollback]\nline = 5\n");
        assert!(error.contains("line"), "{error}");
    }

    #[test]
    fn invalid_values_are_rejected() {
        let error = parse_error("[scrollback]\nlines = -1\n");
        assert!(
            error.contains("scrollback") || error.contains("line 2"),
            "{error}"
        );
        let error = parse_error("[colors]\nforeground = \"blue\"\n");
        assert!(error.contains("invalid color `blue`"), "{error}");
        assert!(parse("[colors]\nansi = [\"#000000\"]").is_err(), "needs 16");
        assert!(parse("[window]\ncolumns = 0").is_err());
        assert!(parse("[window]\npadding = -1").is_err());
        assert!(parse("[renderer]\nbackend = \"vulkan\"").is_err());
        assert!(parse("[font\n").is_err(), "syntax error");
    }

    #[test]
    fn load_reads_files_and_reports_missing_ones() {
        let dir = std::env::temp_dir().join(format!("nxg-config-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("nxgterm.toml");

        assert!(Config::load(&file).unwrap().is_none(), "missing file");

        std::fs::write(&file, "[window]\npadding = 9\n").unwrap();
        assert_eq!(Config::load(&file).unwrap().unwrap().window.padding, 9);

        std::fs::write(&file, "[window]\npadding = \"x\"\n").unwrap();
        let error = Config::load(&file).unwrap_err().to_string();
        assert!(error.contains(&file.display().to_string()), "{error}");

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
