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
    cell: CellSize,
    palette: &Palette,
) {
    for row in 0..term.size().rows() {
        for (col, c) in term.row(row).iter().enumerate() {
            let (_, bg) = cell_colors(c, palette);
            let x = col as u32 * cell.width;
            let y = u32::from(row) * cell.height;
            frame.fill_rect(x, y, cell.width, cell.height, bg);
        }
    }
}

/// Draws a block cursor when it is visible.
pub fn paint_cursor(term: &Terminal, frame: &mut Frame<'_>, cell: CellSize, palette: &Palette) {
    let cursor = term.cursor();
    if cursor.visible {
        let x = u32::from(cursor.col) * cell.width;
        let y = u32::from(cursor.row) * cell.height;
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
        paint_backgrounds(&term, &mut frame, CELL, &palette);
        let (a, b) = (rgb(1, 2, 3), palette.background);
        assert_eq!(pixels, [a, a, b, b, a, a, b, b]);
    }

    #[test]
    fn paints_visible_cursor_only() {
        let palette = Palette::default();
        let mut term = term(2, 1, b" ");
        let mut pixels = vec![0; 4 * 2];
        let mut frame = Frame::new(&mut pixels, 4, 2).unwrap();
        paint_cursor(&term, &mut frame, CELL, &palette);
        let c = palette.cursor;
        assert_eq!(pixels, [0, 0, c, c, 0, 0, c, c]);

        term.advance(b"\x1b[?25l");
        let mut pixels = vec![0; 4 * 2];
        let mut frame = Frame::new(&mut pixels, 4, 2).unwrap();
        paint_cursor(&term, &mut frame, CELL, &palette);
        assert!(pixels.iter().all(|&p| p == 0));
    }
}
