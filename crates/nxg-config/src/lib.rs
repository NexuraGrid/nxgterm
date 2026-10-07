//! Configuration: the TOML file, its defaults, the built-in themes and the
//! key bindings.
//!
//! Pure: no window, GPU or OS APIs. Every key is optional; a missing file
//! or section means the defaults, and unknown keys are errors so typos do
//! not go unnoticed. See [`DEFAULT_CONFIG_TOML`] for the documented format.

pub mod color;
pub mod keybindings;
pub mod path;
pub mod theme;

use std::fmt;
use std::io;
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer};

pub use color::Rgb;
pub use keybindings::{Bindings, KeybindingsConfig};
pub use path::{Platform, config_path, has_env_override};
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
# [shell], [renderer] and the window size, which apply on the next start.

[font]
# Font family. When unset or not installed, the system monospace font is used.
# family = "JetBrains Mono"
# Families searched, in order, for characters the font above lacks, such as
# the Nerd Font icons printed by eza or yazi. After this list, installed
# "Symbols Nerd Font Mono", "Symbols Nerd Font", any other Nerd Font,
# "Noto Sans Symbols 2", "Noto Sans Symbols" and "DejaVu Sans" are tried.
# Only monochrome outline glyphs are drawn (no color emoji).
# fallback = ["Symbols Nerd Font Mono"]
# Size in points at 100% display scale (clamped to 6-72).
size = 14.0

[window]
# Space around the grid, in pixels at 100% display scale.
padding = 4
# Initial size in character cells.
columns = 100
rows = 30
# Tab bar above the grid: auto (only with two or more tabs), always or never.
tab_bar = "auto"

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
# Selected text. Unset, selected cells are drawn with their colors swapped.
# selection_foreground = "#c0caf5"
# selection_background = "#33467c"

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

[keybindings]
# Shortcuts handled by the terminal instead of being sent to the shell, as
# "chord" = "action". Entries are added to the defaults below; map a default
# chord to "none" to free it for the shell.
#
# A chord is modifiers and one key joined with "+", in any order and case.
# Modifiers: ctrl, alt, shift, super (cmd on macOS). Keys: a character,
# f1-f24, tab, enter, escape, space, backspace, delete, insert, home, end,
# pageup, pagedown, up, down, left, right, plus, minus, equal.
#
# Actions: zoom_in, zoom_out, reset_zoom, scroll_page_up, scroll_page_down,
# scroll_to_top, scroll_to_bottom, new_tab, close_tab, next_tab, previous_tab,
# goto_tab_1 to goto_tab_9, command_palette, reload_config (unbound by
# default), none. Scrolling keys reach the application on the alternate
# screen (full-screen programs).
#
# The defaults (on macOS the zoom chords use cmd instead of ctrl):
# "ctrl+equal" = "zoom_in"
# "ctrl+plus" = "zoom_in"
# "ctrl+minus" = "zoom_out"
# "ctrl+0" = "reset_zoom"
# "shift+pageup" = "scroll_page_up"
# "shift+pagedown" = "scroll_page_down"
# "shift+home" = "scroll_to_top"
# "shift+end" = "scroll_to_bottom"
# "ctrl+shift+t" = "new_tab"
# "ctrl+shift+w" = "close_tab"
# "ctrl+tab" = "next_tab"
# "ctrl+shift+tab" = "previous_tab"
# "alt+1" = "goto_tab_1"
# "alt+2" = "goto_tab_2"
# "alt+3" = "goto_tab_3"
# "alt+4" = "goto_tab_4"
# "alt+5" = "goto_tab_5"
# "alt+6" = "goto_tab_6"
# "alt+7" = "goto_tab_7"
# "alt+8" = "goto_tab_8"
# "alt+9" = "goto_tab_9"
# "ctrl+shift+p" = "command_palette"
#
# Examples:
# "ctrl+shift+r" = "reload_config"
# "ctrl+tab" = "none"
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
    pub keybindings: KeybindingsConfig,
}

/// `[font]`
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FontConfig {
    /// Preferred family; `None` means the system monospace font.
    #[serde(deserialize_with = "non_empty")]
    pub family: Option<String>,
    /// Families searched, in order, for glyphs the primary font lacks
    /// (e.g. Nerd Font icons); blank names are dropped.
    #[serde(deserialize_with = "names")]
    pub fallback: Vec<String>,
    /// Points at scale 1.0, already clamped with [`clamp_font_size`].
    #[serde(deserialize_with = "font_size")]
    pub size: f32,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: None,
            fallback: Vec::new(),
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
    /// When the tab bar is shown.
    pub tab_bar: TabBar,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            padding: 4,
            columns: NonZeroU16::new(100).expect("non-zero"),
            rows: NonZeroU16::new(30).expect("non-zero"),
            tab_bar: TabBar::Auto,
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
    pub selection_foreground: Option<Rgb>,
    pub selection_background: Option<Rgb>,
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
            selection_foreground: self.selection_foreground.or(theme.selection_foreground),
            selection_background: self.selection_background.or(theme.selection_background),
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

impl KeybindingsConfig {
    /// The effective bindings: these entries over the defaults. `macos`
    /// selects Cmd instead of Ctrl for the default zoom chords.
    pub fn resolve(&self, macos: bool) -> Bindings {
        Bindings::new(self, macos)
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

/// When the tab bar is shown, one cell row high above the grid.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TabBar {
    /// Only with two or more tabs.
    #[default]
    Auto,
    Always,
    Never,
}

impl TabBar {
    /// Whether the bar shows with `tabs` tabs open.
    pub fn visible(self, tabs: usize) -> bool {
        match self {
            Self::Auto => tabs > 1,
            Self::Always => true,
            Self::Never => false,
        }
    }
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

/// Writes [`DEFAULT_CONFIG_TOML`] to `path`, creating its directories,
/// unless a file is already there. Returns whether it was written; an
/// existing file is never touched.
pub fn write_default(path: &Path) -> io::Result<bool> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // `create_new` fails instead of truncating a file created meanwhile.
    let mut file = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(false),
        Err(error) => return Err(error),
    };
    if let Err(error) = io::Write::write_all(&mut file, DEFAULT_CONFIG_TOML.as_bytes()) {
        // Do not leave a truncated file that would load as a broken config.
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(error);
    }
    Ok(true)
}

/// Treats an empty or blank string as absent.
fn non_empty<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value.filter(|s| !s.trim().is_empty()))
}

