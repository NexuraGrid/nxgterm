//! Configuration: the TOML file, its imports, its defaults, the built-in
//! themes and theme files, and the key bindings.
//!
//! Pure: no window, GPU or OS APIs. Every key is optional; a missing file
//! or section means the defaults, and unknown keys are errors so typos do
//! not go unnoticed. See [`DEFAULT_CONFIG_TOML`] for the documented format.

pub mod color;
pub mod keybindings;
pub mod load;
pub mod path;
pub mod theme;

use std::fmt;
use std::io;
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer};

pub use color::Rgb;
pub use keybindings::{Bindings, KeybindingsConfig};
pub use load::Loaded;
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
# [shell], [renderer], the window size, the window decorations and lowering
# the opacity from 1.0, which apply on the next start.

# Other config files merged first, in order; this file's own values win over
# theirs. Paths are relative to this file's directory; "~" is the home
# directory. Imported files can import others. A missing file is an error.
# import = ["fonts.toml", "~/dotfiles/nxgterm-colors.toml"]

[font]
# Font family, or a list of families tried in order: the first installed one
# is used. When unset or none is installed, the system monospace font is used.
# family = "JetBrains Mono"
# family = ["JetBrainsMono Nerd Font", "Fira Code"]
# Families searched, in order, for characters the font above lacks, such as
# the Nerd Font icons printed by eza or yazi. After this list, installed
# "Symbols Nerd Font Mono", "Symbols Nerd Font", any other Nerd Font,
# "Noto Sans Symbols 2", "Noto Sans Symbols", "DejaVu Sans",
# "Segoe UI Symbol", "Apple Symbols" and "Segoe UI Emoji" are tried.
# Only monochrome outline glyphs are drawn (no color emoji).
# fallback = ["Symbols Nerd Font Mono"]
# Size in points at 100% display scale (clamped to 6-72).
size = 14.0

[window]
# Space around the grid, in pixels at 100% display scale.
padding = 8
# Initial size in character cells.
columns = 100
rows = 30
# Tab bar above the grid: auto (only with two or more tabs), always or never.
tab_bar = "auto"
# Window decorations: "integrated" (the tab bar is the title bar: it always
# shows, drag it to move the window and double-click it to maximize; on
# Windows and Linux it draws minimize, maximize and close buttons and the
# window edges resize it, on macOS the native window buttons stay on its
# left) or "native" (the system title bar, with the tab bar below it).
decorations = "integrated"
# Opacity of the default background, from 0.0 (invisible) to 1.0 (opaque).
# Text, the cursor, colored backgrounds, the selection, the tab bar and
# images stay opaque. Below 1.0 the window is created transparent; it needs
# the GPU renderer and a surface that supports it (macOS, Wayland, X11 with a
# compositor; not Windows yet), otherwise the background stays opaque.
opacity = 1.0
# Ask the system to blur what is behind a translucent background (macOS,
# KDE Plasma on Wayland, Windows 11 Acrylic).
blur = false

[colors]
# Built-in theme: catppuccin-mocha, nxg-dark, nxg-light, tokyo-night,
# gruvbox-dark, dracula, nord, one-dark. Or a theme file: theme = "my-theme"
# reads themes/my-theme.toml next to this file (it wins over a built-in theme
# of the same name). A theme file holds the overrides below, at its top level
# or under [colors]; the colors it leaves out come from catppuccin-mocha.
theme = "catppuccin-mocha"
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

[selection]
# Select with the left button: drag for characters, double-click for a word,
# triple-click for a line, Alt+drag for a block. While a program uses the
# mouse, hold Shift to select. Middle-click pastes the PRIMARY selection
# (Linux).
# Copy selected text to the PRIMARY selection right away (Linux X11 and
# Wayland; ignored on other systems).
copy_on_select = true

