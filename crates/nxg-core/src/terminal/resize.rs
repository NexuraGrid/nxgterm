//! Resizing the main screen without losing content, like Alacritty: rows
//! that no longer fit go into the history and come back when the window
//! grows again, and a width change rewraps soft-wrapped lines (history
//! included) to the new width. The alternate screen is only cut (see
//! [`Screen::resize`]): its programs redraw on SIGWINCH.
//!
//! A reflow touches only rows that need it: a history line that was not
//! wrapped and fits the new width keeps its box (and its old width; the
//! viewport cuts or pads it), so widening costs the wrapped lines and
//! narrowing the lines longer than the new width. Image placements stay
//! where they were on a reflow (they move with their rows otherwise).

use super::screen::Screen;
use super::scrollback::Scrollback;
use crate::TermSize;
use crate::cell::{Cell, Color, Flags, repair_wide, wraps};
use crate::grid::{Grid, Region};

/// Whether `cell` shows nothing: a plain space on the default background.
/// Blank rows below the cursor are the first to go when the screen
/// shrinks.
fn is_blank(cell: &Cell) -> bool {
    cell.ch == ' '
        && cell.marks.is_empty()
        && cell.bg == Color::Default
        && !cell.flags.contains(Flags::INVERSE)
        && !cell.flags.contains(Flags::UNDERLINE)
        && !cell.flags.contains(Flags::WIDE_SPACER)
}

/// The rows of `screen` that hold something: up to the last non-blank row
/// or the cursor row, whichever is lower.
fn used_rows(screen: &Screen) -> u16 {
    let rows = screen.grid.size().rows();
    let content = (0..rows)
        .rev()
        .find(|&row| !screen.grid.row(row).iter().all(is_blank))
        .map_or(0, |row| row + 1);
    content.max(screen.row + 1)
}

/// The first row shown after a resize, out of the history and screen rows
/// laid out one after the other (`end` of them, the cursor on row
/// `cursor`), when row `top` keeps the screen where it was (the line of
/// the cursor starting on the same screen row) and the screen goes from
/// `old_rows` to `new_rows` rows.
///
/// Growing shows that many history rows again at the top (the cursor
/// moves down). Rows that no longer fit go into the history from the top,
/// but only as far as the cursor: then the rows below it are cut instead.
fn first_shown(top: usize, cursor: usize, old_rows: u16, new_rows: u16, end: usize) -> usize {
    let rows = usize::from(new_rows);
    let top = top.min(cursor);
    let first = top.saturating_sub(rows.saturating_sub(usize::from(old_rows)));
    if end > first + rows {
        (end - rows).min(cursor)
    } else {
        first
    }
}

/// Cells up to the last one that shows something.
fn content_len(cells: &[Cell]) -> usize {
    cells
        .iter()
        .rposition(|cell| !is_blank(cell))
        .map_or(0, |at| at + 1)
}

/// A cell position in rows laid out one after the other (history first).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Point {
    row: usize,
    col: usize,
}

/// Rewraps `rows` (oldest first) to `cols` columns: each run of rows
/// joined by [`Flags::WRAPLINE`] is one logical line, its trailing blanks
/// trimmed (never before a point on it) and laid out again, wide chars
/// moved whole to the next row behind a [`Flags::WIDE_LEADER`]. Each of
/// `points` is moved to the cell it was on; a point past the end of its
/// line is reached by padding the line with blanks.
fn rewrap(rows: Vec<Box<[Cell]>>, cols: u16, points: &mut [Point]) -> Vec<Box<[Cell]>> {
    let cols = usize::from(cols);
    let mut out = Vec::with_capacity(rows.len());
    let mut line: Vec<Cell> = Vec::new();
    // (point, offset in the line) for the points on the current line.
    let mut found: Vec<(usize, usize)> = Vec::new();
    // Where the points were: `points` gets where they go.
    let wanted = points.to_vec();
    let mut rows = rows.into_iter().enumerate();
    while let Some((mut index, mut row)) = rows.next() {
        let marked = wanted.iter().any(|p| p.row == index);
        if !wraps(&row) && content_len(&row) <= cols && !marked {
            out.push(row); // Nothing to rewrap: keep the box.
            continue;
        }
        line.clear();
        found.clear();
        loop {
            let wrapped = wraps(&row);
            let leader = wrapped
                && row
                    .last()
                    .is_some_and(|c| c.flags.contains(Flags::WIDE_LEADER));
            let take = row.len() - usize::from(leader);
            for (at, point) in wanted.iter().enumerate() {
                if point.row == index {
                    found.push((at, line.len() + point.col.min(take)));
                }
            }
            line.extend(row[..take].iter().map(|cell| {
                let mut cell = *cell;
                cell.flags.remove(Flags::WRAPLINE);
                cell
            }));
            if !wrapped {
                break;
            }
            match rows.next() {
                Some(next) => (index, row) = next,
                None => break,
            }
        }
        let reach = found
            .iter()
            .map(|&(_, offset)| offset + 1)
            .max()
            .unwrap_or(0);
        let len = content_len(&line).max(reach);
        line.resize(len, Cell::default());
        lay_out(&line, cols, &mut out, &found, points);
    }
    out
}

