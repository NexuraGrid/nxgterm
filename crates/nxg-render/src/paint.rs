//! Font-independent drawing: cell geometry, colors, backgrounds and cursor.

use nxg_core::{Cell, Color, Cursor, Flags, TermSize, Terminal, Underline};

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
/// every side, and pushed right by `left` and down by `top` more pixels
/// (rows drawn above the grid, such as the tab bar, or an overlay placed
/// at a cell of the grid).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub cell: CellSize,
    pub padding: u32,
    pub left: u32,
    pub top: u32,
}

impl Layout {
    /// Top-left pixel of the cell at `col`, `row`.
    pub fn origin(self, col: u32, row: u32) -> (u32, u32) {
        (
            self.padding + self.left + col * self.cell.width,
            self.padding + self.top + row * self.cell.height,
        )
    }

    /// The layout whose cell `0, 0` is this one's cell `col`, `row`, for
    /// an overlay drawn over the grid.
    pub fn at(self, col: u16, row: u16) -> Self {
        Self {
            left: self.left + u32::from(col) * self.cell.width,
            ..self.below(row)
        }
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
            width.saturating_sub(inset).saturating_sub(self.left),
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
            span(size.cols(), self.cell.width).saturating_add(self.left),
            span(size.rows(), self.cell.height).saturating_add(self.top),
        )
    }
}

/// Foreground and background pixels for `cell`, applying bold-as-bright
/// for the first eight ANSI colors, dim (the foreground halfway to the
/// background), inverse video, then hidden (the foreground takes the
/// background).
pub fn cell_colors(cell: &Cell, palette: &Palette) -> (Rgb, Rgb) {
    let fg_color = match cell.fg {
        Color::Indexed(n @ 0..=7) if cell.flags.contains(Flags::BOLD) => Color::Indexed(n + 8),
        other => other,
    };
    let mut fg = palette.resolve(fg_color, palette.foreground);
    let bg = palette.resolve(cell.bg, palette.background);
    if cell.flags.contains(Flags::DIM) {
        fg = dim(fg, bg, 0.5);
    }
    let (fg, bg) = if cell.flags.contains(Flags::INVERSE) {
        (bg, fg)
    } else {
        (fg, bg)
    };
    if cell.flags.contains(Flags::HIDDEN) {
        (bg, bg)
    } else {
        (fg, bg)
    }
}

/// Whether `flags` draw a line over or under the cell (see
/// [`decorations`]).
pub fn is_decorated(flags: Flags) -> bool {
    flags.contains(Flags::UNDERLINE)
        || flags.contains(Flags::STRIKE)
        || flags.contains(Flags::OVERLINE)
}

/// The lines `flags` draw in a `cell` whose text sits on `baseline`, as
/// `(x, y, width, height)` offsets inside the cell, drawn in the
/// foreground color: the underline in its style below the baseline, the
/// strikethrough across the middle of the lowercase letters, the overline
/// along the top. Lines are `cell.height / 14` pixels thick (at least 1).
pub fn decorations(flags: Flags, cell: CellSize, baseline: i32) -> Vec<(u32, u32, u32, u32)> {
    let mut rects = Vec::new();
    let (w, h) = (cell.width, cell.height);
    if w == 0 || h == 0 {
        return rects;
    }
    let t = (h / 14).clamp(1, h);
    let bottom = h - t;
    let baseline = u32::try_from(baseline).unwrap_or(0).min(bottom);
    let under = (baseline + 1).min(bottom);
    match flags.underline() {
        Underline::None => {}
        Underline::Single => rects.push((0, under, w, t)),
        Underline::Double => {
            let second = (under + 2 * t).min(bottom);
            rects.push((0, second.saturating_sub(2 * t), w, t));
            rects.push((0, second, w, t));
        }
        Underline::Curly => {
            // One wave per cell, `t` up and down around its middle row.
            let middle = (under + t).min(bottom.saturating_sub(t)).max(t.min(bottom));
            for x in 0..w {
                let phase = std::f32::consts::TAU * (x as f32 + 0.5) / w as f32;
                let y = middle as f32 - t as f32 * phase.sin();
                rects.push((x, (y.round().max(0.0) as u32).min(bottom), 1, t));
            }
        }
        Underline::Dotted => {
            for x in (0..w).step_by(2 * t as usize) {
                rects.push((x, under, t.min(w - x), t));
            }
        }
        Underline::Dashed => {
            let dash = (w * 3 / 8).max(1);
            rects.push((0, under, dash, t));
            rects.push((w / 2, under, dash.min(w - w / 2), t));
        }
    }
    if flags.contains(Flags::STRIKE) {
        rects.push((0, (baseline - baseline / 3).min(bottom), w, t));
    }
    if flags.contains(Flags::OVERLINE) {
        rects.push((0, 0, w, t));
    }
    rects
}

