//! Cursor movement, scroll regions, index operations and erasing.

use super::State;
use crate::grid::Region;
use vte::Perform as _;

impl State {
    pub(super) fn goto(&mut self, col: u16, row: u16) {
        self.screen.col = col.min(self.last_col());
        self.screen.row = row.min(self.last_row());
        self.screen.wrap_pending = false;
    }

    /// Absolute-addressing target of CUP/HVP/VPA: under DECOM rows count from
    /// the region top and cannot leave the region.
    pub(super) fn goto_addressed(&mut self, col: u16, row: u16) {
        let row = if self.screen.origin {
            let region = self.screen.region;
            region.top.saturating_add(row).min(region.bottom)
        } else {
            row
        };
        self.goto(col, row);
    }

    /// The cursor row as the application addresses it (for CPR).
    pub(super) fn addressed_row(&self) -> u16 {
        if self.screen.origin {
            self.screen.row.saturating_sub(self.screen.region.top)
        } else {
            self.screen.row
        }
    }

    /// DECSTBM with the raw one-based parameters (0 means the default). A
    /// region that is not at least two rows tall is ignored, as in xterm.
    pub(super) fn set_region(&mut self, top: u16, bottom: u16) {
        let rows = self.screen.grid.size().rows();
        let top = top.max(1);
        let bottom = if bottom == 0 { rows } else { bottom.min(rows) };
        if top >= bottom {
            return;
        }
        self.screen.region = Region {
            top: top - 1,
            bottom: bottom - 1,
        };
        self.goto_addressed(0, 0);
    }

    /// DECOM. Both directions home the cursor.
    pub(super) fn set_origin_mode(&mut self, on: bool) {
        self.screen.origin = on;
        self.goto_addressed(0, 0);
    }

    /// Scrolls rows `top..=bottom` up by `n`, taking placements along. The
    /// one entry point for scrolling, so scrollback has a single hook.
    pub(super) fn scroll_up(&mut self, top: u16, bottom: u16, n: u16) {
        let n = n.min(bottom - top + 1);
        let blank = self.blank();
        self.screen.grid.scroll_up_in(top, bottom, n, blank);
        let rows = self.screen.grid.size().rows();
        if (Region { top, bottom }).is_full(rows) {
            self.images.scroll_up(u32::from(n), self.cell);
        } else {
            self.images
                .scroll_region_up(top, bottom, u32::from(n), self.cell);
        }
    }

    /// Mirror of [`State::scroll_up`].
    pub(super) fn scroll_down(&mut self, top: u16, bottom: u16, n: u16) {
        let n = n.min(bottom - top + 1);
        let blank = self.blank();
        self.screen.grid.scroll_down_in(top, bottom, n, blank);
        self.images.scroll_region_down(top, bottom, u32::from(n));
    }

    /// SU: scroll the region, leaving the cursor.
    pub(super) fn scroll_region_up(&mut self, n: u16) {
        let Region { top, bottom } = self.screen.region;
        self.scroll_up(top, bottom, n);
    }

    /// SD: scroll the region, leaving the cursor.
    pub(super) fn scroll_region_down(&mut self, n: u16) {
        let Region { top, bottom } = self.screen.region;
        self.scroll_down(top, bottom, n);
    }

    /// IND, LF, VT, FF: down one row, scrolling the region at its bottom
    /// margin. Below the region the cursor moves but never scrolls.
    pub(super) fn index(&mut self) {
        let Region { top, bottom } = self.screen.region;
        if self.screen.row == bottom {
            self.scroll_up(top, bottom, 1);
        } else if self.screen.row < self.last_row() {
            self.screen.row += 1;
        }
        self.screen.wrap_pending = false;
    }

    /// RI: up one row, scrolling the region down at its top margin.
    pub(super) fn reverse_index(&mut self) {
        let Region { top, bottom } = self.screen.region;
        if self.screen.row == top {
            self.scroll_down(top, bottom, 1);
        } else {
            self.screen.row = self.screen.row.saturating_sub(1);
        }
        self.screen.wrap_pending = false;
    }

