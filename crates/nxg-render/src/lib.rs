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
//! Both also implement [`WindowRenderer`], so a new [`Style`] (font,
//! colors, padding) applies without recreating the surface.

pub mod cpu_window;
pub mod font;
pub mod frame;
pub mod gpu;
pub mod images;
pub mod paint;
pub mod palette;
pub mod renderer;
pub mod style;
#[cfg(test)]
mod test_font;

pub use cpu_window::CpuWindowRenderer;
pub use font::{Font, FontError, FontFaces};
pub use frame::Frame;
pub use gpu::{GpuError, GpuRenderer};
pub use paint::{CellSize, Layout};
pub use palette::Palette;
pub use renderer::CpuRenderer;
pub use style::{Style, WindowRenderer};