/// [`cell_colors`] for a cell drawn `selected` or not. Selected cells take
/// the palette's selection colors; without a selection background their
/// own colors are swapped first, so the selection shows as inverse video.
pub fn shown_colors(cell: &Cell, selected: bool, palette: &Palette) -> (Rgb, Rgb) {
    let (fg, bg) = cell_colors(cell, palette);
    if !selected {
        return (fg, bg);
    }
    let (fg, bg) = match palette.selection_background {
        Some(_) => (fg, bg),
        None => (bg, fg),
    };
    (
        palette.selection_foreground.unwrap_or(fg),
        palette.selection_background.unwrap_or(bg),
    )
}

/// Whether column `col` is in the `selected` columns of its row.
pub fn is_selected(selected: &Option<std::ops::Range<u16>>, col: usize) -> bool {
    selected
        .as_ref()
        .is_some_and(|cols| cols.contains(&(col as u16)))
}

/// How a pane is drawn: whether it has the focus (an unfocused one shows a
/// hollow cursor) and how far its colors are mixed toward the background.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub focused: bool,
    /// 0.0 leaves the colors alone, 1.0 turns them into the background.
    pub dim: f32,
}

impl Look {
    /// Focused and not dimmed: how single terminals, headers and overlays
    /// are drawn.
    pub const ACTIVE: Self = Self {
        focused: true,
        dim: 0.0,
    };
}

/// `color` mixed toward `background` by `amount` (clamped to 0.0..=1.0;
/// NaN leaves it alone), channel by channel.
pub fn dim(color: Rgb, background: Rgb, amount: f32) -> Rgb {
    if amount.is_nan() || amount <= 0.0 {
        return color;
    }
    let amount = amount.min(1.0);
    let mix = |shift: u32| {
        let (c, b) = ((color >> shift) & 0xff, (background >> shift) & 0xff);
        let mixed = (c as f32 + (b as f32 - c as f32) * amount).round() as u32;
        mixed.min(255) << shift
    };
    mix(16) | mix(8) | mix(0)
}

/// [`shown_colors`] mixed toward the palette background by the look's dim.
pub fn look_colors(cell: &Cell, selected: bool, palette: &Palette, look: Look) -> (Rgb, Rgb) {
    let (fg, bg) = shown_colors(cell, selected, palette);
    (
        dim(fg, palette.background, look.dim),
        dim(bg, palette.background, look.dim),
    )
}

/// Fills every cell with its background color.
pub fn paint_backgrounds(
    term: &Terminal,
    frame: &mut Frame<'_>,
    layout: Layout,
    palette: &Palette,
    look: Look,
) {
    let cell = layout.cell;
    for row in 0..term.size().rows() {
        let selected = term.selected_cols(row);
        for (col, c) in term.display_row(row).iter().enumerate() {
            let (_, bg) = look_colors(c, is_selected(&selected, col), palette, look);
            let (x, y) = layout.origin(col as u32, u32::from(row));
            frame.fill_rect(x, y, cell.width, cell.height, bg);
        }
    }
}

