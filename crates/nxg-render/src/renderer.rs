//! CPU renderer: draws a terminal grid into a framebuffer.

use nxg_core::{Flags, Terminal};

use crate::frame::Frame;
use crate::paint::{self, CellSize, Layout};
use crate::style::Style;

/// Software renderer for the terminal grid.
#[derive(Debug)]
pub struct CpuRenderer {
    style: Style,
}

impl CpuRenderer {
    pub fn new(style: Style) -> Self {
        Self { style }
    }

    pub fn cell_size(&self) -> CellSize {
        self.style.font.cell_size()
    }

    pub fn layout(&self) -> Layout {
        self.style.layout()
    }

    pub fn set_style(&mut self, style: Style) {
        self.style = style;
    }

    /// Draws `term` into `frame`; pixels outside the grid (the padding
    /// included) get the background.
    pub fn render(&mut self, term: &Terminal, frame: &mut Frame<'_>) {
        let layout = self.layout();
        let palette = &self.style.palette;
        frame.clear(palette.background);
        paint::paint_backgrounds(term, frame, layout, palette);
        paint::paint_cursor(term, frame, layout, palette);
        let cursor = term.cursor();
        for row in 0..term.size().rows() {
            for (col, c) in term.row(row).iter().enumerate() {
                if c.ch == ' ' {
                    continue;
                }
                let (mut fg, bg) = paint::cell_colors(c, &self.style.palette);
                if cursor.visible && (cursor.col as usize, cursor.row) == (col, row) {
                    fg = bg;
                }
                let (x, y) = layout.origin(col as u32, u32::from(row));
                let (x, y) = (i64::from(x), i64::from(y));
                self.draw_glyph(frame, c.ch, c.flags.contains(Flags::BOLD), x, y, fg);
            }
        }
    }

    fn draw_glyph(&mut self, frame: &mut Frame<'_>, ch: char, bold: bool, x: i64, y: i64, fg: u32) {
        let baseline = i64::from(self.style.font.baseline());
        let glyph = self.style.font.glyph(ch, bold);
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
    use crate::font::{DEFAULT_PX, Font};
    use crate::palette::{Palette, rgb};
    use nxg_core::TermSize;

    fn style(padding: u32) -> Option<Style> {
        let font = Font::system(DEFAULT_PX).ok()?;
        Some(Style {
            font,
            palette: Palette::default(),
            padding,
        })
    }

    #[test]
    fn draws_glyph_pixels_inside_its_cell() {
        let Some(style) = style(0) else { return };
        let mut renderer = CpuRenderer::new(style);
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

    #[test]
    fn padding_shifts_the_grid_and_keeps_the_border_blank() {
        let Some(style) = style(5) else { return };
        let mut renderer = CpuRenderer::new(style);
        let layout = renderer.layout();
        let mut term = Terminal::new(TermSize::new(1, 1).unwrap());
        term.advance(b"\x1b[41m \x1b[0m\x1b[?25l");
        let (w, h) = layout.window_size(term.size());
        let mut pixels = vec![0; (w * h) as usize];
        let mut frame = Frame::new(&mut pixels, w, h).unwrap();
        renderer.render(&term, &mut frame);
        let (bg, red) = (Palette::default().background, Palette::default().ansi[1]);
        assert_eq!(frame.pixel(4, 4), Some(bg), "padding");
        assert_eq!(frame.pixel(5, 5), Some(red), "first cell pixel");
        assert_eq!(
            frame.pixel(w - 5, h - 5),
            Some(bg),
            "padding after the cell"
        );
    }

    #[test]
    fn set_style_switches_colors_and_padding() {
        let Some(style) = style(0) else { return };
        let mut renderer = CpuRenderer::new(style.clone());
        let mut restyled = style;
        restyled.palette.background = rgb(1, 2, 3);
        restyled.padding = 2;
        renderer.set_style(restyled);
        assert_eq!(renderer.layout().padding, 2);
        let term = Terminal::new(TermSize::new(1, 1).unwrap());
        let mut pixels = vec![0; 4];
        let mut frame = Frame::new(&mut pixels, 2, 2).unwrap();
        renderer.render(&term, &mut frame);
        assert_eq!(frame.pixel(0, 0), Some(rgb(1, 2, 3)));
    }
}
