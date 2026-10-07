//! Unpacking the embedded winpty binaries to a per-user directory.
//!
//! `winpty.dll` must be a file on disk to be loaded, and it starts
//! `winpty-agent.exe` from its own directory. The directory name carries a
//! content hash, so a newer build never reuses stale binaries.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Per-user data root: `%LOCALAPPDATA%`, then `%APPDATA%`, then `temp`.
pub(crate) fn data_root(
    local_app_data: Option<OsString>,
    app_data: Option<OsString>,
    temp: PathBuf,
) -> PathBuf {
    [local_app_data, app_data]
        .into_iter()
        .flatten()
        .find(|dir| !dir.is_empty())
        .map_or(temp, PathBuf::from)
}

/// `<root>\nxgterm\winpty\<tag>`.
pub(crate) fn unpack_dir(root: &Path, tag: &str) -> PathBuf {
    root.join("nxgterm").join("winpty").join(tag)
}

/// `<version>-<16 hex digits>`: the version plus a content hash of `files`.
pub(crate) fn tag(version: &str, files: &[&[u8]]) -> String {
    // FNV-1a (64-bit): only needs to change when the binaries change.
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for file in files {
        // Hash each length too so file boundaries count.
        let len = (file.len() as u64).to_le_bytes();
        for &byte in len.iter().chain(file.iter()) {
            hash = (hash ^ u64::from(byte)).wrapping_mul(PRIME);
        }
    }
    format!("{version}-{hash:016x}")
}

/// Whether a file must be (re)written: it is missing, or its size or
/// contents differ from `want`. Contents are only read when sizes match.
pub(crate) fn needs_write(
    existing_len: Option<u64>,
    read_existing: impl FnOnce() -> io::Result<Vec<u8>>,
    want: &[u8],
) -> bool {
    match existing_len {
        Some(len) if len == want.len() as u64 => {
            read_existing().map_or(true, |existing| existing != want)
        }
        _ => true,
    }
}

/// Writes `files` into `dir` unless an identical copy is already there.
///
/// Each file goes to a unique temporary name first and is then renamed, so
/// no reader ever sees a partial file. When another instance wins the race
/// (or has the DLL loaded, which blocks replacing it), an identical file in
/// place counts as success.
pub(crate) fn unpack(dir: &Path, files: &[(&str, &[u8])]) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    for &(name, bytes) in files {
        install(&dir.join(name), bytes)?;
    }
    Ok(())
}

fn install(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let is_current = || {
        let len = fs::metadata(path).ok().map(|meta| meta.len());
        !needs_write(len, || fs::read(path), bytes)
    };
    if is_current() {
        return Ok(());
    }
    let temp = temp_path(path);
    if let Err(error) = fs::write(&temp, bytes) {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    match fs::rename(&temp, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&temp);
            if is_current() { Ok(()) } else { Err(error) }
        }
    }
}

/// A name next to `path` that no other thread or process uses.
fn temp_path(path: &Path) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.{n}.tmp", std::process::id()));
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nxg-unpack-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn data_root_prefers_local_app_data() {
        let root = data_root(Some("L".into()), Some("R".into()), PathBuf::from("T"));
        assert_eq!(root, PathBuf::from("L"));
    }

    #[test]
    fn data_root_falls_back_to_app_data_then_temp() {
        assert_eq!(
            data_root(None, Some("R".into()), PathBuf::from("T")),
            PathBuf::from("R")
        );
        assert_eq!(
            data_root(Some("".into()), None, PathBuf::from("T")),
            PathBuf::from("T")
        );
    }

    #[test]
    fn unpack_dir_is_namespaced_by_app_and_tag() {
        let dir = unpack_dir(Path::new("base"), "0.4.3-abc");
        let expected: PathBuf = ["base", "nxgterm", "winpty", "0.4.3-abc"].iter().collect();
        assert_eq!(dir, expected);
    }

    #[test]
    fn tag_is_stable_and_content_sensitive() {
        let a = tag("0.4.3", &[b"dll", b"agent"]);
        assert_eq!(a, tag("0.4.3", &[b"dll", b"agent"]));
        assert!(a.starts_with("0.4.3-"), "{a}");
        assert_eq!(a.len(), "0.4.3-".len() + 16, "{a}");
        assert!(a["0.4.3-".len()..].bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(a, tag("0.4.3", &[b"dll", b"agenT"]));
        // File boundaries matter, not just the concatenation.
        assert_ne!(a, tag("0.4.3", &[b"dllagent"]));
    }

    #[test]
    fn missing_file_needs_write() {
        assert!(needs_write(None, || panic!("must not read"), b"abc"));
    }

    #[test]
    fn size_mismatch_needs_write_without_reading() {
        assert!(needs_write(Some(2), || panic!("must not read"), b"abc"));
    }

    #[test]
    fn same_size_compares_contents() {
        assert!(!needs_write(Some(3), || Ok(b"abc".to_vec()), b"abc"));
        assert!(needs_write(Some(3), || Ok(b"abd".to_vec()), b"abc"));
        assert!(needs_write(
            Some(3),
            || Err(io::Error::other("locked")),
            b"abc"
        ));
    }

    #[test]
    fn unpack_writes_files_and_leaves_no_temporaries() {
        let dir = temp_dir("write");
        unpack(&dir, &[("a.dll", b"one"), ("b.exe", b"two")]).unwrap();
        assert_eq!(fs::read(dir.join("a.dll")).unwrap(), b"one");
        assert_eq!(fs::read(dir.join("b.exe")).unwrap(), b"two");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unpack_repairs_a_corrupted_copy_and_is_idempotent() {
        let dir = temp_dir("repair");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.dll"), b"bad").unwrap();
        unpack(&dir, &[("a.dll", b"good")]).unwrap();
        unpack(&dir, &[("a.dll", b"good")]).unwrap();
        assert_eq!(fs::read(dir.join("a.dll")).unwrap(), b"good");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn concurrent_unpacks_all_succeed() {
        let dir = temp_dir("race");
        let payload = vec![7_u8; 256 * 1024];
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| scope.spawn(|| unpack(&dir, &[("a.dll", &payload)])))
                .collect();
            for handle in handles {
                handle.join().unwrap().unwrap();
            }
        });
        assert_eq!(fs::read(dir.join("a.dll")).unwrap(), payload);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }
}
