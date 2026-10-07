//! One screen (main or alternate): grid plus everything the cursor owns.

use super::State;
use crate::TermSize;
use crate::cell::Cell;
use crate::grid::{Grid, Region};
use crate::kitty::Graphics;

/// What DECSC remembers.
#[derive(Debug, Clone, Copy)]
pub(super) struct SavedCursor {
    pub col: u16,
    pub row: u16,
    pub pen: Cell,
    pub wrap_pending: bool,
    pub origin: bool,
}

/// A grid with its cursor. The terminal keeps two and swaps them, so a field
/// added here is automatically per screen.
#[derive(Debug)]
pub(super) struct Screen {
    pub grid: Grid,
    pub col: u16,
    pub row: u16,
    /// Set after printing in the last column; the next print wraps first.
    pub wrap_pending: bool,
    pub saved: Option<SavedCursor>,
    pub region: Region,
    pub origin: bool,
}

impl Screen {
    pub fn new(size: TermSize) -> Self {
        Self {
            grid: Grid::new(size),
            col: 0,
            row: 0,
            wrap_pending: false,
            saved: None,
            region: Region::full(size.rows()),
            origin: false,
        }
    }

    pub fn resize(&mut self, size: TermSize) {
        self.grid.resize(size);
        self.region = Region::full(size.rows());
        self.col = self.col.min(size.cols() - 1);
        self.row = self.row.min(size.rows() - 1);
        self.wrap_pending = false;
        if let Some(saved) = &mut self.saved {
            saved.col = saved.col.min(size.cols() - 1);
            saved.row = saved.row.min(size.rows() - 1);
        }
    }

    /// Takes the cursor position of the screen being left: xterm has a single
    /// cursor, and `?1049` overrides it with DECSC/DECRC.
    fn carry_cursor_from(&mut self, from: &Screen) {
        self.col = from.col;
        self.row = from.row;
        self.wrap_pending = from.wrap_pending;
    }

    /// DECSC. The pen is global, so the caller passes it in.
    pub fn save_cursor(&mut self, pen: Cell) {
        self.saved = Some(SavedCursor {
            col: self.col,
            row: self.row,
            pen,
            wrap_pending: self.wrap_pending,
            origin: self.origin,
        });
    }

    /// DECRC. Returns the pen to make current; with nothing saved the
    /// cursor goes home with the default pen.
    pub fn restore_cursor(&mut self) -> Cell {
        let size = self.grid.size();
        let saved = self.saved.unwrap_or(SavedCursor {
            col: 0,
            row: 0,
            pen: Cell::default(),
            wrap_pending: false,
            origin: false,
        });
        // The saved position may predate a shrink that missed this copy.
        self.col = saved.col.min(size.cols() - 1);
        self.row = saved.row.min(size.rows() - 1);
        self.wrap_pending = saved.wrap_pending;
        self.origin = saved.origin;
        saved.pen
    }
}

impl State {
    pub fn resize(&mut self, size: TermSize) {
        self.scrollback.offset = 0;
        self.screen.resize(size);
        if let Some(dormant) = &mut self.dormant {
            dormant.resize(size);
        }
    }

    /// RIS: back to power-on state. `cell` (a fact about the window) and
    /// `responses` (replies the child is still waiting for) are kept, and so
    /// are the stored images, which the budget already bounds.
    pub fn reset(&mut self) {
        let size = self.screen.grid.size();
        self.screen = Screen::new(size);
        self.dormant = None;
        self.alt_active = false;
        self.cursor_visible = true;
        self.pen = Cell::default();
        self.sixel_scrolling = true;
        self.autowrap = true;
        self.app_cursor_keys = false;
        self.last_char = None;
        self.sixel = None;
        self.graphics = Graphics::new();
        self.images.reset_placements();
        self.scrollback.clear();
    }

    /// DECSC.
    pub fn save_cursor(&mut self) {
        self.screen.save_cursor(self.pen);
    }

    /// DECRC.
    pub fn restore_cursor(&mut self) {
        self.pen = self.screen.restore_cursor();
        // The wrap was saved under whatever DECAWM was then; with the mode off
        // now it must not fire.
        self.screen.wrap_pending &= self.autowrap;
    }

