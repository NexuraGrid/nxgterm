//! PTY adapters producing `nxg_core::ports::PtySession`.
//!
//! The native backend goes through `portable-pty` (Unix pty, Windows ConPTY
//! on Win10 1809+ / Server 2019+). On Windows, embedded winpty is the
//! fallback for Windows Server 2016, which has no ConPTY.

mod backend;
mod conpty;
mod shell;
mod winpty;

use std::fmt;
use std::io::{self, Write};
use std::panic::{self, AssertUnwindSafe};

use nxg_core::WinSize;
use nxg_core::fallback::{self, Attempt, Init, Selected};
use nxg_core::ports::{ChildProcess, PtyControl, PtySession};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

pub use backend::{Backend, ENV_VAR, auto_backends, backends_from_env};
pub use conpty::conpty_host;

/// A started session plus the backend that started it and the ones that
/// failed before it.
pub type Spawned = Selected<PtySession, String>;

/// Environment every backend sets for the child.
const CHILD_ENV: [(&str, &str); 3] = [
    ("TERM", "xterm-256color"),
    // Lets programs (Yazi, chafa, timg) detect image protocol support.
    ("TERM_PROGRAM", "nxgterm"),
    ("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION")),
];

/// Every backend failed to start the child.
#[derive(Debug)]
pub struct PtyError {
    pub attempts: Vec<Attempt<String>>,
}

impl fmt::Display for PtyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("no pty backend could start the shell")?;
        for attempt in &self.attempts {
            write!(f, "; {}: {}", attempt.name, attempt.error)?;
        }
        Ok(())
    }
}

impl std::error::Error for PtyError {}

/// Spawns the user's default shell with the [`auto_backends`].
///
/// Unix: `$SHELL`, falling back to `/bin/sh`. Windows: PowerShell 7, then
/// Windows PowerShell 5.1, then `%ComSpec%` (cmd.exe).
pub fn spawn_shell(size: impl Into<WinSize>) -> Result<PtySession, PtyError> {
    spawn(size.into(), auto_backends(), default_shell).map(|spawned| spawned.backend)
}

/// A program to run instead of the default shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellCommand {
    pub program: String,
    pub args: Vec<String>,
}

/// Spawns `shell` when given, otherwise the default shell (see
/// [`spawn_shell`]), trying `backends` in order.
pub fn spawn_shell_with(
    size: impl Into<WinSize>,
    shell: Option<&ShellCommand>,
    backends: &[Backend],
) -> Result<Spawned, PtyError> {
    let size = size.into();
    match shell {
        Some(shell) => spawn(size, backends, || {
            let mut cmd = CommandBuilder::new(&shell.program);
            cmd.args(&shell.args);
            cmd
        }),
        None => spawn(size, backends, default_shell),
    }
}

/// Spawns `program` with `args` with the [`auto_backends`].
pub fn spawn_command(
    size: impl Into<WinSize>,
    program: &str,
    args: &[&str],
) -> Result<PtySession, PtyError> {
    spawn_command_with(size, auto_backends(), program, args).map(|spawned| spawned.backend)
}

/// Spawns `program` with `args`, trying `backends` in order.
pub fn spawn_command_with(
    size: impl Into<WinSize>,
    backends: &[Backend],
    program: &str,
    args: &[&str],
) -> Result<Spawned, PtyError> {
    spawn(size.into(), backends, || {
        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        cmd
    })
}

fn default_shell() -> CommandBuilder {
    #[cfg(windows)]
    return CommandBuilder::new(shell::default_windows_shell());
    #[cfg(not(windows))]
    CommandBuilder::new_default_prog()
}

fn spawn(
    size: WinSize,
    backends: &[Backend],
    command: impl Fn() -> CommandBuilder,
) -> Result<Spawned, PtyError> {
    let command = &command;
    let candidates: Vec<(&'static str, Init<'_, PtySession, String>)> = backends
        .iter()
        .map(|&backend| {
            let init: Init<'_, PtySession, String> =
                Box::new(move || start(backend, size, command()));
            (backend.name(), init)
        })
        .collect();
    fallback::first_available(candidates).map_err(|attempts| PtyError { attempts })
}

/// Starts `cmd` on `backend`; a panic inside the backend becomes an error so
/// the next backend still gets its turn.
fn start(backend: Backend, size: WinSize, cmd: CommandBuilder) -> Result<PtySession, String> {
    panic::catch_unwind(AssertUnwindSafe(|| match backend {
        Backend::Native => spawn_native(size, cmd),
        Backend::Winpty => winpty::spawn(size, cmd.get_argv(), &CHILD_ENV),
    }))
    .unwrap_or_else(|payload| Err(backend::panic_message(payload.as_ref())))
}

/// Whether kernel32 exports `CreatePseudoConsole` (Windows 10 1809+).
#[cfg(windows)]
fn conpty_available() -> bool {
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    // SAFETY: both names are NUL-terminated literals, and kernel32 is loaded
    // in every process.
    unsafe {
        let kernel32 = GetModuleHandleW(windows_sys::w!("kernel32.dll"));
        !kernel32.is_null()
            && GetProcAddress(kernel32, windows_sys::s!("CreatePseudoConsole")).is_some()
    }
}

/// Unix pty or Windows ConPTY through `portable-pty`.
fn spawn_native(size: WinSize, mut cmd: CommandBuilder) -> Result<PtySession, String> {
    // portable-pty fails obscurely (or panics) without ConPTY.
    #[cfg(windows)]
    backend::require_conpty(conpty_available())?;
    for (name, value) in CHILD_ENV {
        cmd.env(name, value);
    }
    let pair = native_pty_system()
        .openpty(pty_size(size))
        .map_err(|e| e.to_string())?;
    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    // Close our copy of the slave so the reader sees EOF/EIO when the child exits.
    drop(pair.slave);
    let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    let control = NativeControl {
        master: pair.master,
        writer,
        killer: child.clone_killer(),
    };
    Ok(PtySession {
        reader,
        control: Box::new(control),
        child: Box::new(NativeChild(child)),
    })
}

fn pty_size(size: WinSize) -> PtySize {
    let (pixel_width, pixel_height) = size.pixels();
    PtySize {
        cols: size.cells.cols(),
        rows: size.cells.rows(),
        pixel_width,
        pixel_height,
    }
}

struct NativeControl {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

impl Write for NativeControl {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writer.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

impl PtyControl for NativeControl {
    fn resize(&mut self, size: WinSize) -> io::Result<()> {
        self.master.resize(pty_size(size)).map_err(io::Error::other)
    }
}

impl Drop for NativeControl {
    fn drop(&mut self) {
        // The child may already be gone; nothing useful to do on failure.
        let _ = self.killer.kill();
    }
}

struct NativeChild(Box<dyn portable_pty::Child + Send + Sync>);

impl ChildProcess for NativeChild {
    fn wait(&mut self) -> io::Result<()> {
        self.0.wait().map(drop)
    }
}
