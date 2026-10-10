//! The selection of a terminal, in absolute line coordinates (see
//! [`crate::selection`]): line `n` is the `n`th line that ever reached the
//! history, so the screen starts at [`State::screen_base`].
//!
//! Clearing rule: output that changes a selected line clears the
//! selection (printing, erasing, inserting or deleting cells there, and
//! any scroll that moves lines without saving them to the history, such
//! as region scrolls and IL/DL). Switching screens, resizing, RIS and
//! clearing the history (ED 3) always clear it. Lines scrolling from the
//! screen into the history keep their numbers, so they never clear it.

use std::ops::Range;

use super::{State, Terminal};
use crate::cell::Cell;
use crate::selection::{self, Lines, Point, Selection, SelectionKind, Span};

/// A selection plus its span, expanded when it last changed. Output that
/// could change the expansion touches selected lines and clears it.
#[derive(Debug, Clone, Copy)]
pub(super) struct Selected {
    selection: Selection,
    span: Span,
}

impl Lines for State {
    fn cols(&self) -> u16 {
        self.screen.grid.size().cols()
    }

    fn line(&self, line: u64) -> Option<&[Cell]> {
        let history = line.checked_sub(self.scrollback.dropped)?;
        let len = self.scrollback.len() as u64;
        if history < len {
            return Some(self.scrollback.line(history as usize));
        }
        let row = u16::try_from(history - len).ok()?;
        (row < self.screen.grid.size().rows()).then(|| self.screen.grid.row(row))
    }
}

impl State {
    /// The absolute line of screen row 0.
    pub(super) fn screen_base(&self) -> u64 {
        self.scrollback.dropped + self.scrollback.len() as u64
    }

    /// Clears the selection when it touches screen rows `top..=bottom`.
    pub(super) fn damage(&mut self, top: u16, bottom: u16) {
        let Some(selected) = &self.selection else {
            return;
        };
        let base = self.screen_base();
        let rows = base + u64::from(top)..=base + u64::from(bottom);
        let lines = selected.span.lines();
        if lines.start() <= rows.end() && rows.start() <= lines.end() {
            self.selection = None;
        }
    }

    fn select(&mut self, selection: Selection) {
        let span = selection.span(self);
        self.selection = Some(Selected { selection, span });
    }
}

impl Terminal {
    /// The absolute position of viewport cell `col`, `row`, following the
    /// scrollback viewport.
    pub fn point_at(&self, col: u16, row: u16) -> Point {
        let top = self.state.screen_base() - self.display_offset() as u64;
        Point::new(top + u64::from(row), col)
    }

    /// Starts a selection of `kind` at `at`, replacing any other.
    pub fn start_selection(&mut self, kind: SelectionKind, at: Point) {
        self.state.select(Selection::new(kind, at));
    }

    /// Moves the free end of the selection to `to`; nothing without one.
    pub fn extend_selection(&mut self, to: Point) {
        if let Some(Selected { mut selection, .. }) = self.state.selection {
            selection.update(to);
            self.state.select(selection);
        }
    }

    /// Selects every line, the history included.
    pub fn select_all(&mut self) {
        let first = self.state.scrollback.dropped;
        let size = self.size();
        let last = self.state.screen_base() + u64::from(size.rows() - 1);
        let mut selection = Selection::new(SelectionKind::Simple, Point::new(first, 0));
        selection.update(Point::new(last, size.cols() - 1));
        self.state.select(selection);
    }

    pub fn clear_selection(&mut self) {
        self.state.selection = None;
    }

    pub fn selection(&self) -> Option<Selection> {
        self.state.selection.map(|selected| selected.selection)
    }

    /// The selected text (see [`selection::text`]); `None` without a
    /// selection or when it covers nothing but blanks.
    pub fn selection_text(&self) -> Option<String> {
        let selected = self.state.selection?;
        let text = selection::text(&selected.span, &self.state);
        (!text.trim().is_empty()).then_some(text)
    }

