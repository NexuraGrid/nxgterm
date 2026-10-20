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

/// The preedit being composed and the pane it was composed for. Keys go
/// to the input method only while it lasts, so it must end whenever the
/// composition can no longer be committed there: the IME ended it (an
/// empty preedit, a commit, disabled), the window lost focus, or another
/// pane (or tab) took focus. A stale preedit would swallow every key.
pub struct Composition<K> {
    current: Option<(K, Preedit)>,
}

impl<K: Copy + PartialEq> Default for Composition<K> {
    fn default() -> Self {
        Self { current: None }
    }
}

impl<K: Copy + PartialEq> Composition<K> {
    /// An `Ime::Preedit` for the `focused` pane: `None` (empty text or no
    /// pane) ends the composition.
    pub fn update(&mut self, focused: Option<K>, preedit: Option<Preedit>) {
        self.current = focused.zip(preedit);
    }

    /// Ends the composition (commit, IME disabled, focus lost).
    pub fn clear(&mut self) {
        self.current = None;
    }

    /// The preedit to draw over the `focused` pane: none when it was
    /// composed for another pane.
    pub fn preedit(&self, focused: Option<K>) -> Option<&Preedit> {
        match &self.current {
            Some((pane, preedit)) if Some(*pane) == focused => Some(preedit),
            _ => None,
        }
    }

    /// Whether a key press goes to the input method instead of the
    /// `focused` pane; a preedit left from another pane is dropped.
    pub fn takes_keys(&mut self, focused: Option<K>) -> bool {
        if self.preedit(focused).is_none() {
            self.current = None;
        }
        self.current.is_some()
    }
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
    fn a_composition_takes_keys_until_it_ends() {
        let mut composition = Composition::default();
        assert!(!composition.takes_keys(Some(1)));
        composition.update(Some(1), Preedit::new("ka", None));
        assert!(composition.takes_keys(Some(1)));
        assert!(composition.preedit(Some(1)).is_some());
        // An empty preedit ends it.
        composition.update(Some(1), Preedit::new("", None));
        assert!(!composition.takes_keys(Some(1)));
        // A commit, the IME disabled or focus lost end it too.
        composition.update(Some(1), Preedit::new("ka", None));
        composition.clear();
        assert!(!composition.takes_keys(Some(1)));
        // No focused pane: nothing to compose for.
        composition.update(None, Preedit::new("ka", None));
        assert!(!composition.takes_keys(None));
    }

    #[test]
    fn a_preedit_from_another_pane_never_swallows_keys() {
        let mut composition = Composition::default();
        composition.update(Some(1), Preedit::new("ka", None));
        assert!(
            composition.preedit(Some(2)).is_none(),
            "not drawn elsewhere"
        );
        assert!(!composition.takes_keys(Some(2)));
        // Dropped for good: focusing the first pane again types normally.
        assert!(!composition.takes_keys(Some(1)));
        assert!(composition.preedit(Some(1)).is_none());
    }

    #[test]
    fn the_preedit_moves_left_to_fit_the_grid() {
        assert_eq!(column(5, 3, 80), 5);
        assert_eq!(column(78, 4, 80), 76);
        assert_eq!(column(10, 100, 80), 0, "wider than the grid");
    }
}
