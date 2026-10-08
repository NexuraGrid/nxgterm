//! Which ConPTY the native Windows backend uses, for diagnostics.
//!
//! `portable-pty` loads `conpty.dll` by bare name, so the copy that the
//! release zip and MSI ship beside `nxgterm.exe` wins over the inbox ConPTY
//! in kernel32. That DLL starts `x64\OpenConsole.exe`; without it, it
//! silently uses the system conhost. Only `OpenConsole.exe` passes Kitty
//! graphics and Sixel through. See `packaging/windows/conpty/README.md`.

use std::path::Path;

/// Describes the ConPTY found next to the executable in `exe_dir`.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn describe(exe_dir: Option<&Path>) -> &'static str {
    let Some(dir) = exe_dir else {
        return "inbox (executable directory unknown); images are dropped";
    };
    let dll = dir.join("conpty.dll").is_file();
    let host = dir.join("x64").join("OpenConsole.exe").is_file();
    match (dll, host) {
        (true, true) => "bundled (conpty.dll, x64\\OpenConsole.exe)",
        (true, false) => "bundled without x64\\OpenConsole.exe; images are dropped",
        (false, _) => "inbox (no conpty.dll beside nxgterm.exe); images are dropped",
    }
}

/// On Windows, which ConPTY a native session uses; `None` elsewhere.
pub fn conpty_host() -> Option<&'static str> {
    #[cfg(windows)]
    {
        let exe = std::env::current_exe().ok();
        Some(describe(exe.as_deref().and_then(Path::parent)))
    }
    #[cfg(not(windows))]
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nxg-conpty-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn dll_and_host_mean_bundled() {
        let dir = temp_dir("full");
        fs::write(dir.join("conpty.dll"), b"").unwrap();
        fs::create_dir(dir.join("x64")).unwrap();
        fs::write(dir.join("x64").join("OpenConsole.exe"), b"").unwrap();
        assert!(describe(Some(&dir)).starts_with("bundled (conpty.dll"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn dll_without_host_drops_images() {
        let dir = temp_dir("nohost");
        fs::write(dir.join("conpty.dll"), b"").unwrap();
        let text = describe(Some(&dir));
        assert!(text.starts_with("bundled without"), "{text}");
        assert!(text.contains("images are dropped"), "{text}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn no_dll_means_inbox() {
        let dir = temp_dir("inbox");
        assert!(describe(Some(&dir)).starts_with("inbox (no conpty.dll"));
        assert!(describe(None).starts_with("inbox"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_windows_reports_a_host() {
        assert_eq!(conpty_host().is_some(), cfg!(windows));
    }
}
