//! The CPU renderer presenting to a window through softbuffer.

use std::num::NonZeroU32;

use nxg_core::Terminal;
use nxg_core::ports::{RenderError, Renderer};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use softbuffer::{Context, SoftBufferError, Surface};

use crate::frame::Frame;
use crate::renderer::CpuRenderer;
use crate::style::{Style, WindowRenderer};

/// [`CpuRenderer`] plus a softbuffer surface; works everywhere.
pub struct CpuWindowRenderer<W: HasDisplayHandle + HasWindowHandle> {
    surface: Surface<W, W>,
    renderer: CpuRenderer,
    width: u32,
    height: u32,
}

impl<W: HasDisplayHandle + HasWindowHandle> std::fmt::Debug for CpuWindowRenderer<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CpuWindowRenderer")
            .field("renderer", &self.renderer)
            .field("size", &(self.width, self.height))
            .finish_non_exhaustive()
    }
}

impl<W: HasDisplayHandle + HasWindowHandle + Clone> CpuWindowRenderer<W> {
    /// Creates a surface for `window`; call [`Renderer::resize`] before
    /// the first draw.
    pub fn new(window: W, style: Style) -> Result<Self, SoftBufferError> {
        let context = Context::new(window.clone())?;
        let surface = Surface::new(&context, window)?;
        Ok(Self {
            surface,
            renderer: CpuRenderer::new(style),
            width: 0,
            height: 0,
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
        self.renderer.render(terminal, &mut frame);
        buffer.present().map_err(fatal)
    }
}

impl<W: HasDisplayHandle + HasWindowHandle> WindowRenderer for CpuWindowRenderer<W> {
    fn set_style(&mut self, style: Style) {
        self.renderer.set_style(style);
    }
}