/// Trims each name and drops the blank ones.
fn names<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    let names = Vec::<String>::deserialize(deserializer)?;
    Ok(names
        .into_iter()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .collect())
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
    fn keybindings_section_merges_over_the_defaults() {
        let config =
            parse("[keybindings]\n\"ctrl+alt+n\" = \"new_tab\"\n\"ctrl+0\" = \"none\"\n").unwrap();
        let bindings = config.keybindings.resolve(false);
        let chord = |text: &str| text.parse::<keybindings::Chord>().unwrap();
        assert_eq!(
            bindings.action(&chord("ctrl+alt+n")),
            Some(keybindings::Action::NewTab)
        );
        assert_eq!(bindings.action(&chord("ctrl+0")), None);
        assert_eq!(
            Config::default().keybindings.resolve(true),
            Bindings::defaults(true)
        );
    }

    #[test]
    fn keybinding_errors_carry_path_and_reason() {
        let error = parse_error("[keybindings]\n\"ctrl+t\" = \"copy\"\n");
        assert!(error.contains("/cfg/nxgterm.toml"), "{error}");
        assert!(error.contains("unknown action `copy`"), "{error}");
        let error = parse_error("[keybindings]\n\"ctrl+bogus\" = \"new_tab\"\n");
        assert!(error.contains("unknown key `bogus`"), "{error}");
    }

    #[test]
    fn documented_sample_lists_every_default_binding() {
        for (chord, action) in Bindings::defaults(false).entries() {
            let line = format!("# \"{chord}\" = \"{action}\"");
            assert!(DEFAULT_CONFIG_TOML.contains(&line), "missing {line}");
        }
        for action in keybindings::Action::ALL {
            assert!(
                DEFAULT_CONFIG_TOML.contains(action.name()),
                "{} is not documented",
                action.name()
            );
        }
    }

    #[test]
    fn defaults_match_the_documentation() {
        let config = Config::default();
        assert_eq!(config.font.family, None);
        assert!(config.font.fallback.is_empty());
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
        assert_eq!(config.window.tab_bar, TabBar::Auto);
    }

    #[test]
    fn tab_bar_takes_auto_always_or_never() {
        let tab_bar = |value: &str| {
            parse(&format!("[window]\ntab_bar = \"{value}\"\n")).map(|c| c.window.tab_bar)
        };
        assert_eq!(tab_bar("auto").unwrap(), TabBar::Auto);
        assert_eq!(tab_bar("always").unwrap(), TabBar::Always);
        assert_eq!(tab_bar("never").unwrap(), TabBar::Never);
        assert!(tab_bar("sometimes").is_err());
    }

    #[test]
    fn tab_bar_shows_by_tab_count() {
        assert!(!TabBar::Auto.visible(1));
        assert!(TabBar::Auto.visible(2));
        assert!(TabBar::Always.visible(1));
        assert!(!TabBar::Never.visible(5));
    }

    #[test]
    fn font_fallback_keeps_order_and_drops_blank_names() {
        let config = parse(
            "[font]\nfallback = [\" Symbols Nerd Font Mono \", \"\", \"Noto Sans Symbols 2\"]",
        )
        .unwrap();
        assert_eq!(
            config.font.fallback,
            ["Symbols Nerd Font Mono", "Noto Sans Symbols 2"]
        );
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
    fn selection_colors_are_unset_unless_configured() {
        let colors = parse("[colors]\nselection_background = \"#334455\"")
            .unwrap()
            .colors
            .resolve();
        assert_eq!(colors.selection_background, Some(Rgb::hex(0x334455)));
        assert_eq!(colors.selection_foreground, None);
        for theme in THEMES {
            assert_eq!(theme.colors.selection_background, None, "{}", theme.name);
        }
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

    fn scratch_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("nxg-config-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn write_default_creates_the_file_and_its_directories() {
        let dir = scratch_dir("write-default");
        let file = dir.join("nested").join("nxgterm.toml");

        assert!(write_default(&file).unwrap(), "created");
        let text = std::fs::read_to_string(&file).unwrap();
        assert_eq!(text, DEFAULT_CONFIG_TOML);
        assert_eq!(Config::load(&file).unwrap(), Some(Config::default()));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn write_default_never_overwrites() {
        let dir = scratch_dir("no-overwrite");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("nxgterm.toml");
        std::fs::write(&file, "[window]\npadding = 9\n").unwrap();

        assert!(!write_default(&file).unwrap(), "already there");
        let text = std::fs::read_to_string(&file).unwrap();
        assert_eq!(text, "[window]\npadding = 9\n");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn write_default_reports_unwritable_locations() {
        let dir = scratch_dir("unwritable");
        std::fs::create_dir_all(&dir).unwrap();
        // A regular file where the parent directory should be.
        let blocker = dir.join("blocker");
        std::fs::write(&blocker, "").unwrap();

        assert!(write_default(&blocker.join("nxgterm.toml")).is_err());

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
