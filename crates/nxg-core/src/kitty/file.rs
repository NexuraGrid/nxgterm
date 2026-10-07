//! File and temp-file transmission (`t=f`, `t=t`).
//!
//! Only regular files are read, never devices, pipes or `/proc`-like
//! pseudo files, and at most [`MAX_DATA`] bytes. A temp file (`t=t`) must
//! live in a temporary directory and have `tty-graphics-protocol` in its
//! path, as the spec requires; it is deleted after reading.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::Command;
use crate::image::decode::{self, MAX_DATA};

/// Marker every temp file path must contain.
const TEMP_MARKER: &str = "tty-graphics-protocol";

/// Reads the file named by the command's base64 payload.
pub fn read(cmd: &Command) -> Result<Vec<u8>, String> {
    let raw = decode::base64(&cmd.payload).map_err(|e| format!("bad path: {e}"))?;
    let path = path_from_bytes(raw)?;
    if !path.is_absolute() {
        return Err("path must be absolute".into());
    }
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("cannot open file: {e}"))?;
    let temp = cmd.medium == b't';
    if temp && !is_temp_file(&canonical) {
        return Err("temporary file outside a temporary directory".into());
    }
    if is_pseudo_fs(&canonical) {
        return Err("not a regular file".into());
    }
    let result = read_regular(&canonical, cmd.data_offset, cmd.data_size);
    if temp {
        // The client handed the file over; remove it even if unreadable.
        let _ = std::fs::remove_file(&canonical);
    }
    result
}

#[cfg(unix)]
fn path_from_bytes(raw: Vec<u8>) -> Result<PathBuf, String> {
    use std::os::unix::ffi::OsStringExt;
    if raw.contains(&0) {
        return Err("bad path".into());
    }
    Ok(PathBuf::from(std::ffi::OsString::from_vec(raw)))
}

#[cfg(not(unix))]
fn path_from_bytes(raw: Vec<u8>) -> Result<PathBuf, String> {
    let text = String::from_utf8(raw).map_err(|_| "bad path".to_owned())?;
    if text.contains('\0') {
        return Err("bad path".into());
    }
    Ok(PathBuf::from(text))
}

/// Whether `path` (canonical) is inside a temporary directory and carries
/// the temp-file marker.
fn is_temp_file(path: &Path) -> bool {
    if !path.to_string_lossy().contains(TEMP_MARKER) {
        return false;
    }
    let mut dirs = vec![std::env::temp_dir()];
    if cfg!(unix) {
        dirs.extend(["/tmp", "/dev/shm"].map(PathBuf::from));
    }
    dirs.iter()
        .filter_map(|dir| dir.canonicalize().ok())
        .any(|dir| path.starts_with(dir))
}

/// Kernel pseudo filesystems whose "files" must never be read.
fn is_pseudo_fs(path: &Path) -> bool {
    if !cfg!(unix) || path.starts_with("/dev/shm") {
        return false;
    }
    ["/proc", "/sys", "/dev"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

/// Reads `size` bytes (0: to the end) from `offset`, refusing anything
/// but regular files and more than [`MAX_DATA`] bytes.
fn read_regular(path: &Path, offset: u64, size: u64) -> Result<Vec<u8>, String> {
    let not_regular = || "not a regular file".to_owned();
    // Check before opening: opening a FIFO would block.
    if !std::fs::metadata(path)
        .map_err(|e| e.to_string())?
        .is_file()
    {
        return Err(not_regular());
    }
    let mut file = File::open(path).map_err(|e| format!("cannot open file: {e}"))?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err(not_regular());
    }
    let available = meta.len().saturating_sub(offset);
    let wanted = if size == 0 {
        available
    } else {
        size.min(available)
    };
    if wanted > MAX_DATA as u64 {
        return Err("file too large".into());
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| e.to_string())?;
    let mut data = Vec::with_capacity(wanted as usize);
    file.take(wanted)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_files_need_marker_and_temp_dir() {
        let tmp = std::env::temp_dir().canonicalize().unwrap();
        assert!(is_temp_file(&tmp.join("tty-graphics-protocol-1")));
        assert!(!is_temp_file(&tmp.join("other")));
        let home = Path::new("/definitely/not/tmp/tty-graphics-protocol");
        assert!(!is_temp_file(home));
    }

    #[cfg(unix)]
    #[test]
    fn pseudo_filesystems_are_refused() {
        assert!(is_pseudo_fs(Path::new("/proc/self/environ")));
        assert!(is_pseudo_fs(Path::new("/dev/zero")));
        assert!(!is_pseudo_fs(Path::new("/dev/shm/x")));
        assert!(!is_pseudo_fs(Path::new("/home/u/a.png")));
    }

    #[cfg(unix)]
    #[test]
    fn devices_and_relative_paths_are_refused() {
        let cmd = |path: &str| Command {
            medium: b'f',
            payload: crate::image::decode::tests::encode_base64(path.as_bytes()).into_bytes(),
            ..Command::default()
        };
        assert!(read(&cmd("/dev/zero")).is_err());
        assert!(read(&cmd("/proc/self/status")).is_err());
        assert!(read(&cmd("relative.png")).is_err());
        assert!(read(&cmd("/no/such/file")).is_err());
    }
}
