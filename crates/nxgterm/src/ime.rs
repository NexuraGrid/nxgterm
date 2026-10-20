//! Input method (IME) composition: the preedit text drawn underlined at
//! the terminal cursor while it is being composed, and where it goes.

use nxg_core::{TermSize, Terminal};

/// The widest preedit drawn, in cells.
const MAX_COLS: u16 = 256;

/// The text an input method is composing, as a one-row terminal drawn
/// over the grid at the cursor: underlined, with the cursor at the IME's
/// caret (hidden when the IME gives none).
pub struct Preedit {
    terminal: Terminal,
    width: u16,
}

impl Preedit {
    /// The preedit `text` with its caret at byte `caret` (winit's
    /// `Ime::Preedit`), or `None` when `text` is empty (composition ended).
    pub fn new(text: &str, caret: Option<usize>) -> Option<Self> {
        if text.is_empty() {
            return None;
        }
        let caret = caret.filter(|&at| text.is_char_boundary(at));
        let (before, after) = caret.map_or((text, ""), |at| text.split_at(at));
        let caret_col = width_of(before);
        let text_width = caret_col + width_of(after);
        // A caret after the text gets a cell of its own.
        let caret_end = caret.map_or(0, |_| caret_col + 1);
        let width = text_width.max(caret_end).clamp(1, MAX_COLS);
        let size = TermSize::new(width, 1).expect("at least one cell");
        let mut terminal = Terminal::new(size);
        let mut out = String::from("\x1b[4m");
        // No autowrap: a preedit wider than the row keeps its start.
        out.push_str("\x1b[?7l");
        out.push_str(text);
        out.push_str("\x1b[0m");
        match caret {
            Some(_) => out.push_str(&format!("\x1b[1;{}H", caret_col.min(width - 1) + 1)),
            None => out.push_str("\x1b[?25l"),
        }
        terminal.advance(out.as_bytes());
        Some(Self { terminal, width })
    }

    /// The terminal to draw as an overlay.
    pub fn terminal(&self) -> &Terminal {
        &self.terminal
    }

    /// Its width in cells.
    pub fn width(&self) -> u16 {
        self.width
    }
}

/// Width in cells of `text`, as a terminal lays it out.
fn width_of(text: &str) -> u16 {
    if text.is_empty() {
        return 0;
    }
    let size = TermSize::new(MAX_COLS, 1).expect("at least one cell");
    let mut terminal = Terminal::new(size);
    terminal.advance(b"\x1b[?7l");
    terminal.advance(text.as_bytes());
    let cursor = terminal.cursor().col;
    // The cursor stays on the last column once the row is full.
    let full = terminal.row(0)[usize::from(MAX_COLS) - 1].ch != ' ';
    if full { MAX_COLS } else { cursor }
}

/// The first column of a preedit `width` cells wide that starts at the
/// cursor column `col` of a grid `cols` wide: moved left so it fits.
pub fn column(col: u16, width: u16, cols: u16) -> u16 {
    col.min(cols.saturating_sub(width))
}

#[cfg(test)]
mod tests {
    use nxg_core::Flags;

    use super::*;

    fn text_of(preedit: &Preedit) -> String {
        preedit
            .terminal()
            .row(0)
            .iter()
            .map(|cell| cell.ch)
            .collect()
    }

    #[test]
    fn empty_text_ends_the_preedit() {
        assert!(Preedit::new("", None).is_none());
        assert!(Preedit::new("", Some(0)).is_none());
    }

    #[test]
    fn preedit_is_as_wide_as_its_text_and_underlined() {
        let preedit = Preedit::new("ka", None).unwrap();
        assert_eq!(preedit.width(), 2);
        assert_eq!(text_of(&preedit), "ka");
        let row = preedit.terminal().row(0);
        assert!(row.iter().all(|cell| cell.flags.contains(Flags::UNDERLINE)));
    }

    #[test]
    fn wide_chars_take_two_cells_each() {
        let preedit = Preedit::new("日本", None).unwrap();
        assert_eq!(preedit.width(), 4);
    }

    #[test]
    fn the_cursor_sits_at_the_caret_or_is_hidden() {
        // Caret after the first char (3 bytes, 2 cells).
        let preedit = Preedit::new("日本", Some(3)).unwrap();
        let cursor = preedit.terminal().cursor();
        assert!(cursor.visible);
        assert_eq!(cursor.col, 2);
        let at_end = Preedit::new("ab", Some(2)).unwrap();
        assert_eq!(at_end.width(), 3, "a cell for the caret after the text");
        assert_eq!(at_end.terminal().cursor().col, 2);
        let hidden = Preedit::new("ab", None).unwrap();
        assert!(!hidden.terminal().cursor().visible);
        let inside_a_char = Preedit::new("日本", Some(1)).unwrap();
        assert!(!inside_a_char.terminal().cursor().visible);
    }

    #[test]
    fn very_long_preedits_are_capped() {
        let text = "x".repeat(1000);
        let preedit = Preedit::new(&text, None).unwrap();
        assert_eq!(preedit.width(), MAX_COLS);
    }

    #[test]
    fn the_preedit_moves_left_to_fit_the_grid() {
        assert_eq!(column(5, 3, 80), 5);
        assert_eq!(column(78, 4, 80), 76);
        assert_eq!(column(10, 100, 80), 0, "wider than the grid");
    }
}
