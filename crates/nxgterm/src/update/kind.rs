//! How this copy of nxgterm was installed, and which release asset
//! updates it. Pure: the filesystem and environment come in through
//! [`Probe`], so every platform's rules are tested on any host.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::version::Version;

/// The operating systems nxgterm ships for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Mac,
    Windows,
}

impl Os {
    /// The OS this build runs on; `None` for one without releases.
    pub fn current() -> Option<Self> {
        match std::env::consts::OS {
            "linux" => Some(Self::Linux),
            "macos" => Some(Self::Mac),
            "windows" => Some(Self::Windows),
            _ => None,
        }
    }
}

/// The CPU architectures nxgterm ships for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    Aarch64,
}

impl Arch {
    /// From a `std::env::consts::ARCH` name.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "x86_64" => Some(Self::X86_64),
            "aarch64" => Some(Self::Aarch64),
            _ => None,
        }
    }

    /// The architecture this build runs on.
    pub fn current() -> Result<Self, String> {
        let name = std::env::consts::ARCH;
        Self::from_name(name)
            .ok_or_else(|| format!("there are no nxgterm releases for the {name} architecture"))
    }
}

/// How this copy was installed, which decides how it is updated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallKind {
    /// An extracted zip (Windows) or tarball (Linux) in `dir`: its files
    /// are replaced in place.
    Portable { dir: PathBuf },
    /// The Windows installer, in Program Files.
    Msi,
    /// A `.deb` package owning `/usr/bin/nxgterm`.
    Deb,
    /// An `.rpm` package owning `/usr/bin/nxgterm`.
    Rpm,
    /// A macOS `nxgterm.app` bundle at `app`, replaced as a whole.
    AppBundle { app: PathBuf },
    /// The `nxgterm` or `nxgterm-bin` AUR package.
    Aur,
    /// A Homebrew cask or formula.
    Homebrew,
    /// `cargo install`.
    Cargo,
    /// Something `nxgterm update` does not touch, and why.
    Unsupported { reason: &'static str },
}

impl InstallKind {
    /// A short name for messages.
    pub fn describe(&self) -> &'static str {
        match self {
            Self::Portable { .. } => "portable archive",
            Self::Msi => "Windows installer (MSI)",
            Self::Deb => "Debian package",
            Self::Rpm => "RPM package",
            Self::AppBundle { .. } => "macOS app bundle",
            Self::Aur => "AUR package",
            Self::Homebrew => "Homebrew",
            Self::Cargo => "cargo install",
            Self::Unsupported { reason } => reason,
        }
    }

    /// For kinds another tool owns, how the user updates them.
    pub fn instructions(&self) -> Option<&'static str> {
        match self {
            Self::Aur => Some("update it with your AUR helper, e.g. `yay -Syu nxgterm-bin`"),
            Self::Homebrew => Some("update it with `brew upgrade nxgterm`"),
            Self::Cargo => Some(
                "update it with `cargo install --locked --git https://github.com/NexuraGrid/nxgterm nxgterm`",
            ),
            Self::Unsupported { .. } => Some(
                "download the release for your system from https://github.com/NexuraGrid/nxgterm/releases/latest",
            ),
            _ => None,
        }
    }
}

/// What detection may look at besides the executable path.
pub trait Probe {
    /// An environment variable.
    fn env(&self, name: &str) -> Option<OsString>;
    /// Whether `path` exists.
    fn exists(&self, path: &Path) -> bool;
    /// The entry names in `dir`; empty when it cannot be read.
    fn dir_names(&self, dir: &Path) -> Vec<String>;
    /// Whether the RPM database has an `nxgterm` package.
    fn rpm_has_nxgterm(&self) -> bool;
}

/// Where a Linux package puts the binary.
const SYSTEM_EXE: &str = "/usr/bin/nxgterm";

/// Homebrew prefixes that Homebrew owns as a whole.
const BREW_PREFIXES: [&str; 2] = ["/opt/homebrew", "/home/linuxbrew/.linuxbrew"];

/// How the copy at `exe` (canonical) was installed on `os`.
pub fn detect(os: Os, exe: &Path, probe: &dyn Probe) -> InstallKind {
    let parts = components(os, &exe.to_string_lossy());
    if is_dev_build(&parts) {
        return InstallKind::Unsupported {
            reason: "a development build",
        };
    }
    if in_cargo_bin(os, &parts, probe) {
        return InstallKind::Cargo;
    }
    match os {
        Os::Windows => detect_windows(exe, &parts, probe),
        Os::Mac => detect_macos(exe, &parts, probe),
        Os::Linux => detect_linux(exe, &parts, probe),
    }
}

