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
            left: 0,
            top: 0,
        }
    }
}

/// A terminal drawn over the grid, such as the command palette, with its
/// cell `0, 0` at the grid's cell `col`, `row` (see [`Layout::at`]). Every
/// cell is opaque, default backgrounds included, and it has no images.
#[derive(Debug, Clone, Copy)]
pub struct Overlay<'a> {
    pub terminal: &'a Terminal,
    pub col: u16,
    pub row: u16,
}

/// A [`Renderer`] whose style can change while it runs (config reload,
/// font zoom, display scale change) without recreating its surface.
pub trait WindowRenderer: Renderer {
    /// Replaces the font, colors and padding; takes effect on the next draw.
    fn set_style(&mut self, style: Style);
    /// Draws and presents one frame with the rows of `header` (such as the
    /// tab bar) at the top of the grid area, `terminal` below them, at
    /// [`Layout::below`] the header's rows, and `overlay` on top of it all.
    /// [`Renderer::draw`] is this with `terminal` alone. The cursors of the
    /// header and the overlay are drawn like any other.
    fn draw_layers(
        &mut self,
        header: Option<&Terminal>,
        terminal: &Terminal,
        overlay: Option<Overlay<'_>>,
    ) -> Result<(), RenderError>;
}
