//! wgpu renderer: a glyph atlas plus instanced quads.
//!
//! Pure parts (instance building, atlas packing, format and adapter
//! choice) are unit tested; [`painter`] holds the GPU work shared by the
//! window renderer and the offscreen tests.

pub mod adapter;
mod atlas;
mod device;
pub mod format;
pub mod instance;
pub mod packer;
mod painter;
mod renderer;
#[cfg(test)]
mod tests;

use std::fmt;

pub use renderer::GpuRenderer;

/// Why the GPU renderer could not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuError(pub String);

impl fmt::Display for GpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GpuError {}
