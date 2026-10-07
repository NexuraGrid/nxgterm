//! Font-independent drawing: cell geometry, colors, backgrounds and cursor.

use nxg_core::{Cell, Color, Flags, TermSize, Terminal};

use crate::frame::Frame;
use crate::palette::{Palette, Rgb};

/// Size of one character cell in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellSize {
    pub width: u32,
    pub height: u32,
}

impl CellSize {
    /// How many whole cells fit in `width x height` pixels (at least 1x1).
    pub fn grid_size(self, width: u32, height: u32) -> TermSize {
        let fit = |pixels: u32, cell: u32| {
            u16::try_from(pixels / cell.max(1))
                .unwrap_or(u16::MAX)
                .max(1)
        };
        TermSize::new(fit(width, self.width), fit(height, self.height))
            .expect("dimensions are clamped to at least 1")
    }
}

/// Where the grid sits in the window: cells inset by `padding` pixels on
/// every side, and pushed down by `top` more pixels (rows drawn above the
/// grid, such as the tab bar).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub cell: CellSize,
    pub padding: u32,
    pub top: u32,
}

impl Layout {
    /// Top-left pixel of the cell at `col`, `row`.
    pub fn origin(self, col: u32, row: u32) -> (u32, u32) {
        (
            self.padding + col * self.cell.width,
            self.padding + self.top + row * self.cell.height,
        )
    }

    /// The layout for a grid below `rows` rows of this one, for a header
    /// such as the tab bar drawn at this layout.
    pub fn below(self, rows: u16) -> Self {
        Self {
            top: self.top + u32::from(rows) * self.cell.height,
            ..self
        }
    }

    /// How many whole cells fit in a `width x height` window once the
    /// padding and the top are taken out (at least 1x1).
    pub fn grid_size(self, width: u32, height: u32) -> TermSize {
        let inset = self.padding.saturating_mul(2);
        self.cell.grid_size(
            width.saturating_sub(inset),
            height.saturating_sub(inset).saturating_sub(self.top),
        )
    }

    /// Window size in pixels that fits `size` cells plus the padding and the
    /// top.
    pub fn window_size(self, size: TermSize) -> (u32, u32) {
        let inset = self.padding.saturating_mul(2);
        let span =
            |cells: u16, cell: u32| u32::from(cells).saturating_mul(cell).saturating_add(inset);
        (
            span(size.cols(), self.cell.width),
            span(size.rows(), self.cell.height).saturating_add(self.top),
        )
    }
}

/// Foreground and background pixels for `cell`, applying inverse video and
/// bold-as-bright for the first eight ANSI colors.
pub fn cell_colors(cell: &Cell, palette: &Palette) -> (Rgb, Rgb) {
    let fg_color = match cell.fg {
        Color::Indexed(n @ 0..=7) if cell.flags.contains(Flags::BOLD) => Color::Indexed(n + 8),
        other => other,
    };
    let fg = palette.resolve(fg_color, palette.foreground);
    let bg = palette.resolve(cell.bg, palette.background);
    if cell.flags.contains(Flags::INVERSE) {
        (bg, fg)
    } else {
        (fg, bg)
    }
}

/// Fills every cell with its background color.
pub fn paint_backgrounds(
    term: &Terminal,
    frame: &mut Frame<'_>,
    layout: Layout,
    palette: &Palette,
) {
    let cell = layout.cell;
    for row in 0..term.size().rows() {
        for (col, c) in term.display_row(row).iter().enumerate() {
            let (_, bg) = cell_colors(c, palette);
            let (x, y) = layout.origin(col as u32, u32::from(row));
            frame.fill_rect(x, y, cell.width, cell.height, bg);
        }
    }
}

