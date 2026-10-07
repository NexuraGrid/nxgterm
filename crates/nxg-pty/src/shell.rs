//! Default shell resolution.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

/// Windows shells by preference: PowerShell 7, then Windows PowerShell 5.1.
/// `cmd.exe` (through `%ComSpec%`) is the last resort.
const WINDOWS_SHELLS: [&str; 2] = ["pwsh.exe", "powershell.exe"];

/// Picks the Windows shell: the first of [`WINDOWS_SHELLS`] that `find`
/// locates, otherwise `comspec`, otherwise `cmd.exe`.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn windows_shell(
    find: impl Fn(&str) -> Option<PathBuf>,
    comspec: Option<OsString>,
) -> OsString {
    WINDOWS_SHELLS
        .iter()
        .find_map(|name| find(name))
        .map(PathBuf::into_os_string)
        .or(comspec)
        .unwrap_or_else(|| "cmd.exe".into())
}

/// Resolves the Windows shell from `PATH`, also checking PowerShell 7's
/// default install directory in case it is not on `PATH`.
#[cfg(windows)]
pub(crate) fn default_windows_shell() -> OsString {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let find = |name: &str| {
        find_in_path(name, &path).or_else(|| {
            let program_files = std::env::var_os("ProgramFiles")?;
            let pwsh = std::path::Path::new(&program_files)
                .join("PowerShell")
                .join("7")
                .join(name);
            (name == "pwsh.exe" && pwsh.is_file()).then_some(pwsh)
        })
    };
    windows_shell(find, std::env::var_os("ComSpec"))
}

/// Finds `name` in a `PATH`-style list of directories.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn find_in_path(name: &str, path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn found(names: &'static [&'static str]) -> impl Fn(&str) -> Option<PathBuf> {
        move |name| {
            names
                .contains(&name)
                .then(|| PathBuf::from(format!("C:\\bin\\{name}")))
        }
    }

    #[test]
    fn prefers_powershell_7() {
        let shell = windows_shell(found(&["pwsh.exe", "powershell.exe"]), None);
        assert_eq!(shell, "C:\\bin\\pwsh.exe");
    }

    #[test]
    fn falls_back_to_windows_powershell() {
        let shell = windows_shell(found(&["powershell.exe"]), None);
        assert_eq!(shell, "C:\\bin\\powershell.exe");
    }

    #[test]
    fn falls_back_to_comspec() {
        let shell = windows_shell(found(&[]), Some("C:\\Windows\\system32\\cmd.exe".into()));
        assert_eq!(shell, "C:\\Windows\\system32\\cmd.exe");
    }

    #[test]
    fn falls_back_to_cmd_without_comspec() {
        assert_eq!(windows_shell(found(&[]), None), "cmd.exe");
    }

    #[test]
    fn finds_executable_in_path_directories() {
        let dir = std::env::temp_dir().join(format!("nxg-shell-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pwsh.exe"), b"").unwrap();
        let path = std::env::join_paths([Path::new("/definitely/missing"), &dir]).unwrap();

        assert_eq!(find_in_path("pwsh.exe", &path), Some(dir.join("pwsh.exe")));
        assert_eq!(find_in_path("powershell.exe", &path), None);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
