//! Crash reports: a panic or a fatal error is appended to a log file, and on
//! Windows (a GUI program without a console) also shown in a message box, so
//! a crash is never invisible.
//!
//! The panic hook only logs; the message box comes from [`guard`], which
//! wraps all of `main`. A panic caught on purpose (a pane's parser, renderer
//! or pty setup) never reaches it, so it never shows a dialog; only one
//! about to end the process does.

use std::any::Any;
use std::backtrace::Backtrace;
use std::cell::Cell;
use std::collections::VecDeque;
use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::panic::{self, PanicHookInfo, UnwindSafe};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use nxg_config::Platform;

/// The log is moved to `crash.log.old` before it grows past this.
const LIMIT: u64 = 1024 * 1024;

const DIR: &str = "nxgterm";
const FILE: &str = "crash.log";

/// Where the log goes, set once by [`install`].
static LOG_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Panics of one pane within [`STOP_WINDOW`] after which it is stopped
/// instead of reset again.
const STOP_AFTER: usize = 3;

/// See [`STOP_AFTER`].
const STOP_WINDOW: Duration = Duration::from_secs(10);

thread_local! {
    /// Panics on this thread are logged without a backtrace (see
    /// [`with_brief_reports`]).
    static BRIEF: Cell<bool> = const { Cell::new(false) };
}

/// Operating systems with different log locations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Os {
    Windows,
    Macos,
    /// Linux and the BSDs: the XDG state directory.
    Other,
}

impl Os {
    fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            Self::Other
        }
    }
}

/// The crash log path, or `None` when no location can be derived:
/// `%LOCALAPPDATA%\nxgterm\crash.log` on Windows,
/// `~/Library/Logs/nxgterm/crash.log` on macOS, else
/// `$XDG_STATE_HOME/nxgterm/crash.log` when it is absolute, falling back to
/// `~/.local/state/nxgterm/crash.log`. `env` looks up environment
/// variables.
fn log_path(os: Os, env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let var = |name: &str| env(name).filter(|value| !value.is_empty());
    let home = || nxg_config::path::home_dir(Platform::Unix, &env);
    let base = match os {
        Os::Windows => PathBuf::from(var("LOCALAPPDATA")?),
        Os::Macos => home()?.join("Library").join("Logs"),
        Os::Other => var("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|dir| dir.has_root())
            .or_else(|| home().map(|home| home.join(".local").join("state")))?,
    };
    Some(base.join(DIR).join(FILE))
}

/// One entry of the log.
#[derive(Debug)]
struct Entry<'a> {
    /// "panic" or "fatal error".
    kind: &'a str,
    /// Seconds since the UNIX epoch.
    time: u64,
    thread: Option<&'a str>,
    message: &'a str,
    location: Option<String>,
    backtrace: Option<String>,
}

/// `entry` as the text appended to the log.
fn format(entry: &Entry<'_>) -> String {
    let mut text = format!(
        "=== nxgterm {} {} at {} (unix seconds) on {}/{} ===\n",
        env!("CARGO_PKG_VERSION"),
        entry.kind,
        entry.time,
        env::consts::OS,
        env::consts::ARCH,
    );
    if let Some(thread) = entry.thread {
        text += &format!("thread: {thread}\n");
    }
    text += &format!("message: {}\n", entry.message);
    if let Some(location) = &entry.location {
        text += &format!("location: {location}\n");
    }
    if let Some(backtrace) = &entry.backtrace {
        text += &format!("backtrace:\n{backtrace}\n");
    }
    text + "\n"
}

/// Appends `text` to the file at `path`, creating its directory; first
/// moves a log that would grow past `limit` bytes to `<path>.old`,
/// replacing the previous one, so the log stays bounded. A rotation that
/// fails still appends (the entry matters more than the bound) until the
/// log would pass twice the limit; then the entry is dropped.
fn append_bounded(path: &Path, text: &str, limit: u64) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let size = fs::metadata(path).map_or(0, |meta| meta.len());
    let grown = size + text.len() as u64;
    if size > 0 && grown > limit {
        let mut old = path.as_os_str().to_owned();
        old.push(".old");
        // While the rotation keeps failing the entry is still appended,
        // but only up to twice the limit, so the log cannot grow forever.
        if fs::rename(path, old).is_err() && grown > limit.saturating_mul(2) {
            return Ok(());
        }
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(text.as_bytes())
}

