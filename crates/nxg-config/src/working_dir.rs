//! Where new shells start: `[shell] working_directory` and
//! `--working-directory`.
//!
//! Pure: the process cwd, the executable's directory, the home directory and
//! a "is this a directory" check are passed in, so every rule is testable on
//! every host.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer};

use crate::path::{Platform, expand_home};

/// The directory new shells start in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum WorkingDirectory {
    /// The process cwd, unless it looks like nxgterm was started by a
    /// launcher (see [`WorkingDirectory::resolve`]); then home.
    #[default]
    Auto,
    /// The process cwd.
    Inherit,
    /// The home directory.
    Home,
    /// This directory, as written: a leading `~` is the home directory and a
    /// relative path is relative to the process cwd.
    Path(PathBuf),
}

impl WorkingDirectory {
    /// Reads `auto`, `inherit`, `home` or a path; a blank value is `auto`.
    pub fn parse(value: &str) -> Self {
        match value.trim() {
            "" | "auto" => Self::Auto,
            "inherit" => Self::Inherit,
            "home" => Self::Home,
            _ => Self::Path(PathBuf::from(value)),
        }
    }

    /// The directory to start shells in, falling back to home and then to
    /// `None` (let the pty backend choose) when it is not a directory.
    ///
    /// `auto` is the process cwd unless it is missing, not a directory, `/`
    /// (Unix), the directory holding the executable, or the Windows or
    /// System32 directory (compared without case on Windows): launchers and
    /// shortcuts start programs there, so home is used instead.
    pub fn resolve(&self, start: &StartContext<'_>, is_dir: impl Fn(&Path) -> bool) -> Resolved {
        let home = start.home.filter(|home| is_dir(home));
        let requested = match self {
            Self::Auto => {
                let cwd = start
                    .cwd
                    .filter(|cwd| is_dir(cwd) && !start.launcher_dir(cwd));
                return match cwd.or(home) {
                    Some(dir) => Resolved::dir(dir),
                    None => Resolved::fallback("no usable working directory or home directory"),
                };
            }
            Self::Inherit => start.cwd.map(Path::to_path_buf),
            Self::Home => start.home.map(Path::to_path_buf),
            Self::Path(path) => {
                let path = expand_home(&path.to_string_lossy(), start.home);
                match start.cwd {
                    Some(cwd) if path.is_relative() => Some(cwd.join(path)),
                    _ => Some(path),
                }
            }
        };
        match requested {
            Some(dir) if is_dir(&dir) => Resolved::dir(&dir),
            requested => {
                let what = match requested {
                    Some(dir) => format!("working directory {} is not a directory", dir.display()),
                    None => format!("working directory {self:?} is unknown"),
                };
                match home {
                    Some(home) => Resolved {
                        dir: Some(home.to_path_buf()),
                        warning: Some(format!("{what}; using {}", home.display())),
                    },
                    None => Resolved::fallback(&what),
                }
            }
        }
    }
}

/// What [`WorkingDirectory::resolve`] looks at.
#[derive(Debug, Clone, Copy)]
pub struct StartContext<'a> {
    pub platform: Platform,
    /// The process cwd; `None` when it cannot be read (e.g. deleted).
    pub cwd: Option<&'a Path>,
    /// The directory containing the running executable.
    pub exe_dir: Option<&'a Path>,
    pub home: Option<&'a Path>,
    /// The Windows directory (`%SystemRoot%`); ignored elsewhere.
    pub system_root: Option<&'a Path>,
}

impl StartContext<'_> {
    /// Whether `dir` is where launchers, not users, start programs.
    fn launcher_dir(&self, dir: &Path) -> bool {
        let same = |other: &Path| same_dir(self.platform, dir, other);
        let system = self.platform == Platform::Windows
            && self
                .system_root
                .is_some_and(|root| same(root) || same(&root.join("System32")));
        let root = self.platform == Platform::Unix && same(Path::new("/"));
        root || system || self.exe_dir.is_some_and(same)
    }
}

/// The outcome of [`WorkingDirectory::resolve`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Where to start shells; `None` lets the pty backend choose.
    pub dir: Option<PathBuf>,
    /// Why the requested directory was not used, to log.
    pub warning: Option<String>,
}

impl Resolved {
    fn dir(dir: &Path) -> Self {
        Self {
            dir: Some(dir.to_path_buf()),
            warning: None,
        }
    }

