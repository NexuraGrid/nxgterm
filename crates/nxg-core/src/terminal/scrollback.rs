//! Scrollback: lines scrolled off the top of the main screen, and the
//! viewport that shows them.
//!
//! Images are not kept: a placement scrolled off the top is dropped as
//! before, so history rows are text only. While the viewport is scrolled
//! back, placements still on screen are drawn shifted down with their rows.

use std::collections::VecDeque;

use super::{Cursor, State, Terminal};
use crate::cell::Cell;

/// Lines of history kept until the embedder sets another limit.
pub const DEFAULT_SCROLLBACK: usize = 10_000;

/// A bounded history of lines plus the viewport position over it.
#[derive(Debug)]
pub(super) struct Scrollback {
    /// Oldest first. Each line keeps the width it had when it left the
    /// screen; a resize does not rewrap it.
    lines: VecDeque<Box<[Cell]>>,
    limit: usize,
    /// Lines ever dropped from the front (or never kept, with a zero
    /// limit); the absolute number of the oldest line kept.
    pub dropped: u64,
    /// Rows the viewport is scrolled back; 0 shows the live screen. Never
    /// more than `lines.len()`.
    pub offset: usize,
}

impl Scrollback {
    pub fn new(limit: usize) -> Self {
        Self {
            lines: VecDeque::new(),
            limit,
            dropped: 0,
            offset: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Line `index`, counted from the oldest. Panics when out of bounds.
    pub fn line(&self, index: usize) -> &[Cell] {
        &self.lines[index]
    }

    pub fn push(&mut self, line: &[Cell]) {
        if self.limit == 0 {
            self.dropped += 1;
            return;
        }
        if self.lines.len() == self.limit {
            self.lines.pop_front();
            self.dropped += 1;
        }
        self.lines.push_back(line.into());
    }

    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit;
        let excess = self.lines.len().saturating_sub(limit);
        self.lines.drain(..excess);
        self.dropped += excess as u64;
        self.offset = self.offset.min(self.lines.len());
    }

    /// Forgets every line (ED 3, RIS); the limit stays.
    pub fn clear(&mut self) {
        self.dropped += self.lines.len() as u64;
        self.lines.clear();
        self.offset = 0;
    }
}

impl Terminal {
    /// Caps the history at `lines` (0 disables it), dropping the oldest
    /// lines beyond the new limit.
    pub fn set_scrollback_limit(&mut self, lines: usize) {
        self.state.scrollback.set_limit(lines);
    }

    /// Lines currently in the history.
    pub fn scrollback_len(&self) -> usize {
        self.state.scrollback.len()
    }

    /// Rows the viewport is scrolled back into the history; 0 is the live
    /// screen.
    pub fn display_offset(&self) -> usize {
        self.state.scrollback.offset
    }

    /// Moves the viewport `lines` rows back into the history (negative:
    /// towards the live screen), clamped to what exists. The alternate
    /// screen has no history, so there it does nothing.
    pub fn scroll_display(&mut self, lines: i32) {
        if self.state.alt_active {
            return;
        }
        let scrollback = &mut self.state.scrollback;
        let target = scrollback.offset as i64 + i64::from(lines);
        scrollback.offset = target.clamp(0, scrollback.len() as i64) as usize;
    }

    /// Shows the live screen again.
    pub fn scroll_display_to_bottom(&mut self) {
        self.state.scrollback.offset = 0;
    }

    /// The cells of viewport row `row`: a history line while scrolled back,
    /// else a screen row. History lines are cut to the grid width but may
    /// be shorter than it. Panics if `row` is out of bounds.
    pub fn display_row(&self, row: u16) -> &[Cell] {
        let scrollback = &self.state.scrollback;
        let offset = scrollback.offset;
        if usize::from(row) < offset {
            let line = scrollback.line(scrollback.len() - offset + usize::from(row));
            &line[..line.len().min(usize::from(self.size().cols()))]
        } else {
            self.row(row - offset as u16)
        }
    }

