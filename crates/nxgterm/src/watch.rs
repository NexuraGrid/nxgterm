//! Watches the config files (the main file, its imports and its theme
//! file) and reports debounced changes.
//!
//! The parent directories are watched rather than the files: editors often
//! save by writing a temporary file and renaming it over the original,
//! which would orphan a watch on the file itself. Events are matched by
//! file name, so a file of the same name in a watched directory triggers a
//! harmless reload too. A directory that does not exist yet (e.g. no
//! `themes` directory) is not watched; it is tried again after each
//! reload.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread;
use std::time::Duration;

use notify::event::{AccessKind, AccessMode, EventKind, MetadataKind, ModifyKind};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

/// Quiet time after the last event before reporting a change; editors
/// produce bursts of events per save.
pub const DEBOUNCE: Duration = Duration::from_millis(200);

/// Whether `event` may have changed a file called one of `names`. Reads
/// (our own reload opens the files) and access-time updates are ignored.
pub fn is_relevant(event: &Event, names: &[OsString]) -> bool {
    let kind_matters = match event.kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        EventKind::Access(_) => false,
        EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime)) => false,
        _ => true,
    };
    kind_matters
        && event
            .paths
            .iter()
            .filter_map(|path| path.file_name())
            .any(|name| names.iter().any(|watched| watched == name))
}

/// The directory holding `path`; `.` for a bare file name.
fn parent_dir(path: &Path) -> PathBuf {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.to_owned(),
        _ => PathBuf::from("."),
    }
}

/// Watches a set of files; stops watching when dropped.
pub struct ConfigWatcher {
    watcher: RecommendedWatcher,
    /// Names of the files watched, shared with the event handler.
    names: Arc<Mutex<Vec<OsString>>>,
    /// Directories watched so far.
    dirs: Vec<PathBuf>,
}

impl std::fmt::Debug for ConfigWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigWatcher")
            .field("dirs", &self.dirs)
            .finish_non_exhaustive()
    }
}

impl ConfigWatcher {
    /// Calls `on_change` (from a background thread) after `main` or any of
    /// `others` is created, written, replaced or removed, at most once per
    /// [`DEBOUNCE`] burst. Fails only when `main`'s directory cannot be
    /// watched.
    pub fn new(
        main: &Path,
        others: &[PathBuf],
        on_change: impl Fn() + Send + 'static,
    ) -> Result<Self, String> {
        let names = Arc::new(Mutex::new(Vec::new()));
        let shared = Arc::clone(&names);
        let (tx, rx) = mpsc::channel::<()>();
        let watcher = notify::recommended_watcher(move |result: notify::Result<Event>| {
            let names = shared.lock().unwrap_or_else(PoisonError::into_inner);
            if result.is_ok_and(|event| is_relevant(&event, &names)) {
                let _ = tx.send(());
            }
        })
        .map_err(|error| error.to_string())?;
        let mut this = Self {
            watcher,
            names,
            dirs: Vec::new(),
        };
        let name = main
            .file_name()
            .ok_or_else(|| format!("{} is not a file path", main.display()))?;
        let dir = parent_dir(main);
        this.watcher
            .watch(&dir, RecursiveMode::NonRecursive)
            .map_err(|error| format!("cannot watch {}: {error}", dir.display()))?;
        this.dirs.push(dir);
        this.names
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(name.to_owned());
        this.add(others);
        thread::spawn(move || {
            // Ends when the watcher (and with it the sender) is dropped.
            while rx.recv().is_ok() {
                while rx.recv_timeout(DEBOUNCE).is_ok() {}
                on_change();
            }
        });
        Ok(this)
    }

