//! Watches the config file and reports debounced changes.
//!
//! The parent directory is watched rather than the file: editors often save
//! by writing a temporary file and renaming it over the original, which
//! would orphan a watch on the file itself.

use std::ffi::OsStr;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use notify::event::{AccessKind, AccessMode, EventKind, MetadataKind, ModifyKind};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

/// Quiet time after the last event before reporting a change; editors
/// produce bursts of events per save.
pub const DEBOUNCE: Duration = Duration::from_millis(200);

/// Whether `event` may have changed the file called `name`. Reads (our own
/// reload opens the file) and access-time updates are ignored.
pub fn is_relevant(event: &Event, name: &OsStr) -> bool {
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
            .any(|path| path.file_name() == Some(name))
}

/// Calls `on_change` (from a background thread) after `path` is created,
/// written, replaced or removed, at most once per [`DEBOUNCE`] burst.
/// The returned watcher stops watching when dropped.
pub fn watch(
    path: &Path,
    on_change: impl Fn() + Send + 'static,
) -> Result<RecommendedWatcher, String> {
    let name = path
        .file_name()
        .ok_or_else(|| format!("{} is not a file path", path.display()))?
        .to_owned();
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.to_owned(),
        _ => Path::new(".").to_owned(),
    };
    let (tx, rx) = mpsc::channel::<()>();
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<Event>| {
        if result.is_ok_and(|event| is_relevant(&event, &name)) {
            let _ = tx.send(());
        }
    })
    .map_err(|error| error.to_string())?;
    watcher
        .watch(&dir, RecursiveMode::NonRecursive)
        .map_err(|error| format!("cannot watch {}: {error}", dir.display()))?;
    thread::spawn(move || {
        // Ends when the watcher (and with it the sender) is dropped.
        while rx.recv().is_ok() {
            while rx.recv_timeout(DEBOUNCE).is_ok() {}
            on_change();
        }
    });
    Ok(watcher)
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
        is_relevant(&event(kind, path), OsStr::new(NAME))
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
        assert!(is_relevant(&event, OsStr::new(NAME)));
    }

    #[test]
    fn other_files_are_ignored() {
        let kind = EventKind::Modify(ModifyKind::Data(DataChange::Any));
        assert!(!relevant(kind, "/cfg/other.toml"));
        assert!(!relevant(kind, "/cfg/nxgterm.toml~"));
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
        let watcher = watch(&file, move || {
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
}
