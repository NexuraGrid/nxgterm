//! Ports implemented by platform adapters.

use std::io::{self, Read, Write};

use crate::TermSize;

/// A pseudo-terminal connected to a child process (usually a shell).
///
/// Adapters: Unix pty, Windows ConPTY, Windows winpty (Server 2016).
/// Reading yields the child's output; writing sends it input.
pub trait Pty: Read + Write + Send {
    fn resize(&mut self, size: TermSize) -> io::Result<()>;
}
