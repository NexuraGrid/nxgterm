//! Reading a config file: its `import`s, merged in order under the file's
//! own values, and the theme file its `[colors] theme` may name.
//!
//! Each file is first checked alone against the schema, so an error points
//! at its line in that file. The files are then merged as TOML tables:
//! tables merge key by key, any other value (arrays included) replaces the
//! earlier one. Imports apply in order and the importing file's own values
//! win, at every level.

use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use toml::{Spanned, Table, Value};

use crate::path;
use crate::theme::{self, ThemeFile, ThemeName, UnknownTheme};
use crate::{Config, ConfigError};

/// Imports nested deeper than this (counting from the main file) are an
/// error, as are cycles.
pub const MAX_IMPORT_DEPTH: usize = 8;

/// Directory, next to the main config file, that holds the theme files.
pub const THEMES_DIR: &str = "themes";

/// A config and every file it was built from: the main file, its imports
/// and the theme file its theme would come from (whether or not it
/// exists), to watch for changes.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub config: Config,
    pub files: Vec<PathBuf>,
}

/// One file read while loading.
struct Source {
    path: PathBuf,
    text: String,
    /// It sets `[colors] theme`.
    sets_theme: bool,
}

struct Loader<'a> {
    home: Option<&'a Path>,
    /// Files in the order their values apply: each one after its imports.
    sources: Vec<Source>,
    /// The chain of files being imported, to detect cycles.
    stack: Vec<PathBuf>,
}

/// Loads the config whose main file at `path` holds `text`. `home`
/// expands `~` in imports.
pub fn load(text: &str, path: &Path, home: Option<&Path>) -> Result<Loaded, ConfigError> {
    let mut loader = Loader {
        home,
        sources: Vec::new(),
        stack: Vec::new(),
    };
    let merged = loader.tree(path, text.to_owned())?;
    let mut config: Config = merged.try_into().map_err(|error| ConfigError::Parse {
        path: path.to_owned(),
        message: error.to_string(),
    })?;
    let mut files: Vec<PathBuf> = loader.sources.iter().map(|s| s.path.clone()).collect();
    let themes_dir = path.parent().unwrap_or(Path::new("")).join(THEMES_DIR);
    let name = config.colors.theme.name().to_owned();
    if let Some(file) = theme::file_path(&themes_dir, &name) {
        files.push(file.clone());
        match std::fs::read_to_string(&file) {
            Ok(text) => {
                let colors = ThemeFile::parse(&text)
                    .map_err(|error| ConfigError::Parse {
                        path: file.clone(),
                        message: error.to_string(),
                    })?
                    .colors();
                config.colors.theme = ThemeName::from_file(&name, colors, file);
                return Ok(Loaded { config, files });
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => return Err(ConfigError::Io { path: file, source }),
        }
    }
    if theme::find(&name).is_none() {
        let unknown = UnknownTheme {
            name,
            custom: theme::file_names(&themes_dir),
            dir: Some(themes_dir),
        };
        return Err(loader.theme_error(path, &unknown));
    }
    Ok(Loaded { config, files })
}

impl Loader<'_> {
    /// The table of the file at `path` holding `text`, its imports merged
    /// under it.
    fn tree(&mut self, path: &Path, text: String) -> Result<Table, ConfigError> {
        let parse_error = |error: toml::de::Error| ConfigError::Parse {
            path: path.to_owned(),
            message: error.to_string(),
        };
        // Checked alone first, so errors point at this file's lines.
        toml::from_str::<Config>(&text).map_err(parse_error)?;
        let mut table: Table = toml::from_str(&text).map_err(parse_error)?;
        let imports: Vec<String> = match table.remove("import") {
            Some(Value::Array(entries)) => entries
                .into_iter()
                .filter_map(|entry| entry.as_str().map(str::to_owned))
                .collect(),
            _ => Vec::new(),
        };
        let dir = path.parent().unwrap_or(Path::new(""));
        let mut merged = Table::new();
        self.stack.push(identity(path));
        for entry in imports {
            let target = path::resolve_import(&entry, dir, self.home);
            let error = |reason: String| ConfigError::Import {
                path: path.to_owned(),
                import: target.clone(),
                reason,
            };
            if self.stack.contains(&identity(&target)) {
                return Err(error("import cycle".into()));
            }
            if self.stack.len() > MAX_IMPORT_DEPTH {
                return Err(error(format!(
                    "imports nest deeper than {MAX_IMPORT_DEPTH} levels"
                )));
            }
            let text = std::fs::read_to_string(&target).map_err(|e| error(e.to_string()))?;
            let imported = self.tree(&target, text)?;
            merge(&mut merged, imported);
        }
        self.stack.pop();
        let sets_theme = table
            .get("colors")
            .and_then(|colors| colors.get("theme"))
            .is_some();
        self.sources.push(Source {
            path: path.to_owned(),
            text,
            sets_theme,
        });
        merge(&mut merged, table);
        Ok(merged)
    }

    /// `unknown` reported at the `theme` line of the file whose value won
    /// (the last one applied that sets it), or of the main file at `path`.
    fn theme_error(&self, path: &Path, unknown: &UnknownTheme) -> ConfigError {
        let message = unknown.to_string();
        let Some(source) = self.sources.iter().rev().find(|source| source.sets_theme) else {
            return ConfigError::Parse {
                path: path.to_owned(),
                message,
            };
        };
        let message = theme_span(&source.text)
            .map(|span| located(&source.text, span, &message))
            .unwrap_or(message);
        ConfigError::Parse {
            path: source.path.clone(),
            message,
        }
    }
}