fn detect_windows(exe: &Path, parts: &[String], probe: &dyn Probe) -> InstallKind {
    let program_files = ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"];
    let in_program_files = program_files.iter().any(|name| {
        probe.env(name).is_some_and(|dir| {
            let dir = components(Os::Windows, &dir.to_string_lossy());
            !dir.is_empty() && parts.len() > dir.len() && parts.starts_with(&dir)
        })
    });
    if in_program_files {
        InstallKind::Msi
    } else {
        InstallKind::Portable {
            dir: parent_dir(Os::Windows, exe),
        }
    }
}

fn detect_macos(exe: &Path, parts: &[String], probe: &dyn Probe) -> InstallKind {
    if in_brew(parts, probe) {
        return InstallKind::Homebrew;
    }
    let bundle = exe
        .parent()
        .filter(|dir| dir.ends_with("Contents/MacOS"))
        .and_then(Path::parent)
        .and_then(Path::parent)
        .filter(|app| app.extension().is_some_and(|ext| ext == "app"));
    let Some(app) = bundle else {
        return InstallKind::Unsupported {
            reason: "not inside an app bundle",
        };
    };
    let cask = brew_roots(probe)
        .iter()
        .any(|root| probe.exists(&root.join("Caskroom/nxgterm")));
    if cask {
        InstallKind::Homebrew
    } else {
        InstallKind::AppBundle {
            app: app.to_path_buf(),
        }
    }
}

fn detect_linux(exe: &Path, parts: &[String], probe: &dyn Probe) -> InstallKind {
    if in_brew(parts, probe) {
        return InstallKind::Homebrew;
    }
    if exe != Path::new(SYSTEM_EXE) {
        return InstallKind::Portable {
            dir: parent_dir(Os::Linux, exe),
        };
    }
    let aur = probe
        .dir_names(Path::new("/var/lib/pacman/local"))
        .iter()
        .any(|entry| is_aur_package(entry));
    if aur {
        InstallKind::Aur
    } else if probe.exists(Path::new("/var/lib/dpkg/info/nxgterm.list")) {
        InstallKind::Deb
    } else if probe.rpm_has_nxgterm() {
        InstallKind::Rpm
    } else {
        InstallKind::Unsupported {
            reason: "not installed by a known package manager",
        }
    }
}

/// A pacman database entry (`<name>-<version>-<release>`) of one of the
/// nxgterm AUR packages.
fn is_aur_package(entry: &str) -> bool {
    ["nxgterm-bin-", "nxgterm-git-", "nxgterm-"]
        .iter()
        .any(|prefix| {
            entry
                .strip_prefix(prefix)
                .is_some_and(|version| version.starts_with(|c: char| c.is_ascii_digit()))
        })
}

/// The directories a Homebrew install of nxgterm may live under.
fn in_brew(parts: &[String], probe: &dyn Probe) -> bool {
    let mut owned: Vec<PathBuf> = BREW_PREFIXES.iter().map(PathBuf::from).collect();
    if let Some(home) = probe.env("HOME") {
        owned.push(PathBuf::from(home).join(".linuxbrew"));
    }
    for root in brew_roots(probe) {
        owned.push(root.join("Cellar"));
        owned.push(root.join("Caskroom"));
    }
    owned.iter().any(|dir| {
        let dir = components(Os::Linux, &dir.to_string_lossy());
        parts.len() > dir.len() && parts.starts_with(&dir)
    })
}

/// Homebrew prefixes, whose `Cellar` and `Caskroom` Homebrew owns.
fn brew_roots(probe: &dyn Probe) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = ["/opt/homebrew", "/usr/local"]
        .iter()
        .map(PathBuf::from)
        .collect();
    roots.extend(probe.env("HOMEBREW_PREFIX").map(PathBuf::from));
    roots
}

