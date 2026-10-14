//! `nxgterm update`: finds the latest release from its `SHA256SUMS`,
//! downloads the asset for this install kind, verifies its SHA-256 and
//! installs it, or prints the package-manager command for copies another
//! tool owns.

mod apply;
mod download;
mod install;
mod kind;
mod sums;
mod version;

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use download::{Client, Source};
use kind::{Arch, InstallKind, Os, Probe};
use version::Version;

/// Runs `nxgterm update`: with `check` only reports the versions; with
/// `force` reinstalls even an up-to-date copy.
pub fn run(check: bool, force: bool) -> Result<(), String> {
    let exe = env::current_exe()
        .and_then(fs::canonicalize)
        .map_err(|error| format!("cannot locate the nxgterm executable: {error}"))?;
    let os = Os::current().ok_or("there are no nxgterm releases for this operating system")?;
    let kind = kind::detect(os, &exe, &SystemProbe);
    if !check && matches!(kind, InstallKind::Portable { .. }) {
        apply::remove_stale_files(&exe);
    }
    let current =
        Version::parse(env!("CARGO_PKG_VERSION")).ok_or("this build has no valid version")?;

    let client = Client::new(Source::from_env());
    eprintln!("nxgterm: checking for the latest release");
    let release = client.latest()?;
    let newer = release.version > current;

    if check {
        println!("current: {current}");
        println!("latest:  {}", release.version);
        println!("install: {}", kind.describe());
        if newer {
            match kind.instructions() {
                Some(how) => println!("an update is available; {how}"),
                None => println!("an update is available; run `nxgterm update`"),
            }
        } else {
            println!("nxgterm is up to date");
        }
        return Ok(());
    }
    if !newer && !force {
        eprintln!(
            "nxgterm: {current} is up to date (latest release: {})",
            release.version
        );
        return Ok(());
    }
    if let Some(how) = kind.instructions() {
        eprintln!(
            "nxgterm: {} is available; this copy is {}, so {how}",
            release.version,
            kind.describe()
        );
        return Ok(());
    }

    let arch = Arch::current()?;
    let asset = kind::asset_name(&kind, os, arch, &release.version).ok_or_else(|| {
        format!(
            "no release asset updates a {} on this system",
            kind.describe()
        )
    })?;
    eprintln!(
        "nxgterm: updating {current} to {} ({})",
        release.version,
        kind.describe()
    );
    let reinstall = !newer;
    match &kind {
        InstallKind::Portable { .. } => install::portable(&client, &release, &asset, &exe),
        InstallKind::AppBundle { app } => install::bundle(&client, &release, &asset, app),
        InstallKind::Msi => install::msi(&client, &release, &asset),
        InstallKind::Deb => install::deb(&client, &release, &asset, reinstall),
        InstallKind::Rpm => install::rpm(&client, &release, &asset, reinstall),
        other => Err(format!("cannot update a {}", other.describe())),
    }
}

/// Removes files a previous update left behind because they were in use
/// (Windows); called at startup, never fails.
pub fn remove_stale_files() {
    if let Ok(exe) = env::current_exe().and_then(fs::canonicalize) {
        apply::remove_stale_files(&exe);
    }
}

/// [`Probe`] backed by this machine.
struct SystemProbe;

impl Probe for SystemProbe {
    fn env(&self, name: &str) -> Option<OsString> {
        env::var_os(name)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn dir_names(&self, dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn rpm_has_nxgterm(&self) -> bool {
        Command::new("rpm")
            .args(["-q", "--quiet", "nxgterm"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }
}

/// A fresh, empty directory for one test.
#[cfg(test)]
fn test_dir(name: &str) -> std::path::PathBuf {
    let dir = env::temp_dir().join(format!("nxgterm-update-test-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}