/// Whether `cells[col]` is a wide char with room for both of its cells
/// (a history line wider than the grid may cut it at the edge).
pub fn is_wide(cells: &[Cell], col: usize) -> bool {
    col + 1 < cells.len() && cells[col].flags.contains(Flags::WIDE)
}

/// The chars drawn in `cell`, in order: its char unless blank (spaces and
/// wide-char spacers), then the combining marks drawn over it.
pub fn glyph_chars(cell: &Cell) -> impl Iterator<Item = char> {
    std::iter::once(cell.ch)
        .filter(|&ch| ch != ' ')
        .chain(cell.marks.iter())
}

/// How many cells the cursor covers: two over a wide char, else one.
pub fn cursor_cells(term: &Terminal, cursor: Cursor) -> u32 {
    let cells = term.display_row(cursor.row);
    1 + u32::from(is_wide(cells, usize::from(cursor.col)))
}

/// The four edges of a cursor outline as `(x, y, width, height)` offsets
/// inside the cell: 1 px thick, 2 px from cell height 24.
pub fn cursor_outline(cell: CellSize) -> [(u32, u32, u32, u32); 4] {
    let t = if cell.height >= 24 { 2 } else { 1 }
        .min(cell.width)
        .min(cell.height);
    let (w, h) = (cell.width, cell.height);
    let side = h.saturating_sub(2 * t);
    [
        (0, 0, w, t),
        (0, h - t, w, t),
        (0, t, t, side),
        (w - t, t, t, side),
    ]
}

