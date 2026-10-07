//! Ports implemented by platform adapters.

use std::fmt;
use std::io::{self, Read, Write};

use crate::TermSize;

/// The writable half of a pseudo-terminal: child input and resizing.
///
/// Lives on the UI thread while the [`PtySession::reader`] is drained on a
/// background thread.
pub trait PtyControl: Write + Send {
    fn resize(&mut self, size: TermSize) -> io::Result<()>;
}

/// The child process attached to a pseudo-terminal.
///
/// Some backends (ConPTY) do not report EOF on the reader when the child
/// exits, so callers wait on this from a background thread to detect exit.
pub trait ChildProcess: Send {
    /// Blocks until the child exits.
    fn wait(&mut self) -> io::Result<()>;
}

/// A pseudo-terminal connected to a child process (usually a shell).
///
/// Adapters: Unix pty, Windows ConPTY, Windows winpty (Server 2016).
/// The reader yields the child's output, the control sends it input and
/// resizes it, and the child reports its exit. Dropping the control is
/// expected to terminate the child.
pub struct PtySession {
    pub reader: Box<dyn Read + Send>,
    pub control: Box<dyn PtyControl>,
    pub child: Box<dyn ChildProcess>,
}

impl fmt::Debug for PtySession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PtySession").finish_non_exhaustive()
    }
}
