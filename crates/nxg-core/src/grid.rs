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