    /// NEL.
    pub(super) fn next_line(&mut self) {
        self.goto(0, self.screen.row);
        self.index();
    }

    /// CUU: stops at the region top when the cursor starts inside the region.
    pub(super) fn cursor_up(&mut self, n: u16) {
        let region = self.screen.region;
        let limit = if region.contains(self.screen.row) {
            region.top
        } else {
            0
        };
        let row = self.screen.row.saturating_sub(n).max(limit);
        self.goto(self.screen.col, row);
    }

    /// CUD: stops at the region bottom when the cursor starts inside it.
    pub(super) fn cursor_down(&mut self, n: u16) {
        let region = self.screen.region;
        let limit = if region.contains(self.screen.row) {
            region.bottom
        } else {
            self.last_row()
        };
        let row = self.screen.row.saturating_add(n).min(limit);
        self.goto(self.screen.col, row);
    }

    /// CNL: down by lines, to column 0, with the CUD limits.
    pub(super) fn cursor_next_line(&mut self, n: u16) {
        self.cursor_down(n);
        self.goto(0, self.screen.row);
    }

    /// CPL: up by lines, to column 0, with the CUU limits.
    pub(super) fn cursor_previous_line(&mut self, n: u16) {
        self.cursor_up(n);
        self.goto(0, self.screen.row);
    }

    /// ICH. The cursor stays; a pending wrap is cancelled like any edit at
    /// the cursor.
    pub(super) fn insert_chars(&mut self, n: u16) {
        let blank = self.blank();
        let (col, row) = (self.screen.col, self.screen.row);
        self.screen.grid.insert_cells(row, col, n, blank);
        self.screen.wrap_pending = false;
    }

    /// DCH.
    pub(super) fn delete_chars(&mut self, n: u16) {
        let blank = self.blank();
        let (col, row) = (self.screen.col, self.screen.row);
        self.screen.grid.delete_cells(row, col, n, blank);
        self.screen.wrap_pending = false;
    }

    /// ECH: blanks `n` cells from the cursor without moving anything.
    pub(super) fn erase_chars(&mut self, n: u16) {
        let blank = self.blank();
        let (col, row) = (self.screen.col, self.screen.row);
        let cols = col..col.saturating_add(n);
        self.screen.grid.erase_cells(row, cols.clone(), blank);
        self.images.remove_sixels_over(row, cols, self.cell);
        self.screen.wrap_pending = false;
    }

    /// IL: opens lines at the cursor row inside the region; outside it the
    /// sequence is ignored entirely, cursor included.
    pub(super) fn insert_lines(&mut self, n: u16) {
        let Region { bottom, .. } = self.screen.region;
        let row = self.screen.row;
        if self.screen.region.contains(row) {
            self.scroll_down(row, bottom, n);
            self.goto(0, row);
        }
    }

    /// DL: mirror of [`State::insert_lines`].
    pub(super) fn delete_lines(&mut self, n: u16) {
        let Region { bottom, .. } = self.screen.region;
        let row = self.screen.row;
        if self.screen.region.contains(row) {
            self.scroll_up(row, bottom, n);
            self.goto(0, row);
        }
    }

    /// REP. The count is capped at one screenful so a hostile `CSI 65535 b`
    /// cannot stall the parser on a tiny terminal.
    pub(super) fn repeat_last_char(&mut self, n: u16) {
        let Some(ch) = self.last_char else { return };
        let size = self.screen.grid.size();
        let cap = u32::from(size.cols()) * u32::from(size.rows());
        for _ in 0..u32::from(n).min(cap) {
            self.print(ch);
        }
    }

