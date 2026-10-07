//! Platform-agnostic terminal core.
//!
//! Holds the domain types and the ports (traits) that platform adapters
//! implement. Nothing in this crate knows about windows, GPUs or OS APIs.

pub mod cell;
pub mod fallback;
pub mod grid;
pub mod ports;
pub mod size;
pub mod terminal;

pub use cell::{Cell, Color, Flags};
pub use size::{SizeError, TermSize};
pub use terminal::{Cursor, Terminal};