[panes]
# Split panes (see the split_* and focus_pane_* actions below). Color of the
# lines between panes, as "#rrggbb" (or "#rgb"). Unset, the foreground faded
# into the background.
# divider_color = "#414868"
# Thickness of those lines in pixels (at most one cell).
divider_width = 1
# How far panes without focus fade toward the background, from 0.0 (not at
# all) to 1.0 (hidden). Images are not dimmed.
inactive_dim = 0.25

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
# goto_tab_1 to goto_tab_9, split_right, split_down, focus_pane_left,
# focus_pane_right, focus_pane_up, focus_pane_down, resize_pane_left,
# resize_pane_right, resize_pane_up, resize_pane_down, close_pane, zoom_pane,
# equalize_panes (unbound by default), command_palette, copy, paste,
# select_all (unbound by default), reload_config (unbound by default), none. Scrolling keys reach
# the application on the alternate screen (full-screen programs). Copy does
# nothing without a selection; ctrl+c stays an interrupt for the shell.
#
# The defaults (on macOS the zoom chords use cmd instead of ctrl, copy and
# paste are cmd+c and cmd+v, and the pane focus and resize chords use cmd
# instead of ctrl, because ctrl+arrows belong to Mission Control). On Linux
# desktops (KDE, GNOME) ctrl+alt+arrows may switch workspaces: rebind the
# focus_pane_* chords or free them with "none":
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
# "ctrl+shift+o" = "split_right"
# "ctrl+shift+e" = "split_down"
# "ctrl+alt+left" = "focus_pane_left"
# "ctrl+alt+right" = "focus_pane_right"
# "ctrl+alt+up" = "focus_pane_up"
# "ctrl+alt+down" = "focus_pane_down"
# "ctrl+alt+shift+left" = "resize_pane_left"
# "ctrl+alt+shift+right" = "resize_pane_right"
# "ctrl+alt+shift+up" = "resize_pane_up"
# "ctrl+alt+shift+down" = "resize_pane_down"
# "ctrl+shift+x" = "close_pane"
# "ctrl+shift+enter" = "zoom_pane"
# "ctrl+shift+p" = "command_palette"
# "ctrl+shift+c" = "copy"
# "ctrl+shift+v" = "paste"
# "shift+insert" = "paste"
#
# Examples:
# "ctrl+shift+r" = "reload_config"
# "ctrl+tab" = "none"
"##;

/// The whole configuration file.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Files merged under this one, as written; [`Config::load`] applies
    /// them and leaves this empty.
    pub import: Vec<String>,
    pub font: FontConfig,
    pub window: WindowConfig,
    pub colors: ColorsConfig,
    pub shell: ShellConfig,
    pub renderer: RendererConfig,
    pub scrollback: ScrollbackConfig,
    pub selection: SelectionConfig,
    pub panes: PanesConfig,
    pub keybindings: KeybindingsConfig,
}

/// `[font]`
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FontConfig {
    /// Preferred families, in order: the first installed one is used.
    /// Empty (or none installed) means the system monospace font. A
    /// single name or a list; blank names are dropped.
    #[serde(deserialize_with = "one_or_more_names")]
    pub family: Vec<String>,
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
            family: Vec::new(),
            fallback: Vec::new(),
            size: DEFAULT_FONT_SIZE,
        }
    }
}

/// `[window]`
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WindowConfig {
    /// Pixels around the grid at scale 1.0.
    pub padding: u16,
    /// Initial grid size in cells.
    pub columns: NonZeroU16,
    pub rows: NonZeroU16,
    /// When the tab bar is shown.
    pub tab_bar: TabBar,
    /// The system title bar, or the tab bar as the title bar.
    pub decorations: Decorations,
    /// Opacity of the default background, already clamped with
    /// [`clamp_opacity`]; 1.0 keeps the window opaque.
    #[serde(deserialize_with = "opacity")]
    pub opacity: f32,
    /// Ask the system to blur what is behind a translucent background.
    pub blur: bool,
}

impl WindowConfig {
    /// Whether the default background is drawn translucent.
    pub fn translucent(&self) -> bool {
        self.opacity < 1.0
    }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            padding: 8,
            columns: NonZeroU16::new(100).expect("non-zero"),
            rows: NonZeroU16::new(30).expect("non-zero"),
            tab_bar: TabBar::Auto,
            decorations: Decorations::Integrated,
            opacity: 1.0,
            blur: false,
        }
    }
}

/// `[colors]`: a built-in theme or theme file plus optional overrides.
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
        let theme = self.theme.colors();
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

/// `[panes]`
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PanesConfig {
    /// Color of the lines between panes; `None` fades the foreground into
    /// the background.
    pub divider_color: Option<Rgb>,
    /// Thickness of those lines in pixels (the renderer clamps it to the
    /// cell).
    pub divider_width: NonZeroU16,
    /// How far panes without focus fade toward the background, 0.0 to 1.0,
    /// already clamped with [`clamp_dim`].
    #[serde(deserialize_with = "inactive_dim")]
    pub inactive_dim: f32,
}

