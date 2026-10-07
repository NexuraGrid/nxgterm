//! CPU renderer: draws a terminal grid into a framebuffer.

use nxg_core::{Flags, Terminal};

use crate::font::Font;
use crate::frame::Frame;
use crate::paint::{self, CellSize};
use crate::palette::Palette;

/// Software renderer for the terminal grid.
#[derive(Debug)]
pub struct CpuRenderer {
    font: Font,
    palette: Palette,
}

impl CpuRenderer {
    pub fn new(font: Font, palette: Palette) -> Self {
        Self { font, palette }
    }

    pub fn cell_size(&self) -> CellSize {
        self.font.cell_size()
    }

    /// Draws `term` into `frame`; pixels outside the grid get the background.
    pub fn render(&mut self, term: &Terminal, frame: &mut Frame<'_>) {
        let cell = self.cell_size();
        frame.clear(self.palette.background);
        paint::paint_backgrounds(term, frame, cell, &self.palette);
        paint::paint_cursor(term, frame, cell, &self.palette);
        let cursor = term.cursor();
        for row in 0..term.size().rows() {
            for (col, c) in term.row(row).iter().enumerate() {
                if c.ch == ' ' {
                    continue;
                }
                let (mut fg, bg) = paint::cell_colors(c, &self.palette);
                if cursor.visible && (cursor.col as usize, cursor.row) == (col, row) {
                    fg = bg;
                }
                let x = i64::from(col as u32 * cell.width);
                let y = i64::from(u32::from(row) * cell.height);
                self.draw_glyph(frame, c.ch, c.flags.contains(Flags::BOLD), x, y, fg);
            }
        }
    }

    fn draw_glyph(&mut self, frame: &mut Frame<'_>, ch: char, bold: bool, x: i64, y: i64, fg: u32) {
        let baseline = i64::from(self.font.baseline());
        let glyph = self.font.glyph(ch, bold);
        let left = x + i64::from(glyph.xmin);
        let top = y + baseline - i64::from(glyph.ymin) - glyph.height as i64;
        for (i, &alpha) in glyph.coverage.iter().enumerate() {
            if alpha > 0 {
                let gx = (i % glyph.width) as i64;
                let gy = (i / glyph.width) as i64;
                frame.blend(left + gx, top + gy, fg, alpha);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::DEFAULT_PX;
    use nxg_core::TermSize;

    #[test]
    fn draws_glyph_pixels_inside_its_cell() {
        let Ok(font) = Font::system(DEFAULT_PX) else {
            return;
        };
        let mut renderer = CpuRenderer::new(font, Palette::default());
        let cell = renderer.cell_size();
        let mut term = Terminal::new(TermSize::new(2, 1).unwrap());
        term.advance(b"\x1b[?25lW");
        let (w, h) = (cell.width * 2, cell.height);
        let mut pixels = vec![0; (w * h) as usize];
        let mut frame = Frame::new(&mut pixels, w, h).unwrap();
        renderer.render(&term, &mut frame);
        let bg = Palette::default().background;
        let inked =
            |x0: u32, x1: u32| (x0..x1).any(|x| (0..h).any(|y| frame.pixel(x, y) != Some(bg)));
        assert!(inked(0, cell.width), "first cell has the glyph");
        assert!(!inked(cell.width, w), "second cell stays blank");
    }
}
