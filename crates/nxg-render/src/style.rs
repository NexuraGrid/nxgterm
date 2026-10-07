//! What a renderer draws with, and how to change it at runtime.

use nxg_core::ports::Renderer;

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
        }
    }
}

/// A [`Renderer`] whose style can change while it runs (config reload,
/// font zoom, display scale change) without recreating its surface.
pub trait WindowRenderer: Renderer {
    /// Replaces the font, colors and padding; takes effect on the next draw.
    fn set_style(&mut self, style: Style);
}
