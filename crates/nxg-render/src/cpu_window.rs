//! The CPU renderer presenting to a window through softbuffer.
//!
//! softbuffer has no alpha channel, so this renderer is always opaque: a
//! background opacity below 1.0 is ignored (see
//! [`WindowRenderer::translucent`]).

use std::num::NonZeroU32;

use nxg_core::Terminal;
use nxg_core::ports::{RenderError, Renderer};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use softbuffer::{Context, SoftBufferError, Surface};

use crate::frame::Frame;
use crate::renderer::CpuRenderer;
use crate::shape::Shape;
use crate::style::{Overlay, Style, WindowRenderer};

/// [`CpuRenderer`] plus a softbuffer surface; works everywhere.
pub struct CpuWindowRenderer<W: HasDisplayHandle + HasWindowHandle> {
    surface: Surface<W, W>,
    renderer: CpuRenderer,
    width: u32,
    height: u32,
    /// The window was created transparent: fill the top byte of each pixel.
    transparent_window: bool,
}

impl<W: HasDisplayHandle + HasWindowHandle> std::fmt::Debug for CpuWindowRenderer<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CpuWindowRenderer")
            .field("renderer", &self.renderer)
            .field("size", &(self.width, self.height))
            .field("transparent_window", &self.transparent_window)
            .finish_non_exhaustive()
    }
}

impl<W: HasDisplayHandle + HasWindowHandle + Clone> CpuWindowRenderer<W> {
    /// Creates a surface for `window`; call [`Renderer::resize`] before
    /// the first draw. `transparent_window` tells that the window was
    /// created transparent, so its pixels are made explicitly opaque.
    pub fn new(window: W, style: Style, transparent_window: bool) -> Result<Self, SoftBufferError> {
        let context = Context::new(window.clone())?;
        let surface = Surface::new(&context, window)?;
        Ok(Self {
            surface,
            renderer: CpuRenderer::new(style),
            width: 0,
            height: 0,
            transparent_window,
        })
    }
}

impl<W: HasDisplayHandle + HasWindowHandle> Renderer for CpuWindowRenderer<W> {
    fn name(&self) -> &'static str {
        "cpu"
    }

    fn cell_size(&self) -> (u32, u32) {
        let cell = self.renderer.cell_size();
        (cell.width, cell.height)
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
    }

    fn draw(&mut self, terminal: &Terminal) -> Result<(), RenderError> {
        self.draw_layers(None, terminal, None, &[])
    }
}

impl<W: HasDisplayHandle + HasWindowHandle> WindowRenderer for CpuWindowRenderer<W> {
    fn set_style(&mut self, style: Style) {
        self.renderer.set_style(style);
    }

    fn translucent(&self) -> bool {
        false
    }

    fn draw_layers(
        &mut self,
        header: Option<&Terminal>,
        terminal: &Terminal,
        overlay: Option<Overlay<'_>>,
        shapes: &[Shape],
    ) -> Result<(), RenderError> {
        let (Some(width), Some(height)) =
            (NonZeroU32::new(self.width), NonZeroU32::new(self.height))
        else {
            return Ok(()); // Minimized.
        };
        let fatal = |error: SoftBufferError| RenderError::Fatal(error.to_string());
        self.surface.resize(width, height).map_err(fatal)?;
        let mut buffer = self.surface.buffer_mut().map_err(fatal)?;
        let mut frame = Frame::new(&mut buffer, width.get(), height.get())
            .ok_or_else(|| RenderError::Fatal("surface buffer smaller than the window".into()))?;
        self.renderer
            .render_layers(header, terminal, overlay, shapes, &mut frame);
        if self.transparent_window {
            set_opaque_alpha(&mut buffer);
        }
        buffer.present().map_err(fatal)
    }
}

/// Sets the top byte of every 0RGB pixel to 0xff. softbuffer asks for it
/// to be zero, but on a transparent window (a 32-bit X11 visual, a Windows
/// window with blur-behind) it reaches the compositor as the alpha, and
/// zero would make the whole window invisible.
fn set_opaque_alpha(pixels: &mut [u32]) {
    for pixel in pixels {
        *pixel |= 0xff00_0000;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_alpha_fills_the_top_byte_and_keeps_the_color() {
        let mut pixels = [0x0012_3456, 0, 0x00ff_ffff];
        set_opaque_alpha(&mut pixels);
        assert_eq!(pixels, [0xff12_3456, 0xff00_0000, 0xffff_ffff]);
    }
}
