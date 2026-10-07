//! Fixed-size grid of cells.

use crate::TermSize;
use crate::cell::Cell;

/// An inclusive range of rows (a scroll region). Valid regions keep
/// `top < bottom < rows`; anything else means the whole screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub top: u16,
    pub bottom: u16,
}

impl Region {
    /// The region covering all `rows` rows.
    pub fn full(rows: u16) -> Self {
        Self {
            top: 0,
            bottom: rows.saturating_sub(1),
        }
    }

    pub fn is_full(self, rows: u16) -> bool {
        self == Self::full(rows)
    }

    pub fn contains(self, row: u16) -> bool {
        (self.top..=self.bottom).contains(&row)
    }
}

/// A `cols x rows` grid stored row-major.
#[derive(Debug, Clone)]
pub struct Grid {
    size: TermSize,
    cells: Vec<Cell>,
}

impl Grid {
    pub fn new(size: TermSize) -> Self {
        let len = usize::from(size.cols()) * usize::from(size.rows());
        Self {
            size,
            cells: vec![Cell::default(); len],
        }
    }

    pub fn size(&self) -> TermSize {
        self.size
    }

    /// The cells of `row`. Panics if `row` is out of bounds.
    pub fn row(&self, row: u16) -> &[Cell] {
        &self.cells[self.span(row)]
    }

    pub fn row_mut(&mut self, row: u16) -> &mut [Cell] {
        let span = self.span(row);
        &mut self.cells[span]
    }

    /// Moves rows `top..=bottom` up by `n`, blanking the rows freed at the
    /// bottom. Rows outside the region never move; `n` is clamped to the
    /// region height.
    pub fn scroll_up_in(&mut self, top: u16, bottom: u16, n: u16, blank: Cell) {
        let Some((start, end, shift)) = self.region_span(top, bottom, n) else {
            return;
        };
        self.cells.copy_within(start + shift..end, start);
        self.cells[end - shift..end].fill(blank);
    }

    /// Mirror of [`Grid::scroll_up_in`]: rows move down, the top is blanked.
    pub fn scroll_down_in(&mut self, top: u16, bottom: u16, n: u16, blank: Cell) {
        let Some((start, end, shift)) = self.region_span(top, bottom, n) else {
            return;
        };
        self.cells.copy_within(start..end - shift, start + shift);
        self.cells[start..start + shift].fill(blank);
    }

    /// Cell offsets `(start, end, shift)` of a scroll, or `None` when there
    /// is nothing to move. Panics on rows outside the grid, like `row()`.
    fn region_span(&self, top: u16, bottom: u16, n: u16) -> Option<(usize, usize, usize)> {
        let start = self.span(top).start;
        let end = self.span(bottom).end;
        let cols = usize::from(self.size.cols());
        let shift = usize::from(n) * cols;
        (start < end && shift > 0).then_some((start, end, shift.min(end - start)))
    }

    /// ICH: opens `n` blank cells at `col`, pushing the rest of the row right;
    /// cells pushed past the edge are lost. `n` is clamped to the remaining
    /// width. Panics on a row outside the grid, like `row()`.
    pub fn insert_cells(&mut self, row: u16, col: u16, n: u16, blank: Cell) {
        let cells = self.row_mut(row);
        let col = usize::from(col).min(cells.len());
        let n = usize::from(n).min(cells.len() - col);
        cells[col..].rotate_right(n);
        cells[col..col + n].fill(blank);
    }

    /// DCH: removes `n` cells at `col`, pulling the rest of the row left and
    /// blanking the freed tail.
    pub fn delete_cells(&mut self, row: u16, col: u16, n: u16, blank: Cell) {
        let cells = self.row_mut(row);
        let col = usize::from(col).min(cells.len());
        let n = usize::from(n).min(cells.len() - col);
        cells[col..].rotate_left(n);
        let len = cells.len();
        cells[len - n..].fill(blank);
    }

    /// ECH: blanks `cols` in place; the range is clamped to the row.
    pub fn erase_cells(&mut self, row: u16, cols: std::ops::Range<u16>, blank: Cell) {
        let cells = self.row_mut(row);
        let end = usize::from(cols.end).min(cells.len());
        let start = usize::from(cols.start).min(end);
        cells[start..end].fill(blank);
    }