    fn fallback(what: &str) -> Self {
        Self {
            dir: None,
            warning: Some(format!("{what}; using the default")),
        }
    }
}

/// `%SystemRoot%`, or `%windir%` when it is unset. `env` looks up
/// environment variables.
pub fn system_root(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let var = |name: &str| env(name).filter(|value| !value.is_empty());
    var("SystemRoot")
        .or_else(|| var("windir"))
        .map(PathBuf::from)
}

/// Whether `a` and `b` name the same directory as written: component-wise
/// on Unix; on Windows also ignoring case, separators, a trailing separator
/// and the `\\?\` prefix.
fn same_dir(platform: Platform, a: &Path, b: &Path) -> bool {
    match platform {
        Platform::Unix => a.components().eq(b.components()),
        Platform::Windows => windows_key(a) == windows_key(b),
    }
}

fn windows_key(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\");
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    text.trim_end_matches('\\').to_lowercase()
}

/// `working_directory` in the config file: like [`WorkingDirectory::parse`],
/// but a path must be absolute or start with `~`, since the file does not
/// know where nxgterm will be started.
pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<WorkingDirectory, D::Error> {
    let value = String::deserialize(deserializer)?;
    match WorkingDirectory::parse(&value) {
        WorkingDirectory::Path(path) if !starts_at_home(&value) && !path.is_absolute() => {
            Err(serde::de::Error::custom(format!(
                "invalid working_directory `{value}`, expected auto, inherit, home, \
                 an absolute path or a path starting with ~"
            )))
        }
        parsed => Ok(parsed),
    }
}

