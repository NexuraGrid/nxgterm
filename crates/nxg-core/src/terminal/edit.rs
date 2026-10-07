//! Cursor movement and erase primitives shared by the control and CSI handlers.

use super::State;

impl State {
    pub(super) fn goto(&mut self, col: u16, row: u16) {
        self.screen.col = col.min(self.last_col());
        self.screen.row = row.min(self.last_row());
        self.screen.wrap_pending = false;
    }

    pub(super) fn line_feed(&mut self) {
        if self.screen.row == self.last_row() {
            let blank = self.blank();
            self.screen.grid.scroll_up(blank);
            self.images.scroll_up(1, self.cell);
        } else {
            self.screen.row += 1;
        }
        self.screen.wrap_pending = false;
    }

    /// Fills `cols` of `row` with blanks.
    pub(super) fn erase(&mut self, row: u16, cols: std::ops::Range<u16>) {
        let blank = self.blank();
        self.screen.grid.row_mut(row)[usize::from(cols.start)..usize::from(cols.end)].fill(blank);
    }

    pub(super) fn erase_display(&mut self, mode: u16) {
        let (col, row) = (self.screen.col, self.screen.row);
        let cols = self.screen.grid.size().cols();
        let rows = self.screen.grid.size().rows();
        match mode {
            0 => {
                self.erase(row, col..cols);
                (row + 1..rows).for_each(|r| self.erase(r, 0..cols));
            }
            1 => {
                (0..row).for_each(|r| self.erase(r, 0..cols));
                self.erase(row, 0..col + 1);
            }
            2 | 3 => {
                (0..rows).for_each(|r| self.erase(r, 0..cols));
                // Like kitty, a full clear removes image placements.
                self.images.clear_placements();
            }
            _ => {}
        }
    }

    pub(super) fn erase_line(&mut self, mode: u16) {
        let (col, row) = (self.screen.col, self.screen.row);
        let cols = self.screen.grid.size().cols();
        match mode {
            0 => self.erase(row, col..cols),
            1 => self.erase(row, 0..col + 1),
            2 => self.erase(row, 0..cols),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{pos, sized, text};

    // Guards the move out of `mod.rs`: the helpers still work through `Terminal`.
    #[test]
    fn moved_helpers_drive_the_cursor_and_erase_through_terminal() {
        let mut t = sized(5, 3);
        t.advance(b"abc\x1b[2;3H");
        assert_eq!(pos(&t), (2, 1));
        t.advance(b"\x1b[1;2H\x1b[K\n");
        assert_eq!(text(&t, 0), "a");
        assert_eq!(pos(&t), (1, 1));
    }
}