    /// Switches to the alternate screen. With `fresh` a kept alternate
    /// screen is discarded first. Entering while already there is a no-op,
    /// so unbalanced sequences cannot desynchronise the screens.
    pub fn enter_alt(&mut self, fresh: bool) {
        if self.alt_active {
            return;
        }
        // The alternate screen has no history to look at.
        self.scrollback.offset = 0;
        let mut alt = match self.dormant.take() {
            Some(kept) if !fresh => kept,
            _ => Box::new(Screen::new(self.screen.grid.size())),
        };
        alt.carry_cursor_from(&self.screen);
        // Region and origin are not carried in, so an app cannot leak them
        // into the shell; main gets its own back on the way out.
        alt.region = Region::full(alt.grid.size().rows());
        alt.origin = false;
        std::mem::swap(&mut self.screen, &mut *alt);
        self.dormant = Some(alt);
        self.alt_active = true;
        self.images.stash_placements();
    }

    /// Switches back to the main screen. The alternate one is kept for a
    /// later `?47h` only when `keep` is set. Leaving while on main is a no-op
    /// (no stale restore).
    pub fn leave_alt(&mut self, keep: bool) {
        if !self.alt_active {
            return;
        }
        let Some(mut main) = self.dormant.take() else {
            return;
        };
        main.carry_cursor_from(&self.screen);
        std::mem::swap(&mut self.screen, &mut *main);
        self.dormant = keep.then_some(main);
        self.alt_active = false;
        self.images.restore_placements();
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use super::*;
    use crate::Color;
    use crate::cell::Flags;

    fn size(cols: u16, rows: u16) -> TermSize {
        TermSize::new(cols, rows).unwrap()
    }

    fn bold_red() -> Cell {
        let mut pen = Cell {
            fg: Color::Indexed(1),
            ..Cell::default()
        };
        pen.flags.insert(Flags::BOLD);
        pen
    }

    #[test]
    fn new_screen_is_blank_at_home_with_nothing_saved() {
        let screen = Screen::new(size(4, 3));
        assert_eq!(screen.grid.size(), size(4, 3));
        assert!((0..3).all(|r| screen.grid.row(r).iter().all(|c| *c == Cell::default())));
        assert_eq!((screen.col, screen.row), (0, 0));
        assert!(!screen.wrap_pending);
        assert!(screen.saved.is_none());
        assert!(screen.region.is_full(3));
        assert!(!screen.origin);
    }

    #[test]
    fn restore_returns_the_saved_pen_and_position() {
        let mut screen = Screen::new(size(10, 5));
        (screen.col, screen.row, screen.wrap_pending) = (4, 2, true);
        screen.save_cursor(bold_red());
        (screen.col, screen.row, screen.wrap_pending) = (0, 0, false);
        let pen = screen.restore_cursor();
        assert_eq!(pen, bold_red());
        assert_eq!((screen.col, screen.row, screen.wrap_pending), (4, 2, true));
    }

    #[test]
    fn restore_brings_back_origin_mode() {
        let mut screen = Screen::new(size(10, 5));
        screen.origin = true;
        screen.save_cursor(Cell::default());
        screen.origin = false;
        screen.restore_cursor();
        assert!(screen.origin);
    }

    #[test]
    fn restore_without_save_homes_and_resets() {
        let mut screen = Screen::new(size(10, 5));
        (screen.col, screen.row, screen.wrap_pending, screen.origin) = (4, 4, true, true);
        let pen = screen.restore_cursor();
        assert_eq!(pen, Cell::default());
        assert_eq!((screen.col, screen.row), (0, 0));
        assert!(!screen.wrap_pending && !screen.origin);
    }

    #[test]
    fn restore_clamps_a_position_saved_before_a_shrink() {
        let mut screen = Screen::new(size(10, 5));
        (screen.col, screen.row) = (9, 4);
        screen.save_cursor(Cell::default());
        screen.grid.resize(size(3, 2));
        screen.restore_cursor();
        assert_eq!((screen.col, screen.row), (2, 1));
    }

    #[test]
    fn resize_clamps_both_cursors_and_cancels_the_pending_wrap() {
        let mut screen = Screen::new(size(10, 5));
        (screen.col, screen.row, screen.wrap_pending) = (9, 4, true);
        screen.save_cursor(Cell::default());
        screen.resize(size(3, 2));
        assert_eq!((screen.col, screen.row), (2, 1));
        assert!(!screen.wrap_pending);
        let saved = screen.saved.unwrap();
        assert_eq!((saved.col, saved.row), (2, 1));
        assert_eq!(screen.grid.size(), size(3, 2));
    }

    #[test]
    fn resize_resets_the_region_to_the_new_height() {
        let mut screen = Screen::new(size(4, 6));
        screen.region = Region { top: 1, bottom: 3 };
        screen.resize(size(4, 8));
        assert!(screen.region.is_full(8));
    }

    #[test]
    fn ris_from_the_alternate_screen_returns_to_a_blank_main() {
        let mut t = sized(6, 4);
        t.advance(b"main\x1b[?1049h\x1b[2;3r\x1b[?6hxyz");
        t.advance(&kitty_rgba(1, 10, 20, ""));
        t.advance(b"\x1bc");
        assert!(!t.state.alt_active);
        assert!(t.state.dormant.is_none());
        assert_eq!((0..4).map(|r| text(&t, r)).collect::<String>(), "");
        assert_eq!(pos(&t), (0, 0));
        assert!(t.state.screen.region.is_full(4));
        assert!(!t.state.screen.origin && t.state.screen.saved.is_none());
        assert!(t.images().placements().is_empty());
        assert_eq!(t.images().len(), 1, "image data is kept");
    }

    #[test]
    fn ris_drops_the_stashed_main_placements_too() {
        let mut t = sized(6, 4);
        t.advance(&kitty_rgba(1, 10, 20, ""));
        t.advance(b"\x1b[?1049h\x1bc\x1b[?1049h\x1b[?1049l");
        assert!(t.images().placements().is_empty());
    }

    #[test]
    fn ris_resets_pen_visibility_and_sixel_scrolling_but_keeps_pending_replies() {
        let mut t = sized(6, 4);
        t.advance(b"\x1b[31;1m\x1b[?25l\x1b[?80h\x1b[6n");
        t.advance(b"\x1bcX");
        assert_eq!(t.row(0)[0].ch, 'X');
        assert_eq!(
            t.row(0)[0],
            crate::cell::Cell {
                ch: 'X',
                ..Default::default()
            }
        );
        assert!(t.cursor().visible);
        assert!(t.state.sixel_scrolling);
        assert_eq!(t.take_responses(), b"\x1b[1;1R", "the reply survives");
        assert_eq!(
            t.cell_pixels(),
            crate::CellPixels::new(10, 20),
            "so does the cell size"
        );
    }

    #[test]
    fn ris_on_the_main_screen_also_clears_the_grid_and_saved_cursor() {
        let mut t = sized(6, 4);
        t.advance(b"abc\x1b[3;3H\x1b7\x1bc\x1b8");
        assert_eq!(text(&t, 0), "");
        assert_eq!(pos(&t), (0, 0), "the saved cursor is gone");
    }

    #[test]
    fn resize_resets_the_region_on_both_screens_and_clamps_both_cursors() {
        let mut t = sized(10, 6);
        t.advance(b"\x1b[2;4r\x1b[6;9H\x1b7\x1b[?1049h\x1b[2;5r\x1b[6;9H\x1b7");
        t.resize(TermSize::new(4, 3).unwrap());
        let alt = &t.state.screen;
        let main = t.state.dormant.as_ref().unwrap();
        for screen in [alt, main] {
            assert!(screen.region.is_full(3));
            assert!(screen.col <= 3 && screen.row <= 2);
            let saved = screen.saved.unwrap();
            assert!(saved.col <= 3 && saved.row <= 2);
        }
    }

    #[test]
    fn ris_restores_autowrap_and_forgets_the_last_character() {
        let mut t = term(3, 2);
        t.advance(b"\x1b[?7lx\x1bc");
        t.advance(b"\x1b[3b");
        assert_eq!(text(&t, 0), "", "REP has nothing to repeat after RIS");
        t.advance(b"abcd");
        assert_eq!(text(&t, 0), "abc", "autowrap is back on");
        assert_eq!(text(&t, 1), "d");
    }

    #[test]
    fn decrc_does_not_revive_a_pending_wrap_after_autowrap_was_turned_off() {
        let mut t = term(3, 2);
        t.advance(b"abc\x1b7\x1b[?7l\x1b8d");
        assert_eq!(text(&t, 0), "abd", "the last column is overwritten");
        assert_eq!(text(&t, 1), "");
        assert_eq!(pos(&t), (2, 0));
        // With autowrap on, the same restore keeps the pending wrap.
        let mut t = term(3, 2);
        t.advance(b"abc\x1b7\x1b8d");
        assert_eq!(text(&t, 1), "d");
    }
}
