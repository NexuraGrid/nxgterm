//! CPU renderer: draws a terminal grid into a framebuffer.

use nxg_core::{Flags, Terminal};

use crate::frame::Frame;
use crate::images;
use crate::paint::{self, CellSize, Layout, Look};
use crate::shape;
use crate::shape::Shape;
use crate::style::{Overlay, PaneView, Style};

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
        self.render_layers(None, &[PaneView::single(term)], None, &[], frame);
    }

    /// Draws the rows of `header` at the top of the grid area, `panes`
    /// below them (see [`Layout::below`], [`Layout::at`]) and `overlay` over
    /// the grid and `shapes` over everything. The cursors of the
    /// header and the overlay are drawn like any other.
    pub fn render_layers(
        &mut self,
        header: Option<&Terminal>,
        panes: &[PaneView<'_>],
        overlay: Option<Overlay<'_>>,
        shapes: &[Shape],
        frame: &mut Frame<'_>,
    ) {
        let mut layout = self.layout();
        frame.clear(self.style.palette.background);
        if let Some(header) = header {
            self.paint(header, frame, layout, Look::ACTIVE);
            layout = layout.below(header.size().rows());
        }
        for pane in panes {
            let at = layout.at(pane.col, pane.row);
            self.paint(pane.terminal, frame, at, pane.look());
        }
        if let Some(overlay) = overlay {
            let at = layout.at(overlay.col, overlay.row);
            self.paint(overlay.terminal, frame, at, Look::ACTIVE);
        }
        shape::paint(shapes, frame);
    }

    /// Paints `term` at `layout` over what `frame` holds. Order: cell
    /// backgrounds, images with `z < 0`, cursor, glyphs, then the other
    /// images. Images are clipped to the terminal's own grid.
    fn paint(&mut self, term: &Terminal, frame: &mut Frame<'_>, layout: Layout, look: Look) {
        let palette = &self.style.palette;
        paint::paint_backgrounds(term, frame, layout, palette, look);
        images::paint(term, frame, layout, false);
        paint::paint_cursor(term, frame, layout, palette, look);
        let cursor = term.display_cursor();
        for row in 0..term.size().rows() {
            let selected = term.selected_cols(row);
            for (col, c) in term.display_row(row).iter().enumerate() {
                if c.ch == ' ' {
                    continue;
                }
                let selected = paint::is_selected(&selected, col);
                let (mut fg, bg) = paint::look_colors(c, selected, &self.style.palette, look);
                let on_block = look.focused && cursor.visible;
                if on_block && (cursor.col as usize, cursor.row) == (col, row) {
                    fg = bg;
                }
                let (x, y) = layout.origin(col as u32, u32::from(row));
                let (x, y) = (i64::from(x), i64::from(y));
                self.draw_glyph(frame, c.ch, c.flags.contains(Flags::BOLD), x, y, fg);
            }
        }
        images::paint(term, frame, layout, true);
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
    use crate::style::PaneView;
    use nxg_core::TermSize;

    fn style(padding: u32) -> Option<Style> {
        let font = Font::system(DEFAULT_PX).ok()?;
        Some(Style {
            font,
            palette: Palette::default(),
            padding,
            background_opacity: 1.0,
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
    fn draws_a_glyph_missing_from_the_font_from_a_fallback_face() {
        use crate::font::parse_face;
        use crate::test_font::box_font;
        const ICON: char = '\u{e5ff}';
        let primary = || Font::from_bytes(box_font(&['M'], 600), 0, None, 20.0).unwrap();
        let fallback = parse_face(box_font(&[ICON], 600), 0).unwrap();
        let ink = |font: Font| {
            let mut renderer = CpuRenderer::new(Style {
                font,
                palette: Palette::default(),
                padding: 0,
                background_opacity: 1.0,
            });
            let cell = renderer.cell_size();
            let mut term = Terminal::new(TermSize::new(1, 1).unwrap());
            term.advance("\x1b[?25l\u{e5ff}".as_bytes());
            let mut pixels = vec![0; (cell.width * cell.height) as usize];
            let mut frame = Frame::new(&mut pixels, cell.width, cell.height).unwrap();
            renderer.render(&term, &mut frame);
            let bg = Palette::default().background;
            pixels.iter().filter(|&&pixel| pixel != bg).count()
        };
        assert_eq!(ink(primary()), 0, "the primary font has no icon");
        let with_fallback = primary().with_fallbacks(vec![fallback].into());
        assert!(ink(with_fallback) > 0, "the fallback face draws it");
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
    fn a_header_takes_the_top_rows_and_pushes_the_grid_down() {
        let Some(style) = style(1) else { return };
        let mut renderer = CpuRenderer::new(style);
        let cell = renderer.cell_size();
        let mut header = Terminal::new(TermSize::new(1, 1).unwrap());
        header.advance(b"\x1b[?25l\x1b[41m \x1b[0m");
        let mut term = Terminal::new(TermSize::new(1, 1).unwrap());
        term.advance(b"\x1b[?25l\x1b[44m \x1b[0m");
        let layout = renderer.layout().below(1);
        let (w, h) = layout.window_size(term.size());
        assert_eq!(h, 2 * cell.height + 2);
        let mut pixels = vec![0; (w * h) as usize];
        let mut frame = Frame::new(&mut pixels, w, h).unwrap();
        renderer.render_layers(
            Some(&header),
            &[PaneView::single(&term)],
            None,
            &[],
            &mut frame,
        );
        let palette = Palette::default();
        assert_eq!(frame.pixel(1, 1), Some(palette.ansi[1]), "header row");
        assert_eq!(
            frame.pixel(1, 1 + cell.height),
            Some(palette.ansi[4]),
            "grid below the header"
        );
        assert_eq!(frame.pixel(0, 0), Some(palette.background), "padding");
    }

    #[test]
    fn an_overlay_covers_the_grid_at_its_cell_with_opaque_backgrounds() {
        let Some(style) = style(1) else { return };
        let mut renderer = CpuRenderer::new(style);
        let mut header = Terminal::new(TermSize::new(3, 1).unwrap());
        header.advance(b"\x1b[?25l");
        let mut term = Terminal::new(TermSize::new(3, 2).unwrap());
        term.advance(b"\x1b[?25l\x1b[44m      \x1b[0m");
        // Default background on the left, red on the right.
        let mut overlay = Terminal::new(TermSize::new(2, 1).unwrap());
        overlay.advance(b"\x1b[?25l \x1b[41m \x1b[0m");
        let layout = renderer.layout().below(1);
        let (w, h) = layout.window_size(term.size());
        let mut pixels = vec![0; (w * h) as usize];
        let mut frame = Frame::new(&mut pixels, w, h).unwrap();
        let overlay = Overlay {
            terminal: &overlay,
            col: 1,
            row: 1,
        };
        renderer.render_layers(
            Some(&header),
            &[PaneView::single(&term)],
            Some(overlay),
            &[],
            &mut frame,
        );
        let palette = Palette::default();
        let at = |col: u32, row: u32| {
            let (x, y) = layout.origin(col, row);
            frame.pixel(x, y)
        };
        assert_eq!(at(0, 1), Some(palette.ansi[4]), "grid left of the overlay");
        assert_eq!(at(1, 0), Some(palette.ansi[4]), "grid above the overlay");
        assert_eq!(at(1, 1), Some(palette.background), "opaque default cell");
        assert_eq!(at(2, 1), Some(palette.ansi[1]), "overlay colors");
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

    /// Renders "W" with an opaque red image over its cell at `z`.
    fn render_glyph_with_image(z: i32) -> Option<(Vec<u32>, u32, u32)> {
        let mut renderer = CpuRenderer::new(style(0)?);
        let cell = renderer.cell_size();
        let mut term = Terminal::new(TermSize::new(2, 1).unwrap());
        term.set_cell_pixels(cell.width, cell.height);
        // Exactly one cell of pixels at native size.
        let (cw, ch) = (cell.width, cell.height);
        let data =
            crate::images::tests::encode_base64(&[255, 0, 0, 255].repeat((cw * ch) as usize));
        let image = format!("\x1b_Ga=T,s={cw},v={ch},C=1,z={z};{data}\x1b\\");
        term.advance(format!("\x1b[?25lW\x1b[H{image}").as_bytes());
        let (w, h) = (cell.width * 2, cell.height);
        let mut pixels = vec![0; (w * h) as usize];
        renderer.render(&term, &mut Frame::new(&mut pixels, w, h).unwrap());
        Some((pixels, w, cell.width))
    }

    #[test]
    fn negative_z_images_go_below_text_and_others_above() {
        let Some((below, w, cell_w)) = render_glyph_with_image(-1) else {
            return;
        };
        let red = rgb(255, 0, 0);
        let first_cell = |pixels: &[u32]| -> Vec<u32> {
            pixels
                .iter()
                .enumerate()
                .filter(|(i, _)| (*i as u32 % w) < cell_w)
                .map(|(_, &p)| p)
                .collect()
        };
        let cell = first_cell(&below);
        assert!(cell.contains(&red), "image visible behind the glyph");
        assert!(cell.iter().any(|&p| p != red), "glyph drawn over the image");
        let (above, _, _) = render_glyph_with_image(0).unwrap();
        assert!(
            first_cell(&above).iter().all(|&p| p == red),
            "image covers the glyph"
        );
    }

    /// A style whose font draws every `M` as a solid box, whatever fonts
    /// the machine has.
    fn box_style() -> Style {
        let font = Font::from_bytes(crate::test_font::box_font(&['M'], 600), 0, None, 20.0);
        Style {
            font: font.unwrap(),
            palette: Palette::default(),
            padding: 0,
            background_opacity: 1.0,
        }
    }

    fn pane(terminal: &Terminal, id: u64, col: u16) -> PaneView<'_> {
        PaneView {
            id,
            col,
            ..PaneView::single(terminal)
        }
    }

    fn draw(
        renderer: &mut CpuRenderer,
        panes: &[PaneView<'_>],
        shapes: &[Shape],
        cols: u32,
    ) -> Vec<u32> {
        let cell = renderer.cell_size();
        let (w, h) = (cell.width * cols, cell.height);
        let mut pixels = vec![0; (w * h) as usize];
        let mut frame = Frame::new(&mut pixels, w, h).unwrap();
        renderer.render_layers(None, panes, None, shapes, &mut frame);
        pixels
    }

    #[test]
    fn two_panes_draw_inside_their_rects_and_the_divider_goes_over() {
        let mut renderer = CpuRenderer::new(box_style());
        let cell = renderer.cell_size();
        let mut left = Terminal::new(TermSize::new(2, 1).unwrap());
        left.advance(b"\x1b[?25l\x1b[41m  ");
        let mut right = Terminal::new(TermSize::new(2, 1).unwrap());
        right.advance(b"\x1b[?25l\x1b[44m  ");
        let green = rgb(0, 255, 0);
        let line_x = 2 * cell.width + cell.width / 2;
        let divider = Shape::Rect {
            x: line_x as i32,
            y: 0,
            width: 1,
            height: cell.height,
            color: green,
        };
        let panes = [pane(&left, 1, 0), pane(&right, 2, 3)];
        let pixels = draw(&mut renderer, &panes, &[divider], 5);
        let w = (5 * cell.width) as usize;
        let at = |x: u32| pixels[x as usize + w * (cell.height as usize / 2)];
        let palette = Palette::default();
        assert!(
            (0..2 * cell.width).all(|x| at(x) == palette.ansi[1]),
            "left pane"
        );
        assert!(
            (3 * cell.width..5 * cell.width).all(|x| at(x) == palette.ansi[4]),
            "right pane"
        );
        assert_eq!(at(2 * cell.width), palette.background, "divider cell");
        assert_eq!(at(line_x), green, "divider line");
        assert_eq!(at(line_x + 1), palette.background);
        // Each pane looks like it does alone.
        let alone = draw(&mut renderer, &[PaneView::single(&left)], &[], 2);
        let alone_w = (2 * cell.width) as usize;
        for y in 0..cell.height as usize {
            assert_eq!(
                pixels[y * w..y * w + alone_w],
                alone[y * alone_w..(y + 1) * alone_w]
            );
        }
    }

    #[test]
    fn an_unfocused_pane_has_a_hollow_cursor_and_keeps_the_glyph_color() {
        let mut renderer = CpuRenderer::new(box_style());
        let cell = renderer.cell_size();
        let mut term = Terminal::new(TermSize::new(1, 1).unwrap());
        term.advance(b"M\x1b[D"); // The cursor sits on the glyph.
        let palette = Palette::default();
        let center = ((cell.height / 2 * cell.width) + cell.width / 2) as usize;
        let focused = draw(&mut renderer, &[PaneView::single(&term)], &[], 1);
        assert_eq!(focused[0], palette.cursor, "block cursor");
        assert_eq!(
            focused[center], palette.background,
            "glyph inverted over the block"
        );
        let inactive = PaneView {
            focused: false,
            ..PaneView::single(&term)
        };
        let hollow = draw(&mut renderer, &[inactive], &[], 1);
        assert_eq!(hollow[0], palette.cursor, "outline corner");
        assert_eq!(hollow[center], palette.foreground, "glyph keeps its color");
        let inside = (cell.width + 1) as usize; // Second row, second pixel.
        assert_eq!(hollow[inside], palette.background, "center unfilled");
    }

    #[test]
    fn inactive_panes_are_dimmed_and_a_dim_of_zero_changes_nothing() {
        let mut renderer = CpuRenderer::new(box_style());
        let cell = renderer.cell_size();
        let mut term = Terminal::new(TermSize::new(2, 1).unwrap());
        term.advance(b"\x1b[?25l\x1b[41mM\x1b[0m");
        let palette = Palette::default();
        let inactive = |dim| PaneView {
            focused: false,
            dim,
            ..PaneView::single(&term)
        };
        let center = ((cell.height / 2 * cell.width * 2) + cell.width / 2) as usize;
        let dimmed = draw(&mut renderer, &[inactive(0.5)], &[], 2);
        let bg = palette.background;
        assert_eq!(
            dimmed[center],
            paint::dim(palette.foreground, bg, 0.5),
            "glyph"
        );
        assert_eq!(
            dimmed[0],
            paint::dim(palette.ansi[1], bg, 0.5),
            "background"
        );
        let plain = draw(&mut renderer, &[PaneView::single(&term)], &[], 2);
        assert_eq!(plain[center], palette.foreground);
        assert_eq!(
            draw(&mut renderer, &[inactive(0.0)], &[], 2),
            plain,
            "dim 0"
        );
    }
}