/// Draws a block cursor when it is visible.
pub fn paint_cursor(term: &Terminal, frame: &mut Frame<'_>, layout: Layout, palette: &Palette) {
    let cursor = term.display_cursor();
    if cursor.visible {
        let (x, y) = layout.origin(u32::from(cursor.col), u32::from(cursor.row));
        let cell = layout.cell;
        frame.fill_rect(x, y, cell.width, cell.height, palette.cursor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::rgb;

    const CELL: CellSize = CellSize {
        width: 2,
        height: 2,
    };
    const FLUSH: Layout = Layout {
        cell: CELL,
        padding: 0,
        top: 0,
    };

    fn term(cols: u16, rows: u16, input: &[u8]) -> Terminal {
        let mut term = Terminal::new(TermSize::new(cols, rows).unwrap());
        term.advance(input);
        term
    }

    #[test]
    fn grid_size_counts_whole_cells_and_never_hits_zero() {
        let cell = CellSize {
            width: 8,
            height: 16,
        };
        assert_eq!(cell.grid_size(800, 480), TermSize::new(100, 30).unwrap());
        assert_eq!(cell.grid_size(807, 495), TermSize::new(100, 30).unwrap());
        assert_eq!(cell.grid_size(3, 3), TermSize::new(1, 1).unwrap());
    }

    #[test]
    fn layout_origin_is_offset_by_padding() {
        let layout = Layout {
            cell: CELL,
            padding: 3,
            top: 0,
        };
        assert_eq!(layout.origin(0, 0), (3, 3));
        assert_eq!(layout.origin(2, 1), (7, 5));
        let flush = Layout {
            cell: CELL,
            padding: 0,
            top: 0,
        };
        assert_eq!(flush.origin(2, 1), (4, 2));
    }

    #[test]
    fn layout_grid_size_subtracts_padding_on_both_sides() {
        let cell = CellSize {
            width: 8,
            height: 16,
        };
        let layout = Layout {
            cell,
            padding: 4,
            top: 0,
        };
        assert_eq!(layout.grid_size(808, 488), TermSize::new(100, 30).unwrap());
        assert_eq!(layout.grid_size(807, 487), TermSize::new(99, 29).unwrap());
        // Smaller than the padding itself: still at least one cell.
        assert_eq!(layout.grid_size(5, 5), TermSize::new(1, 1).unwrap());
    }

    #[test]
    fn layout_window_size_round_trips_with_grid_size() {
        let cell = CellSize {
            width: 9,
            height: 19,
        };
        let layout = Layout {
            cell,
            padding: 6,
            top: 0,
        };
        let size = TermSize::new(100, 30).unwrap();
        assert_eq!(layout.window_size(size), (912, 582));
        let (w, h) = layout.window_size(size);
        assert_eq!(layout.grid_size(w, h), size);
    }

    #[test]
    fn top_pushes_the_grid_down_and_takes_rows_away() {
        let cell = CellSize {
            width: 8,
            height: 16,
        };
        let layout = Layout {
            cell,
            padding: 4,
            top: 16,
        };
        assert_eq!(layout.origin(0, 0), (4, 20));
        assert_eq!(layout.origin(1, 2), (12, 52));
        assert_eq!(layout.grid_size(808, 504), TermSize::new(100, 30).unwrap());
        let size = TermSize::new(100, 30).unwrap();
        assert_eq!(layout.window_size(size), (808, 504));
        // Not even one row left under the top: still one row.
        assert_eq!(layout.grid_size(808, 20), TermSize::new(100, 1).unwrap());
        assert_eq!(layout.below(2).top, 48, "below adds whole rows");
    }

    #[test]
    fn colors_default_to_theme_and_swap_on_inverse() {
        let palette = Palette::default();
        let mut cell = Cell::default();
        assert_eq!(
            cell_colors(&cell, &palette),
            (palette.foreground, palette.background)
        );
        cell.flags.insert(Flags::INVERSE);
        assert_eq!(
            cell_colors(&cell, &palette),
            (palette.background, palette.foreground)
        );
    }

    #[test]
    fn bold_brightens_basic_ansi_foreground() {
        let palette = Palette::default();
        let mut cell = Cell {
            fg: Color::Indexed(1),
            ..Cell::default()
        };
        cell.flags.insert(Flags::BOLD);
        assert_eq!(cell_colors(&cell, &palette).0, palette.ansi[9]);
    }

    #[test]
    fn paints_cell_backgrounds() {
        let palette = Palette::default();
        let term = term(2, 1, b"\x1b[48;2;1;2;3m \x1b[0m");
        let mut pixels = vec![0; 4 * 2];
        let mut frame = Frame::new(&mut pixels, 4, 2).unwrap();
        paint_backgrounds(&term, &mut frame, FLUSH, &palette);
        let (a, b) = (rgb(1, 2, 3), palette.background);
        assert_eq!(pixels, [a, a, b, b, a, a, b, b]);
    }

    #[test]
    fn padding_offsets_backgrounds_and_cursor() {
        let palette = Palette::default();
        let term = term(1, 1, b"\x1b[48;2;1;2;3m \x1b[0m");
        let layout = Layout {
            cell: CELL,
            padding: 1,
            top: 0,
        };
        let mut pixels = vec![0; 4 * 4];
        let mut frame = Frame::new(&mut pixels, 4, 4).unwrap();
        paint_backgrounds(&term, &mut frame, layout, &palette);
        let a = rgb(1, 2, 3);
        #[rustfmt::skip]
        assert_eq!(pixels, [
            0, 0, 0, 0,
            0, a, a, 0,
            0, a, a, 0,
            0, 0, 0, 0,
        ]);

        let term = self::term(1, 1, b"");
        let mut pixels = vec![0; 4 * 4];
        let mut frame = Frame::new(&mut pixels, 4, 4).unwrap();
        paint_cursor(&term, &mut frame, layout, &palette);
        let c = palette.cursor;
        assert_eq!(pixels[5..7], [c, c]);
        assert_eq!(pixels[0], 0);
    }

    #[test]
    fn paints_visible_cursor_only() {
        let palette = Palette::default();
        let mut term = term(2, 1, b" ");
        let mut pixels = vec![0; 4 * 2];
        let mut frame = Frame::new(&mut pixels, 4, 2).unwrap();
        paint_cursor(&term, &mut frame, FLUSH, &palette);
        let c = palette.cursor;
        assert_eq!(pixels, [0, 0, c, c, 0, 0, c, c]);

        term.advance(b"\x1b[?25l");
        let mut pixels = vec![0; 4 * 2];
        let mut frame = Frame::new(&mut pixels, 4, 2).unwrap();
        paint_cursor(&term, &mut frame, FLUSH, &palette);
        assert!(pixels.iter().all(|&p| p == 0));
    }

    #[test]
    fn scrolled_back_viewport_paints_history_and_no_cursor() {
        let palette = Palette::default();
        let mut term = term(2, 1, b"\x1b[48;2;1;2;3m  \x1b[0m\r\n");
        term.scroll_display(1);
        let mut pixels = vec![0; 4 * 2];
        let mut frame = Frame::new(&mut pixels, 4, 2).unwrap();
        paint_backgrounds(&term, &mut frame, FLUSH, &palette);
        paint_cursor(&term, &mut frame, FLUSH, &palette);
        assert_eq!(pixels, [rgb(1, 2, 3); 8]);
    }
}
