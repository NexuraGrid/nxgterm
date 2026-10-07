//! Ports implemented by platform adapters.

use std::fmt;
use std::io::{self, Read, Write};

use crate::{TermSize, Terminal};

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

/// Why a frame could not be drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    /// The frame was skipped but the renderer is still usable (surface
    /// outdated, acquire timeout). Callers just draw again later.
    Transient(String),
    /// The renderer is unusable (device lost, out of memory). Callers must
    /// replace it, typically by falling back to another renderer.
    Fatal(String),
}

impl RenderError {
    pub fn is_fatal(&self) -> bool {
        matches!(self, Self::Fatal(_))
    }
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transient(reason) => write!(f, "frame skipped: {reason}"),
            Self::Fatal(reason) => write!(f, "fatal render error: {reason}"),
        }
    }
}

impl std::error::Error for RenderError {}

/// Draws a [`Terminal`] onto a window it owns.
///
/// Adapters: wgpu (GPU) and a software renderer. Presentation is the
/// adapter's job, so callers never touch the graphics API.
pub trait Renderer {
    /// Short identifier for logs, e.g. `"gpu"` or `"cpu"`.
    fn name(&self) -> &'static str;
    /// Size of one character cell in pixels as `(width, height)`.
    fn cell_size(&self) -> (u32, u32);
    /// Resizes the drawable area to `width x height` pixels; zero means
    /// minimized and makes [`Renderer::draw`] a no-op.
    fn resize(&mut self, width: u32, height: u32);
    /// Draws and presents one frame.
    fn draw(&mut self, terminal: &Terminal) -> Result<(), RenderError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_fatal_errors_are_fatal() {
        assert!(RenderError::Fatal("device lost".into()).is_fatal());
        assert!(!RenderError::Transient("outdated".into()).is_fatal());
    }

    #[test]
    fn display_includes_kind_and_reason() {
        let fatal = RenderError::Fatal("device lost".into()).to_string();
        let transient = RenderError::Transient("timeout".into()).to_string();
        assert_eq!(fatal, "fatal render error: device lost");
        assert_eq!(transient, "frame skipped: timeout");
    }
}
