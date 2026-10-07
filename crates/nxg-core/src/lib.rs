//! Platform-agnostic terminal core.
//!
//! Holds the domain types and the ports (traits) that platform adapters
//! implement. Nothing in this crate knows about windows, GPUs or OS APIs.

pub mod apc;
pub mod cell;
pub mod fallback;
pub mod grid;
pub mod image;
pub mod kitty;
pub mod ports;
pub mod sixel;
pub mod size;
pub mod terminal;

pub use cell::{Cell, Color, Flags};
pub use size::{CellPixels, SizeError, TermSize, WinSize};
pub use terminal::{Cursor, DEFAULT_SCROLLBACK, Modes, Terminal};