impl Default for PanesConfig {
    fn default() -> Self {
        Self {
            divider_color: None,
            divider_width: NonZeroU16::MIN,
            inactive_dim: 0.25,
        }
    }
}

/// `[selection]`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SelectionConfig {
    /// Copy selected text to the PRIMARY selection as soon as it is made
    /// (Linux X11 and Wayland; ignored elsewhere).
    pub copy_on_select: bool,
}

impl Default for SelectionConfig {
    fn default() -> Self {
        Self {
            copy_on_select: true,
        }
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

/// How the window is framed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decorations {
    /// The system title bar and borders.
    Native,
    /// No system title bar: the tab bar takes its place.
    #[default]
    Integrated,
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

/// Clamps a background opacity to 0.0..=1.0; `None` when it is not a
/// finite number.
pub fn clamp_opacity(opacity: f64) -> Option<f32> {
    opacity.is_finite().then(|| opacity.clamp(0.0, 1.0) as f32)
}

/// Why a config file could not be used.
#[derive(Debug)]
pub enum ConfigError {
    /// The file exists but could not be read.
    Io { path: PathBuf, source: io::Error },
    /// The file is not valid TOML or does not match the schema. `message`
    /// carries the line, column and offending snippet.
    Parse { path: PathBuf, message: String },
    /// An `import` of the file at `path` cannot be used: unreadable, a
    /// cycle or nested too deep.
    Import {
        path: PathBuf,
        import: PathBuf,
        reason: String,
    },
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
            Self::Import {
                path,
                import,
                reason,
            } => write!(
                f,
                "invalid config {}: cannot import {}: {reason}",
                path.display(),
                import.display()
            ),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Parse { .. } | Self::Import { .. } => None,
        }
    }
}

impl Config {
    /// Parses `text`, the contents of the main config file at `path`: its
    /// imports are read relative to it and its theme files from the
    /// `themes` directory next to it (see [`load::load`]).
    pub fn parse(text: &str, path: &Path) -> Result<Self, ConfigError> {
        Self::parse_with_files(text, path).map(|loaded| loaded.config)
    }

    /// [`Config::parse`], with the files the config was built from.
    pub fn parse_with_files(text: &str, path: &Path) -> Result<Loaded, ConfigError> {
        let home = path::home_dir(Platform::current(), |name| std::env::var_os(name));
        load::load(text, path, home.as_deref())
    }

    /// Reads and parses the file at `path`. A missing file yields
    /// `Ok(None)` so callers can fall back to the defaults.
    pub fn load(path: &Path) -> Result<Option<Self>, ConfigError> {
        Ok(Self::load_with_files(path)?.map(|loaded| loaded.config))
    }

    /// [`Config::load`], with the files the config was built from.
    pub fn load_with_files(path: &Path) -> Result<Option<Loaded>, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse_with_files(&text, path).map(Some),
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

/// A name or a list of names; trims each one and drops the blank ones.
fn one_or_more_names<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged, expecting = "a string or an array of strings")]
    enum OneOrMore {
        One(String),
        More(Vec<String>),
    }
    let names = match OneOrMore::deserialize(deserializer)? {
        OneOrMore::One(name) => vec![name],
        OneOrMore::More(names) => names,
    };
    Ok(trimmed(names))
}

/// Trims each name and drops the blank ones.
fn names<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    Ok(trimmed(Vec::<String>::deserialize(deserializer)?))
}

fn trimmed(names: Vec<String>) -> Vec<String> {
    names
        .into_iter()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .collect()
}

/// An integer or a float, as TOML has both.
#[derive(Deserialize)]
#[serde(untagged)]
enum Number {
    Int(i64),
    Float(f64),
}

impl Number {
    fn get(self) -> f64 {
        match self {
            Self::Int(n) => n as f64,
            Self::Float(n) => n,
        }
    }
}

/// Accepts integers or floats, clamps them and rejects `nan` and `inf`.
fn opacity<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    let value = Number::deserialize(deserializer)?.get();
    clamp_opacity(value).ok_or_else(|| {
        serde::de::Error::custom(format!(
            "invalid opacity `{value}`, expected a number from 0.0 to 1.0"
        ))
    })
}

/// Clamps the dimming of inactive panes to 0.0..=1.0; `None` when it is
/// not a finite number.
pub fn clamp_dim(dim: f64) -> Option<f32> {
    dim.is_finite().then(|| dim.clamp(0.0, 1.0) as f32)
}

