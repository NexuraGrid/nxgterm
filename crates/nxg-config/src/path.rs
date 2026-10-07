//! Where the config file lives.

use std::ffi::OsString;
use std::path::PathBuf;

/// Environment variable that points at a specific config file.
pub const ENV_VAR: &str = "NXGTERM_CONFIG";

/// Directory and file names under the platform config directory.
const DIR: &str = "nxgterm";
const FILE: &str = "nxgterm.toml";

/// Platform families with different config locations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// Linux, the BSDs and macOS: XDG layout (`~/.config`).
    Unix,
    /// `%APPDATA%`.
    Windows,
}

impl Platform {
    /// The platform this binary was built for.
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unix
        }
    }
}

/// The config file path, or `None` when no location can be derived.
///
/// `NXGTERM_CONFIG` wins when set and non-empty. Otherwise, on Unix (macOS
/// included, where terminal users expect `~/.config`):
/// `$XDG_CONFIG_HOME/nxgterm/nxgterm.toml` when `XDG_CONFIG_HOME` is an
/// absolute path (as the XDG spec requires; checked with `has_root` so tests\n/// behave the same on every host), else
/// `$HOME/.config/nxgterm/nxgterm.toml`. On Windows:
/// `%APPDATA%\nxgterm\nxgterm.toml`.
///
/// `env` looks up environment variables, so this stays pure and testable.
pub fn config_path(platform: Platform, env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let var = |name: &str| env(name).filter(|value| !value.is_empty());
    if let Some(path) = var(ENV_VAR) {
        return Some(PathBuf::from(path));
    }
    let base = match platform {
        Platform::Windows => PathBuf::from(var("APPDATA")?),
        Platform::Unix => var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|dir| dir.has_root())
            .or_else(|| var("HOME").map(|home| PathBuf::from(home).join(".config")))?,
    };
    Some(base.join(DIR).join(FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(vars: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
        move |name| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        }
    }

    fn under(base: &str) -> PathBuf {
        PathBuf::from(base).join("nxgterm").join("nxgterm.toml")
    }

    #[test]
    fn env_override_wins_everywhere() {
        let vars = env(&[
            ("NXGTERM_CONFIG", "/tmp/custom.toml"),
            ("XDG_CONFIG_HOME", "/xdg"),
            ("HOME", "/home/me"),
            ("APPDATA", "C:\\Users\\me\\AppData\\Roaming"),
        ]);
        let expected = Some(PathBuf::from("/tmp/custom.toml"));
        assert_eq!(config_path(Platform::Unix, &vars), expected);
        assert_eq!(config_path(Platform::Windows, &vars), expected);
    }

    #[test]
    fn empty_env_override_is_ignored() {
        let vars = env(&[("NXGTERM_CONFIG", ""), ("HOME", "/home/me")]);
        assert_eq!(
            config_path(Platform::Unix, vars),
            Some(under("/home/me/.config"))
        );
    }

    #[test]
    fn unix_prefers_xdg_config_home() {
        let vars = env(&[("XDG_CONFIG_HOME", "/xdg"), ("HOME", "/home/me")]);
        assert_eq!(config_path(Platform::Unix, vars), Some(under("/xdg")));
    }

    #[test]
    fn unix_ignores_empty_or_relative_xdg_config_home() {
        for xdg in ["", "relative/dir"] {
            let vars = move |name: &str| match name {
                "XDG_CONFIG_HOME" => Some(OsString::from(xdg)),
                "HOME" => Some(OsString::from("/home/me")),
                _ => None,
            };
            assert_eq!(
                config_path(Platform::Unix, vars),
                Some(under("/home/me/.config")),
                "XDG_CONFIG_HOME={xdg:?}"
            );
        }
    }

    #[test]
    fn unix_without_home_has_no_location() {
        assert_eq!(config_path(Platform::Unix, env(&[("HOME", "")])), None);
        assert_eq!(config_path(Platform::Unix, env(&[])), None);
    }

    #[test]
    fn windows_uses_appdata_only() {
        let appdata = "C:\\Users\\me\\AppData\\Roaming";
        let vars = env(&[
            ("APPDATA", "C:\\Users\\me\\AppData\\Roaming"),
            ("XDG_CONFIG_HOME", "/xdg"),
        ]);
        assert_eq!(config_path(Platform::Windows, vars), Some(under(appdata)));
        assert_eq!(config_path(Platform::Windows, env(&[("HOME", "/h")])), None);
    }
}