/// Merges `over` into `base`: tables key by key, anything else replaced.
pub fn merge(base: &mut Table, over: Table) {
    for (key, value) in over {
        match (base.get_mut(&key), value) {
            (Some(Value::Table(base)), Value::Table(over)) => merge(base, over),
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

/// `path` resolved through links when it exists, so one file reached two
/// ways is still a cycle.
fn identity(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

/// Where `[colors] theme` is in `text`, in bytes.
fn theme_span(text: &str) -> Option<std::ops::Range<usize>> {
    #[derive(Deserialize)]
    struct Probe {
        colors: Option<ProbeColors>,
    }
    #[derive(Deserialize)]
    struct ProbeColors {
        theme: Option<Spanned<String>>,
    }
    let probe: Probe = toml::from_str(text).ok()?;
    Some(probe.colors?.theme?.span())
}

/// `message` with the line, column and snippet at `span` of `text`, laid
/// out like the TOML parser's own errors.
fn located(text: &str, span: std::ops::Range<usize>, message: &str) -> String {
    let start = span.start.min(text.len());
    let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[start..].find('\n').map_or(text.len(), |i| start + i);
    let line = text[line_start..line_end].trim_end_matches('\r');
    let number = text[..start].matches('\n').count() + 1;
    let column = text[line_start..start].chars().count() + 1;
    let width = text[start..span.end.clamp(start, line_end)]
        .chars()
        .count()
        .max(1);
    let gutter = " ".repeat(number.to_string().len());
    format!(
        "TOML parse error at line {number}, column {column}\n{gutter} |\n{number} | {line}\n{gutter} | {}{}\n{message}\n",
        " ".repeat(column - 1),
        "^".repeat(width)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rgb;

    /// A fresh directory for one test.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nxg-load-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    }

    /// Loads `main` (written to `dir/nxgterm.toml`).
    fn load_main(dir: &Path, main: &str) -> Result<Loaded, ConfigError> {
        let path = write(dir, "nxgterm.toml", main);
        load(main, &path, Some(Path::new("/nonexistent-home")))
    }

    #[test]
    fn tables_merge_key_by_key_and_other_values_are_replaced() {
        let mut base: Table = toml::from_str("a = 1\nlist = [1, 2]\n[t]\nx = 1\ny = 2").unwrap();
        let over: Table = toml::from_str("list = [3]\n[t]\ny = 3\nz = 4").unwrap();
        merge(&mut base, over);
        let expected: Table =
            toml::from_str("a = 1\nlist = [3]\n[t]\nx = 1\ny = 3\nz = 4").unwrap();
        assert_eq!(base, expected);
    }

    #[test]
    fn imports_merge_in_order_under_the_main_file() {
        let dir = scratch("order");
        write(
            &dir,
            "a.toml",
            "[font]\nsize = 10\n[window]\npadding = 1\ncolumns = 50",
        );
        write(&dir, "b.toml", "[font]\nsize = 11\n[window]\npadding = 2");
        let main = "import = [\"a.toml\", \"b.toml\"]\n[window]\npadding = 3\n";
        let loaded = load_main(&dir, main).unwrap();
        let config = loaded.config;
        assert_eq!(config.font.size, 11.0, "the later import wins");
        assert_eq!(config.window.padding, 3, "the main file wins");
        assert_eq!(config.window.columns.get(), 50, "kept from the first");
        assert!(config.import.is_empty(), "applied");
        assert_eq!(
            loaded.files,
            [
                dir.join("a.toml"),
                dir.join("b.toml"),
                dir.join("nxgterm.toml"),
                dir.join("themes").join("catppuccin-mocha.toml"),
            ]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn nested_imports_are_relative_to_the_importing_file() {
        let dir = scratch("nested");
        write(&dir, "parts/colors.toml", "import = [\"deeper/x.toml\"]\n");
        write(&dir, "parts/deeper/x.toml", "[scrollback]\nlines = 7\n");
        let absolute = write(&dir, "elsewhere/abs.toml", "[window]\nrows = 12\n");
        let main = format!(
            "import = [\"parts/colors.toml\", {:?}]\n",
            absolute.to_str().unwrap()
        );
        let config = load_main(&dir, &main).unwrap().config;
        assert_eq!(config.scrollback.lines, 7);
        assert_eq!(config.window.rows.get(), 12);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cycles_are_reported_with_the_importing_file() {
        let dir = scratch("cycle");
        write(&dir, "a.toml", "import = [\"b.toml\"]\n");
        write(&dir, "b.toml", "import = [\"a.toml\"]\n");
        let error = load_main(&dir, "import = [\"a.toml\"]\n").unwrap_err();
        let text = error.to_string();
        assert!(text.contains("import cycle"), "{text}");
        assert!(
            text.contains(&dir.join("b.toml").display().to_string()),
            "{text}"
        );
        let error = load_main(&dir, "import = [\"nxgterm.toml\"]\n").unwrap_err();
        assert!(error.to_string().contains("import cycle"), "self import");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn deep_imports_are_capped() {
        let dir = scratch("deep");
        for i in 0..=MAX_IMPORT_DEPTH {
            write(
                &dir,
                &format!("{i}.toml"),
                &format!("import = [\"{}.toml\"]\n", i + 1),
            );
        }
        write(&dir, &format!("{}.toml", MAX_IMPORT_DEPTH + 1), "");
        let error = load_main(&dir, "import = [\"0.toml\"]\n").unwrap_err();
        assert!(error.to_string().contains("deeper than 8"), "{error}");
        // One level less fits.
        write(&dir, &format!("{}.toml", MAX_IMPORT_DEPTH - 1), "");
        assert!(load_main(&dir, "import = [\"0.toml\"]\n").is_ok());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_import_is_an_error_naming_it() {
        let dir = scratch("missing");
        let error = load_main(&dir, "import = [\"nope.toml\"]\n").unwrap_err();
        let text = error.to_string();
        assert!(
            text.contains(&dir.join("nope.toml").display().to_string()),
            "{text}"
        );
        assert!(
            text.contains(&dir.join("nxgterm.toml").display().to_string()),
            "{text}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn errors_in_an_import_point_at_its_own_line() {
        let dir = scratch("import-error");
        write(&dir, "fonts.toml", "[font]\nsize = 12\nfamilly = \"x\"\n");
        let error = load_main(&dir, "import = [\"fonts.toml\"]\n").unwrap_err();
        let text = error.to_string();
        assert!(
            text.contains(&dir.join("fonts.toml").display().to_string()),
            "{text}"
        );
        assert!(
            text.contains("line 3") && text.contains("familly"),
            "{text}"
        );
        assert!(
            load_main(&dir, "import = \"fonts.toml\"\n").is_err(),
            "a list"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn theme_files_come_first_then_built_in_themes() {
        let dir = scratch("themes");
        let custom = write(
            &dir,
            "themes/custom-theme.toml",
            "foreground = \"#010203\"\nansi = [\"#000000\", \"#000001\", \"#000002\", \"#000003\", \"#000004\", \"#000005\", \"#000006\", \"#000007\", \"#000008\", \"#000009\", \"#00000a\", \"#00000b\", \"#00000c\", \"#00000d\", \"#00000e\", \"#00000f\"]\n",
        );
        let loaded = load_main(&dir, "[colors]\ntheme = \"custom-theme\"\n").unwrap();
        let theme = &loaded.config.colors.theme;
        assert_eq!(theme.name(), "custom-theme");
        assert_eq!(theme.file(), Some(custom.as_path()));
        let colors = loaded.config.colors.resolve();
        assert_eq!(colors.foreground, Rgb::new(1, 2, 3));
        assert_eq!(colors.ansi[15], Rgb::hex(0x0f));
        let base = theme::THEMES[0].colors;
        assert_eq!(colors.background, base.background, "left out: default");
        assert!(loaded.files.contains(&custom));

        // A theme file named like a built-in theme replaces it.
        write(
            &dir,
            "themes/nord.toml",
            "[colors]\nbackground = \"#000000\"\n",
        );
        let nord = load_main(&dir, "[colors]\ntheme = \"nord\"\n").unwrap();
        assert_eq!(nord.config.colors.resolve().background, Rgb::hex(0));

        // Without a file, the built-in theme; its file is still watched.
        let dracula = load_main(&dir, "[colors]\ntheme = \"dracula\"\n").unwrap();
        assert_eq!(dracula.config.colors.theme.file(), None);
        assert_eq!(
            dracula.config.colors.resolve(),
            theme::find("dracula").unwrap().colors
        );
        assert!(
            dracula
                .files
                .contains(&dir.join("themes").join("dracula.toml"))
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn overrides_apply_over_a_theme_file() {
        let dir = scratch("theme-overrides");
        write(&dir, "themes/mine.toml", "background = \"#000000\"\n");
        let main = "[colors]\ntheme = \"mine\"\nbackground = \"#111111\"\n";
        let colors = load_main(&dir, main).unwrap().config.colors.resolve();
        assert_eq!(colors.background, Rgb::hex(0x111111));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn errors_in_a_theme_file_name_it() {
        let dir = scratch("theme-error");
        write(&dir, "themes/broken.toml", "foreground = \"blue\"\n");
        let error = load_main(&dir, "[colors]\ntheme = \"broken\"\n").unwrap_err();
        let text = error.to_string();
        let file = dir.join("themes").join("broken.toml");
        assert!(text.contains(&file.display().to_string()), "{text}");
        assert!(text.contains("invalid color `blue`"), "{text}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unknown_themes_list_built_in_and_theme_files_at_their_line() {
        let dir = scratch("unknown-theme");
        write(&dir, "themes/work.toml", "");
        write(&dir, "themes/home.toml", "");
        write(&dir, "themes/notes.txt", "");
        let error = load_main(&dir, "[window]\npadding = 1\n[colors]\ntheme = \"nope\"\n")
            .unwrap_err()
            .to_string();
        assert!(error.contains("line 4, column 9"), "{error}");
        assert!(error.contains("theme = \"nope\""), "{error}");
        assert!(error.contains("unknown theme `nope`"), "{error}");
        assert!(error.contains("available: catppuccin-mocha"), "{error}");
        assert!(error.contains(": home, work"), "{error}");
        assert!(!error.contains("notes"), "{error}");

        // Set in an import: reported there.
        let colors = write(&dir, "colors.toml", "[colors]\ntheme = \"nope\"\n");
        let error = load_main(&dir, "import = [\"colors.toml\"]\n").unwrap_err();
        let text = error.to_string();
        assert!(text.contains(&colors.display().to_string()), "{text}");
        assert!(text.contains("line 2"), "{text}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn located_messages_mark_the_span() {
        let text = "a = 1\nb = \"xy\"\n";
        let message = located(text, 10..14, "bad");
        assert_eq!(
            message,
            "TOML parse error at line 2, column 5\n  |\n2 | b = \"xy\"\n  |     ^^^^\nbad\n"
        );
    }
}
