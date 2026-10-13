//! What a renderer draws with, and how to change it at runtime.

use nxg_core::Terminal;
use nxg_core::ports::{RenderError, Renderer};

use crate::font::Font;
use crate::paint::{Layout, Look};
use crate::palette::Palette;
use crate::shape::Shape;

/// Font, colors and padding, all in physical pixels.
#[derive(Debug, Clone)]
pub struct Style {
    pub font: Font,
    pub palette: Palette,
    /// Pixels between the window edge and the grid.
    pub padding: u32,
    /// Opacity (0.0-1.0) of the default background: the padding and the
    /// grid cells that show [`Palette::background`]. Everything else stays
    /// opaque. Only renderers whose surface supports it honor it (see
    /// [`WindowRenderer::translucent`]); the others draw it opaque.
    pub background_opacity: f32,
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

/// What sits above the panes: a band `top` pixels tall at the window top,
/// left to the caller's [`Shape`]s (such as an integrated title bar), then
/// the rows of `rows` (such as the tab bar) at the top of the grid area.
/// The default is neither: the panes start at the padding.
#[derive(Debug, Clone, Copy, Default)]
pub struct Header<'a> {
    pub top: u32,
    pub rows: Option<&'a Terminal>,
}

impl<'a> Header<'a> {
    /// Just the rows of `rows`, without a band.
    pub fn rows(rows: &'a Terminal) -> Self {
        Self {
            top: 0,
            rows: Some(rows),
        }
    }

    /// Where the rows sit, given the window-level `layout`: below the band.
    pub fn layout(self, layout: Layout) -> Layout {
        Layout {
            top: layout.top + self.top,
            ..layout
        }
    }

    /// Where the grid sits, given the window-level `layout`: below the band
    /// and the rows.
    pub fn grid(self, layout: Layout) -> Layout {
        let rows = self.rows.map_or(0, |rows| rows.size().rows());
        self.layout(layout).below(rows)
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

/// A pane of the grid area: a terminal with its cell `0, 0` at the cell
/// `col`, `row` below the header (see [`Layout::at`]). Panes do not overlap.
#[derive(Debug, Clone, Copy)]
pub struct PaneView<'a> {
    /// Identifies the pane across frames (image textures are cached by it);
    /// never reused for another terminal.
    pub id: u64,
    pub terminal: &'a Terminal,
    pub col: u16,
    pub row: u16,
    /// The focused pane has a block cursor; the others an outline.
    pub focused: bool,
    /// 0.0..=1.0: how far the pane's colors are mixed toward the
    /// background (0 for the focused pane).
    pub dim: f32,
}

impl<'a> PaneView<'a> {
    /// `terminal` alone at the origin, focused and not dimmed.
    pub fn single(terminal: &'a Terminal) -> Self {
        Self {
            id: 0,
            terminal,
            col: 0,
            row: 0,
            focused: true,
            dim: 0.0,
        }
    }

    pub fn look(&self) -> Look {
        Look {
            focused: self.focused,
            dim: self.dim,
        }
    }
}

/// A [`Renderer`] whose style can change while it runs (config reload,
/// font zoom, display scale change) without recreating its surface.
pub trait WindowRenderer: Renderer {
    /// Replaces the font, colors and padding; takes effect on the next draw.
    fn set_style(&mut self, style: Style);
    /// Whether [`Style::background_opacity`] below 1.0 shows: the surface
    /// lets the system blend the window with what is behind it.
    fn translucent(&self) -> bool;
    /// Draws and presents one frame with the [`Header`] band and rows
    /// (such as the title bar and the tab bar) at the top, `panes` below
    /// them (see [`Header::grid`]), `overlay` on top of them and `shapes`
    /// over everything. [`Renderer::draw`] is this with one
    /// [`PaneView::single`] and no header. The cursors of the header and
    /// the overlay are drawn like any other.
    fn draw_layers(
        &mut self,
        header: Header<'_>,
        panes: &[PaneView<'_>],
        overlay: Option<Overlay<'_>>,
        shapes: &[Shape],
    ) -> Result<(), RenderError>;
}