/// Accepts integers or floats, clamps them and rejects `nan` and `inf`.
fn inactive_dim<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    let value = Number::deserialize(deserializer)?.get();
    clamp_dim(value).ok_or_else(|| {
        serde::de::Error::custom(format!(
            "invalid inactive_dim `{value}`, expected a number from 0.0 to 1.0"
        ))
    })
}

/// Accepts integers or floats and clamps them.
fn font_size<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    let size = Number::deserialize(deserializer)?.get() as f32;
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
        let error = parse_error("[keybindings]\n\"ctrl+t\" = \"cut\"\n");
        assert!(error.contains("/cfg/nxgterm.toml"), "{error}");
        assert!(error.contains("unknown action `cut`"), "{error}");
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
        assert!(config.font.family.is_empty());
        assert!(config.font.fallback.is_empty());
        assert_eq!(config.font.size, 14.0);
        assert_eq!(config.window.padding, 8);
        assert_eq!(
            (config.window.columns.get(), config.window.rows.get()),
            (100, 30)
        );
        assert_eq!(config.colors.theme.name(), "catppuccin-mocha");
        assert_eq!(config.shell, ShellConfig::default());
        assert_eq!(config.renderer.backend, Backend::Auto);
        assert_eq!(config.scrollback.lines, 10_000);
        assert_eq!(config.window.tab_bar, TabBar::Auto);
        assert_eq!(config.window.decorations, Decorations::Integrated);
        assert_eq!(config.window.opacity, 1.0);
        assert!(!config.window.translucent());
        assert!(!config.window.blur);
        assert!(config.selection.copy_on_select);
        assert_eq!(
            config.colors.resolve().selection_background,
            Some(Rgb::hex(0x585b70))
        );
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
    fn decorations_take_native_or_integrated() {
        let decorations = |value: &str| {
            parse(&format!("[window]\ndecorations = \"{value}\"\n")).map(|c| c.window.decorations)
        };
        assert_eq!(decorations("native").unwrap(), Decorations::Native);
        assert_eq!(decorations("integrated").unwrap(), Decorations::Integrated);
        assert!(decorations("none").is_err());
    }

    #[test]
    fn opacity_takes_integers_or_floats_and_is_clamped() {
        let window = |text: &str| parse(&format!("[window]\n{text}\n")).map(|c| c.window);
        let translucent = window("opacity = 0.85\nblur = true").unwrap();
        assert_eq!(translucent.opacity, 0.85);
        assert!(translucent.translucent() && translucent.blur);
        assert_eq!(window("opacity = 0").unwrap().opacity, 0.0);
        assert_eq!(window("opacity = 1").unwrap().opacity, 1.0);
        assert_eq!(window("opacity = 1.5").unwrap().opacity, 1.0);
        assert_eq!(window("opacity = -0.2").unwrap().opacity, 0.0);
        assert!(!window("opacity = 2").unwrap().translucent());
    }

    #[test]
    fn non_finite_opacity_is_rejected() {
        for value in ["nan", "inf", "-inf"] {
            let error = parse_error(&format!("[window]\nopacity = {value}\n"));
            assert!(error.contains("invalid opacity"), "{error}");
            assert!(error.contains("line 2"), "{error}");
        }
        assert!(parse("[window]\nopacity = \"half\"").is_err());
        assert!(parse("[window]\nblur = \"yes\"").is_err());
    }

    #[test]
    fn clamp_opacity_handles_bounds_and_non_finite() {
        assert_eq!(clamp_opacity(0.5), Some(0.5));
        assert_eq!(clamp_opacity(-1.0), Some(0.0));
        assert_eq!(clamp_opacity(7.0), Some(1.0));
        assert_eq!(clamp_opacity(f64::NAN), None);
        assert_eq!(clamp_opacity(f64::NEG_INFINITY), None);
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
            [selection]
            copy_on_select = false
            "##,
        )
        .unwrap();
        assert_eq!(config.font.family, ["JetBrains Mono"]);
        assert_eq!(config.font.size, 12.0);
        assert_eq!(config.window.padding, 0);
        assert_eq!(
            (config.window.columns.get(), config.window.rows.get()),
            (80, 24)
        );
        assert_eq!(config.colors.theme.name(), "dracula");
        assert_eq!(config.colors.background, Some(Rgb::hex(0)));
        assert_eq!(config.shell.program.as_deref(), Some("pwsh.exe"));
        assert_eq!(config.shell.args, ["-NoLogo"]);
        assert_eq!(config.renderer.backend, Backend::Cpu);
        assert_eq!(config.scrollback.lines, 0);
        assert!(!config.selection.copy_on_select);
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
        assert!(config.font.family.is_empty());
        assert_eq!(config.shell.program, None);
    }

    #[test]
    fn font_family_takes_a_name_or_a_list() {
        let family =
            |value: &str| parse(&format!("[font]\nfamily = {value}\n")).map(|c| c.font.family);
        assert_eq!(family("\" Iosevka \"").unwrap(), ["Iosevka"]);
        assert_eq!(
            family(r#"["JetBrainsMono Nerd Font", "Fira Code"]"#).unwrap(),
            ["JetBrainsMono Nerd Font", "Fira Code"]
        );
        assert!(family("[]").unwrap().is_empty());
        assert_eq!(family(r#"["", "  ", " Hack "]"#).unwrap(), ["Hack"]);
        let error = family("12").unwrap_err().to_string();
        assert!(error.contains("a string or an array of strings"), "{error}");
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
    fn selection_colors_come_from_the_theme_unless_configured() {
        let colors = parse("[colors]\nselection_background = \"#334455\"")
            .unwrap()
            .colors
            .resolve();
        assert_eq!(colors.selection_background, Some(Rgb::hex(0x334455)));
        assert_eq!(colors.selection_foreground, Some(Rgb::hex(0xcdd6f4)));
        let swapped = parse("[colors]\ntheme = \"nxg-dark\"")
            .unwrap()
            .colors
            .resolve();
        assert_eq!(swapped.selection_foreground, None);
        assert_eq!(swapped.selection_background, None);
    }

    #[test]
    fn errors_carry_path_line_and_reason() {
        let error = parse_error("[colors]\ntheme = \"solarized\"\n");
        assert!(error.contains("/cfg/nxgterm.toml"), "{error}");
        assert!(error.contains("line 2"), "{error}");
        assert!(error.contains("unknown theme `solarized`"), "{error}");
        assert!(error.contains("available: catppuccin-mocha"), "{error}");
    }

    #[test]
    fn panes_section_defaults_and_keys() {
        let defaults = Config::default().panes;
        assert_eq!(defaults.divider_color, None);
        assert_eq!(defaults.divider_width.get(), 1);
        assert_eq!(defaults.inactive_dim, 0.25);
        let panes =
            parse("[panes]\ndivider_color = \"#f80\"\ndivider_width = 3\ninactive_dim = 0.5\n")
                .unwrap()
                .panes;
        assert_eq!(panes.divider_color, Some(Rgb::hex(0xff8800)));
        assert_eq!(panes.divider_width.get(), 3);
        assert_eq!(panes.inactive_dim, 0.5);
        // Every key is optional.
        assert_eq!(parse("[panes]\n").unwrap().panes, defaults);
    }

    #[test]
    fn inactive_dim_is_clamped_and_must_be_finite() {
        let dim = |value: &str| parse(&format!("[panes]\ninactive_dim = {value}\n"));
        assert_eq!(dim("2").unwrap().panes.inactive_dim, 1.0);
        assert_eq!(dim("-0.5").unwrap().panes.inactive_dim, 0.0);
        assert_eq!(dim("0").unwrap().panes.inactive_dim, 0.0);
        for value in ["nan", "inf", "-inf"] {
            let error = dim(value).unwrap_err().to_string();
            assert!(error.contains("invalid inactive_dim"), "{error}");
        }
    }

    #[test]
    fn panes_section_rejects_bad_values_and_unknown_keys() {
        let error = parse_error("[panes]\ngap = 1\n");
        assert!(error.contains("gap") && error.contains("line 2"), "{error}");
        assert!(parse("[panes]\ndivider_width = 0\n").is_err(), "non-zero");
        assert!(parse("[panes]\ndivider_width = -1\n").is_err());
        let error = parse_error("[panes]\ndivider_color = \"red\"\n");
        assert!(error.contains("invalid color `red`"), "{error}");
    }

    #[test]
    fn documented_sample_lists_the_panes_keys() {
        for key in ["[panes]", "divider_color", "divider_width", "inactive_dim"] {
            assert!(DEFAULT_CONFIG_TOML.contains(key), "{key} is not documented");
        }
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
