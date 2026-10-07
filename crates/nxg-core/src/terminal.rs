//! Terminal state machine: feeds child output through a VT parser into a grid.

use crate::TermSize;
use crate::cell::{Cell, Color, Flags};
use crate::grid::Grid;

/// Cursor position (zero-based) and visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub col: u16,
    pub row: u16,
    pub visible: bool,
}

/// Screen state driven by [`Terminal::advance`].
pub struct Terminal {
    parser: vte::Parser,
    state: State,
}

impl std::fmt::Debug for Terminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Terminal")
            .field("size", &self.size())
            .field("cursor", &self.cursor())
            .finish_non_exhaustive()
    }
}

/// Everything the parser mutates; split from the parser to satisfy borrows.
#[derive(Debug)]
struct State {
    grid: Grid,
    cursor: Cursor,
    pen: Cell,
    /// Set after printing in the last column; the next print wraps first.
    wrap_pending: bool,
}

impl Terminal {
    pub fn new(size: TermSize) -> Self {
        Self {
            parser: vte::Parser::new(),
            state: State {
                grid: Grid::new(size),
                cursor: Cursor {
                    col: 0,
                    row: 0,
                    visible: true,
                },
                pen: Cell::default(),
                wrap_pending: false,
            },
        }
    }

    /// Feeds raw child output.
    pub fn advance(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.state, bytes);
    }

    pub fn resize(&mut self, size: TermSize) {
        let state = &mut self.state;
        state.grid.resize(size);
        state.cursor.col = state.cursor.col.min(size.cols() - 1);
        state.cursor.row = state.cursor.row.min(size.rows() - 1);
        state.wrap_pending = false;
    }

    pub fn size(&self) -> TermSize {
        self.state.grid.size()
    }

    pub fn cursor(&self) -> Cursor {
        self.state.cursor
    }

    /// The cells of `row`. Panics if `row` is out of bounds.
    pub fn row(&self, row: u16) -> &[Cell] {
        self.state.grid.row(row)
    }
}

const TAB_WIDTH: u16 = 8;

impl State {
    fn last_col(&self) -> u16 {
        self.grid.size().cols() - 1
    }

    fn last_row(&self) -> u16 {
        self.grid.size().rows() - 1
    }

    /// Blank cell used by erase operations (keeps the pen background).
    fn blank(&self) -> Cell {
        Cell {
            bg: self.pen.bg,
            ..Cell::default()
        }
    }

    fn goto(&mut self, col: u16, row: u16) {
        self.cursor.col = col.min(self.last_col());
        self.cursor.row = row.min(self.last_row());
        self.wrap_pending = false;
    }

    fn line_feed(&mut self) {
        if self.cursor.row == self.last_row() {
            let blank = self.blank();
            self.grid.scroll_up(blank);
        } else {
            self.cursor.row += 1;
        }
        self.wrap_pending = false;
    }

    /// Fills `cols` of `row` with blanks.
    fn erase(&mut self, row: u16, cols: std::ops::Range<u16>) {
        let blank = self.blank();
        self.grid.row_mut(row)[usize::from(cols.start)..usize::from(cols.end)].fill(blank);
    }

    fn erase_display(&mut self, mode: u16) {
        let Cursor { col, row, .. } = self.cursor;
        let cols = self.grid.size().cols();
        let rows = self.grid.size().rows();
        match mode {
            0 => {
                self.erase(row, col..cols);
                (row + 1..rows).for_each(|r| self.erase(r, 0..cols));
            }
            1 => {
                (0..row).for_each(|r| self.erase(r, 0..cols));
                self.erase(row, 0..col + 1);
            }
            2 | 3 => (0..rows).for_each(|r| self.erase(r, 0..cols)),
            _ => {}
        }
    }

    fn erase_line(&mut self, mode: u16) {
        let Cursor { col, row, .. } = self.cursor;
        let cols = self.grid.size().cols();
        match mode {
            0 => self.erase(row, col..cols),
            1 => self.erase(row, 0..col + 1),
            2 => self.erase(row, 0..cols),
            _ => {}
        }
    }

