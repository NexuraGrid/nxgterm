//! Resizing the main screen without losing content, like Alacritty: rows
//! that no longer fit go into the history and come back when the window
//! grows again. The alternate screen is only cut (see [`Screen::resize`]):
//! its programs redraw on SIGWINCH.

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
/// `cursor`, which was row `cursor_row` of an `old_rows` high screen).
///
/// The cursor stays on its screen row when it can. Growing shows that many
/// history rows again at the top (the cursor moves down). Rows that no
/// longer fit go into the history from the top, but only as far as the
/// cursor: then the rows below it are cut instead.
fn first_shown(cursor: usize, cursor_row: u16, old_rows: u16, new_rows: u16, end: usize) -> usize {
    let rows = usize::from(new_rows);
    let top = cursor.saturating_sub(usize::from(cursor_row));
    let first = top.saturating_sub(rows.saturating_sub(usize::from(old_rows)));
    if end > first + rows {
        (end - rows).min(cursor)
    } else {
        first
    }
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
    let history = scrollback.len();
    let used = usize::from(used_rows(screen));
    let cursor = history + usize::from(screen.row);
    let first = first_shown(cursor, screen.row, old.rows(), size.rows(), history + used);
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

#[cfg(test)]
mod tests {
    use super::super::testing::*;
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
}
