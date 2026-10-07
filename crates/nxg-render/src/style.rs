//! What a renderer draws with, and how to change it at runtime.

use nxg_core::Terminal;
use nxg_core::ports::{RenderError, Renderer};

use crate::font::Font;
use crate::paint::Layout;
use crate::palette::Palette;

/// Font, colors and padding, all in physical pixels.
#[derive(Debug, Clone)]
pub struct Style {
    pub font: Font,
    pub palette: Palette,
    /// Pixels between the window edge and the grid.
    pub padding: u32,
}

impl Style {
    pub fn layout(&self) -> Layout {
        Layout {
            cell: self.font.cell_size(),
            padding: self.padding,
            top: 0,
        }
    }
}

/// A [`Renderer`] whose style can change while it runs (config reload,
/// font zoom, display scale change) without recreating its surface.
pub trait WindowRenderer: Renderer {
    /// Replaces the font, colors and padding; takes effect on the next draw.
    fn set_style(&mut self, style: Style);
    /// Draws and presents one frame with the rows of `header` (such as the
    /// tab bar) at the top of the grid area and `terminal` below them, at
    /// [`Layout::below`] the header's rows. [`Renderer::draw`] is this
    /// without a header. The header's cursor is drawn, so callers hide it.
    fn draw_with_header(
        &mut self,
        header: Option<&Terminal>,
        terminal: &Terminal,
    ) -> Result<(), RenderError>;
}
