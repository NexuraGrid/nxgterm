//! Text selection: what is selected and the text it covers.
//!
//! Positions are [`Point`]s in absolute line coordinates: line 0 is the
//! first line that ever entered the history, and every line keeps its
//! number while it scrolls from the screen into the history and while the
//! viewport moves. Lines dropped from the front of a full history keep
//! the numbers of the lines after them unchanged, so a selection survives
//! scrolling and new output below it; lines it covered that are gone are
//! simply left out of its text.
//!
//! Pure: the cells come through the [`Lines`] trait, implemented by the
//! terminal (and by plain vectors in the tests).

use std::ops::{Range, RangeInclusive};

use crate::cell::{Cell, wraps};

/// A cell position: absolute line, then column. Ordered in reading order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Point {
    pub line: u64,
    pub col: u16,
}

impl Point {
    pub fn new(line: u64, col: u16) -> Self {
        Self { line, col }
    }
}

/// How a selection grows from the cells it was made with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKind {
    /// Cell by cell, in reading order (click and drag).
    Simple,
    /// Whole words (double click); see [`is_word_char`].
    Word,
    /// Whole lines, soft-wrapped continuations included (triple click).
    Line,
    /// A rectangle of columns (Alt and drag).
    Block,
}

/// The lines a selection reads from.
pub trait Lines {
    /// Columns of the grid; rows shorter than this end in blanks.
    fn cols(&self) -> u16;
    /// The cells of absolute `line`, `None` when it no longer (or does not
    /// yet) exist. History lines may be shorter or longer than [`Lines::cols`].
    fn line(&self, line: u64) -> Option<&[Cell]>;
}

/// The selected cells, after expanding words and lines. `start` and `end`
/// are both included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: Point,
    pub end: Point,
    /// Only the columns `start.col..=end.col` of every line.
    pub block: bool,
}

impl Span {
    /// The absolute lines the span touches.
    pub fn lines(&self) -> RangeInclusive<u64> {
        self.start.line..=self.end.line
    }

    /// The selected columns of `line` on a grid `cols` wide.
    pub fn cols_on(&self, line: u64, cols: u16) -> Option<Range<u16>> {
        if !self.lines().contains(&line) || cols == 0 {
            return None;
        }
        let last = cols - 1;
        let (first, end) = if self.block {
            (self.start.col, self.end.col)
        } else {
            let first = if line == self.start.line {
                self.start.col
            } else {
                0
            };
            let end = if line == self.end.line {
                self.end.col
            } else {
                last
            };
            (first, end)
        };
        let end = end.min(last);
        (first <= end).then(|| first..end + 1)
    }

    /// Whether `point` is selected.
    pub fn contains(&self, point: Point) -> bool {
        // Columns past the grid never count; u16::MAX is wide enough.
        self.cols_on(point.line, u16::MAX)
            .is_some_and(|cols| cols.contains(&point.col))
    }
}

/// A selection being made: where it started (`anchor`) and where the
/// pointer is now (`head`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    kind: SelectionKind,
    anchor: Point,
    head: Point,
}

impl Selection {
    /// A selection of `kind` started at `at`.
    pub fn new(kind: SelectionKind, at: Point) -> Self {
        Self {
            kind,
            anchor: at,
            head: at,
        }
    }

    pub fn kind(&self) -> SelectionKind {
        self.kind
    }

    pub fn anchor(&self) -> Point {
        self.anchor
    }

    pub fn head(&self) -> Point {
        self.head
    }

    /// Moves the free end to `head`.
    pub fn update(&mut self, head: Point) {
        self.head = head;
    }

    /// The selected cells, words and lines expanded over `lines`.
    pub fn span(&self, lines: &impl Lines) -> Span {
        let (start, end) = if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        };
        let last = lines.cols().saturating_sub(1);
        match self.kind {
            SelectionKind::Simple => Span {
                start,
                end,
                block: false,
            },
            SelectionKind::Word => Span {
                start: word_start(lines, start),
                end: word_end(lines, end),
                block: false,
            },
            SelectionKind::Line => Span {
                start: Point::new(logical_first(lines, start.line), 0),
                end: Point::new(logical_last(lines, end.line), last),
                block: false,
            },
            SelectionKind::Block => Span {
                start: Point::new(start.line, self.anchor.col.min(self.head.col)),
                end: Point::new(end.line, self.anchor.col.max(self.head.col)),
                block: true,
            },
        }
    }
}

