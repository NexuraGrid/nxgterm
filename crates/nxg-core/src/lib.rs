//! Platform-agnostic terminal core.
//!
//! Holds the domain types and the ports (traits) that platform adapters
//! implement. Nothing in this crate knows about windows, GPUs or OS APIs.

pub mod fallback;
pub mod ports;
pub mod size;

pub use size::{SizeError, TermSize};