/// Appends logical `line` to `out` as rows `cols` wide, moving each of
/// `found` (point, offset in the line) to its new row and column.
fn lay_out(
    line: &[Cell],
    cols: usize,
    out: &mut Vec<Box<[Cell]>>,
    found: &[(usize, usize)],
    points: &mut [Point],
) {
    fn finish(row: &mut Vec<Cell>, cols: usize, wrapped: bool, out: &mut Vec<Box<[Cell]>>) {
        row.resize(cols, Cell::default());
        if wrapped {
            row[cols - 1].flags.insert(Flags::WRAPLINE);
        }
        out.push(std::mem::replace(row, Vec::with_capacity(cols)).into_boxed_slice());
    }
    let mut row = Vec::with_capacity(cols);
    let mut i = 0;
    while i < line.len() {
        let mut cell = line[i];
        let pair = cell.flags.contains(Flags::WIDE)
            && line
                .get(i + 1)
                .is_some_and(|next| next.flags.contains(Flags::WIDE_SPACER));
        let width = if pair && cols > 1 { 2 } else { 1 };
        if pair && cols == 1 {
            cell.unwide(); // A one-column grid cannot hold it.
        }
        if row.len() + width > cols {
            let mut leader = Cell::default();
            leader.flags.insert(Flags::WIDE_LEADER);
            row.push(leader);
            finish(&mut row, cols, true, out);
        }
        let span = if pair { 2 } else { 1 };
        for &(point, offset) in found {
            if (i..i + span).contains(&offset) {
                let col = row.len() + (offset - i).min(width - 1);
                points[point] = Point {
                    row: out.len(),
                    col,
                };
            }
        }
        row.push(cell);
        if width == 2 {
            row.push(line[i + 1]);
        }
        i += span;
        if row.len() == cols && i < line.len() {
            finish(&mut row, cols, true, out);
        }
    }
    finish(&mut row, cols, false, out);
}

/// Copies history or screen row `src` into grid row `dst` of maybe another
/// width, keeping its soft wrap on the last cell.
fn copy_row(dst: &mut [Cell], src: &[Cell]) {
    let n = dst.len().min(src.len());
    dst[..n].copy_from_slice(&src[..n]);
    if n > 0 && n < dst.len() {
        dst[n - 1].flags.remove(Flags::WRAPLINE);
    }
    if wraps(src) {
        if let Some(last) = dst.last_mut() {
            last.flags.insert(Flags::WRAPLINE);
        }
    }
    repair_wide(dst);
}

