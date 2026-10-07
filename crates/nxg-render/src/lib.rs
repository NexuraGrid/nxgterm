//! Renderer adapters.
//!
//! Phase 2: a CPU renderer drawing into a caller-owned 0RGB framebuffer.
//! Phase 3 adds a wgpu GPU renderer, selected at runtime through
//! `nxg_core::fallback`.

pub mod font;
pub mod frame;
pub mod paint;
pub mod palette;
pub mod renderer;

pub use font::{Font, FontError};
pub use frame::Frame;
pub use paint::CellSize;
pub use palette::Palette;
pub use renderer::CpuRenderer;