/// Whether `entry` starts with `~` alone or before a separator.
fn starts_at_home(entry: &str) -> bool {
    entry
        .strip_prefix('~')
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', '\\']))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CWD: &str = "/work/project";
    const HOME: &str = "/home/me";
    const EXE_DIR: &str = "/opt/nxgterm";

    fn unix(cwd: Option<&'static str>) -> StartContext<'static> {
        StartContext {
            platform: Platform::Unix,
            cwd: cwd.map(Path::new),
            exe_dir: Some(Path::new(EXE_DIR)),
            home: Some(Path::new(HOME)),
            system_root: Some(Path::new("/windows")),
        }
    }

    fn windows(cwd: &'static str) -> StartContext<'static> {
        StartContext {
            platform: Platform::Windows,
            cwd: Some(Path::new(cwd)),
            exe_dir: Some(Path::new(r"C:\Program Files\nxgterm")),
            home: Some(Path::new(r"C:\Users\me")),
            system_root: Some(Path::new(r"C:\WINDOWS")),
        }
    }

    fn every_dir(_: &Path) -> bool {
        true
    }

    fn dir_of(resolved: Resolved) -> Option<PathBuf> {
        resolved.dir
    }

    fn some(path: &str) -> Option<PathBuf> {
        Some(PathBuf::from(path))
    }

    #[test]
    fn values_parse_to_keywords_or_paths() {
        assert_eq!(WorkingDirectory::parse("auto"), WorkingDirectory::Auto);
        assert_eq!(WorkingDirectory::parse(""), WorkingDirectory::Auto);
        assert_eq!(WorkingDirectory::parse("  "), WorkingDirectory::Auto);
        assert_eq!(
            WorkingDirectory::parse("inherit"),
            WorkingDirectory::Inherit
        );
        assert_eq!(WorkingDirectory::parse("home"), WorkingDirectory::Home);
        assert_eq!(
            WorkingDirectory::parse("~/src"),
            WorkingDirectory::Path("~/src".into())
        );
        assert_eq!(
            WorkingDirectory::parse("./home"),
            WorkingDirectory::Path("./home".into())
        );
    }

    #[test]
    fn auto_inherits_a_normal_cwd() {
        let resolved = WorkingDirectory::Auto.resolve(&unix(Some(CWD)), every_dir);
        assert_eq!(resolved, Resolved::dir(Path::new(CWD)));
    }

    #[test]
    fn auto_uses_home_for_launcher_directories() {
        for cwd in ["/", "/opt/nxgterm", "/opt/nxgterm/"] {
            let resolved = WorkingDirectory::Auto.resolve(&unix(Some(cwd)), every_dir);
            assert_eq!(resolved, Resolved::dir(Path::new(HOME)), "cwd {cwd}");
        }
        // The Windows directory is only special on Windows.
        let resolved = WorkingDirectory::Auto.resolve(&unix(Some("/windows")), every_dir);
        assert_eq!(dir_of(resolved), some("/windows"));
    }

    #[test]
    fn auto_uses_home_for_a_missing_or_unusable_cwd() {
        assert_eq!(
            dir_of(WorkingDirectory::Auto.resolve(&unix(None), every_dir)),
            some(HOME)
        );
        let only_home = |dir: &Path| dir == Path::new(HOME);
        assert_eq!(
            dir_of(WorkingDirectory::Auto.resolve(&unix(Some(CWD)), only_home)),
            some(HOME)
        );
    }

    #[test]
    fn auto_on_windows_skips_system_and_program_directories() {
        for cwd in [
            r"C:\Windows\System32",
            r"c:\windows\system32\",
            r"C:\WINDOWS",
            r"C:\Program Files\nxgterm",
            r"\\?\C:\Program Files\NXGTERM",
        ] {
            let resolved = WorkingDirectory::Auto.resolve(&windows(cwd), every_dir);
            assert_eq!(dir_of(resolved), some(r"C:\Users\me"), "cwd {cwd}");
        }
        // A drive root is a normal directory.
        for cwd in [r"C:\", r"D:\code", r"C:\Windows\Temp"] {
            let resolved = WorkingDirectory::Auto.resolve(&windows(cwd), every_dir);
            assert_eq!(dir_of(resolved), some(cwd), "cwd {cwd}");
        }
    }

    #[test]
    fn inherit_and_home_ignore_launcher_directories() {
        let start = unix(Some("/"));
        assert_eq!(
            dir_of(WorkingDirectory::Inherit.resolve(&start, every_dir)),
            some("/")
        );
        assert_eq!(
            dir_of(WorkingDirectory::Home.resolve(&unix(Some(CWD)), every_dir)),
            some(HOME)
        );
    }

    #[test]
    fn paths_expand_home_and_join_relative_ones_to_the_cwd() {
        let start = unix(Some(CWD));
        let path = |text: &str| dir_of(WorkingDirectory::parse(text).resolve(&start, every_dir));
        assert_eq!(path("/srv/data"), some("/srv/data"));
        assert_eq!(path("~/src"), Some(Path::new(HOME).join("src")));
        assert_eq!(path("~"), some(HOME));
        assert_eq!(path("sub"), Some(Path::new(CWD).join("sub")));
    }

    #[test]
    fn a_missing_directory_falls_back_to_home_then_to_the_default() {
        let only_home = |dir: &Path| dir == Path::new(HOME);
        let start = unix(Some(CWD));
        let resolved = WorkingDirectory::parse("/gone").resolve(&start, only_home);
        assert_eq!(resolved.dir, some(HOME));
        let warning = resolved.warning.unwrap();
        assert!(warning.contains("/gone"), "{warning}");

        let nothing = |_: &Path| false;
        for wanted in [
            WorkingDirectory::Auto,
            WorkingDirectory::Inherit,
            WorkingDirectory::Home,
            WorkingDirectory::parse("/gone"),
        ] {
            let resolved = wanted.resolve(&start, nothing);
            assert_eq!(resolved.dir, None, "{wanted:?}");
            assert!(resolved.warning.is_some(), "{wanted:?}");
        }
    }

    #[test]
    fn without_home_a_tilde_path_is_relative_to_the_cwd() {
        let start = StartContext {
            home: None,
            ..unix(Some(CWD))
        };
        let resolved = WorkingDirectory::parse("~/src").resolve(&start, every_dir);
        assert_eq!(resolved.dir, Some(Path::new(CWD).join("~/src")));
        assert_eq!(WorkingDirectory::Home.resolve(&start, every_dir).dir, None);
    }

    #[test]
    fn system_root_prefers_systemroot() {
        let env = |vars: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                vars.iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| OsString::from(value))
            }
        };
        assert_eq!(
            system_root(env(&[("SystemRoot", r"C:\Windows"), ("windir", r"D:\W")])),
            some(r"C:\Windows")
        );
        assert_eq!(
            system_root(env(&[("SystemRoot", ""), ("windir", r"D:\W")])),
            some(r"D:\W")
        );
        assert_eq!(system_root(env(&[])), None);
    }
}