    /// Resizes keeping the top-left content; new cells are blank.
    pub fn resize(&mut self, size: TermSize) {
        let mut next = Self::new(size);
        let cols = usize::from(size.cols().min(self.size.cols()));
        for row in 0..size.rows().min(self.size.rows()) {
            next.row_mut(row)[..cols].copy_from_slice(&self.row(row)[..cols]);
        }
        *self = next;
    }

    fn span(&self, row: u16) -> std::ops::Range<usize> {
        assert!(row < self.size.rows(), "row {row} out of bounds");
        let cols = usize::from(self.size.cols());
        let start = usize::from(row) * cols;
        start..start + cols
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(cols: u16, rows: u16) -> TermSize {
        TermSize::new(cols, rows).unwrap()
    }

    fn put(grid: &mut Grid, col: u16, row: u16, ch: char) {
        grid.row_mut(row)[col as usize].ch = ch;
    }

    #[test]
    fn full_region_spans_every_row() {
        let region = Region::full(5);
        assert_eq!((region.top, region.bottom), (0, 4));
        assert!(region.is_full(5));
        assert!(
            !region.is_full(6),
            "a stale region is not full after growth"
        );
        assert!(!Region { top: 1, bottom: 4 }.is_full(5));
    }

    #[test]
    fn region_contains_is_inclusive() {
        let region = Region { top: 1, bottom: 3 };
        let inside: Vec<u16> = (0..5).filter(|&r| region.contains(r)).collect();
        assert_eq!(inside, [1, 2, 3]);
    }

    #[test]
    fn new_grid_is_blank() {
        let grid = Grid::new(size(3, 2));
        assert_eq!(grid.row(0), &[Cell::default(); 3]);
        assert_eq!(grid.row(1).len(), 3);
    }

    fn letters(rows: u16) -> Grid {
        let mut grid = Grid::new(size(2, rows));
        for r in 0..rows {
            put(&mut grid, 0, r, (b'a' + r as u8) as char);
        }
        grid
    }

    fn column(grid: &Grid) -> String {
        (0..grid.size().rows())
            .map(|r| grid.row(r)[0].ch)
            .map(|c| if c == ' ' { '.' } else { c })
            .collect()
    }

    fn dot() -> Cell {
        Cell {
            ch: '.',
            ..Cell::default()
        }
    }

    #[test]
    fn scroll_up_in_shifts_only_the_region_rows() {
        let mut grid = letters(5);
        grid.scroll_up_in(1, 3, 1, dot());
        assert_eq!(column(&grid), "acd.e");
        assert_eq!(grid.row(3), &[dot(); 2], "the new row takes the blank cell");
    }

    #[test]
    fn scroll_down_in_shifts_only_the_region_rows() {
        let mut grid = letters(5);
        grid.scroll_down_in(1, 3, 1, dot());
        assert_eq!(column(&grid), "a.bce");
    }

    #[test]
    fn scrolling_by_more_than_one_row_moves_every_row() {
        let mut up = letters(5);
        up.scroll_up_in(0, 4, 2, dot());
        assert_eq!(column(&up), "cde..");
        let mut down = letters(5);
        down.scroll_down_in(0, 4, 2, dot());
        assert_eq!(column(&down), "..abc");
    }

    #[test]
    fn scroll_count_is_clamped_to_the_region_height() {
        let mut up = letters(5);
        up.scroll_up_in(1, 3, 99, dot());
        assert_eq!(column(&up), "a...e");
        let mut down = letters(5);
        down.scroll_down_in(1, 3, 99, dot());
        assert_eq!(column(&down), "a...e");
    }

    #[test]
    fn scrolling_by_zero_changes_nothing() {
        let mut grid = letters(3);
        grid.scroll_up_in(0, 2, 0, dot());
        grid.scroll_down_in(0, 2, 0, dot());
        assert_eq!(column(&grid), "abc");
    }

    #[test]
    fn a_one_row_region_is_blanked() {
        let mut grid = letters(3);
        grid.scroll_up_in(1, 1, 1, dot());
        assert_eq!(column(&grid), "a.c");
        let mut grid = letters(3);
        grid.scroll_down_in(2, 2, 1, dot());
        assert_eq!(column(&grid), "ab.");
    }

    #[test]
    fn a_one_row_grid_scrolls_without_panicking() {
        let mut grid = letters(1);
        grid.scroll_up_in(0, 0, 1, dot());
        assert_eq!(column(&grid), ".");
    }

    #[test]
    #[should_panic(expected = "out of bounds")]
    fn scrolling_a_region_past_the_grid_panics_like_row() {
        letters(3).scroll_up_in(1, 3, 1, dot());
    }

    /// One row of `abcde` (plus a second row that must never change).
    fn word() -> Grid {
        let mut grid = Grid::new(size(5, 2));
        for (i, ch) in "abcde".chars().enumerate() {
            put(&mut grid, i as u16, 0, ch);
            put(&mut grid, i as u16, 1, 'z');
        }
        grid
    }

    fn line(grid: &Grid, row: u16) -> String {
        grid.row(row).iter().map(|c| c.ch).collect()
    }

    #[test]
    fn insert_cells_shifts_right_and_drops_the_overflow() {
        let mut grid = word();
        grid.insert_cells(0, 1, 2, dot());
        assert_eq!(line(&grid, 0), "a..bc");
        assert_eq!(line(&grid, 1), "zzzzz", "other rows are untouched");
    }

    #[test]
    fn delete_cells_shifts_left_and_blanks_the_tail() {
        let mut grid = word();
        grid.delete_cells(0, 1, 2, dot());
        assert_eq!(line(&grid, 0), "ade..");
        assert_eq!(line(&grid, 1), "zzzzz");
    }

    #[test]
    fn erase_cells_blanks_the_range_in_place() {
        let mut grid = word();
        grid.erase_cells(0, 1..3, dot());
        assert_eq!(line(&grid, 0), "a..de");
    }

    #[test]
    fn cell_edit_counts_are_clamped_to_the_remaining_width() {
        let mut grid = word();
        grid.insert_cells(0, 3, 99, dot());
        assert_eq!(line(&grid, 0), "abc..");
        let mut grid = word();
        grid.delete_cells(0, 3, 99, dot());
        assert_eq!(line(&grid, 0), "abc..");
        let mut grid = word();
        grid.erase_cells(0, 3..99, dot());
        assert_eq!(line(&grid, 0), "abc..");
    }

    #[test]
    fn cell_edits_at_the_last_column_touch_only_that_cell() {
        let mut grid = word();
        grid.insert_cells(0, 4, 1, dot());
        assert_eq!(line(&grid, 0), "abcd.");
        let mut grid = word();
        grid.delete_cells(0, 4, 1, dot());
        assert_eq!(line(&grid, 0), "abcd.");
    }

    #[test]
    fn empty_cell_edits_change_nothing() {
        let mut grid = word();
        grid.insert_cells(0, 1, 0, dot());
        grid.delete_cells(0, 1, 0, dot());
        grid.erase_cells(0, 2..2, dot());
        // Built from variables: clippy rejects a literal reversed range.
        let (start, end) = (3, 1);
        grid.erase_cells(0, start..end, dot());
        grid.insert_cells(0, 5, 2, dot());
        grid.delete_cells(0, 7, 2, dot());
        assert_eq!(line(&grid, 0), "abcde");
    }

    #[test]
    #[should_panic(expected = "out of bounds")]
    fn cell_edits_past_the_last_row_panic_like_row() {
        word().insert_cells(2, 0, 1, dot());
    }

    #[test]
    fn resize_keeps_top_left_content() {
        let mut grid = Grid::new(size(3, 3));
        put(&mut grid, 0, 0, 'a');
        put(&mut grid, 2, 2, 'z');
        grid.resize(size(2, 4));
        assert_eq!(grid.size(), size(2, 4));
        assert_eq!(grid.row(0)[0].ch, 'a');
        assert_eq!(grid.row(2), &[Cell::default(); 2]);
        assert_eq!(grid.row(3).len(), 2);
    }
}
