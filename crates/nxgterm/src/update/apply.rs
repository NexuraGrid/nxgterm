//! Filesystem side of an update: unpacking archives and swapping files
//! in place, with every rename undone when a later one fails.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

/// The binary's name inside the release archives.
const EXE_NAME: &str = if cfg!(windows) {
    "nxgterm.exe"
} else {
    "nxgterm"
};

/// Files that may still be in use by another nxgterm window when they
/// are replaced (Windows cannot delete them then); removed next time.
const IN_USE: [&str; 2] = ["conpty.dll", "x64/OpenConsole.exe"];

/// A scratch directory removed on drop unless [`WorkDir::keep`] is called.
#[derive(Debug)]
pub struct WorkDir {
    path: PathBuf,
    keep: bool,
}

impl WorkDir {
    /// Creates `.nxgterm-update-<pid>` in `parent`, replacing a stale one.
    pub fn create(parent: &Path) -> io::Result<Self> {
        let path = parent.join(format!(".nxgterm-update-{}", std::process::id()));
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
        fs::create_dir(&path)?;
        Ok(Self { path, keep: false })
    }

    /// Like [`WorkDir::create`], explaining a permission error.
    pub fn create_in_install(parent: &Path) -> Result<Self, String> {
        Self::create(parent).map_err(|error| {
            let mut message = format!("cannot write to {}: {error}", parent.display());
            if error.kind() == io::ErrorKind::PermissionDenied {
                message.push_str(if cfg!(windows) {
                    "; run `nxgterm update` from an Administrator prompt"
                } else {
                    "; rerun it with sudo"
                });
            }
            message
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Leaves the directory on disk (an installer still reads it).
    pub fn keep(mut self) -> PathBuf {
        self.keep = true;
        self.path.clone()
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

/// `path` with `.old` appended to its file name.
fn old_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    path.with_file_name(name)
}

/// Best-effort removal of the `.old` files a previous update could not
/// delete because they were in use (Windows).
pub fn remove_stale_files(exe: &Path) {
    let _ = fs::remove_file(old_path(exe));
    if let Some(dir) = exe.parent() {
        for file in IN_USE {
            let _ = fs::remove_file(old_path(&dir.join(file)));
        }
    }
}

/// Unpacks a `.tar.gz` into `dest` (entries escaping it are skipped).
pub fn unpack_tar_gz(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = File::open(archive)
        .map_err(|error| format!("cannot open {}: {error}", archive.display()))?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    tar.set_overwrite(true);
    tar.unpack(dest)
        .map_err(|error| format!("cannot unpack {}: {error}", archive.display()))
}

/// Unpacks a `.zip` into `dest` (entries escaping it are refused).
#[cfg(windows)]
pub fn unpack_zip(archive: &Path, dest: &Path) -> Result<(), String> {
    let fail =
        |error: &dyn std::fmt::Display| format!("cannot unpack {}: {error}", archive.display());
    let file = File::open(archive).map_err(|error| fail(&error))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|error| fail(&error))?;
    zip.extract(dest).map_err(|error| fail(&error))
}

/// The directory in `dest` that holds `marker`: `dest` itself or its
/// only subdirectory that does (archives wrap their files in one).
pub fn archive_root(dest: &Path, marker: &str) -> Result<PathBuf, String> {
    if dest.join(marker).exists() {
        return Ok(dest.to_path_buf());
    }
    let entries =
        fs::read_dir(dest).map_err(|error| format!("cannot read {}: {error}", dest.display()))?;
    let roots: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.join(marker).exists())
        .collect();
    match roots.as_slice() {
        [root] => Ok(root.clone()),
        _ => Err(format!("the release archive has no {marker}")),
    }
}

/// Replaces the files of the portable install around `exe` with those
/// under `new_root` (the unpacked archive): each file the release ships
/// that also exists in the install. The archive's binary replaces `exe`
/// whatever it is called. Returns how many files were replaced.
pub fn replace_portable(new_root: &Path, exe: &Path) -> Result<usize, String> {
    let install_dir = exe.parent().ok_or("the executable has no directory")?;
    let mut plan = Vec::new();
    let mut has_exe = false;
    for new in files_under(new_root)
        .map_err(|error| format!("cannot read the unpacked release: {error}"))?
    {
        let relative = new.strip_prefix(new_root).unwrap_or(&new);
        let target = if relative == Path::new(EXE_NAME) {
            has_exe = true;
            exe.to_path_buf()
        } else {
            install_dir.join(relative)
        };
        if target.is_file() {
            plan.push((new, target));
        }
    }
    if !has_exe {
        return Err(format!("the release archive has no {EXE_NAME}"));
    }
    swap_files(&plan)?;
    Ok(plan.len())
}

/// The regular files under `dir`, recursively (symlinks are skipped).
fn files_under(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Moves each `(new, target)` into place: `target` is renamed to
/// `target.old` first (Windows allows renaming a running exe or loaded
/// DLL, not overwriting it), then `new` takes its name with the old
/// file's permissions. Any failure undoes the swaps done so far. The
/// `.old` files are then deleted where possible.
fn swap_files(plan: &[(PathBuf, PathBuf)]) -> Result<(), String> {
    let mut done: Vec<(&Path, PathBuf)> = Vec::new();
    for (new, target) in plan {
        if let Err(error) = swap_one(new, target) {
            undo(&done);
            return Err(error);
        }
        done.push((target, old_path(target)));
    }
    for (_, old) in &done {
        let _ = fs::remove_file(old);
    }
    Ok(())
}

fn swap_one(new: &Path, target: &Path) -> Result<(), String> {
    let old = old_path(target);
    match fs::remove_file(&old) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "cannot remove {} left by a previous update: {error}; close every nxgterm window and retry",
                old.display()
            ));
        }
    }
    #[cfg(unix)]
    if let Ok(metadata) = fs::metadata(target) {
        let _ = fs::set_permissions(new, metadata.permissions());
    }
    fs::rename(target, &old)
        .map_err(|error| format!("cannot move {} aside: {error}", target.display()))?;
    if let Err(error) = fs::rename(new, target) {
        let _ = fs::rename(&old, target);
        return Err(format!("cannot install {}: {error}", target.display()));
    }
    Ok(())
}