/// Resizes the main screen, moving rows between it and `scrollback` (see
/// [`first_shown`]). Returns how many rows moved up into the history
/// (negative: came back down from it), for the images on screen.
pub(super) fn resize_main(
    screen: &mut Screen,
    scrollback: &mut Scrollback,
    size: TermSize,
) -> isize {
    let old = screen.grid.size();
    screen.region = Region::full(size.rows());
    if old == size {
        return 0;
    }
    if old.cols() != size.cols() {
        reflow_main(screen, scrollback, size);
        return 0;
    }
    let history = scrollback.len();
    let used = usize::from(used_rows(screen));
    let cursor = history + usize::from(screen.row);
    let first = first_shown(history, cursor, old.rows(), size.rows(), history + used);
    let mut grid = Grid::new(size);
    let rows = usize::from(size.rows());
    let mut at = 0;
    if first < history {
        let mut pulled = Vec::with_capacity(history - first);
        pulled.extend((first..history).filter_map(|_| scrollback.pop()));
        for line in pulled.iter().rev() {
            copy_row(grid.row_mut(at as u16), line);
            at += 1;
        }
    }
    let skip = first.saturating_sub(history);
    for row in 0..skip {
        scrollback.push(screen.grid.row(row as u16));
    }
    for row in skip..usize::from(old.rows()) {
        if at == rows {
            break;
        }
        copy_row(grid.row_mut(at as u16), screen.grid.row(row as u16));
        at += 1;
    }
    screen.grid = grid;
    let place = |row: u16, col: u16| {
        let row = (history + usize::from(row))
            .saturating_sub(first)
            .min(rows - 1);
        (col.min(size.cols() - 1), row as u16)
    };
    (screen.col, screen.row) = place(screen.row, screen.col);
    screen.wrap_pending &= old.cols() == size.cols();
    if let Some(saved) = &mut screen.saved {
        (saved.col, saved.row) = place(saved.row, saved.col);
    }
    first as isize - history as isize
}