/// Draws the cursor when it is visible: a block, or an outline when the
/// pane is not focused, over both cells of a wide char.
pub fn paint_cursor(
    term: &Terminal,
    frame: &mut Frame<'_>,
    layout: Layout,
    palette: &Palette,
    look: Look,
) {
    let cursor = term.display_cursor();
    if !cursor.visible {
        return;
    }
    let (x, y) = layout.origin(u32::from(cursor.col), u32::from(cursor.row));
    let cell = CellSize {
        width: layout.cell.width * cursor_cells(term, cursor),
        ..layout.cell
    };
    if look.focused {
        frame.fill_rect(x, y, cell.width, cell.height, palette.cursor);
        return;
    }
    for (dx, dy, w, h) in cursor_outline(cell) {
        frame.fill_rect(x + dx, y + dy, w, h, palette.cursor);
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
        left: 0,
        top: 0,
    };

    fn term(cols: u16, rows: u16, input: &[u8]) -> Terminal {
        let mut term = Terminal::new(TermSize::new(cols, rows).unwrap());
        term.advance(input);
        term
    }

    const DECO_CELL: CellSize = CellSize {
        width: 10,
        height: 20,
    };

    fn lines(sgr: &str) -> Vec<(u32, u32, u32, u32)> {
        let t = term(1, 1, format!("{sgr} ").as_bytes());
        decorations(t.row(0)[0].flags, DECO_CELL, 15)
    }

    fn inside(rects: &[(u32, u32, u32, u32)]) -> bool {
        rects.iter().all(|&(x, y, w, h)| {
            w > 0 && h > 0 && x + w <= DECO_CELL.width && y + h <= DECO_CELL.height
        })
    }

    #[test]
    fn decorations_are_lines_inside_the_cell() {
        assert!(lines("").is_empty(), "plain cells have none");
        let single = lines("\x1b[4m");
        assert_eq!(single.len(), 1);
        let (x, y, w, _) = single[0];
        assert_eq!((x, w), (0, 10), "full width");
        assert!(y >= 15, "below the baseline: {y}");
        let double = lines("\x1b[21m");
        assert_eq!(double.len(), 2);
        assert_ne!(double[0].1, double[1].1, "two rows");
        let strike = lines("\x1b[9m");
        assert!(strike.len() == 1 && strike[0].1 < 15 && strike[0].1 > 0);
        assert_eq!(lines("\x1b[53m"), [(0, 0, 10, 1)], "overline on top");
        for sgr in ["\x1b[4m", "\x1b[21m", "\x1b[4:3m", "\x1b[4:4m", "\x1b[4:5m"] {
            assert!(inside(&lines(sgr)), "{sgr:?}: {:?}", lines(sgr));
        }
        assert_eq!(lines("\x1b[4;9;53m").len(), 3, "all three at once");
    }

    #[test]
    fn styled_underlines_have_their_shape() {
        let curly = lines("\x1b[4:3m");
        let covered: u32 = curly.iter().map(|r| r.2).sum();
        assert_eq!(covered, 10, "a wave across every column");
        let rows: std::collections::HashSet<u32> = curly.iter().map(|r| r.1).collect();
        assert!(rows.len() > 1, "it goes up and down: {curly:?}");
        let dotted = lines("\x1b[4:4m");
        assert!(dotted.len() > 1 && dotted.iter().map(|r| r.2).sum::<u32>() < 10);
        let dashed = lines("\x1b[4:5m");
        assert_eq!(dashed.len(), 2, "two dashes per cell: {dashed:?}");
    }

    #[test]
    fn dim_mixes_the_foreground_halfway_and_hidden_hides_it() {
        let palette = Palette::default();
        let plain = term(1, 1, b"x");
        let (fg, bg) = cell_colors(&plain.row(0)[0], &palette);
        let dimmed = term(1, 1, b"\x1b[2mx");
        assert_eq!(
            cell_colors(&dimmed.row(0)[0], &palette),
            (dim(fg, bg, 0.5), bg)
        );
        let hidden = term(1, 1, b"\x1b[8mx");
        assert_eq!(cell_colors(&hidden.row(0)[0], &palette), (bg, bg));
        let inverse = term(1, 1, b"\x1b[7;8mx");
        assert_eq!(cell_colors(&inverse.row(0)[0], &palette), (fg, fg));
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
            left: 0,
            top: 0,
        };
        assert_eq!(layout.origin(0, 0), (3, 3));
        assert_eq!(layout.origin(2, 1), (7, 5));
        let flush = Layout {
            cell: CELL,
            padding: 0,
            left: 0,
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
            left: 0,
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
            left: 0,
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
            left: 0,
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
    fn at_moves_cell_zero_to_a_cell_of_the_grid() {
        let cell = CellSize {
            width: 8,
            height: 16,
        };
        let layout = Layout {
            cell,
            padding: 4,
            left: 0,
            top: 16,
        };
        let moved = layout.at(3, 2);
        assert_eq!(moved.origin(0, 0), layout.origin(3, 2));
        assert_eq!(moved.origin(1, 1), layout.origin(4, 3));
        let size = TermSize::new(10, 5).unwrap();
        let (w, h) = moved.window_size(size);
        assert_eq!((w, h), (4 + 24 + 80 + 4, 4 + 16 + 32 + 80 + 4));
        assert_eq!(moved.grid_size(w, h), size);
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
    fn selected_cells_swap_colors_unless_the_palette_has_selection_colors() {
        let mut palette = Palette::default();
        let cell = Cell::default();
        let (fg, bg) = (palette.foreground, palette.background);
        assert_eq!(shown_colors(&cell, false, &palette), (fg, bg));
        assert_eq!(shown_colors(&cell, true, &palette), (bg, fg));
        palette.selection_background = Some(rgb(1, 1, 1));
        assert_eq!(shown_colors(&cell, true, &palette), (fg, rgb(1, 1, 1)));
        palette.selection_foreground = Some(rgb(2, 2, 2));
        assert_eq!(
            shown_colors(&cell, true, &palette),
            (rgb(2, 2, 2), rgb(1, 1, 1))
        );
        palette.selection_background = None;
        assert_eq!(shown_colors(&cell, true, &palette), (rgb(2, 2, 2), fg));
    }

    #[test]
    fn selected_cells_paint_their_selection_background() {
        let palette = Palette::default();
        let mut term = term(3, 1, b"abc");
        term.start_selection(
            nxg_core::selection::SelectionKind::Simple,
            term.point_at(1, 0),
        );
        let mut pixels = vec![0; 6 * 2];
        let mut frame = Frame::new(&mut pixels, 6, 2).unwrap();
        paint_backgrounds(&term, &mut frame, FLUSH, &palette, Look::ACTIVE);
        let (b, f) = (palette.background, palette.foreground);
        assert_eq!(pixels[..6], [b, b, f, f, b, b]);
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
        paint_backgrounds(&term, &mut frame, FLUSH, &palette, Look::ACTIVE);
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
            left: 0,
            top: 0,
        };
        let mut pixels = vec![0; 4 * 4];
        let mut frame = Frame::new(&mut pixels, 4, 4).unwrap();
        paint_backgrounds(&term, &mut frame, layout, &palette, Look::ACTIVE);
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
        paint_cursor(&term, &mut frame, layout, &palette, Look::ACTIVE);
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
        paint_cursor(&term, &mut frame, FLUSH, &palette, Look::ACTIVE);
        let c = palette.cursor;
        assert_eq!(pixels, [0, 0, c, c, 0, 0, c, c]);

        term.advance(b"\x1b[?25l");
        let mut pixels = vec![0; 4 * 2];
        let mut frame = Frame::new(&mut pixels, 4, 2).unwrap();
        paint_cursor(&term, &mut frame, FLUSH, &palette, Look::ACTIVE);
        assert!(pixels.iter().all(|&p| p == 0));
    }

    #[test]
    fn the_cursor_covers_both_cells_of_a_wide_char() {
        let palette = Palette::default();
        let term = term(2, 1, "日\x1b[1G".as_bytes());
        let mut pixels = vec![0; 4 * 2];
        let mut frame = Frame::new(&mut pixels, 4, 2).unwrap();
        paint_cursor(&term, &mut frame, FLUSH, &palette, Look::ACTIVE);
        assert_eq!(pixels, [palette.cursor; 8]);
    }

    #[test]
    fn scrolled_back_viewport_paints_history_and_no_cursor() {
        let palette = Palette::default();
        let mut term = term(2, 1, b"\x1b[48;2;1;2;3m  \x1b[0m\r\n");
        term.scroll_display(1);
        let mut pixels = vec![0; 4 * 2];
        let mut frame = Frame::new(&mut pixels, 4, 2).unwrap();
        paint_backgrounds(&term, &mut frame, FLUSH, &palette, Look::ACTIVE);
        paint_cursor(&term, &mut frame, FLUSH, &palette, Look::ACTIVE);
        assert_eq!(pixels, [rgb(1, 2, 3); 8]);
    }

    #[test]
    fn dim_mixes_toward_the_background() {
        let (fg, bg) = (rgb(200, 100, 0), rgb(0, 100, 200));
        assert_eq!(dim(fg, bg, 0.0), fg, "zero is the identity");
        assert_eq!(dim(fg, bg, 1.0), bg, "one reaches the background");
        assert_eq!(dim(fg, bg, 0.5), rgb(100, 100, 100));
        assert_eq!(dim(fg, bg, 0.25), rgb(150, 100, 50));
        assert_eq!(dim(fg, bg, 7.0), bg, "above one clamps");
        assert_eq!(dim(fg, bg, -1.0), fg, "below zero clamps");
        assert_eq!(dim(fg, bg, f32::NAN), fg, "nan leaves the color alone");
    }

    #[test]
    fn look_colors_dim_both_colors_and_active_leaves_them() {
        let palette = Palette::default();
        let cell = Cell::default();
        let shown = shown_colors(&cell, false, &palette);
        assert_eq!(look_colors(&cell, false, &palette, Look::ACTIVE), shown);
        let look = Look {
            focused: false,
            dim: 0.5,
        };
        let bg = palette.background;
        assert_eq!(
            look_colors(&cell, false, &palette, look),
            (dim(palette.foreground, bg, 0.5), bg)
        );
        let red = Cell {
            bg: Color::Indexed(1),
            ..Cell::default()
        };
        assert_eq!(
            look_colors(&red, false, &palette, look).1,
            dim(palette.ansi[1], bg, 0.5)
        );
    }

    #[test]
    fn dimmed_backgrounds_are_painted_dimmed() {
        let palette = Palette::default();
        let term = term(1, 1, b"\x1b[48;2;200;0;0m \x1b[0m");
        let look = Look {
            focused: false,
            dim: 1.0,
        };
        let mut pixels = vec![0; 2 * 2];
        let mut frame = Frame::new(&mut pixels, 2, 2).unwrap();
        paint_backgrounds(&term, &mut frame, FLUSH, &palette, look);
        assert_eq!(pixels, [palette.background; 4]);
    }

    const BIG: Layout = Layout {
        cell: CellSize {
            width: 6,
            height: 6,
        },
        padding: 0,
        left: 0,
        top: 0,
    };
    const UNFOCUSED: Look = Look {
        focused: false,
        dim: 0.0,
    };

    #[test]
    fn an_unfocused_cursor_is_a_one_pixel_outline() {
        let palette = Palette::default();
        let term = term(1, 1, b"");
        let mut pixels = vec![0; 36];
        let mut frame = Frame::new(&mut pixels, 6, 6).unwrap();
        paint_cursor(&term, &mut frame, BIG, &palette, UNFOCUSED);
        let c = palette.cursor;
        for y in 0..6 {
            for x in 0..6 {
                let edge = x == 0 || y == 0 || x == 5 || y == 5;
                assert_eq!(pixels[y * 6 + x], if edge { c } else { 0 }, "({x}, {y})");
            }
        }
    }

    #[test]
    fn the_outline_is_two_pixels_on_tall_cells() {
        let palette = Palette::default();
        let layout = Layout {
            cell: CellSize {
                width: 8,
                height: 24,
            },
            ..BIG
        };
        let term = term(1, 1, b"");
        let mut pixels = vec![0; 8 * 24];
        let mut frame = Frame::new(&mut pixels, 8, 24).unwrap();
        paint_cursor(&term, &mut frame, layout, &palette, UNFOCUSED);
        let at = |x: usize, y: usize| pixels[y * 8 + x];
        let c = palette.cursor;
        assert_eq!([at(0, 5), at(1, 5), at(2, 5)], [c, c, 0]);
        assert_eq!([at(3, 0), at(3, 1), at(3, 2)], [c, c, 0]);
        assert_eq!([at(3, 21), at(3, 22), at(3, 23)], [0, c, c]);
        assert_eq!([at(5, 5), at(6, 5), at(7, 5)], [0, c, c]);
    }

    #[test]
    fn an_unfocused_cursor_outlines_both_cells_of_a_wide_char() {
        let palette = Palette::default();
        let term = term(2, 1, "日\x1b[1G".as_bytes());
        let mut pixels = vec![0; 12 * 6];
        let mut frame = Frame::new(&mut pixels, 12, 6).unwrap();
        paint_cursor(&term, &mut frame, BIG, &palette, UNFOCUSED);
        let c = palette.cursor;
        for y in 0..6 {
            for x in 0..12 {
                let edge = x == 0 || y == 0 || x == 11 || y == 5;
                assert_eq!(pixels[y * 12 + x], if edge { c } else { 0 }, "({x}, {y})");
            }
        }
    }

    #[test]
    fn an_unfocused_hidden_cursor_draws_nothing() {
        let term = term(1, 1, b"\x1b[?25l");
        let mut pixels = vec![0; 36];
        let mut frame = Frame::new(&mut pixels, 6, 6).unwrap();
        paint_cursor(&term, &mut frame, BIG, &Palette::default(), UNFOCUSED);
        assert!(pixels.iter().all(|&p| p == 0));
    }
}