/// The crash log path, once [`install`] ran and a location was found.
pub fn log_path_in_use() -> Option<&'static Path> {
    LOG_PATH.get().and_then(Option::as_deref)
}

/// Installs the panic hook: the default one still prints to stderr, then
/// the panic is appended to the crash log. Call it first in `main`.
pub fn install() {
    LOG_PATH.get_or_init(|| log_path(Os::current(), |name| env::var_os(name)));
    let default = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        default(info);
        log_panic(info);
    }));
}

fn log_panic(info: &PanicHookInfo<'_>) {
    let current = thread::current();
    let message = payload_message(info.payload());
    write(&Entry {
        kind: "panic",
        time: now(),
        thread: Some(current.name().unwrap_or("<unnamed>")),
        message: &message,
        location: info.location().map(ToString::to_string),
        backtrace: (!is_brief()).then(|| Backtrace::force_capture().to_string()),
    });
}

/// Runs `f` with panics on this thread logged without a backtrace when
/// `brief`: a pane that panicked once does not fill the log with the same
/// trace again.
pub fn with_brief_reports<R>(brief: bool, f: impl FnOnce() -> R) -> R {
    let before = BRIEF.replace(brief);
    // Restored even when `f` panics, as the caller catches it.
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            BRIEF.set(self.0);
        }
    }
    let _restore = Restore(before);
    f()
}

fn is_brief() -> bool {
    BRIEF.get()
}

/// What to do with a pane whose parser panicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recovery {
    /// Reset its terminal and go on.
    Reset,
    /// It keeps panicking: stop it, so it does not churn the log and stall
    /// the window.
    Stop,
}

/// The recent parser panics of one pane.
#[derive(Debug, Default)]
pub struct Recoveries {
    /// When each panic within [`STOP_WINDOW`] happened.
    recent: VecDeque<Instant>,
    /// Panics so far.
    total: u64,
}

impl Recoveries {
    /// Notes a panic at `now`: the [`STOP_AFTER`]th within [`STOP_WINDOW`]
    /// stops the pane.
    pub fn record(&mut self, now: Instant) -> Recovery {
        self.total += 1;
        while self
            .recent
            .front()
            .is_some_and(|&at| now.saturating_duration_since(at) >= STOP_WINDOW)
        {
            self.recent.pop_front();
        }
        self.recent.push_back(now);
        if self.recent.len() >= STOP_AFTER {
            Recovery::Stop
        } else {
            Recovery::Reset
        }
    }

    /// Whether the next panic is logged briefly: only the first one of a
    /// pane carries a backtrace.
    pub fn brief(&self) -> bool {
        self.total > 0
    }
}

/// Logs a fatal error that ends the process (e.g. no renderer could start)
/// and, on Windows, shows it.
pub fn fatal(message: &str) {
    write(&Entry {
        kind: "fatal error",
        time: now(),
        thread: None,
        message,
        location: None,
        backtrace: None,
    });
    show(&format!("nxgterm could not continue:\n\n{message}"));
}

/// Logs a diagnostic that is no crash (e.g. a failed pty write) under
/// the heading `kind`; nothing is shown.
pub fn note(kind: &str, message: &str) {
    write(&Entry {
        kind,
        time: now(),
        thread: None,
        message,
        location: None,
        backtrace: None,
    });
}

/// Shows an error that ends the process but is no crash (e.g. the
/// elevated relaunch failed) on Windows; nothing is logged.
pub fn error(message: &str) {
    message_box(&error_text(message));
}

fn error_text(message: &str) -> String {
    format!("nxgterm could not start:\n\n{message}")
}

/// Runs `main`; a panic escaping it (already logged by the hook) is shown
/// on Windows and ends the process with the usual panic exit code.
pub fn guard(main: impl FnOnce() -> ExitCode + UnwindSafe) -> ExitCode {
    panic::catch_unwind(main).unwrap_or_else(|_| {
        show("nxgterm crashed.");
        ExitCode::from(101)
    })
}