    /// Also watches `files`, e.g. the imports and theme file of a reloaded
    /// config. Directories that cannot be watched (yet) are skipped.
    pub fn add(&mut self, files: &[PathBuf]) {
        for file in files {
            let Some(name) = file.file_name() else {
                continue;
            };
            let dir = parent_dir(file);
            if !self.dirs.contains(&dir) && dir.is_dir() {
                if self
                    .watcher
                    .watch(&dir, RecursiveMode::NonRecursive)
                    .is_err()
                {
                    continue;
                }
                self.dirs.push(dir);
            }
            let mut names = self.names.lock().unwrap_or_else(PoisonError::into_inner);
            if !names.iter().any(|known| known == name) {
                names.push(name.to_owned());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, DataChange, RemoveKind, RenameMode};
    use std::path::PathBuf;

    fn event(kind: EventKind, path: &str) -> Event {
        Event::new(kind).add_path(PathBuf::from(path))
    }

    const NAME: &str = "nxgterm.toml";

    fn relevant(kind: EventKind, path: &str) -> bool {
        is_relevant(&event(kind, path), &[NAME.into()])
    }

    #[test]
    fn writes_creates_renames_and_removals_of_the_file_count() {
        let path = "/cfg/nxgterm/nxgterm.toml";
        assert!(relevant(
            EventKind::Modify(ModifyKind::Data(DataChange::Any)),
            path
        ));
        assert!(relevant(EventKind::Create(CreateKind::File), path));
        assert!(relevant(EventKind::Remove(RemoveKind::File), path));
        assert!(relevant(
            EventKind::Modify(ModifyKind::Name(RenameMode::To)),
            path
        ));
        assert!(relevant(
            EventKind::Access(AccessKind::Close(AccessMode::Write)),
            path
        ));
        assert!(relevant(EventKind::Any, path));
    }

    #[test]
    fn rename_over_the_file_counts_when_any_path_matches() {
        let event = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(PathBuf::from("/cfg/.nxgterm.toml.swp"))
            .add_path(PathBuf::from("/cfg/nxgterm.toml"));
        assert!(is_relevant(&event, &[NAME.into()]));
    }

    #[test]
    fn other_files_are_ignored() {
        let kind = EventKind::Modify(ModifyKind::Data(DataChange::Any));
        assert!(!relevant(kind, "/cfg/other.toml"));
        assert!(!relevant(kind, "/cfg/nxgterm.toml~"));
    }

    #[test]
    fn any_watched_name_counts() {
        let kind = EventKind::Modify(ModifyKind::Data(DataChange::Any));
        let names: [OsString; 2] = ["nxgterm.toml".into(), "fonts.toml".into()];
        assert!(is_relevant(&event(kind, "/cfg/fonts.toml"), &names));
        assert!(!is_relevant(&event(kind, "/cfg/colors.toml"), &names));
        assert!(!is_relevant(&event(kind, "/cfg/fonts.toml"), &[]));
    }

    #[test]
    fn reads_and_access_time_updates_are_ignored() {
        let path = "/cfg/nxgterm.toml";
        assert!(!relevant(
            EventKind::Access(AccessKind::Open(AccessMode::Read)),
            path
        ));
        assert!(!relevant(EventKind::Access(AccessKind::Read), path));
        assert!(!relevant(
            EventKind::Access(AccessKind::Close(AccessMode::Read)),
            path
        ));
        assert!(!relevant(
            EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime)),
            path
        ));
    }

    #[test]
    fn reports_a_debounced_change_for_the_file() {
        let dir = std::env::temp_dir().join(format!("nxg-watch-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(NAME);
        let (tx, rx) = mpsc::channel();
        let watcher = ConfigWatcher::new(&file, &[], move || {
            let _ = tx.send(());
        });
        let Ok(_watcher) = watcher else {
            eprintln!("skipping watch test: {}", watcher.unwrap_err());
            return;
        };
        std::fs::write(dir.join("unrelated.txt"), "x").unwrap();
        std::fs::write(&file, "[window]\n").unwrap();
        std::fs::write(&file, "[window]\npadding = 1\n").unwrap();
        let got = rx.recv_timeout(Duration::from_secs(5));
        assert!(got.is_ok(), "no change reported");
        // Both writes fall in one burst, so only one change is reported.
        assert!(rx.recv_timeout(DEBOUNCE * 3).is_err(), "not debounced");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reports_changes_to_added_files_in_other_directories() {
        let dir = std::env::temp_dir().join(format!("nxg-watch-add-test-{}", std::process::id()));
        let themes = dir.join("themes");
        std::fs::create_dir_all(&themes).unwrap();
        let (tx, rx) = mpsc::channel();
        let watcher = ConfigWatcher::new(&dir.join(NAME), &[], move || {
            let _ = tx.send(());
        });
        let Ok(mut watcher) = watcher else {
            eprintln!("skipping watch test: {}", watcher.unwrap_err());
            return;
        };
        // Not watched yet.
        std::fs::write(themes.join("mine.toml"), "").unwrap();
        assert!(rx.recv_timeout(DEBOUNCE * 3).is_err(), "unwatched file");
        watcher.add(&[themes.join("mine.toml"), dir.join("missing/x.toml")]);
        assert_eq!(watcher.dirs, [dir.clone(), themes.clone()]);
        std::fs::write(themes.join("mine.toml"), "cursor = \"#fff\"\n").unwrap();
        assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok(), "no change");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
