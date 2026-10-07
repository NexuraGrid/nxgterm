//! Renderer adapters.
//!
//! Both implement the [`nxg_core::ports::Renderer`] port and own their
//! window surface:
//!
//! - [`GpuRenderer`]: wgpu, glyph atlas plus instanced quads.
//! - [`CpuWindowRenderer`]: [`CpuRenderer`] (a pure 0RGB framebuffer
//!   renderer) presented through softbuffer.
//!
//! The application picks one at runtime through `nxg_core::fallback`.

pub mod cpu_window;
pub mod font;
pub mod frame;
pub mod gpu;
pub mod paint;
pub mod palette;
pub mod renderer;

pub use cpu_window::CpuWindowRenderer;
pub use font::{Font, FontError};
pub use frame::Frame;
pub use gpu::{GpuError, GpuRenderer};
pub use paint::CellSize;
pub use palette::Palette;
pub use renderer::CpuRenderer;
