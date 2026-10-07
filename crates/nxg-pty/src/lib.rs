//! PTY adapters producing `nxg_core::ports::PtySession`.
//!
//! Phase 2: the native backend through `portable-pty` (Unix pty, Windows
//! ConPTY on Win10 1809+ / Server 2019+). Phase 6 adds embedded winpty as a
//! fallback for Windows Server 2016.

mod shell;

use std::fmt;
use std::io::{self, Write};

use nxg_core::TermSize;
use nxg_core::fallback::{self, Attempt, Init};
use nxg_core::ports::{ChildProcess, PtyControl, PtySession};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

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

/// Spawns the user's default shell.
///
/// Unix: `$SHELL`, falling back to `/bin/sh`. Windows: PowerShell 7, then
/// Windows PowerShell 5.1, then `%ComSpec%` (cmd.exe).
pub fn spawn_shell(size: TermSize) -> Result<PtySession, PtyError> {
    #[cfg(windows)]
    let shell = shell::default_windows_shell();
    #[cfg(windows)]
    return spawn(size, || CommandBuilder::new(&shell));
    #[cfg(not(windows))]
    spawn(size, CommandBuilder::new_default_prog)
}

/// Spawns `program` with `args`.
pub fn spawn_command(size: TermSize, program: &str, args: &[&str]) -> Result<PtySession, PtyError> {
    spawn(size, || {
        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        cmd
    })
}

fn spawn(size: TermSize, command: impl Fn() -> CommandBuilder) -> Result<PtySession, PtyError> {
    let candidates: Vec<(&'static str, Init<'_, PtySession, String>)> = vec![
        ("native", Box::new(|| spawn_native(size, command()))),
        // Phase 6: ("winpty", ...) as the fallback for Windows Server 2016,
        // where ConPTY is unavailable.
    ];
    fallback::first_available(candidates)
        .map(|selected| selected.backend)
        .map_err(|attempts| PtyError { attempts })
}

/// Unix pty or Windows ConPTY through `portable-pty`.
fn spawn_native(size: TermSize, mut cmd: CommandBuilder) -> Result<PtySession, String> {
    cmd.env("TERM", "xterm-256color");
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

fn pty_size(size: TermSize) -> PtySize {
    PtySize {
        cols: size.cols(),
        rows: size.rows(),
        pixel_width: 0,
        pixel_height: 0,
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
    fn resize(&mut self, size: TermSize) -> io::Result<()> {
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