/// [`resize_main`] for a width change: the history and the used screen
/// rows are rewrapped (see [`rewrap`]); the line of the cursor keeps its
/// first screen row when it can.
fn reflow_main(screen: &mut Screen, scrollback: &mut Scrollback, size: TermSize) {
    let old = screen.grid.size();
    let history = scrollback.len();
    let used = used_rows(screen);
    let mut rows = scrollback.take();
    rows.reserve(usize::from(used));
    rows.extend((0..used).map(|row| Box::<[Cell]>::from(screen.grid.row(row))));
    let cursor = history + usize::from(screen.row);
    let start = (0..cursor)
        .rev()
        .take_while(|&row| wraps(&rows[row]))
        .last()
        .unwrap_or(cursor);
    let point = |row: u16, col: u16| Point {
        row: history + usize::from(row),
        col: usize::from(col),
    };
    let mut points = vec![Point { row: start, col: 0 }, point(screen.row, screen.col)];
    // A saved cursor below the used rows keeps its distance to the cursor.
    let saved = screen.saved.filter(|saved| saved.row < used);
    if let Some(saved) = saved {
        points.push(point(saved.row, saved.col));
    }
    let laid = rewrap(rows, size.cols(), &mut points);
    let (new_start, new_cursor) = (points[0].row, points[1].row);
    let top = (new_start + history).saturating_sub(start);
    let first = first_shown(top, new_cursor, old.rows(), size.rows(), laid.len());
    let mut laid = laid.into_iter();
    for line in laid.by_ref().take(first) {
        scrollback.push_owned(line);
    }
    let mut grid = Grid::new(size);
    for (at, line) in laid.take(usize::from(size.rows())).enumerate() {
        copy_row(grid.row_mut(at as u16), &line);
    }
    screen.grid = grid;
    let (last_row, last_col) = (usize::from(size.rows()) - 1, size.cols() - 1);
    let place = |point: Point| {
        let row = point.row.saturating_sub(first).min(last_row) as u16;
        (row, (point.col.min(usize::from(last_col))) as u16)
    };
    let old_row = screen.row;
    let (row, col) = place(points[1]);
    screen.row = row;
    // The next char goes after the one the cursor was on: a pending wrap
    // stays pending only in the last column.
    (screen.col, screen.wrap_pending) = match screen.wrap_pending && col < last_col {
        true => (col + 1, false),
        false => (col, screen.wrap_pending),
    };
    if let Some(kept) = &mut screen.saved {
        (kept.row, kept.col) = match saved {
            Some(_) => place(points[2]),
            None => place(Point {
                row: new_cursor + usize::from(kept.row - old_row),
                col: usize::from(kept.col),
            }),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use crate::cell::{Cell, Flags};
    use crate::{TermSize, Terminal};

    fn resize(t: &mut Terminal, cols: u16, rows: u16) {
        t.resize(TermSize::new(cols, rows).unwrap());
        t.state.assert_invariants();
    }

    /// The whole history, oldest first, trailing blanks trimmed.
    fn history(t: &Terminal) -> Vec<String> {
        let scrollback = &t.state.scrollback;
        (0..scrollback.len())
            .map(|i| {
                let line: String = scrollback.line(i).iter().map(|c| c.ch).collect();
                line.trim_end().to_owned()
            })
            .collect()
    }

    fn screen(t: &Terminal) -> Vec<String> {
        (0..t.size().rows()).map(|r| text(t, r)).collect()
    }

    #[test]
    fn shrinking_keeps_the_prompt_and_pushes_the_top_rows_into_history() {
        let mut t = term(10, 5);
        t.advance(b"a\r\nb\r\nc\r\nd\r\n$ ");
        resize(&mut t, 10, 3);
        assert_eq!(screen(&t), ["c", "d", "$"]);
        assert_eq!(history(&t), ["a", "b"]);
        assert_eq!(pos(&t), (2, 2));
    }

    #[test]
    fn shrinking_drops_blank_rows_below_the_cursor_first() {
        let mut t = term(10, 5);
        t.advance(b"a\r\n$ ");
        resize(&mut t, 10, 3);
        assert_eq!(screen(&t), ["a", "$", ""]);
        assert!(history(&t).is_empty());
        assert_eq!(pos(&t), (2, 1));
    }

    #[test]
    fn shrinking_keeps_text_below_the_cursor_while_the_cursor_stays_visible() {
        let mut t = term(10, 5);
        t.advance(b"a\r\nb\r\nc\r\nd\r\ne\x1b[2;1H");
        resize(&mut t, 10, 3);
        // Two rows must go: the top one, then (to keep the cursor) the
        // bottom one.
        assert_eq!(history(&t), ["a"]);
        assert_eq!(screen(&t), ["b", "c", "d"]);
        assert_eq!(pos(&t), (0, 0));
    }

    #[test]
    fn growing_pulls_rows_back_from_history() {
        let mut t = term(10, 5);
        t.advance(b"a\r\nb\r\nc\r\nd\r\n$ ");
        resize(&mut t, 10, 3);
        resize(&mut t, 10, 4);
        assert_eq!(screen(&t), ["b", "c", "d", "$"]);
        assert_eq!(history(&t), ["a"]);
        resize(&mut t, 10, 7);
        assert_eq!(screen(&t), ["a", "b", "c", "d", "$", "", ""]);
        assert!(history(&t).is_empty());
        assert_eq!(pos(&t), (2, 4));
    }

    #[test]
    fn the_saved_cursor_moves_with_its_row() {
        let mut t = term(10, 5);
        t.advance(b"a\r\nb\r\nc\r\nd\x1b7\r\n$ ");
        resize(&mut t, 10, 3);
        t.advance(b"\x1b8");
        assert_eq!(pos(&t), (1, 1), "still after the d");
    }

    #[test]
    fn the_history_limit_holds_while_shrinking() {
        let mut t = term(10, 5);
        t.set_scrollback_limit(1);
        t.advance(b"a\r\nb\r\nc\r\nd\r\n$ ");
        resize(&mut t, 10, 2);
        assert_eq!(history(&t), ["c"]);
        assert_eq!(screen(&t), ["d", "$"]);
    }

    #[test]
    fn the_alternate_screen_is_cut_and_leaves_the_history_alone() {
        let mut t = term(10, 5);
        t.advance(b"m1\r\nm2\r\nm3\r\nm4\r\n$ \x1b[?1049h");
        t.advance(b"\ra1\r\na2\r\na3\r\na4\r\na5");
        resize(&mut t, 10, 3);
        assert_eq!(screen(&t), ["a1", "a2", "a3"]);
        assert_eq!(pos(&t), (2, 2));
        // The main screen behind it kept its prompt.
        t.advance(b"\x1b[?1049l");
        assert_eq!(screen(&t), ["m3", "m4", "$"]);
        assert_eq!(history(&t), ["m1", "m2"]);
        // Growing on the alternate screen pulls nothing into it.
        t.advance(b"\x1b[?1049h");
        resize(&mut t, 10, 5);
        assert_eq!(t.state.scrollback.len(), 0, "pulled back into main");
        assert_eq!(text(&t, 0), "", "a fresh alternate screen");
    }

    fn cells(t: &Terminal) -> Vec<Vec<Cell>> {
        (0..t.size().rows()).map(|r| t.row(r).to_vec()).collect()
    }

    #[test]
    fn narrowing_wraps_long_lines_and_widening_joins_them_back_exactly() {
        let mut t = term(10, 3);
        t.advance(b"\x1b[1m0123\x1b[31m456789\x1b[0mabcde\r\n$ ");
        let before = cells(&t);
        resize(&mut t, 4, 3);
        assert_eq!(history(&t), ["0123", "4567"]);
        assert_eq!(screen(&t), ["89ab", "cde", "$"]);
        assert_eq!(pos(&t), (2, 2));
        assert!(t.row(0)[3].flags.contains(Flags::WRAPLINE));
        assert!(!t.row(1)[3].flags.contains(Flags::WRAPLINE));
        resize(&mut t, 10, 3);
        assert!(history(&t).is_empty());
        assert_eq!(cells(&t), before, "attributes and wraps restored");
        assert_eq!(pos(&t), (2, 2));
    }

    #[test]
    fn a_line_wrapped_in_the_history_joins_back_onto_the_screen() {
        let mut t = term(4, 2);
        t.advance(b"abcdefghij\r\n$ ");
        assert_eq!(history(&t), ["abcd", "efgh"]);
        resize(&mut t, 12, 2);
        assert!(history(&t).is_empty());
        assert_eq!(screen(&t), ["abcdefghij", "$"]);
        assert_eq!(pos(&t), (2, 1));
    }

    #[test]
    fn widening_pulls_history_down_to_keep_the_prompt_in_place() {
        let mut t = term(5, 3);
        t.advance(b"old\r\nabcdefgh\r\n$ ");
        assert_eq!(history(&t), ["old"]);
        resize(&mut t, 10, 3);
        assert_eq!(screen(&t), ["old", "abcdefgh", "$"]);
        assert_eq!(pos(&t), (2, 2));
    }

    #[test]
    fn a_wide_char_at_the_edge_moves_whole_to_the_next_row() {
        let mut t = term(6, 3);
        t.advance("abc日本".as_bytes());
        assert!(t.row(0)[5].flags.contains(Flags::WIDE_LEADER));
        assert_eq!(t.row(1)[0].ch, '本');
        resize(&mut t, 10, 3);
        assert_eq!(t.row(0)[3].ch, '日');
        assert_eq!(t.row(0)[5].ch, '本', "the leader is gone");
        assert!(t.row(0)[5].flags.contains(Flags::WIDE));
        assert_eq!(pos(&t), (7, 0));
        resize(&mut t, 4, 3);
        let row = t.row(0);
        assert_eq!(row[2].ch, 'c');
        assert!(row[3].flags.contains(Flags::WIDE_LEADER));
        assert!(row[3].flags.contains(Flags::WRAPLINE));
        let row = t.row(1);
        assert!(row[0].ch == '日' && row[0].flags.contains(Flags::WIDE));
        assert!(row[1].flags.contains(Flags::WIDE_SPACER));
        assert_eq!(row[2].ch, '本');
        // The cursor after 本 is past the full row: on the next one.
        assert_eq!(pos(&t), (0, 2));
        resize(&mut t, 1, 3);
        resize(&mut t, 6, 3);
    }

    #[test]
    fn the_cursor_keeps_its_place_in_a_reflowed_line() {
        let mut t = term(10, 4);
        t.advance(b"0123456789abc\x1b[1;8H");
        resize(&mut t, 5, 4);
        assert_eq!(screen(&t), ["01234", "56789", "abc", ""]);
        assert_eq!(pos(&t), (2, 1), "still on the 7");
        t.advance(b"\x1b[3;2H"); // On the b.
        resize(&mut t, 4, 4);
        assert_eq!(screen(&t), ["0123", "4567", "89ab", "c"]);
        assert_eq!(pos(&t), (3, 2));
    }

    #[test]
    fn a_pending_wrap_continues_after_the_last_char() {
        let mut t = term(5, 2);
        t.advance(b"abcde");
        resize(&mut t, 8, 2);
        t.advance(b"x");
        assert_eq!(text(&t, 0), "abcdex");
        let mut t = term(5, 2);
        t.advance(b"abcde");
        resize(&mut t, 3, 2);
        assert_eq!(screen(&t), ["abc", "de"]);
        t.advance(b"x");
        assert_eq!(screen(&t), ["abc", "dex"]);
        t.advance(b"y");
        assert_eq!(text(&t, 1), "y", "the wrap was still pending");
    }

    #[test]
    fn the_history_limit_holds_while_narrowing() {
        let mut t = term(8, 2);
        t.set_scrollback_limit(3);
        t.advance(b"aaaaaaaa\r\nbbbbbbbb\r\n$");
        resize(&mut t, 2, 2);
        assert_eq!(t.state.scrollback.len(), 3);
        assert_eq!(history(&t), ["bb", "bb", "bb"]);
        assert_eq!(screen(&t), ["bb", "$"]);
    }

    #[test]
    fn the_saved_cursor_follows_its_char_through_a_reflow() {
        let mut t = term(10, 3);
        t.advance(b"0123456789abc\x1b[2;2H\x1b7\x1b[3;1H");
        resize(&mut t, 4, 4);
        t.advance(b"\x1b8");
        assert_eq!(t.row(t.cursor().row)[usize::from(t.cursor().col)].ch, 'b');
    }

    #[test]
    fn reflowing_the_alternate_screen_never_happens() {
        let mut t = term(10, 3);
        t.advance(b"\x1b[?1049h0123456789ab");
        resize(&mut t, 5, 3);
        assert_eq!(screen(&t), ["01234", "ab", ""]);
        assert_eq!(t.state.scrollback.len(), 0);
    }

    /// A tiny deterministic generator, so the test needs no dependency.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self, below: u64) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.0 >> 33) % below
        }
    }

    /// The text of every logical line in history and on screen: wrapped
    /// rows joined, spacers and leaders left out, trailing blanks trimmed.
    fn logical_lines(t: &Terminal) -> Vec<String> {
        let scrollback = &t.state.scrollback;
        let rows = (0..scrollback.len())
            .map(|i| scrollback.line(i))
            .chain((0..t.size().rows()).map(|r| t.row(r)));
        let mut lines = vec![String::new()];
        for row in rows {
            let line = lines.last_mut().unwrap();
            for cell in row {
                if !cell.flags.contains(Flags::WIDE_SPACER)
                    && !cell.flags.contains(Flags::WIDE_LEADER)
                {
                    line.push(cell.ch);
                }
            }
            if !crate::cell::wraps(row) {
                lines.push(String::new());
            }
        }
        let mut lines: Vec<String> = lines.iter().map(|l| l.trim_end().to_owned()).collect();
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        lines
    }

    #[test]
    fn random_resizes_keep_every_logical_line() {
        let words = ["a", "bc", "def", "ghij", "日本", "x", "é", "klmno", "🙂"];
        for seed in 1..40 {
            let mut rng = Lcg(seed);
            let mut t = term(2 + rng.next(30) as u16, 2 + rng.next(10) as u16);
            t.set_scrollback_limit(100_000);
            let mut expected = Vec::new();
            for _ in 0..1 + rng.next(12) {
                let mut line = String::new();
                for _ in 0..rng.next(14) {
                    line.push_str(words[rng.next(words.len() as u64) as usize]);
                    if rng.next(3) == 0 {
                        line.push(' ');
                    }
                }
                expected.push(line);
            }
            let input = expected.join("\r\n");
            t.advance(input.as_bytes());
            for _ in 0..8 {
                let cols = 2 + rng.next(40) as u16;
                let rows = 1 + rng.next(12) as u16;
                resize(&mut t, cols, rows);
            }
            let expected: Vec<String> = expected.iter().map(|l| l.trim_end().to_owned()).collect();
            let mut expected = expected;
            while expected.last().is_some_and(String::is_empty) {
                expected.pop();
            }
            assert_eq!(logical_lines(&t), expected, "seed {seed}");
        }
    }
}
