//! Installing a verified release asset, per install kind.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::apply::{self, WorkDir};
use super::download::{Client, Release};

/// Portable zip/tarball: unpack next to the install (same filesystem, so
/// files are renamed, not copied) and swap the files in place.
pub fn portable(client: &Client, release: &Release, asset: &str, exe: &Path) -> Result<(), String> {
    let dir = exe.parent().ok_or("the executable has no directory")?;
    let work = WorkDir::create_in_install(dir)?;
    let archive = client.download(release, asset, work.path())?;
    let unpacked = work.path().join("unpacked");
    std::fs::create_dir(&unpacked)
        .map_err(|error| format!("cannot create {}: {error}", unpacked.display()))?;
    unpack(&archive, &unpacked)?;
    let exe_name = if cfg!(windows) {
        "nxgterm.exe"
    } else {
        "nxgterm"
    };
    let root = apply::archive_root(&unpacked, exe_name)?;
    let count = apply::replace_portable(&root, exe)?;
    eprintln!(
        "nxgterm: installed {} in {} ({count} files replaced)",
        release.version,
        dir.display()
    );
    Ok(())
}

#[cfg(windows)]
fn unpack(archive: &Path, dest: &Path) -> Result<(), String> {
    apply::unpack_zip(archive, dest)
}

#[cfg(not(windows))]
fn unpack(archive: &Path, dest: &Path) -> Result<(), String> {
    apply::unpack_tar_gz(archive, dest)
}

/// macOS: unpack the universal tarball beside the bundle and swap the
/// whole `.app`.
pub fn bundle(client: &Client, release: &Release, asset: &str, app: &Path) -> Result<(), String> {
    let parent = app.parent().ok_or("the app bundle has no directory")?;
    let work = WorkDir::create_in_install(parent)?;
    let archive = client.download(release, asset, work.path())?;
    let unpacked = work.path().join("unpacked");
    std::fs::create_dir(&unpacked)
        .map_err(|error| format!("cannot create {}: {error}", unpacked.display()))?;
    apply::unpack_tar_gz(&archive, &unpacked)?;
    let new_app = unpacked.join("nxgterm.app");
    if !new_app.join("Contents/MacOS/nxgterm").is_file() {
        return Err(format!("{asset} has no nxgterm.app"));
    }
    apply::replace_bundle(&new_app, app)?;
    eprintln!(
        "nxgterm: installed {} at {}",
        release.version,
        app.display()
    );
    Ok(())
}

/// Windows installer: download, verify, start `msiexec` and return so
/// this process does not hold the files it replaces. The `.msi` stays in
/// the temp directory for msiexec to read.
pub fn msi(client: &Client, release: &Release, asset: &str) -> Result<(), String> {
    let work = WorkDir::create(&std::env::temp_dir())
        .map_err(|error| format!("cannot create a temporary directory: {error}"))?;
    let msi = client.download(release, asset, work.path())?;
    let mut command = Command::new("msiexec");
    command.arg("/i").arg(&msi).args(["/passive", "/norestart"]);
    eprintln!("nxgterm: running {}", show(&command));
    command
        .spawn()
        .map_err(|error| format!("cannot start msiexec: {error}"))?;
    work.keep();
    eprintln!("nxgterm: the installer is running; close open nxgterm windows if it asks you to");
    Ok(())
}

/// `.deb`: `sudo apt install <file>` (`--reinstall` for the same version).
pub fn deb(client: &Client, release: &Release, asset: &str, reinstall: bool) -> Result<(), String> {
    let work = temp_work_dir()?;
    let package = client.download(release, asset, work.path())?;
    let mut args: Vec<OsString> = vec!["apt".into(), "install".into()];
    if reinstall {
        args.push("--reinstall".into());
    }
    args.push(package.into());
    run_as_root(args)
}

/// `.rpm`: `sudo dnf install <file>` (`reinstall` for the same version),
/// or `sudo rpm -U` where dnf is missing.
pub fn rpm(client: &Client, release: &Release, asset: &str, reinstall: bool) -> Result<(), String> {
    let work = temp_work_dir()?;
    let package = client.download(release, asset, work.path())?;
    let args: Vec<OsString> = if find_in_path("dnf").is_some() {
        let verb = if reinstall { "reinstall" } else { "install" };
        vec!["dnf".into(), verb.into(), package.into()]
    } else {
        let mut args: Vec<OsString> = vec!["rpm".into(), "-U".into()];
        if reinstall {
            args.push("--replacepkgs".into());
        }
        args.push(package.into());
        args
    };
    run_as_root(args)
}

fn temp_work_dir() -> Result<WorkDir, String> {
    WorkDir::create(&std::env::temp_dir())
        .map_err(|error| format!("cannot create a temporary directory: {error}"))
}

/// Runs `args` through `sudo` (when it is installed), with this
/// terminal's stdio so the package manager can ask its questions.
fn run_as_root(args: Vec<OsString>) -> Result<(), String> {
    let mut command = match find_in_path("sudo") {
        Some(sudo) => {
            let mut command = Command::new(sudo);
            command.args(&args);
            command
        }
        None => {
            let mut command = Command::new(&args[0]);
            command.args(&args[1..]);
            command
        }
    };
    eprintln!("nxgterm: running {}", show(&command));
    let status = command
        .status()
        .map_err(|error| format!("cannot run {}: {error}", show(&command)))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{} failed ({status})", show(&command)))
    }
}

/// `command` as the user would type it.
fn show(command: &Command) -> String {
    let program = Path::new(command.get_program())
        .file_name()
        .unwrap_or(command.get_program())
        .to_string_lossy()
        .into_owned();
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned());
    format!(
        "`{}`",
        std::iter::once(program)
            .chain(args)
            .collect::<Vec<_>>()
            .join(" ")
    )
}

/// `name` in a `PATH` directory.
fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}