/// The one-line notice written into a pane whose terminal was reset after
/// a panic.
pub fn recovered_notice() -> String {
    match log_path_in_use() {
        Some(path) => format!(
            "\r\n[nxgterm: recovered from an internal error, see {}]\r\n",
            path.display()
        ),
        None => "\r\n[nxgterm: recovered from an internal error]\r\n".to_owned(),
    }
}

/// The notice written into a pane stopped after repeated panics: its
/// output is no longer shown and keys no longer reach it.
pub fn stopped_notice() -> String {
    let see = match log_path_in_use() {
        Some(path) => format!(", see {}", path.display()),
        None => String::new(),
    };
    format!("\r\n[nxgterm: this pane kept failing and was stopped{see}; close it to go on]\r\n")
}

/// Appends `entry` to the log; errors are ignored, as there is nowhere left
/// to report them.
fn write(entry: &Entry<'_>) {
    if let Some(path) = log_path_in_use() {
        let _ = append_bounded(path, &format(entry), LIMIT);
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn payload_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "<non-string panic payload>".to_owned()
    }
}

/// Shows `headline` and where the log is in a message box on Windows; the
/// other platforms have stderr.
fn show(headline: &str) {
    let text = match log_path_in_use() {
        Some(path) => format!("{headline}\n\nA report was saved to:\n{}", path.display()),
        None => headline.to_owned(),
    };
    message_box(&text);
}