    /// Fills `cols` of `row` with blanks, removing the sixels under them.
    pub(super) fn erase(&mut self, row: u16, cols: std::ops::Range<u16>) {
        let blank = self.blank();
        self.screen.grid.row_mut(row)[usize::from(cols.start)..usize::from(cols.end)].fill(blank);
        self.images.remove_sixels_over(row, cols, self.cell);
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
    use super::super::testing::*;
    use crate::grid::Region;

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

    /// A 5x5 terminal whose rows read `a`..`e`, cursor parked at home.
    fn lettered() -> crate::Terminal {
        let mut t = sized(5, 5);
        t.advance(b"a\r\nb\r\nc\r\nd\r\ne\x1b[H");
        t
    }

    fn rows(t: &crate::Terminal) -> String {
        (0..t.size().rows())
            .map(|r| text(t, r))
            .map(|l| if l.is_empty() { ".".into() } else { l })
            .collect()
    }

    fn region(t: &crate::Terminal) -> Region {
        t.state.screen.region
    }

    #[test]
    fn decstbm_without_parameters_or_with_zeros_is_the_full_screen() {
        let mut t = lettered();
        t.advance(b"\x1b[2;4r");
        assert_eq!(region(&t), Region { top: 1, bottom: 3 });
        t.advance(b"\x1b[r");
        assert_eq!(region(&t), Region::full(5));
        t.advance(b"\x1b[2;4r\x1b[0;0r");
        assert_eq!(region(&t), Region::full(5));
    }

    #[test]
    fn decstbm_defaults_a_missing_bound_and_clamps_to_the_screen() {
        let mut t = lettered();
        t.advance(b"\x1b[;3r");
        assert_eq!(region(&t), Region { top: 0, bottom: 2 });
        t.advance(b"\x1b[2r");
        assert_eq!(region(&t), Region { top: 1, bottom: 4 });
        t.advance(b"\x1b[2;99r");
        assert_eq!(region(&t), Region { top: 1, bottom: 4 });
    }

    #[test]
    fn an_invalid_region_is_ignored_and_leaves_the_cursor_alone() {
        let mut t = lettered();
        t.advance(b"\x1b[2;4r\x1b[3;2H");
        for invalid in [&b"\x1b[3;3r"[..], b"\x1b[4;2r", b"\x1b[99;99r"] {
            t.advance(invalid);
            assert_eq!(region(&t), Region { top: 1, bottom: 3 });
            assert_eq!(pos(&t), (1, 2), "{invalid:?} must not home the cursor");
        }
    }

    #[test]
    fn a_valid_region_homes_the_cursor() {
        let mut t = lettered();
        t.advance(b"\x1b[4;4H\x1b[2;4r");
        assert_eq!(pos(&t), (0, 0));
        t.advance(b"\x1b[?6h\x1b[4;4H\x1b[3;5r");
        assert_eq!(pos(&t), (0, 2), "region top-left under DECOM");
    }

    #[test]
    fn origin_mode_makes_cup_hvp_and_vpa_relative_to_the_region() {
        let mut t = lettered();
        t.advance(b"\x1b[2;4r\x1b[?6h");
        t.advance(b"\x1b[1;1H");
        assert_eq!(pos(&t), (0, 1));
        t.advance(b"\x1b[99;3H");
        assert_eq!(pos(&t), (2, 3), "clamped to the region bottom");
        t.advance(b"\x1b[2;1f");
        assert_eq!(pos(&t), (0, 2));
        t.advance(b"\x1b[1d");
        assert_eq!(pos(&t), (0, 1));
        t.advance(b"\x1b[?6l\x1b[1;1H");
        assert_eq!(pos(&t), (0, 0), "absolute again with DECOM off");
    }

    #[test]
    fn cursor_position_report_is_relative_to_the_region_under_origin_mode() {
        let mut t = lettered();
        t.advance(b"\x1b[3;5r\x1b[?6h\x1b[2;2H\x1b[6n");
        assert_eq!(pos(&t), (1, 3), "row 2 of the region is screen row 4");
        assert_eq!(t.take_responses(), b"\x1b[2;2R");
        t.advance(b"\x1b[?6l\x1b[4;2H\x1b[6n");
        assert_eq!(t.take_responses(), b"\x1b[4;2R", "absolute with DECOM off");
    }

    #[test]
    fn toggling_origin_mode_homes_the_cursor() {
        let mut t = lettered();
        t.advance(b"\x1b[2;4r\x1b[4;4H\x1b[?6h");
        assert_eq!(pos(&t), (0, 1));
        t.advance(b"\x1b[3;3H\x1b[?6l");
        assert_eq!(pos(&t), (0, 0));
    }

    #[test]
    fn origin_mode_is_saved_and_restored_with_the_cursor() {
        let mut t = lettered();
        t.advance(b"\x1b[2;4r\x1b[?6h\x1b7\x1b[?6l\x1b8");
        t.advance(b"\x1b[1;1H");
        assert_eq!(pos(&t), (0, 1), "DECRC brought DECOM back");
    }

    #[test]
    fn lf_at_the_region_bottom_scrolls_only_the_region() {
        let mut t = lettered();
        t.advance(b"\x1b[2;4r\x1b[4;1H\n");
        assert_eq!(rows(&t), "acd.e");
        assert_eq!(pos(&t), (0, 3));
    }

    #[test]
    fn lf_inside_or_below_the_region_moves_without_scrolling() {
        let mut t = lettered();
        t.advance(b"\x1b[1;3r\x1b[2;1H\n");
        assert_eq!(pos(&t), (0, 2), "inside the region");
        t.advance(b"\x1b[4;1H\n");
        assert_eq!(pos(&t), (0, 4), "below the region the cursor still moves");
        t.advance(b"\n");
        assert_eq!(pos(&t), (0, 4), "last row outside the region: no scroll");
        assert_eq!(rows(&t), "abcde");
    }

    #[test]
    fn vt_ff_and_ind_scroll_like_lf() {
        for seq in [&b"\x0b"[..], b"\x0c", b"\x1bD"] {
            let mut t = lettered();
            t.advance(b"\x1b[2;4r\x1b[4;1H");
            t.advance(seq);
            assert_eq!(rows(&t), "acd.e", "{seq:?}");
        }
    }

    #[test]
    fn nel_is_a_carriage_return_plus_index() {
        let mut t = lettered();
        t.advance(b"\x1b[2;4r\x1b[4;3H\x1bE");
        assert_eq!(rows(&t), "acd.e");
        assert_eq!(pos(&t), (0, 3));
        t.advance(b"\x1b[2;3H\x1bE");
        assert_eq!(pos(&t), (0, 2), "inside the region it only moves down");
    }

    #[test]
    fn ri_at_the_region_top_scrolls_the_region_down() {
        let mut t = lettered();
        t.advance(b"\x1b[2;4r\x1b[2;1H\x1bM");
        assert_eq!(rows(&t), "a.bce");
        assert_eq!(pos(&t), (0, 1));
    }

    #[test]
    fn ri_elsewhere_moves_up_and_stops_at_the_screen_top() {
        let mut t = lettered();
        t.advance(b"\x1b[3;1H\x1bM");
        assert_eq!((pos(&t), rows(&t).as_str()), ((0, 1), "abcde"));
        t.advance(b"\x1b[1;1H\x1bM");
        assert_eq!(rows(&t), ".abcd", "the full-screen region scrolls down");
        t.advance(b"\x1b[2;4r\x1b[1;1H\x1bM");
        assert_eq!(
            rows(&t),
            ".abcd",
            "above the region it stays on the top row"
        );
    }

    #[test]
    fn index_operations_cancel_a_pending_wrap() {
        for seq in [&b"\n"[..], b"\x1bD", b"\x1bM", b"\x1bE"] {
            let mut t = sized(3, 3);
            t.advance(b"abc");
            assert!(t.state.screen.wrap_pending);
            t.advance(seq);
            assert!(!t.state.screen.wrap_pending, "{seq:?}");
        }
    }

    #[test]
    fn printing_past_the_region_bottom_scrolls_the_region() {
        let mut t = sized(2, 4);
        t.advance(b"a\r\nb\r\nc\r\nd\x1b[2;3r\x1b[3;1Hxyz");
        assert_eq!(rows(&t), "axyzd");
    }

    #[test]
    fn region_scroll_moves_placements_inside_it_and_spares_the_rest() {
        let mut t = lettered();
        t.advance(&kitty_rgba(1, 10, 20, ""));
        t.advance(b"\x1b[4;1H");
        t.advance(&kitty_rgba(2, 10, 20, ""));
        t.advance(b"\x1b[2;4r\x1b[4;1H\n");
        let rows: Vec<(u32, i32)> = t
            .images()
            .placements()
            .iter()
            .map(|p| (t.images().image(p.image).unwrap().id, p.row))
            .collect();
        assert_eq!(rows, [(1, 0), (2, 2)], "row 0 is above the region");
    }

    #[test]
    fn a_full_screen_lf_still_scrolls_every_placement() {
        let mut t = lettered();
        t.advance(&kitty_rgba(1, 10, 20, ""));
        t.advance(b"\x1b[5;1H\n");
        assert!(t.images().placements().is_empty());
    }

    #[test]
    fn cuu_and_cud_stop_at_the_region_edge_when_starting_inside() {
        let mut t = lettered();
        t.advance(b"\x1b[2;4r\x1b[3;1H\x1b[9A");
        assert_eq!(pos(&t), (0, 1));
        t.advance(b"\x1b[9B");
        assert_eq!(pos(&t), (0, 3));
    }

    #[test]
    fn cuu_and_cud_use_the_screen_edge_when_starting_outside() {
        let mut t = lettered();
        t.advance(b"\x1b[2;4r\x1b[5;1H\x1b[9A");
        assert_eq!(pos(&t), (0, 0), "below the region it may cross it");
        t.advance(b"\x1b[9B");
        assert_eq!(pos(&t), (0, 4));
    }

    #[test]
    fn su_scrolls_the_region_up_and_leaves_the_cursor() {
        let mut t = lettered();
        t.advance(b"\x1b[2;4r\x1b[3;2H\x1b[2S");
        assert_eq!(rows(&t), "ad..e");
        assert_eq!(pos(&t), (1, 2));
        t.advance(b"\x1b[99S");
        assert_eq!(rows(&t), "a...e", "n is clamped to the region height");
    }

    #[test]
    fn sd_scrolls_the_region_down_and_leaves_the_cursor() {
        let mut t = lettered();
        t.advance(b"\x1b[2;4r\x1b[3;2H\x1b[T");
        assert_eq!(rows(&t), "a.bce");
        assert_eq!(pos(&t), (1, 2));
        t.advance(b"\x1b[99T");
        assert_eq!(rows(&t), "a...e");
    }

    #[test]
    fn su_without_a_region_scrolls_the_whole_screen() {
        let mut t = lettered();
        t.advance(b"\x1b[S");
        assert_eq!(rows(&t), "bcde.");
    }

    fn cells(t: &crate::Terminal, row: u16) -> String {
        t.row(row).iter().map(|c| c.ch).collect()
    }

    #[test]
    fn ich_opens_blank_cells_and_pushes_the_rest_right() {
        let mut t = term(5, 1);
        t.advance(b"abcd\x1b[2G\x1b[2@");
        assert_eq!(cells(&t, 0), "a  bc");
        assert_eq!(pos(&t), (1, 0), "the cursor stays");
        t.advance(b"\x1b[@");
        assert_eq!(cells(&t, 0), "a   b", "no parameter means one");
        t.advance(b"\x1b[0@");
        assert_eq!(cells(&t, 0), "a    ", "zero also means one");
    }

    #[test]
    fn dch_pulls_the_rest_left_and_ech_blanks_in_place() {
        let mut t = term(5, 1);
        t.advance(b"abcde\x1b[2G\x1b[2P");
        assert_eq!(cells(&t, 0), "ade  ");
        assert_eq!(pos(&t), (1, 0));
        let mut t = term(5, 1);
        t.advance(b"abcde\x1b[2G\x1b[2X");
        assert_eq!(cells(&t, 0), "a  de");
        assert_eq!(pos(&t), (1, 0));
        t.advance(b"\x1b[99X");
        assert_eq!(cells(&t, 0), "a    ", "n is clamped to the row");
    }

    #[test]
    fn cell_edits_blank_with_the_pen_background_and_cancel_a_pending_wrap() {
        use crate::Color;
        let mut t = term(5, 1);
        t.advance(b"abcde\x1b[44m\x1b[@");
        let last = t.row(0)[4];
        assert_eq!((last.ch, last.bg), (' ', Color::Indexed(4)));
        assert_eq!(
            t.row(0)[0].bg,
            Color::default(),
            "only the blank is colored"
        );
        t.advance(b"x");
        assert_eq!(
            cells(&t, 0),
            "abcdx",
            "no wrap: the pending wrap was cancelled"
        );
        assert_eq!(pos(&t), (4, 0));
    }

    #[test]
    fn dch_and_ech_cancel_a_pending_wrap_too() {
        for edit in [&b"\x1b[P"[..], &b"\x1b[X"[..]] {
            let mut t = term(5, 2);
            t.advance(b"abcde");
            t.advance(edit);
            t.advance(b"x");
            assert_eq!(cells(&t, 0), "abcdx");
            assert_eq!(text(&t, 1), "");
        }
    }

    #[test]
    fn il_inserts_lines_inside_the_region_and_homes_the_column() {
        let mut t = lettered();
        t.advance(b"\x1b[2;4r\x1b[3;3H\x1b[L");
        assert_eq!(rows(&t), "ab.ce");
        assert_eq!(pos(&t), (0, 2));
        t.advance(b"\x1b[99L");
        assert_eq!(
            rows(&t),
            "ab..e",
            "n is clamped to the rows below the cursor"
        );
    }

    #[test]
    fn il_at_the_region_top_shifts_rows_down_like_vim_ctrl_y() {
        let mut t = lettered();
        // Vim scrolls the window back one line with a region and IL at its top.
        t.advance(b"\x1b[2;4r\x1b[2;1H\x1b[L");
        assert_eq!(rows(&t), "a.bce", "the row at the region bottom is dropped");
        assert_eq!(pos(&t), (0, 1));
    }

    #[test]
    fn dl_deletes_lines_inside_the_region_and_pulls_the_bottom_up() {
        let mut t = lettered();
        t.advance(b"\x1b[2;4r\x1b[3;3H\x1b[M");
        assert_eq!(rows(&t), "abd.e");
        assert_eq!(pos(&t), (0, 2));
        t.advance(b"\x1b[2M");
        assert_eq!(rows(&t), "ab..e");
    }

    #[test]
    fn il_and_dl_do_nothing_outside_the_region() {
        let mut t = lettered();
        t.advance(b"\x1b[2;4r\x1b[5;3H\x1b[L\x1b[M");
        assert_eq!(rows(&t), "abcde");
        assert_eq!(pos(&t), (2, 4), "not even the column moves");
        t.advance(b"\x1b[1;3H\x1b[L\x1b[M");
        assert_eq!(rows(&t), "abcde");
        assert_eq!(pos(&t), (2, 0));
    }

    #[test]
    fn il_and_dl_without_a_region_edit_the_whole_screen() {
        let mut t = lettered();
        t.advance(b"\x1b[2;1H\x1b[L");
        assert_eq!(rows(&t), "a.bcd");
        t.advance(b"\x1b[M");
        assert_eq!(rows(&t), "abcd.");
    }

    #[test]
    fn il_and_dl_blank_with_the_pen_background_and_cancel_a_pending_wrap() {
        use crate::Color;
        let mut t = term(3, 3);
        t.advance(b"abc\x1b[44m\x1b[L");
        assert_eq!(t.row(0)[0].bg, Color::Indexed(4));
        t.advance(b"x");
        assert_eq!(pos(&t), (1, 0), "column 0 and no pending wrap");
    }

    #[test]
    fn il_and_dl_shift_placements_from_the_cursor_row() {
        let place_at = |row: &[u8]| {
            let mut t = lettered();
            t.advance(b"\x1b[3;1H");
            t.advance(&kitty_rgba(1, 10, 20, ""));
            t.advance(b"\x1b[2;4r");
            t.advance(row);
            t
        };
        let anchor = |t: &crate::Terminal| -> Vec<i32> {
            t.images().placements().iter().map(|p| p.row).collect()
        };
        assert_eq!(anchor(&place_at(b"\x1b[2;1H\x1b[L")), [3]);
        assert_eq!(anchor(&place_at(b"\x1b[2;1H\x1b[M")), [1]);
        assert_eq!(
            anchor(&place_at(b"\x1b[4;1H\x1b[L")),
            [2],
            "rows above the cursor row are not part of the edit"
        );
    }

    #[test]
    fn rep_repeats_the_last_printed_character() {
        let mut t = term(8, 1);
        t.advance(b"x\x1b[3b");
        assert_eq!(text(&t, 0), "xxxx");
        t.advance(b"\x1b[b");
        assert_eq!(text(&t, 0), "xxxxx", "no parameter means one");
        t.advance(b"\x1b[0b");
        assert_eq!(text(&t, 0), "xxxxxx", "zero also means one");
        t.advance(b"y\x1b[b");
        assert_eq!(text(&t, 0), "xxxxxxyy");
    }

    #[test]
    fn rep_does_nothing_without_a_printed_character() {
        let mut t = term(8, 1);
        t.advance(b"\x1b[3b");
        assert_eq!(text(&t, 0), "");
        assert_eq!(pos(&t), (0, 0));
    }

    #[test]
    fn controls_csi_and_esc_forget_the_last_character() {
        for between in [&b"\r"[..], &b"\x1b[m"[..], &b"\x1b7"[..], &b"\n"[..]] {
            let mut t = term(8, 2);
            t.advance(b"x");
            t.advance(between);
            let before = (cells(&t, 0), pos(&t));
            t.advance(b"\x1b[3b");
            assert_eq!((cells(&t, 0), pos(&t)), before, "after {between:?}");
        }
    }

    #[test]
    fn rep_count_is_capped_at_the_screen_size() {
        let mut t = term(80, 24);
        t.advance(b"x\x1b[65535b");
        // 1 + 80 * 24 characters end one column into the last row.
        assert_eq!(pos(&t), (1, 23));
    }

    #[test]
    fn cnl_and_cpl_move_by_lines_to_column_zero() {
        let mut t = lettered();
        t.advance(b"\x1b[2;4H\x1b[2E");
        assert_eq!(pos(&t), (0, 3));
        t.advance(b"\x1b[3;4H\x1b[F");
        assert_eq!(pos(&t), (0, 1));
        t.advance(b"\x1b[3;4H\x1b[0E");
        assert_eq!(pos(&t), (0, 3), "zero means one");
    }

    #[test]
    fn cnl_and_cpl_stop_at_the_region_edge_only_when_starting_inside() {
        let mut t = lettered();
        t.advance(b"\x1b[2;4r\x1b[3;3H\x1b[99E");
        assert_eq!(pos(&t), (0, 3));
        t.advance(b"\x1b[3;3H\x1b[99F");
        assert_eq!(pos(&t), (0, 1));
        t.advance(b"\x1b[5;3H\x1b[99F");
        assert_eq!(pos(&t), (0, 0), "below the region it may cross it");
        t.advance(b"\x1b[1;3H\x1b[99E");
        assert_eq!(pos(&t), (0, 4), "above the region it may cross it");
    }
}