    /// The selected columns of viewport row `row`, for drawing.
    pub fn selected_cols(&self, row: u16) -> Option<Range<u16>> {
        let selected = self.state.selection?;
        let line = self.point_at(0, row).line;
        selected.span.cols_on(line, self.size().cols())
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use crate::TermSize;
    use crate::selection::{Point, SelectionKind};

    /// Selects viewport cells `from` to `to` (col, row) cell by cell.
    fn drag(t: &mut crate::Terminal, from: (u16, u16), to: (u16, u16)) {
        let start = t.point_at(from.0, from.1);
        t.start_selection(SelectionKind::Simple, start);
        let end = t.point_at(to.0, to.1);
        t.extend_selection(end);
    }

    fn selected(t: &crate::Terminal) -> Option<String> {
        t.selection_text()
    }

    #[test]
    fn viewport_cells_map_to_absolute_lines_counting_the_history() {
        let mut t = term(5, 2);
        assert_eq!(t.point_at(1, 1), Point::new(1, 1));
        t.advance(b"a\r\nb\r\nc");
        assert_eq!(t.scrollback_len(), 1);
        assert_eq!(t.point_at(0, 0), Point::new(1, 0), "row 0 shows line 1");
        t.scroll_display(1);
        assert_eq!(t.point_at(0, 0), Point::new(0, 0), "the history line");
    }

    #[test]
    fn the_selection_survives_scrolling_the_viewport() {
        let mut t = term(5, 2);
        t.advance(b"a\r\nb\r\nc");
        drag(&mut t, (0, 0), (0, 1));
        assert_eq!(selected(&t).as_deref(), Some("b\nc"));
        assert_eq!(t.selected_cols(0), Some(0..5));
        t.scroll_display(1);
        assert_eq!(selected(&t).as_deref(), Some("b\nc"));
        assert_eq!(t.selected_cols(0), None, "row 0 now shows line a");
        assert_eq!(t.selected_cols(1), Some(0..5));
    }

    #[test]
    fn the_selection_follows_its_lines_into_the_history() {
        let mut t = term(5, 2);
        t.advance(b"a\r\nb");
        drag(&mut t, (0, 1), (0, 1));
        t.advance(b"\r\nc\r\nd");
        assert_eq!(selected(&t).as_deref(), Some("b"));
        t.scroll_display(2);
        assert_eq!(t.selected_cols(1), Some(0..1));
    }

    #[test]
    fn text_is_read_from_the_history_and_joins_soft_wraps() {
        let mut t = term(3, 2);
        t.advance(b"abcdef\r\nxy\r\nz");
        assert_eq!(t.scrollback_len(), 2);
        t.select_all();
        assert_eq!(selected(&t).as_deref(), Some("abcdef\nxy\nz"));
    }

    #[test]
    fn output_on_a_selected_line_clears_the_selection() {
        let mut t = term(5, 3);
        t.advance(b"one\r\ntwo\r\n");
        drag(&mut t, (0, 0), (2, 0));
        t.advance(b"x");
        assert!(t.selection().is_some(), "row 2 is not selected");
        t.advance(b"\x1b[1;5Hy");
        assert_eq!(t.selection(), None);
    }

    #[test]
    fn erasing_or_editing_a_selected_line_clears_the_selection() {
        for edit in [
            &b"\x1b[1;1H\x1b[K"[..],
            b"\x1b[1;1H\x1b[P",
            b"\x1b[1;1H\x1b[@",
            b"\x1b[1;1H\x1b[X",
            b"\x1b[2J",
            b"\x1b[1;1H\x1b[M",
            b"\x1b[1;1H\x1b[L",
            b"\x1b[1;1H\x1bM",
        ] {
            let mut t = term(5, 3);
            t.advance(b"one\r\ntwo");
            drag(&mut t, (0, 0), (2, 0));
            t.advance(edit);
            assert_eq!(t.selection(), None, "{edit:?}");
        }
    }

    #[test]
    fn a_region_scroll_clears_the_selection_it_moves() {
        let mut t = term(5, 4);
        t.advance(b"a\r\nb\r\nc\r\nd");
        drag(&mut t, (0, 3), (0, 3));
        t.advance(b"\x1b[1;3r\x1b[3;1H\n");
        assert_eq!(t.scrollback_len(), 1, "the top line was saved");
        assert_eq!(t.selection(), None, "the status line kept its row");
    }

    #[test]
    fn screen_switches_resizes_resets_and_ed3_clear_the_selection() {
        for input in [&b"\x1b[?1049h"[..], b"\x1bc", b"\x1b[3J"] {
            let mut t = term(5, 2);
            t.advance(b"a\r\nb\r\nc");
            drag(&mut t, (0, 0), (0, 0));
            t.advance(input);
            assert_eq!(t.selection(), None, "{input:?}");
        }
        let mut t = term(5, 2);
        drag(&mut t, (0, 0), (0, 0));
        t.resize(TermSize::new(4, 2).unwrap());
        assert_eq!(t.selection(), None, "resize");
        let mut t = term(5, 2);
        t.advance(b"\x1b[?1049h");
        drag(&mut t, (0, 0), (0, 0));
        t.advance(b"\x1b[?1049l");
        assert_eq!(t.selection(), None, "leaving the alternate screen");
    }

    #[test]
    fn lines_dropped_from_a_full_history_keep_the_numbering() {
        let mut t = term(5, 1);
        t.set_scrollback_limit(1);
        t.advance(b"a\r\nb");
        drag(&mut t, (0, 0), (0, 0));
        t.advance(b"\r\nc\r\nd");
        assert_eq!(t.scrollback_len(), 1);
        assert_eq!(selected(&t), None, "b fell off the history");
        t.scroll_display(1);
        assert_eq!(t.point_at(0, 0), Point::new(2, 0), "c is line 2");
    }

    #[test]
    fn without_a_history_lines_still_get_new_numbers() {
        let mut t = term(5, 2);
        t.set_scrollback_limit(0);
        t.advance(b"a\r\nb");
        drag(&mut t, (0, 1), (0, 1));
        t.advance(b"\r\nc");
        assert_eq!(selected(&t).as_deref(), Some("b"), "b moved up a row");
        assert_eq!(t.selected_cols(0), Some(0..1));
    }

    #[test]
    fn lowering_the_limit_keeps_the_numbering() {
        let mut t = term(5, 1);
        t.advance(b"a\r\nb\r\nc");
        drag(&mut t, (0, 0), (0, 0));
        t.set_scrollback_limit(1);
        assert_eq!(selected(&t).as_deref(), Some("c"));
    }

    #[test]
    fn blank_selections_have_no_text_and_clearing_forgets_it() {
        let mut t = term(5, 2);
        drag(&mut t, (0, 0), (3, 1));
        assert!(t.selection().is_some());
        assert_eq!(selected(&t), None);
        t.clear_selection();
        assert_eq!(t.selection(), None);
        t.extend_selection(Point::new(0, 1));
        assert_eq!(t.selection(), None, "nothing to extend");
    }

    #[test]
    fn word_selection_reads_the_terminal_cells() {
        let mut t = term(20, 1);
        t.advance(b"cat ~/a/b.txt|x");
        t.start_selection(SelectionKind::Word, t.point_at(6, 0));
        assert_eq!(selected(&t).as_deref(), Some("~/a/b.txt|x"));
    }

    #[test]
    fn wide_char_spacers_are_left_out_of_the_text() {
        let mut t = term(10, 1);
        t.advance("日本x 語".as_bytes());
        drag(&mut t, (0, 0), (7, 0));
        assert_eq!(selected(&t).as_deref(), Some("日本x 語"));
        // A word runs through the spacers, clicked on either half.
        t.start_selection(SelectionKind::Word, t.point_at(1, 0));
        assert_eq!(selected(&t).as_deref(), Some("日本x"));
    }
}