/// Characters that end a word besides whitespace. Slashes, dots, dashes
/// and the like are not here, so paths and most of a URL select whole.
pub const WORD_SEPARATORS: &str = "()[]{}<>'\"`,;:│";

/// Whether `ch` belongs to a word for double-click selection.
pub fn is_word_char(ch: char) -> bool {
    !ch.is_whitespace() && !WORD_SEPARATORS.contains(ch)
}

/// The character at `point`, blank past the end of a short line.
fn char_at(lines: &impl Lines, point: Point) -> Option<char> {
    let line = lines.line(point.line)?;
    Some(line.get(usize::from(point.col)).map_or(' ', |cell| cell.ch))
}

/// The cell before `point`, continuing on the end of the previous line
/// when that line soft-wraps into this one.
fn before(lines: &impl Lines, point: Point) -> Option<Point> {
    if point.col > 0 {
        return Some(Point::new(point.line, point.col - 1));
    }
    let prev = point.line.checked_sub(1)?;
    let cells = lines.line(prev)?;
    wraps(cells).then(|| Point::new(prev, last_col(lines, cells)))
}

/// Mirror of [`before`].
fn after(lines: &impl Lines, point: Point) -> Option<Point> {
    let cells = lines.line(point.line)?;
    let last = last_col(lines, cells);
    if point.col < last {
        return Some(Point::new(point.line, point.col + 1));
    }
    let next = point.line + 1;
    (wraps(cells) && lines.line(next).is_some()).then_some(Point::new(next, 0))
}

/// The last column of a line: the grid's, or the line's own when it is a
/// longer history line.
fn last_col(lines: &impl Lines, cells: &[Cell]) -> u16 {
    let len = u16::try_from(cells.len()).unwrap_or(u16::MAX);
    lines.cols().max(len).saturating_sub(1)
}

fn word_start(lines: &impl Lines, mut point: Point) -> Point {
    if !char_at(lines, point).is_some_and(is_word_char) {
        return point;
    }
    while let Some(prev) = before(lines, point) {
        if !char_at(lines, prev).is_some_and(is_word_char) {
            break;
        }
        point = prev;
    }
    point
}

fn word_end(lines: &impl Lines, mut point: Point) -> Point {
    if !char_at(lines, point).is_some_and(is_word_char) {
        return point;
    }
    while let Some(next) = after(lines, point) {
        if !char_at(lines, next).is_some_and(is_word_char) {
            break;
        }
        point = next;
    }
    point
}

/// The first line of the logical (unwrapped) line holding `line`.
fn logical_first(lines: &impl Lines, mut line: u64) -> u64 {
    while let Some(prev) = line.checked_sub(1) {
        if !lines.line(prev).is_some_and(wraps) {
            break;
        }
        line = prev;
    }
    line
}

/// The last line of the logical line holding `line`.
fn logical_last(lines: &impl Lines, mut line: u64) -> u64 {
    while lines.line(line).is_some_and(wraps) && lines.line(line + 1).is_some() {
        line += 1;
    }
    line
}

