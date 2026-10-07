//! One screen (main or alternate): grid plus everything the cursor owns.

use super::State;
use crate::TermSize;
use crate::cell::Cell;
use crate::grid::{Grid, Region};

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
    /// Read by the scrolling operations once scroll regions land.
    #[allow(dead_code)]
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
        self.screen.resize(size);
        if let Some(dormant) = &mut self.dormant {
            dormant.resize(size);
        }
    }

    /// DECSC.
    pub fn save_cursor(&mut self) {
        self.screen.save_cursor(self.pen);
    }

    /// DECRC.
    pub fn restore_cursor(&mut self) {
        self.pen = self.screen.restore_cursor();
    }

    /// Switches to the alternate screen. With `fresh` a kept alternate
    /// screen is discarded first. Entering while already there is a no-op,
    /// so unbalanced sequences cannot desynchronise the screens.
    pub fn enter_alt(&mut self, fresh: bool) {
        if self.alt_active {
            return;
        }
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
}