/// Puts the `.old` files back, newest swap first.
fn undo(done: &[(&Path, PathBuf)]) {
    for (target, old) in done.iter().rev() {
        let _ = fs::remove_file(target);
        if let Err(error) = fs::rename(old, target) {
            eprintln!(
                "nxgterm: cannot restore {} from {}: {error}",
                target.display(),
                old.display()
            );
        }
    }
}

/// Replaces the bundle at `app` with `new_app` (on the same filesystem):
/// the old one is renamed to `<app>.old`, put back if the move fails, and
/// deleted otherwise.
pub fn replace_bundle(new_app: &Path, app: &Path) -> Result<(), String> {
    let old = old_path(app);
    if old.exists() {
        fs::remove_dir_all(&old)
            .map_err(|error| format!("cannot remove {}: {error}", old.display()))?;
    }
    fs::rename(app, &old)
        .map_err(|error| format!("cannot move {} aside: {error}", app.display()))?;
    if let Err(error) = fs::rename(new_app, app) {
        let _ = fs::rename(&old, app);
        return Err(format!("cannot install {}: {error}", app.display()));
    }
    let _ = fs::remove_dir_all(&old);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::test_dir;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    #[test]
    fn replaces_the_files_both_sides_have() {
        let root = test_dir("portable");
        let install = root.join("install");
        let exe = install.join("my-nxgterm");
        write(&exe, "old exe");
        write(&install.join("profile/install.sh"), "old profile");
        write(&install.join("notes.txt"), "mine");
        let new = root.join("new/nxgterm-9.9.9-x86_64-linux");
        write(&new.join(EXE_NAME), "new exe");
        write(&new.join("profile/install.sh"), "new profile");
        write(&new.join("README.md"), "readme");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&exe, fs::Permissions::from_mode(0o750)).unwrap();
        }

        assert_eq!(replace_portable(&new, &exe), Ok(2));
        assert_eq!(read(&exe), "new exe");
        assert_eq!(read(&install.join("profile/install.sh")), "new profile");
        assert_eq!(read(&install.join("notes.txt")), "mine");
        assert!(
            !install.join("README.md").exists(),
            "only existing files are replaced"
        );
        assert!(!old_path(&exe).exists());
        assert!(!old_path(&install.join("profile/install.sh")).exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&exe).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o750);
        }
    }

    #[test]
    fn an_archive_without_the_binary_changes_nothing() {
        let root = test_dir("portable-no-exe");
        let exe = root.join("install").join(EXE_NAME);
        write(&exe, "old exe");
        write(&root.join("new/README.md"), "readme");
        let error = replace_portable(&root.join("new"), &exe).unwrap_err();
        assert!(error.contains(EXE_NAME), "{error}");
        assert_eq!(read(&exe), "old exe");
    }

    #[test]
    fn a_failed_swap_restores_every_file() {
        let root = test_dir("rollback");
        let (a, b) = (root.join("a"), root.join("b"));
        write(&a, "old a");
        write(&b, "old b");
        write(&root.join("new-a"), "new a");
        // `new-b` does not exist, so the second swap fails after `b` was
        // moved aside.
        let plan = vec![
            (root.join("new-a"), a.clone()),
            (root.join("new-b"), b.clone()),
        ];
        let error = swap_files(&plan).unwrap_err();
        assert!(error.contains("cannot install"), "{error}");
        assert_eq!(read(&a), "old a");
        assert_eq!(read(&b), "old b");
        assert!(!old_path(&a).exists() && !old_path(&b).exists());
    }

    #[test]
    fn stale_old_files_are_removed() {
        let root = test_dir("stale");
        let exe = root.join(EXE_NAME);
        write(&exe, "exe");
        write(&old_path(&exe), "previous");
        write(&old_path(&root.join("x64/OpenConsole.exe")), "previous");
        write(&root.join("keep.old"), "not ours");
        remove_stale_files(&exe);
        assert!(!old_path(&exe).exists());
        assert!(!old_path(&root.join("x64/OpenConsole.exe")).exists());
        assert!(root.join("keep.old").exists());
        assert!(exe.exists());
    }

    #[test]
    fn bundles_are_replaced_whole() {
        let root = test_dir("bundle");
        let app = root.join("nxgterm.app");
        write(&app.join("Contents/MacOS/nxgterm"), "old");
        write(&app.join("Contents/Resources/gone"), "old only");
        let new_app = root.join("work/nxgterm.app");
        write(&new_app.join("Contents/MacOS/nxgterm"), "new");
        replace_bundle(&new_app, &app).unwrap();
        assert_eq!(read(&app.join("Contents/MacOS/nxgterm")), "new");
        assert!(!app.join("Contents/Resources/gone").exists());
        assert!(!old_path(&app).exists());
    }

    #[test]
    fn unpacks_a_tarball_and_finds_its_root() {
        let root = test_dir("tarball");
        let archive = root.join("a.tar.gz");
        {
            let file = File::create(&archive).unwrap();
            let gz = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
            let mut tar = tar::Builder::new(gz);
            let mut header = tar::Header::new_gnu();
            let body = b"binary";
            header.set_size(body.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            tar.append_data(
                &mut header,
                format!("nxgterm-9.9.9-x86_64-linux/{EXE_NAME}"),
                &body[..],
            )
            .unwrap();
            tar.into_inner().unwrap().finish().unwrap();
        }
        let dest = root.join("out");
        fs::create_dir(&dest).unwrap();
        unpack_tar_gz(&archive, &dest).unwrap();
        let found = archive_root(&dest, EXE_NAME).unwrap();
        assert_eq!(found, dest.join("nxgterm-9.9.9-x86_64-linux"));
        assert_eq!(read(&found.join(EXE_NAME)), "binary");
        assert!(archive_root(&dest, "missing").is_err());
    }
}