/// The text of `span`: each line's selected cells with trailing blanks
/// trimmed, lines joined with `\n`, except that a line that soft-wraps
/// into the next joins it directly (and keeps its trailing blanks, which
/// are real spaces). Lines that no longer exist are left out.
pub fn text(span: &Span, lines: &impl Lines) -> String {
    let mut out = String::new();
    for line in span.lines() {
        let Some(cells) = lines.line(line) else {
            continue;
        };
        let Some(cols) = span.cols_on(line, lines.cols().max(1)) else {
            continue;
        };
        // A middle line of a stream selection runs to its own end, which
        // may lie past the grid for a longer history line.
        let to_end = !span.block && line != span.end.line;
        let end = if to_end {
            cells.len()
        } else {
            usize::from(cols.end).min(cells.len())
        };
        let start = usize::from(cols.start).min(end);
        let chunk: String = cells[start..end].iter().map(|cell| cell.ch).collect();
        let joined = to_end && wraps(cells);
        if joined {
            out.push_str(&chunk);
        } else {
            out.push_str(chunk.trim_end_matches(' '));
            if line != span.end.line {
                out.push('\n');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::Flags;

    /// Lines of text, a trailing `\` marking a soft wrap; `first` is the
    /// absolute number of the first one.
    struct Text {
        cols: u16,
        first: u64,
        rows: Vec<Vec<Cell>>,
    }

    fn lines(cols: u16, first: u64, text: &[&str]) -> Text {
        let rows = text
            .iter()
            .map(|row| {
                let (row, wrapped) = match row.strip_suffix('\\') {
                    Some(row) => (row, true),
                    None => (*row, false),
                };
                let mut cells: Vec<Cell> = row
                    .chars()
                    .map(|ch| Cell {
                        ch,
                        ..Cell::default()
                    })
                    .collect();
                cells.resize(usize::from(cols), Cell::default());
                if wrapped {
                    cells.last_mut().unwrap().flags.insert(Flags::WRAPLINE);
                }
                cells
            })
            .collect();
        Text { cols, first, rows }
    }

    impl Lines for Text {
        fn cols(&self) -> u16 {
            self.cols
        }

        fn line(&self, line: u64) -> Option<&[Cell]> {
            let index = line.checked_sub(self.first)?;
            self.rows
                .get(usize::try_from(index).ok()?)
                .map(Vec::as_slice)
        }
    }

    fn p(line: u64, col: u16) -> Point {
        Point::new(line, col)
    }

    fn select(kind: SelectionKind, from: Point, to: Point, src: &Text) -> (Span, String) {
        let mut selection = Selection::new(kind, from);
        selection.update(to);
        let span = selection.span(src);
        (span, text(&span, src))
    }

    #[test]
    fn points_order_by_line_then_column() {
        assert!(p(1, 9) < p(2, 0));
        assert!(p(2, 1) < p(2, 3));
    }

    #[test]
    fn a_simple_selection_runs_in_reading_order_either_way() {
        let src = lines(6, 0, &["hello", "world"]);
        let (span, text) = select(SelectionKind::Simple, p(0, 3), p(1, 1), &src);
        assert_eq!((span.start, span.end), (p(0, 3), p(1, 1)));
        assert_eq!(text, "lo\nwo");
        let (_, backwards) = select(SelectionKind::Simple, p(1, 1), p(0, 3), &src);
        assert_eq!(backwards, "lo\nwo");
    }

    #[test]
    fn a_single_cell_is_selected_on_its_own() {
        let src = lines(6, 0, &["hello"]);
        let (_, text) = select(SelectionKind::Simple, p(0, 1), p(0, 1), &src);
        assert_eq!(text, "e");
    }

    #[test]
    fn trailing_blanks_of_each_row_are_trimmed() {
        let src = lines(8, 0, &["ab", "  cd", ""]);
        let (_, text) = select(SelectionKind::Simple, p(0, 0), p(2, 7), &src);
        assert_eq!(text, "ab\n  cd\n");
    }

    #[test]
    fn soft_wrapped_rows_join_without_a_newline() {
        let src = lines(4, 0, &["echo\\", " hi \\", "x", "next"]);
        let (_, text) = select(SelectionKind::Simple, p(0, 0), p(3, 3), &src);
        assert_eq!(text, "echo hi x\nnext", "the space at the wrap is kept");
    }

    #[test]
    fn coordinates_are_absolute_line_numbers() {
        let src = lines(5, 100, &["old", "new"]);
        let (_, text) = select(SelectionKind::Simple, p(100, 0), p(101, 4), &src);
        assert_eq!(text, "old\nnew");
    }

    #[test]
    fn lines_that_are_gone_are_left_out() {
        let src = lines(5, 10, &["kept", "too"]);
        let (_, text) = select(SelectionKind::Simple, p(8, 0), p(11, 4), &src);
        assert_eq!(text, "kept\ntoo");
    }

    #[test]
    fn history_lines_longer_than_the_grid_are_read_whole() {
        let mut src = lines(3, 0, &["abc", "de"]);
        src.rows[0].push(Cell {
            ch: 'X',
            ..Cell::default()
        });
        let (_, text) = select(SelectionKind::Simple, p(0, 1), p(1, 2), &src);
        assert_eq!(text, "bcX\nde");
    }

    #[test]
    fn word_chars_keep_paths_and_urls_whole() {
        for ch in [
            'a', 'Z', '0', '/', '.', '-', '_', '~', '@', '=', '?', '&', '%', 'é',
        ] {
            assert!(is_word_char(ch), "{ch:?}");
        }
        for ch in [
            ' ', '\t', '(', ')', '[', ']', '{', '}', '<', '>', '\'', '"', '`', ',', ';', ':', '│',
        ] {
            assert!(!is_word_char(ch), "{ch:?}");
        }
    }

    #[test]
    fn a_double_click_selects_the_word_under_it() {
        let src = lines(30, 0, &["ls (~/src/nxg-term.rs) done"]);
        let (span, text) = select(SelectionKind::Word, p(0, 8), p(0, 8), &src);
        assert_eq!(text, "~/src/nxg-term.rs");
        assert_eq!((span.start, span.end), (p(0, 4), p(0, 20)));
    }

    #[test]
    fn a_double_click_on_a_separator_selects_just_it() {
        let src = lines(10, 0, &["a (b)"]);
        let (_, text) = select(SelectionKind::Word, p(0, 2), p(0, 2), &src);
        assert_eq!(text, "(");
    }

    #[test]
    fn dragging_a_word_selection_extends_by_whole_words() {
        let src = lines(20, 0, &["one two three", "four five"]);
        let (_, text) = select(SelectionKind::Word, p(0, 5), p(1, 1), &src);
        assert_eq!(text, "two three\nfour");
        let (_, back) = select(SelectionKind::Word, p(1, 1), p(0, 5), &src);
        assert_eq!(back, "two three\nfour");
    }

    #[test]
    fn words_continue_across_a_soft_wrap() {
        let src = lines(4, 0, &["a ht\\", "tp:/\\", "/x y"]);
        let (_, text) = select(SelectionKind::Word, p(1, 1), p(1, 1), &src);
        assert_eq!(text, "http", "the colon ends it");
        let (_, text) = select(SelectionKind::Word, p(2, 1), p(2, 1), &src);
        assert_eq!(text, "//x");
    }

    #[test]
    fn a_triple_click_selects_the_whole_logical_line() {
        let src = lines(4, 0, &["one", "long\\", "line", "end"]);
        let (span, text) = select(SelectionKind::Line, p(2, 1), p(2, 1), &src);
        assert_eq!((span.start, span.end), (p(1, 0), p(2, 3)));
        assert_eq!(text, "longline");
        let (_, text) = select(SelectionKind::Line, p(0, 2), p(3, 0), &src);
        assert_eq!(text, "one\nlongline\nend");
    }

    #[test]
    fn a_block_selection_takes_the_same_columns_of_every_line() {
        let src = lines(6, 0, &["abcdef", "ghijkl", "mn"]);
        let (span, text) = select(SelectionKind::Block, p(0, 4), p(2, 1), &src);
        assert!(span.block);
        assert_eq!(span.cols_on(1, 6), Some(1..5));
        assert_eq!(text, "bcde\nhijk\nn");
    }

    #[test]
    fn cols_on_covers_the_first_middle_and_last_lines() {
        let span = Span {
            start: p(5, 3),
            end: p(7, 1),
            block: false,
        };
        assert_eq!(span.cols_on(4, 10), None);
        assert_eq!(span.cols_on(5, 10), Some(3..10));
        assert_eq!(span.cols_on(6, 10), Some(0..10));
        assert_eq!(span.cols_on(7, 10), Some(0..2));
        assert_eq!(span.cols_on(8, 10), None);
        assert_eq!(span.cols_on(5, 2), None, "starts past a narrower grid");
        assert!(span.contains(p(6, 99)) && !span.contains(p(7, 2)));
    }
}