#[cfg(windows)]
fn message_box(text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let text = wide(text);
    let caption = wide("nxgterm");
    // SAFETY: both strings are NUL-terminated and outlive the call; a null
    // owner window is allowed.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
fn message_box(_text: &str) {}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    fn env(vars: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
        move |name| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        }
    }

    #[test]
    fn windows_logs_under_local_app_data() {
        let vars = env(&[("LOCALAPPDATA", r"C:\Users\me\AppData\Local")]);
        assert_eq!(
            log_path(Os::Windows, vars),
            Some(
                PathBuf::from(r"C:\Users\me\AppData\Local")
                    .join("nxgterm")
                    .join("crash.log")
            )
        );
        assert_eq!(log_path(Os::Windows, env(&[("LOCALAPPDATA", "")])), None);
    }

    #[test]
    fn macos_logs_under_library_logs() {
        let vars = env(&[("HOME", "/Users/me"), ("XDG_STATE_HOME", "/x")]);
        assert_eq!(
            log_path(Os::Macos, vars),
            Some(PathBuf::from("/Users/me/Library/Logs/nxgterm/crash.log"))
        );
    }

    #[test]
    fn other_systems_log_under_the_xdg_state_dir() {
        let vars = env(&[("HOME", "/home/me"), ("XDG_STATE_HOME", "/state")]);
        assert_eq!(
            log_path(Os::Other, vars),
            Some(PathBuf::from("/state/nxgterm/crash.log"))
        );
        let relative = env(&[("HOME", "/home/me"), ("XDG_STATE_HOME", "state")]);
        assert_eq!(
            log_path(Os::Other, relative),
            Some(PathBuf::from("/home/me/.local/state/nxgterm/crash.log")),
            "a relative XDG_STATE_HOME is ignored"
        );
        assert_eq!(
            log_path(Os::Other, env(&[("HOME", "/home/me")])),
            Some(PathBuf::from("/home/me/.local/state/nxgterm/crash.log"))
        );
        assert_eq!(log_path(Os::Other, env(&[])), None);
    }

    #[test]
    fn a_report_has_every_detail() {
        let text = format(&Entry {
            kind: "panic",
            time: 1_700_000_000,
            thread: Some("main"),
            message: "index out of bounds",
            location: Some("src/grid.rs:10:5".into()),
            backtrace: Some("0: frame".into()),
        });
        for part in [
            "panic",
            "1700000000",
            env!("CARGO_PKG_VERSION"),
            env::consts::OS,
            env::consts::ARCH,
            "thread: main",
            "message: index out of bounds",
            "location: src/grid.rs:10:5",
            "0: frame",
        ] {
            assert!(text.contains(part), "{part:?} missing from {text:?}");
        }
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn a_fatal_error_report_leaves_out_what_it_lacks() {
        let text = format(&Entry {
            kind: "fatal error",
            time: 1,
            thread: None,
            message: "no renderer available",
            location: None,
            backtrace: None,
        });
        assert!(text.contains("fatal error"));
        assert!(text.contains("message: no renderer available"));
        assert!(!text.contains("thread:"));
        assert!(!text.contains("location:"));
        assert!(!text.contains("backtrace:"));
    }

    /// A fresh directory under the system temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = env::temp_dir().join(format!(
                "nxgterm-crash-{name}-{}-{unique}",
                std::process::id()
            ));
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn appending_creates_the_directory_and_keeps_earlier_entries() {
        let dir = TempDir::new("append");
        let path = dir.0.join("nested").join("crash.log");
        append_bounded(&path, "one\n", 100).unwrap();
        append_bounded(&path, "two\n", 100).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "one\ntwo\n");
    }

    #[test]
    fn a_log_that_would_grow_past_the_limit_is_rotated() {
        let dir = TempDir::new("rotate");
        let path = dir.0.join("crash.log");
        let old = dir.0.join("crash.log.old");
        append_bounded(&path, "first\n", 10).unwrap();
        append_bounded(&path, "second\n", 10).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "second\n");
        assert_eq!(fs::read_to_string(&old).unwrap(), "first\n");
        append_bounded(&path, "third\n", 10).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "third\n");
        assert_eq!(
            fs::read_to_string(&old).unwrap(),
            "second\n",
            "only the previous log is kept"
        );
    }

    #[test]
    fn a_failed_rotation_still_appends_the_entry() {
        let dir = TempDir::new("rotate-fails");
        let path = dir.0.join("crash.log");
        // A non-empty directory in the way of `crash.log.old`.
        fs::create_dir_all(dir.0.join("crash.log.old").join("x")).unwrap();
        append_bounded(&path, "first\n", 10).unwrap();
        append_bounded(&path, "second\n", 10).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "first\nsecond\n");
    }

    #[test]
    fn a_log_that_cannot_rotate_stops_growing_at_twice_the_limit() {
        let dir = TempDir::new("rotate-keeps-failing");
        let path = dir.0.join("crash.log");
        fs::create_dir_all(dir.0.join("crash.log.old").join("x")).unwrap();
        for _ in 0..10 {
            append_bounded(&path, "entry\n", 10).unwrap();
        }
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.len() <= 20, "{} bytes", text.len());
        assert!(text.starts_with("entry\nentry\n"), "earlier entries kept");
    }

    #[test]
    fn a_plain_error_is_not_reported_as_a_crash() {
        let text = error_text("the elevation was cancelled");
        assert!(text.contains("the elevation was cancelled"), "{text:?}");
        assert!(!text.to_lowercase().contains("crash"), "{text:?}");
        assert!(!text.contains("report"), "{text:?}");
    }

    #[test]
    fn repeated_panics_stop_recovery_within_the_window() {
        let base = Instant::now();
        let at = |s: u64| base + Duration::from_secs(s);
        let mut recoveries = Recoveries::default();
        assert!(!recoveries.brief(), "the first panic is reported in full");
        assert_eq!(recoveries.record(at(0)), Recovery::Reset);
        assert!(recoveries.brief(), "later ones briefly");
        assert_eq!(recoveries.record(at(1)), Recovery::Reset);
        assert_eq!(recoveries.record(at(2)), Recovery::Stop);
    }

    #[test]
    fn panics_spread_out_keep_recovering() {
        let base = Instant::now();
        let mut recoveries = Recoveries::default();
        for s in [0, 6, 12, 18, 24, 30] {
            let now = base + Duration::from_secs(s);
            assert_eq!(recoveries.record(now), Recovery::Reset, "at {s} s");
        }
    }

    #[test]
    fn brief_reports_apply_only_inside_the_call() {
        assert!(!is_brief());
        assert!(with_brief_reports(true, is_brief));
        assert!(!with_brief_reports(false, is_brief));
        assert!(!is_brief(), "restored after");
    }
}