    /// The cursor as drawn: hidden while the viewport is scrolled back.
    pub fn display_cursor(&self) -> Cursor {
        let mut cursor = self.cursor();
        cursor.visible &= self.state.scrollback.offset == 0;
        cursor
    }
}

impl State {
    /// Scrolls like [`State::scroll_up`], first saving the rows that leave
    /// the top of the main screen. Only IND/LF and SU save, as in xterm; DL
    /// discards.
    ///
    /// Saved rows keep their absolute line numbers, so a selection over
    /// them stays; rows below a partial region do not move but their
    /// numbers do, so then a selection on the screen is cleared.
    pub(super) fn scroll_up_saving(&mut self, top: u16, bottom: u16, n: u16) {
        if top != 0 || self.alt_active {
            self.scroll_up(top, bottom, n);
            return;
        }
        if bottom != self.last_row() {
            self.damage(0, self.last_row());
        }
        let n = n.min(bottom - top + 1);
        for row in 0..n {
            self.scrollback.push(self.screen.grid.row(row));
        }
        self.move_rows_up(top, bottom, n);
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use crate::{TermSize, Terminal};

    /// Row `row` of the viewport, trailing blanks trimmed.
    fn shown(t: &Terminal, row: u16) -> String {
        let line: String = t.display_row(row).iter().map(|c| c.ch).collect();
        line.trim_end().to_owned()
    }

    /// The whole history, oldest first, read through the viewport.
    fn history(t: &mut Terminal) -> Vec<String> {
        let len = t.scrollback_len();
        t.scroll_display(len as i32);
        let lines = (0..len).map(|r| shown(t, r as u16)).collect();
        t.scroll_display_to_bottom();
        lines
    }

    #[test]
    fn lines_scrolled_off_the_top_are_kept_oldest_first() {
        let mut t = term(5, 2);
        t.advance(b"a\r\nb\r\nc\r\nd");
        assert_eq!(t.scrollback_len(), 2);
        assert_eq!(history(&mut t), ["a", "b"]);
        assert_eq!(text(&t, 0), "c");
    }

    #[test]
    fn the_limit_drops_the_oldest_lines() {
        let mut t = term(5, 1);
        t.set_scrollback_limit(2);
        t.advance(b"a\r\nb\r\nc\r\nd\r\ne");
        assert_eq!(history(&mut t), ["c", "d"]);
    }

    #[test]
    fn lowering_the_limit_trims_the_history() {
        let mut t = term(5, 1);
        t.advance(b"a\r\nb\r\nc\r\nd");
        t.set_scrollback_limit(1);
        assert_eq!(history(&mut t), ["c"]);
    }

    #[test]
    fn a_zero_limit_disables_the_history() {
        let mut t = term(5, 1);
        t.set_scrollback_limit(0);
        t.advance(b"a\r\nb\r\nc");
        assert_eq!(t.scrollback_len(), 0);
    }

    #[test]
    fn the_alternate_screen_never_feeds_the_history() {
        let mut t = term(5, 2);
        t.advance(b"\x1b[?1049ha\r\nb\r\nc\r\nd");
        assert_eq!(t.scrollback_len(), 0);
        t.advance(b"\x1b[?1049l");
        assert_eq!(t.scrollback_len(), 0);
    }

    #[test]
    fn only_regions_at_the_top_margin_feed_the_history() {
        let mut t = term(5, 4);
        t.advance(b"a\r\nb\r\nc\r\nd");
        t.advance(b"\x1b[2;4r\x1b[4;1H\n\n");
        assert_eq!(
            t.scrollback_len(),
            0,
            "rows leaving a lower region are lost"
        );
        t.advance(b"\x1b[1;3r\x1b[3;1H\n");
        assert_eq!(
            history(&mut t),
            ["a"],
            "a region at the top saves, like xterm"
        );
    }

    #[test]
    fn scroll_up_saves_but_delete_lines_does_not() {
        let mut t = term(5, 2);
        t.advance(b"a\r\nb\x1b[S");
        assert_eq!(history(&mut t), ["a"]);
        t.advance(b"\x1b[Hc\x1b[M");
        assert_eq!(t.scrollback_len(), 1, "DL discards");
    }

    #[test]
    fn a_large_scroll_saves_at_most_one_screen() {
        let mut t = term(5, 2);
        t.advance(b"a\r\nb\x1b[9S");
        assert_eq!(history(&mut t), ["a", "b"]);
    }

    #[test]
    fn ed_3_clears_the_history_and_ed_2_keeps_it() {
        let mut t = term(5, 1);
        t.advance(b"a\r\nb\x1b[2J");
        assert_eq!(t.scrollback_len(), 1);
        t.advance(b"\x1b[3J");
        assert_eq!(t.scrollback_len(), 0);
    }

    #[test]
    fn ris_clears_the_history_but_keeps_the_limit() {
        let mut t = term(5, 1);
        t.set_scrollback_limit(1);
        t.advance(b"a\r\nb\x1bc");
        assert_eq!(t.scrollback_len(), 0);
        t.advance(b"c\r\nd\r\ne");
        assert_eq!(history(&mut t), ["d"]);
    }

    #[test]
    fn the_viewport_shows_history_above_the_screen() {
        let mut t = term(5, 2);
        t.advance(b"a\r\nb\r\nc\r\nd");
        assert_eq!(t.display_offset(), 0);
        assert_eq!((shown(&t, 0), shown(&t, 1)), ("c".into(), "d".into()));
        t.scroll_display(1);
        assert_eq!(t.display_offset(), 1);
        assert_eq!((shown(&t, 0), shown(&t, 1)), ("b".into(), "c".into()));
        t.scroll_display(-1);
        assert_eq!(shown(&t, 0), "c");
    }

    #[test]
    fn the_viewport_offset_is_clamped_to_the_history() {
        let mut t = term(5, 2);
        t.advance(b"a\r\nb\r\nc");
        t.scroll_display(50);
        assert_eq!(t.display_offset(), 1);
        t.scroll_display(-50);
        assert_eq!(t.display_offset(), 0);
    }

    #[test]
    fn the_cursor_is_hidden_while_scrolled_back() {
        let mut t = term(5, 2);
        t.advance(b"a\r\nb\r\nc");
        assert!(t.display_cursor().visible);
        t.scroll_display(1);
        let cursor = t.display_cursor();
        assert!(!cursor.visible);
        assert!(t.cursor().visible, "the terminal's own cursor is unchanged");
    }

    #[test]
    fn new_output_snaps_the_viewport_to_the_bottom() {
        let mut t = term(5, 2);
        t.advance(b"a\r\nb\r\nc");
        t.scroll_display(1);
        t.advance(b"");
        assert_eq!(t.display_offset(), 1, "no bytes, no snap");
        t.advance(b"x");
        assert_eq!(t.display_offset(), 0);
    }

    #[test]
    fn the_alternate_screen_shows_no_history() {
        let mut t = term(5, 2);
        t.advance(b"a\r\nb\r\nc");
        t.scroll_display(1);
        t.advance(b"\x1b[?1049h");
        assert_eq!(t.display_offset(), 0);
        t.scroll_display(1);
        assert_eq!(
            t.display_offset(),
            0,
            "nothing to scroll on the alternate screen"
        );
    }

    #[test]
    fn resize_resets_the_viewport_and_old_lines_keep_their_width() {
        let mut t = term(6, 1);
        t.advance(b"abcdef\r\nx");
        t.scroll_display(1);
        t.resize(TermSize::new(3, 1).unwrap());
        assert_eq!(t.display_offset(), 0);
        t.scroll_display(1);
        assert_eq!(t.display_row(0).len(), 3, "wider lines are cut to the grid");
        assert_eq!(shown(&t, 0), "abc");
        t.resize(TermSize::new(9, 1).unwrap());
        t.scroll_display(1);
        assert_eq!(t.display_row(0).len(), 6, "narrower lines stay short");
    }
}