    fn sgr(&mut self, params: &vte::Params) {
        let params: Vec<&[u16]> = params.iter().collect();
        if params.is_empty() {
            self.pen = Cell::default();
            return;
        }
        let mut i = 0;
        while i < params.len() {
            let param = params[i];
            i += 1;
            match param[0] {
                0 => self.pen = Cell::default(),
                1 => self.pen.flags.insert(Flags::BOLD),
                3 => self.pen.flags.insert(Flags::ITALIC),
                4 => self.pen.flags.insert(Flags::UNDERLINE),
                7 => self.pen.flags.insert(Flags::INVERSE),
                22 => self.pen.flags.remove(Flags::BOLD),
                23 => self.pen.flags.remove(Flags::ITALIC),
                24 => self.pen.flags.remove(Flags::UNDERLINE),
                27 => self.pen.flags.remove(Flags::INVERSE),
                n @ 30..=37 => self.pen.fg = Color::Indexed((n - 30) as u8),
                n @ 40..=47 => self.pen.bg = Color::Indexed((n - 40) as u8),
                n @ 90..=97 => self.pen.fg = Color::Indexed((n - 90 + 8) as u8),
                n @ 100..=107 => self.pen.bg = Color::Indexed((n - 100 + 8) as u8),
                39 => self.pen.fg = Color::Default,
                49 => self.pen.bg = Color::Default,
                n @ (38 | 48) => {
                    let color = if param.len() > 1 {
                        extended_color_colon(&param[1..])
                    } else {
                        let (color, used) = extended_color_semicolon(&params[i..]);
                        i += used;
                        color
                    };
                    if let Some(color) = color {
                        if n == 38 {
                            self.pen.fg = color;
                        } else {
                            self.pen.bg = color;
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// Parses `5:n` or `2[:colorspace]:r:g:b` sub-parameters.
fn extended_color_colon(sub: &[u16]) -> Option<Color> {
    match sub {
        [5, n] => Some(Color::Indexed(byte(*n))),
        [2, r, g, b] | [2, _, r, g, b, ..] => Some(Color::Rgb(byte(*r), byte(*g), byte(*b))),
        _ => None,
    }
}

/// Parses `;5;n` or `;2;r;g;b`, returning the color and parameters consumed.
fn extended_color_semicolon(rest: &[&[u16]]) -> (Option<Color>, usize) {
    let arg = |k: usize| rest.get(k).map(|p| byte(p[0]));
    match rest.first().map(|p| p[0]) {
        Some(5) => match arg(1) {
            Some(n) => (Some(Color::Indexed(n)), 2),
            None => (None, rest.len()),
        },
        Some(2) => match (arg(1), arg(2), arg(3)) {
            (Some(r), Some(g), Some(b)) => (Some(Color::Rgb(r, g, b)), 4),
            _ => (None, rest.len()),
        },
        _ => (None, 0),
    }
}

fn byte(value: u16) -> u8 {
    value.min(255) as u8
}

impl vte::Perform for State {
    fn print(&mut self, ch: char) {
        // TODO: wide (CJK/emoji) chars occupy two cells; treated as width 1.
        if self.wrap_pending {
            self.cursor.col = 0;
            self.line_feed();
        }
        let Cursor { col, row, .. } = self.cursor;
        self.grid.row_mut(row)[usize::from(col)] = Cell { ch, ..self.pen };
        if col == self.last_col() {
            self.wrap_pending = true;
        } else {
            self.cursor.col += 1;
        }
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\r' => self.goto(0, self.cursor.row),
            b'\n' | 0x0b | 0x0c => self.line_feed(),
            0x08 => self.goto(self.cursor.col.saturating_sub(1), self.cursor.row),
            b'\t' => {
                let next = (self.cursor.col / TAB_WIDTH + 1) * TAB_WIDTH;
                self.goto(next, self.cursor.row);
            }
            _ => {} // BEL and other controls are ignored.
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        ignore: bool,
        action: char,
    ) {
        if ignore {
            return;
        }
        let private = intermediates == b"?";
        if !intermediates.is_empty() && !private {
            return;
        }
        let first = params.iter().next().map_or(0, |p| p[0]);
        let second = params.iter().nth(1).map_or(0, |p| p[0]);
        // Movement counts and positions treat 0 as 1.
        let n = first.max(1);
        let Cursor { col, row, .. } = self.cursor;
        match (private, action) {
            (true, 'h' | 'l') => {
                if params.iter().any(|p| p[0] == 25) {
                    self.cursor.visible = action == 'h';
                }
            }
            (true, _) => {}
            (false, 'A') => self.goto(col, row.saturating_sub(n)),
            (false, 'B') => self.goto(col, row.saturating_add(n)),
            (false, 'C') => self.goto(col.saturating_add(n), row),
            (false, 'D') => self.goto(col.saturating_sub(n), row),
            (false, 'H' | 'f') => self.goto(second.max(1) - 1, n - 1),
            (false, 'G') => self.goto(n - 1, row),
            (false, 'd') => self.goto(col, n - 1),
            (false, 'J') => self.erase_display(first),
            (false, 'K') => self.erase_line(first),
            (false, 'm') => self.sgr(params),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(cols: u16, rows: u16) -> Terminal {
        Terminal::new(TermSize::new(cols, rows).unwrap())
    }

    fn text(term: &Terminal, row: u16) -> String {
        let line: String = term.row(row).iter().map(|c| c.ch).collect();
        line.trim_end().to_owned()
    }

    fn pos(term: &Terminal) -> (u16, u16) {
        let c = term.cursor();
        (c.col, c.row)
    }

    #[test]
    fn prints_text_and_handles_crlf() {
        let mut t = term(10, 3);
        t.advance(b"ab\r\nc");
        assert_eq!(text(&t, 0), "ab");
        assert_eq!(text(&t, 1), "c");
        assert_eq!(pos(&t), (1, 1));
    }

    #[test]
    fn lf_keeps_column() {
        let mut t = term(10, 3);
        t.advance(b"ab\nc");
        assert_eq!(text(&t, 1), "  c");
    }

    #[test]
    fn wraps_at_last_column() {
        let mut t = term(3, 3);
        t.advance(b"abc");
        assert_eq!(
            pos(&t),
            (2, 0),
            "cursor stays on last column until next print"
        );
        t.advance(b"d");
        assert_eq!(text(&t, 0), "abc");
        assert_eq!(text(&t, 1), "d");
        assert_eq!(pos(&t), (1, 1));
    }

    #[test]
    fn cr_cancels_pending_wrap() {
        let mut t = term(3, 2);
        t.advance(b"abc\rx");
        assert_eq!(text(&t, 0), "xbc");
        assert_eq!(pos(&t), (1, 0));
    }

    #[test]
    fn lf_at_bottom_scrolls_up() {
        let mut t = term(5, 2);
        t.advance(b"one\r\ntwo\r\nsix");
        assert_eq!(text(&t, 0), "two");
        assert_eq!(text(&t, 1), "six");
        assert_eq!(pos(&t), (3, 1));
    }

    #[test]
    fn wrap_at_bottom_scrolls_up() {
        let mut t = term(2, 2);
        t.advance(b"abcde");
        assert_eq!(text(&t, 0), "cd");
        assert_eq!(text(&t, 1), "e");
    }

    #[test]
    fn backspace_moves_left_and_stops_at_zero() {
        let mut t = term(5, 1);
        t.advance(b"ab\x08\x08\x08x");
        assert_eq!(text(&t, 0), "xb");
    }

    #[test]
    fn tab_moves_to_next_multiple_of_eight_and_clamps() {
        let mut t = term(20, 1);
        t.advance(b"a\tb");
        assert_eq!(t.row(0)[8].ch, 'b');
        t.advance(b"\t\t\t");
        assert_eq!(pos(&t), (19, 0));
    }

    #[test]
    fn bell_is_ignored() {
        let mut t = term(5, 1);
        t.advance(b"a\x07b");
        assert_eq!(text(&t, 0), "ab");
    }

    #[test]
    fn cup_moves_cursor_one_based_and_clamps() {
        let mut t = term(10, 5);
        t.advance(b"\x1b[2;3Hx");
        assert_eq!(t.row(1)[2].ch, 'x');
        t.advance(b"\x1b[99;99H");
        assert_eq!(pos(&t), (9, 4));
        t.advance(b"\x1b[H");
        assert_eq!(pos(&t), (0, 0));
        t.advance(b"\x1b[3;4f");
        assert_eq!(pos(&t), (3, 2));
    }

    #[test]
    fn relative_moves_clamp_to_bounds() {
        let mut t = term(10, 5);
        t.advance(b"\x1b[3;3H\x1b[A");
        assert_eq!(pos(&t), (2, 1));
        t.advance(b"\x1b[10A");
        assert_eq!(pos(&t), (2, 0));
        t.advance(b"\x1b[2B");
        assert_eq!(pos(&t), (2, 2));
        t.advance(b"\x1b[50B");
        assert_eq!(pos(&t), (2, 4));
        t.advance(b"\x1b[3C");
        assert_eq!(pos(&t), (5, 4));
        t.advance(b"\x1b[50C");
        assert_eq!(pos(&t), (9, 4));
        t.advance(b"\x1b[D");
        assert_eq!(pos(&t), (8, 4));
        t.advance(b"\x1b[50D");
        assert_eq!(pos(&t), (0, 4));
    }

    #[test]
    fn cha_and_vpa_set_single_axis() {
        let mut t = term(10, 5);
        t.advance(b"\x1b[2;2H\x1b[7G");
        assert_eq!(pos(&t), (6, 1));
        t.advance(b"\x1b[4d");
        assert_eq!(pos(&t), (6, 3));
    }

    #[test]
    fn erase_display_variants() {
        let mut t = term(3, 3);
        t.advance(b"abc\r\ndef\r\nghi\x1b[2;2H\x1b[J");
        assert_eq!([text(&t, 0), text(&t, 1), text(&t, 2)], ["abc", "d", ""]);

        let mut t = term(3, 3);
        t.advance(b"abc\r\ndef\r\nghi\x1b[2;2H\x1b[1J");
        assert_eq!([text(&t, 0), text(&t, 1), text(&t, 2)], ["", "  f", "ghi"]);

        let mut t = term(3, 3);
        t.advance(b"abc\r\ndef\r\nghi\x1b[2;2H\x1b[2J");
        assert!((0..3).all(|r| text(&t, r).is_empty()));
        assert_eq!(pos(&t), (1, 1), "ED 2 does not move the cursor");
    }

    #[test]
    fn erase_line_variants() {
        let mut t = term(5, 1);
        t.advance(b"abcde\x1b[3G\x1b[K");
        assert_eq!(text(&t, 0), "ab");
        let mut t = term(5, 1);
        t.advance(b"abcde\x1b[3G\x1b[1K");
        assert_eq!(text(&t, 0), "   de");
        let mut t = term(5, 1);
        t.advance(b"abcde\x1b[3G\x1b[2K");
        assert_eq!(text(&t, 0), "");
    }

    #[test]
    fn sgr_sets_basic_colors_and_resets() {
        let mut t = term(10, 1);
        t.advance(b"\x1b[31;42ma\x1b[0mb\x1b[91;103mc\x1b[39;49md");
        let row = t.row(0);
        assert_eq!(
            (row[0].fg, row[0].bg),
            (Color::Indexed(1), Color::Indexed(2))
        );
        assert_eq!((row[1].fg, row[1].bg), (Color::Default, Color::Default));
        assert_eq!(
            (row[2].fg, row[2].bg),
            (Color::Indexed(9), Color::Indexed(11))
        );
        assert_eq!((row[3].fg, row[3].bg), (Color::Default, Color::Default));
    }

    #[test]
    fn empty_sgr_resets() {
        let mut t = term(10, 1);
        t.advance(b"\x1b[1;31m\x1b[ma");
        assert_eq!(
            t.row(0)[0],
            Cell {
                ch: 'a',
                ..Cell::default()
            }
        );
    }

    #[test]
    fn sgr_sets_256_and_truecolor() {
        let mut t = term(10, 1);
        t.advance(b"\x1b[38;5;200;48;2;1;2;3ma\x1b[38:2::4:5:6;48:5:17mb");
        let row = t.row(0);
        assert_eq!(
            (row[0].fg, row[0].bg),
            (Color::Indexed(200), Color::Rgb(1, 2, 3))
        );
        assert_eq!(
            (row[1].fg, row[1].bg),
            (Color::Rgb(4, 5, 6), Color::Indexed(17))
        );
    }

    #[test]
    fn sgr_sets_and_clears_flags() {
        let mut t = term(10, 1);
        t.advance(b"\x1b[1;7ma\x1b[22mb\x1b[27mc");
        let row = t.row(0);
        assert!(row[0].flags.contains(Flags::BOLD) && row[0].flags.contains(Flags::INVERSE));
        assert!(!row[1].flags.contains(Flags::BOLD) && row[1].flags.contains(Flags::INVERSE));
        assert_eq!(row[2].flags, Flags::default());
    }

    #[test]
    fn erase_uses_current_background() {
        let mut t = term(3, 1);
        t.advance(b"\x1b[44m\x1b[2K");
        assert!(t.row(0).iter().all(|c| c.bg == Color::Indexed(4)));
    }

    #[test]
    fn dectcem_hides_and_shows_cursor() {
        let mut t = term(5, 1);
        assert!(t.cursor().visible);
        t.advance(b"\x1b[?25l");
        assert!(!t.cursor().visible);
        t.advance(b"\x1b[?25h");
        assert!(t.cursor().visible);
    }

    #[test]
    fn unknown_and_malformed_sequences_are_ignored() {
        let mut t = term(5, 2);
        t.advance(b"\x1b[?1049h\x1b]0;title\x07\x1b[38;5m\x1b[38;2;1m\x1b[99999;99999H");
        t.advance(b"\x1b[5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5m");
        t.advance(b"\x1b(B\x1bPdcs\x1b\\\x1b[>c\x1b[Z\x1b[2;1Hok");
        assert_eq!(text(&t, 1), "ok");
    }

    #[test]
    fn utf8_is_decoded() {
        let mut t = term(5, 1);
        t.advance("é→".as_bytes());
        assert_eq!(text(&t, 0), "é→");
    }

    #[test]
    fn resize_keeps_content_and_clamps_cursor() {
        let mut t = term(10, 5);
        t.advance(b"hello\x1b[5;10H");
        t.resize(TermSize::new(3, 2).unwrap());
        assert_eq!(t.size(), TermSize::new(3, 2).unwrap());
        assert_eq!(text(&t, 0), "hel");
        assert_eq!(pos(&t), (2, 1));
        t.advance(b"\r\nxy");
        assert_eq!(text(&t, 1), "xy");
    }
}