/// Whether `exe` is in `$CARGO_HOME/bin` or `~/.cargo/bin`.
fn in_cargo_bin(os: Os, parts: &[String], probe: &dyn Probe) -> bool {
    let home = if os == Os::Windows {
        "USERPROFILE"
    } else {
        "HOME"
    };
    // Joined as text, not with `Path::join`, which uses the host's separator:
    // [`components`] splits Unix paths on `/` only, and Windows paths on both.
    let bins = [
        probe
            .env("CARGO_HOME")
            .map(|dir| format!("{}/bin", dir.to_string_lossy())),
        probe
            .env(home)
            .map(|dir| format!("{}/.cargo/bin", dir.to_string_lossy())),
    ];
    let dir = &parts[..parts.len().saturating_sub(1)];
    bins.iter().flatten().any(|bin| components(os, bin) == dir)
}

/// A binary in a cargo `target/<profile>` directory.
fn is_dev_build(parts: &[String]) -> bool {
    let n = parts.len();
    let at = |back: usize| n.checked_sub(back).map(|i| parts[i].as_str());
    matches!(at(2), Some("debug" | "release"))
        && (at(3) == Some("target") || at(4) == Some("target"))
}

/// The path's components, for comparing paths of any OS on any host:
/// Windows paths lose a `\\?\` prefix, split on both separators and
/// compare case-insensitively (lowercased).
fn components(os: Os, path: &str) -> Vec<String> {
    match os {
        Os::Windows => {
            let path = path.strip_prefix(r"\\?\").unwrap_or(path);
            path.split(['\\', '/'])
                .filter(|part| !part.is_empty())
                .map(str::to_lowercase)
                .collect()
        }
        Os::Linux | Os::Mac => path
            .split('/')
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
            .collect(),
    }
}

/// The directory holding `exe`; a Windows path is split by hand so it
/// also works on a Unix host.
fn parent_dir(os: Os, exe: &Path) -> PathBuf {
    match os {
        Os::Windows => {
            let text = exe.to_string_lossy();
            let end = text.rfind(['\\', '/']).unwrap_or(0);
            PathBuf::from(&text[..end])
        }
        Os::Linux | Os::Mac => exe.parent().map(Path::to_path_buf).unwrap_or_default(),
    }
}

/// The release asset that updates `kind`; `None` when no asset does.
pub fn asset_name(kind: &InstallKind, os: Os, arch: Arch, version: &Version) -> Option<String> {
    let v = version;
    let x86 = arch == Arch::X86_64;
    let name = match (kind, os) {
        (InstallKind::Portable { .. }, Os::Linux) => {
            let arch = if x86 { "x86_64" } else { "aarch64" };
            format!("nxgterm-{v}-{arch}-linux.tar.gz")
        }
        (InstallKind::Portable { .. }, Os::Windows) if x86 => {
            format!("nxgterm-{v}-x86_64-windows.zip")
        }
        (InstallKind::Msi, Os::Windows) if x86 => format!("nxgterm-{v}-x86_64.msi"),
        (InstallKind::Deb, Os::Linux) => {
            let arch = if x86 { "amd64" } else { "arm64" };
            format!("nxgterm_{v}-1_{arch}.deb")
        }
        (InstallKind::Rpm, Os::Linux) => {
            let arch = if x86 { "x86_64" } else { "aarch64" };
            format!("nxgterm-{v}-1.{arch}.rpm")
        }
        (InstallKind::AppBundle { .. }, Os::Mac) => format!("nxgterm-{v}-universal-macos.tar.gz"),
        _ => return None,
    };
    Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Fake {
        env: HashMap<&'static str, &'static str>,
        files: Vec<&'static str>,
        dirs: HashMap<&'static str, Vec<&'static str>>,
        rpm: bool,
    }

    impl Probe for Fake {
        fn env(&self, name: &str) -> Option<OsString> {
            self.env.get(name).map(OsString::from)
        }
        fn exists(&self, path: &Path) -> bool {
            self.files.iter().any(|file| Path::new(file) == path)
        }
        fn dir_names(&self, dir: &Path) -> Vec<String> {
            let names = self.dirs.iter().find(|(d, _)| Path::new(d) == dir);
            names
                .map(|(_, names)| names.iter().map(|n| n.to_string()).collect())
                .unwrap_or_default()
        }
        fn rpm_has_nxgterm(&self) -> bool {
            self.rpm
        }
    }

    fn detect_with(os: Os, exe: &str, probe: &Fake) -> InstallKind {
        detect(os, Path::new(exe), probe)
    }

    fn portable(dir: &str) -> InstallKind {
        InstallKind::Portable { dir: dir.into() }
    }

    #[test]
    fn windows_program_files_is_the_msi() {
        let probe = Fake {
            env: HashMap::from([
                ("ProgramFiles", r"C:\Program Files"),
                ("ProgramW6432", r"C:\Program Files"),
            ]),
            ..Fake::default()
        };
        let msi = [
            r"C:\Program Files\nxgterm\nxgterm.exe",
            r"\\?\C:\Program Files\nxgterm\nxgterm.exe",
            r"c:\program files\NXGTERM\nxgterm.exe",
        ];
        for exe in msi {
            assert_eq!(
                detect_with(Os::Windows, exe, &probe),
                InstallKind::Msi,
                "{exe}"
            );
        }
        assert_eq!(
            detect_with(
                Os::Windows,
                r"C:\Program Files Extra\nxgterm\nxgterm.exe",
                &probe
            ),
            portable(r"C:\Program Files Extra\nxgterm")
        );
        assert_eq!(
            detect_with(Os::Windows, r"\\?\D:\Tools\nxgterm\nxgterm.exe", &probe),
            portable(r"\\?\D:\Tools\nxgterm")
        );
    }

    #[test]
    fn linux_packages_own_usr_bin() {
        let aur = Fake {
            dirs: HashMap::from([(
                "/var/lib/pacman/local",
                vec!["glibc-2.40-1", "nxgterm-bin-0.10.1-1"],
            )]),
            ..Fake::default()
        };
        assert_eq!(detect_with(Os::Linux, SYSTEM_EXE, &aur), InstallKind::Aur);
        let aur_source = Fake {
            dirs: HashMap::from([("/var/lib/pacman/local", vec!["nxgterm-0.10.1-1"])]),
            ..Fake::default()
        };
        assert_eq!(
            detect_with(Os::Linux, SYSTEM_EXE, &aur_source),
            InstallKind::Aur
        );
        let other_pacman = Fake {
            dirs: HashMap::from([("/var/lib/pacman/local", vec!["nxgterm-themes-1.0-1"])]),
            ..Fake::default()
        };
        assert_ne!(
            detect_with(Os::Linux, SYSTEM_EXE, &other_pacman),
            InstallKind::Aur
        );

        let deb = Fake {
            files: vec!["/var/lib/dpkg/info/nxgterm.list"],
            ..Fake::default()
        };
        assert_eq!(detect_with(Os::Linux, SYSTEM_EXE, &deb), InstallKind::Deb);
        let rpm = Fake {
            rpm: true,
            ..Fake::default()
        };
        assert_eq!(detect_with(Os::Linux, SYSTEM_EXE, &rpm), InstallKind::Rpm);
    }

    #[test]
    fn a_package_does_not_own_a_portable_copy() {
        let deb = Fake {
            files: vec!["/var/lib/dpkg/info/nxgterm.list"],
            rpm: true,
            ..Fake::default()
        };
        assert_eq!(
            detect_with(Os::Linux, "/home/u/opt/nxgterm/nxgterm", &deb),
            portable("/home/u/opt/nxgterm")
        );
    }

    #[test]
    fn unowned_usr_bin_is_unsupported() {
        let kind = detect_with(Os::Linux, SYSTEM_EXE, &Fake::default());
        assert!(matches!(kind, InstallKind::Unsupported { .. }), "{kind:?}");
    }

    #[test]
    fn homebrew_and_cargo() {
        let probe = Fake {
            env: HashMap::from([("HOME", "/home/u")]),
            ..Fake::default()
        };
        assert_eq!(
            detect_with(
                Os::Linux,
                "/home/linuxbrew/.linuxbrew/Cellar/nxgterm/0.10.1/bin/nxgterm",
                &probe
            ),
            InstallKind::Homebrew
        );
        assert_eq!(
            detect_with(Os::Linux, "/home/u/.cargo/bin/nxgterm", &probe),
            InstallKind::Cargo
        );
        let cargo_home = Fake {
            env: HashMap::from([("CARGO_HOME", "/opt/cargo")]),
            ..Fake::default()
        };
        assert_eq!(
            detect_with(Os::Linux, "/opt/cargo/bin/nxgterm", &cargo_home),
            InstallKind::Cargo
        );
    }

    #[test]
    fn development_builds_are_left_alone() {
        for exe in [
            "/src/nxgterm/target/debug/nxgterm",
            "/src/nxgterm/target/x86_64-unknown-linux-gnu/release/nxgterm",
        ] {
            let kind = detect_with(Os::Linux, exe, &Fake::default());
            assert!(
                matches!(kind, InstallKind::Unsupported { .. }),
                "{exe}: {kind:?}"
            );
        }
        let kind = detect_with(
            Os::Windows,
            r"C:\src\nxgterm\target\release\nxgterm.exe",
            &Fake::default(),
        );
        assert!(matches!(kind, InstallKind::Unsupported { .. }), "{kind:?}");
    }

    #[test]
    fn macos_bundles_casks_and_others() {
        let plain = Fake::default();
        assert_eq!(
            detect_with(
                Os::Mac,
                "/Applications/nxgterm.app/Contents/MacOS/nxgterm",
                &plain
            ),
            InstallKind::AppBundle {
                app: "/Applications/nxgterm.app".into()
            }
        );
        let cask = Fake {
            files: vec!["/opt/homebrew/Caskroom/nxgterm"],
            ..Fake::default()
        };
        assert_eq!(
            detect_with(
                Os::Mac,
                "/Applications/nxgterm.app/Contents/MacOS/nxgterm",
                &cask
            ),
            InstallKind::Homebrew
        );
        let intel_cask = Fake {
            files: vec!["/usr/local/Caskroom/nxgterm"],
            ..Fake::default()
        };
        assert_eq!(
            detect_with(
                Os::Mac,
                "/Applications/nxgterm.app/Contents/MacOS/nxgterm",
                &intel_cask
            ),
            InstallKind::Homebrew
        );
        assert_eq!(
            detect_with(Os::Mac, "/opt/homebrew/bin/nxgterm", &plain),
            InstallKind::Homebrew
        );
        let kind = detect_with(Os::Mac, "/Users/u/bin/nxgterm", &plain);
        assert!(matches!(kind, InstallKind::Unsupported { .. }), "{kind:?}");
    }

    #[test]
    fn asset_names_follow_the_release_layout() {
        let v = Version::parse("0.11.0").unwrap();
        let name = |kind: &InstallKind, os, arch| asset_name(kind, os, arch, &v);
        let linux = portable("/opt/nxgterm");
        assert_eq!(
            name(&linux, Os::Linux, Arch::X86_64).as_deref(),
            Some("nxgterm-0.11.0-x86_64-linux.tar.gz")
        );
        assert_eq!(
            name(&linux, Os::Linux, Arch::Aarch64).as_deref(),
            Some("nxgterm-0.11.0-aarch64-linux.tar.gz")
        );
        assert_eq!(
            name(&linux, Os::Windows, Arch::X86_64).as_deref(),
            Some("nxgterm-0.11.0-x86_64-windows.zip")
        );
        assert_eq!(name(&linux, Os::Windows, Arch::Aarch64), None);
        assert_eq!(
            name(&InstallKind::Msi, Os::Windows, Arch::X86_64).as_deref(),
            Some("nxgterm-0.11.0-x86_64.msi")
        );
        assert_eq!(
            name(&InstallKind::Deb, Os::Linux, Arch::X86_64).as_deref(),
            Some("nxgterm_0.11.0-1_amd64.deb")
        );
        assert_eq!(
            name(&InstallKind::Deb, Os::Linux, Arch::Aarch64).as_deref(),
            Some("nxgterm_0.11.0-1_arm64.deb")
        );
        assert_eq!(
            name(&InstallKind::Rpm, Os::Linux, Arch::Aarch64).as_deref(),
            Some("nxgterm-0.11.0-1.aarch64.rpm")
        );
        let app = InstallKind::AppBundle {
            app: "/Applications/nxgterm.app".into(),
        };
        for arch in [Arch::X86_64, Arch::Aarch64] {
            assert_eq!(
                name(&app, Os::Mac, arch).as_deref(),
                Some("nxgterm-0.11.0-universal-macos.tar.gz")
            );
        }
        for kind in [InstallKind::Aur, InstallKind::Homebrew, InstallKind::Cargo] {
            assert_eq!(name(&kind, Os::Linux, Arch::X86_64), None);
        }
    }

    #[test]
    fn unknown_architectures_are_named() {
        assert_eq!(Arch::from_name("x86_64"), Some(Arch::X86_64));
        assert_eq!(Arch::from_name("aarch64"), Some(Arch::Aarch64));
        assert_eq!(Arch::from_name("riscv64"), None);
    }
}
