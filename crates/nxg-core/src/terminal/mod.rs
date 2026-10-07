//! Terminal state machine: feeds child output through a VT parser into a grid.

use crate::apc::{ApcFilter, Event};
use crate::cell::{Cell, Color, Flags};
use crate::image::{ImageStore, Placement, SrcRect};
use crate::kitty::{self, Graphics};
use crate::sixel::{self, SixelDecoder};
use crate::{CellPixels, TermSize};

mod edit;
mod modes;
mod screen;
mod scrollback;
#[cfg(test)]
mod testing;

pub use modes::Modes;
use screen::Screen;
pub use scrollback::DEFAULT_SCROLLBACK;
use scrollback::Scrollback;

/// Cursor position (zero-based) and visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub col: u16,
    pub row: u16,
    pub visible: bool,
}

/// Screen state driven by [`Terminal::advance`].
pub struct Terminal {
    /// Pulls kitty graphics APC strings out before vte (which drops them).
    filter: ApcFilter,
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
    /// The screen being shown and driven.
    screen: Screen,
    /// The other screen: the main one while the alternate is active, else
    /// the alternate kept by `?47` (if any).
    dormant: Option<Box<Screen>>,
    alt_active: bool,
    /// Global like in xterm: a full-screen app hiding the cursor must not
    /// leave the shell's hidden after it exits.
    cursor_visible: bool,
    pen: Cell,
    /// Replies to queries, waiting to be written to the child.
    responses: Vec<u8>,
    /// Pixel size of a cell, for image geometry and size reports.
    cell: CellPixels,
    images: ImageStore,
    graphics: Graphics,
    /// The sixel image being received through a DCS string.
    sixel: Option<SixelDecoder>,
    /// Sixel scrolling (DECSDM reset): images go at the cursor and move it.
    sixel_scrolling: bool,
    /// DECAWM. Global like in xterm: not saved by DECSC nor swapped with the
    /// screens.
    autowrap: bool,
    /// DECCKM. Global too: an application sets it once and expects it to hold
    /// across screen switches.
    app_cursor_keys: bool,
    /// The last printed character, for REP. Anything but another REP clears it.
    last_char: Option<char>,
    /// Lines scrolled off the main screen and the viewport over them.
    scrollback: Scrollback,
}

impl Terminal {
    pub fn new(size: TermSize) -> Self {
        Self {
            filter: ApcFilter::new(),
            parser: vte::Parser::new(),
            state: State {
                screen: Screen::new(size),
                dormant: None,
                alt_active: false,
                cursor_visible: true,
                pen: Cell::default(),
                responses: Vec::new(),
                cell: CellPixels::default(),
                images: ImageStore::default(),
                graphics: Graphics::new(),
                sixel: None,
                sixel_scrolling: true,
                autowrap: true,
                app_cursor_keys: false,
                last_char: None,
                scrollback: Scrollback::new(DEFAULT_SCROLLBACK),
            },
        }
    }

    /// Feeds raw child output. Any output brings the viewport back to the
    /// live screen.
    pub fn advance(&mut self, bytes: &[u8]) {
        if !bytes.is_empty() {
            self.scroll_display_to_bottom();
        }
        let Self {
            filter,
            parser,
            state,
        } = self;
        filter.feed(bytes, |event| match event {
            Event::Text(text) => parser.advance(state, text),
            Event::Apc(payload) => {
                // `ESC _` ends any string or sequence vte is inside, as it
                // would have had vte seen it.
                parser.advance(state, b"\x1b\\");
                state.apc(payload);
            }
        });
    }

    pub fn resize(&mut self, size: TermSize) {
        self.state.resize(size);
    }

    pub fn size(&self) -> TermSize {
        self.state.screen.grid.size()
    }

    pub fn cursor(&self) -> Cursor {
        let screen = &self.state.screen;
        Cursor {
            col: screen.col,
            row: screen.row,
            visible: self.state.cursor_visible,
        }
    }

    /// Bytes the terminal must send back to the child (e.g. replies to
    /// cursor position and device attribute queries). Drains the buffer.
    pub fn take_responses(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.state.responses)
    }

    /// The cells of `row`. Panics if `row` is out of bounds.
    pub fn row(&self, row: u16) -> &[Cell] {
        self.state.screen.grid.row(row)
    }

    /// Sets the pixel size of a cell; the app calls this whenever the
    /// renderer's font or scale changes. Zero is treated as one.
    pub fn set_cell_pixels(&mut self, width: u32, height: u32) {
        self.state.cell = CellPixels::new(width, height);
    }

    pub fn cell_pixels(&self) -> CellPixels {
        self.state.cell
    }

    /// Stored images and their placements on screen.
    pub fn images(&self) -> &ImageStore {
        &self.state.images
    }
}

const TAB_WIDTH: u16 = 8;

impl State {
    fn last_col(&self) -> u16 {
        self.screen.grid.size().cols() - 1
    }

    fn last_row(&self) -> u16 {
        self.screen.grid.size().rows() - 1
    }

    /// Blank cell used by erase operations (keeps the pen background).
    fn blank(&self) -> Cell {
        Cell {
            bg: self.pen.bg,
            ..Cell::default()
        }
    }

    /// Runs an APC string; only kitty graphics (`G…`) are understood.
    fn apc(&mut self, payload: &[u8]) {
        let Some(body) = payload.strip_prefix(b"G") else {
            return;
        };
        let mut ctx = kitty::Context {
            store: &mut self.images,
            cursor: (u32::from(self.screen.col), i32::from(self.screen.row)),
            cell: self.cell,
        };
        let outcome = self.graphics.handle(body, &mut ctx);
        if let Some(reply) = outcome.reply {
            self.responses.extend_from_slice(&reply);
        }
        if let Some((cols, rows)) = outcome.advance {
            self.advance_over_image(cols, rows);
        }
    }

    /// Kitty cursor movement after a placement: right by `cols`, down by
    /// `rows - 1` (scrolling as needed). Past the right edge the cursor
    /// waits in the last column to wrap on the next print.
    fn advance_over_image(&mut self, cols: u32, rows: u32) {
        let rows = rows.min(u32::from(self.screen.grid.size().rows()));
        for _ in 1..rows {
            self.index();
        }
        let col = u32::from(self.screen.col).saturating_add(cols);
        if col > u32::from(self.last_col()) {
            self.screen.col = self.last_col();
            self.screen.wrap_pending = self.autowrap;
        } else {
            self.screen.col = col as u16;
            self.screen.wrap_pending = false;
        }
    }

    /// Stores a finished sixel image and places it: at the cursor, moving
    /// the cursor to the line below the image, or at the origin without
    /// moving it when sixel scrolling is off (DECSDM).
    fn finish_sixel(&mut self, decoder: SixelDecoder) {
        let Some(image) = decoder.finish() else {
            return;
        };
        let id = self.images.unused_id();
        let (width, height) = (image.width, image.height);
        let Some(key) = self.images.insert(id, 0, width, height, image.pixels) else {
            return;
        };
        let (col, row) = if self.sixel_scrolling {
            (self.screen.col, self.screen.row)
        } else {
            (0, 0)
        };
        self.images.place(Placement {
            image: key,
            id: 0,
            row: i32::from(row),
            col: u32::from(col),
            offset_x: 0,
            offset_y: 0,
            src: SrcRect {
                x: 0,
                y: 0,
                width,
                height,
            },
            cols: 0,
            rows: 0,
            z: 0,
            sixel: true,
        });
        if self.sixel_scrolling {
            let rows = self.cell.rows_for(height);
            for _ in 0..rows.min(u32::from(self.screen.grid.size().rows())) {
                self.index();
            }
        }
    }

    /// XTWINOPS size reports (`CSI 14/16/18 t`).
    fn window_report(&mut self, what: u16) {
        let size = self.screen.grid.size();
        let (width, height) = self.cell.text_area(size);
        let reply = match what {
            14 => format!("\x1b[4;{height};{width}t"),
            16 => format!("\x1b[6;{};{}t", self.cell.height, self.cell.width),
            18 => format!("\x1b[8;{};{}t", size.rows(), size.cols()),
            _ => return,
        };
        self.responses.extend_from_slice(reply.as_bytes());
    }

    /// XTSMGRAPHICS (`CSI ? Pi ; Pa S`): reports color registers and the
    /// largest sixel image; nothing can be changed.
    fn graphics_attributes(&mut self, item: u16) {
        let reply = match item {
            1 => "\x1b[?1;0;256S".to_owned(),
            2 => {
                let (w, h) = self.cell.text_area(self.screen.grid.size());
                let (w, h) = (w.min(sixel::MAX_SIZE), h.min(sixel::MAX_SIZE));
                format!("\x1b[?2;0;{w};{h}S")
            }
            n => format!("\x1b[?{n};1;0S"),
        };
        self.responses.extend_from_slice(reply.as_bytes());
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
        if self.screen.wrap_pending {
            self.screen.col = 0;
            self.index();
        }
        let (col, row) = (self.screen.col, self.screen.row);
        self.screen.grid.row_mut(row)[usize::from(col)] = Cell { ch, ..self.pen };
        self.images.remove_sixels_over(row, col..col + 1, self.cell);
        self.last_char = Some(ch);
        // Without autowrap the last column is overwritten in place.
        self.screen.wrap_pending = col == self.last_col() && self.autowrap;
        if col != self.last_col() {
            self.screen.col += 1;
        }
    }

    fn execute(&mut self, byte: u8) {
        self.last_char = None;
        match byte {
            b'\r' => self.goto(0, self.screen.row),
            b'\n' | 0x0b | 0x0c => self.index(),
            0x08 => self.goto(self.screen.col.saturating_sub(1), self.screen.row),
            b'\t' => {
                let next = (self.screen.col / TAB_WIDTH + 1) * TAB_WIDTH;
                self.goto(next, self.screen.row);
            }
            _ => {} // BEL and other controls are ignored.
        }
    }

    fn hook(&mut self, params: &vte::Params, intermediates: &[u8], ignore: bool, action: char) {
        self.sixel = None;
        if action == 'q' && intermediates.is_empty() && !ignore {
            let params: Vec<u16> = params.iter().map(|p| p[0]).collect();
            self.sixel = Some(SixelDecoder::new(&params));
        }
    }

    fn put(&mut self, byte: u8) {
        if let Some(sixel) = &mut self.sixel {
            sixel.put(byte);
        }
    }

    fn unhook(&mut self) {
        if let Some(sixel) = self.sixel.take() {
            self.finish_sixel(sixel);
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        self.last_char = None;
        if ignore || !intermediates.is_empty() {
            return;
        }
        match byte {
            b'7' => self.save_cursor(),
            b'8' => self.restore_cursor(),
            b'D' => self.index(),
            b'E' => self.next_line(),
            b'M' => self.reverse_index(),
            b'c' => self.reset(),
            // DECKPAM/DECKPNM: nothing here depends on the keypad mode.
            _ => {}
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        ignore: bool,
        action: char,
    ) {
        // REP must survive itself so that it can be chained.
        if !(action == 'b' && intermediates.is_empty()) {
            self.last_char = None;
        }
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
        // `CSI s` / `CSI u` act as DECSC/DECRC only without parameters; vte
        // reports a missing parameter as a single 0.
        let bare = params.len() <= 1 && first == 0;
        let (col, row) = (self.screen.col, self.screen.row);
        match (private, action) {
            (true, 'h' | 'l') => {
                for p in params.iter() {
                    self.set_private_mode(p[0], action == 'h');
                }
            }
            (true, 'S') => self.graphics_attributes(first),
            (true, _) => {}
            (false, 'A') => self.cursor_up(n),
            (false, 'B') => self.cursor_down(n),
            (false, 'C') => self.goto(col.saturating_add(n), row),
            (false, 'D') => self.goto(col.saturating_sub(n), row),
            (false, 'H' | 'f') => self.goto_addressed(second.max(1) - 1, n - 1),
            (false, 'E') => self.cursor_next_line(n),
            (false, 'F') => self.cursor_previous_line(n),
            (false, 'G') => self.goto(n - 1, row),
            (false, '@') => self.insert_chars(n),
            (false, 'P') => self.delete_chars(n),
            (false, 'X') => self.erase_chars(n),
            (false, 'L') => self.insert_lines(n),
            (false, 'M') => self.delete_lines(n),
            (false, 'b') => self.repeat_last_char(n),
            (false, 'd') => self.goto_addressed(col, n - 1),
            (false, 'r') => self.set_region(first, second),
            (false, 'S') => self.scroll_region_up(n),
            (false, 'T') => self.scroll_region_down(n),
            (false, 'J') => self.erase_display(first),
            (false, 'K') => self.erase_line(first),
            (false, 's') if bare => self.save_cursor(),
            (false, 'u') if bare => self.restore_cursor(),
            (false, 'm') => self.sgr(params),
            // DSR. ConPTY blocks at startup until it gets the CPR reply.
            (false, 'n') if first == 5 => self.responses.extend_from_slice(b"\x1b[0n"),
            (false, 'n') if first == 6 => {
                let reply = format!("\x1b[{};{}R", self.addressed_row() + 1, col + 1);
                self.responses.extend_from_slice(reply.as_bytes());
            }
            // DA1: a VT220 with sixel graphics (4) and ANSI color (22).
            (false, 'c') if first == 0 => self.responses.extend_from_slice(b"\x1b[?62;4;22c"),
            (false, 't') => self.window_report(first),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

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
        t.advance(b"\x1b[?9999h\x1b[1!z\x1b]0;title\x07\x1b[38;5m\x1b[38;2;1m\x1b[99999;99999H");
        t.advance(b"\x1b[5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5;5m");
        t.advance(b"\x1b(B\x1b#7\x1bPdcs\x1b\\\x1b[>c\x1b[Z\x1b[2;1Hok");
        assert_eq!(text(&t, 1), "ok");
        assert!(
            !t.state.alt_active,
            "nothing in the stream switches screens"
        );
        assert!(t.state.screen.saved.is_none(), "nor saves the cursor");
    }

    #[test]
    fn cursor_reports_position_and_visibility() {
        let mut t = term(10, 5);
        t.advance(b"\x1b[2;3H\x1b[?25l");
        assert_eq!(
            t.cursor(),
            Cursor {
                col: 2,
                row: 1,
                visible: false
            }
        );
    }

    #[test]
    fn decsc_and_decrc_save_and_restore_the_position() {
        let mut t = term(10, 5);
        t.advance(b"ab\x1b7\x1b[4;4H");
        assert_eq!(pos(&t), (3, 3));
        t.advance(b"\x1b8");
        assert_eq!(pos(&t), (2, 0));
    }

    #[test]
    fn decrc_restores_the_pen() {
        let mut t = term(10, 5);
        t.advance(b"ab\x1b[1;31m\x1b7\x1b[0m\x1b[5;5H\x1b8x");
        let cell = t.row(0)[2];
        assert_eq!(cell.ch, 'x');
        assert_eq!(cell.fg, Color::Indexed(1));
        assert!(cell.flags.contains(Flags::BOLD));
    }

    #[test]
    fn decrc_without_save_goes_home_with_the_default_pen() {
        let mut t = term(10, 5);
        t.advance(b"\x1b[5;5H\x1b[31m\x1b8x");
        assert_eq!(t.row(0)[0].ch, 'x');
        assert_eq!(t.row(0)[0].fg, Color::Default);
        assert_eq!(pos(&t), (1, 0));
    }

    #[test]
    fn decsc_keeps_a_pending_wrap() {
        let mut t = term(3, 2);
        t.advance(b"abc\x1b7\x1b[2;1H\x1b8d");
        assert_eq!(text(&t, 0), "abc");
        assert_eq!(text(&t, 1), "d", "the restored cursor still wraps first");
    }

    #[test]
    fn keypad_mode_escapes_are_accepted_and_ignored() {
        let mut t = term(5, 2);
        t.advance(b"a\x1b=b\x1b>c");
        assert_eq!(text(&t, 0), "abc");
        assert_eq!(pos(&t), (3, 0));
    }

    #[test]
    fn esc_with_intermediates_or_unknown_finals_is_ignored() {
        let mut t = term(5, 4);
        t.advance(b"\x1b[4;4H\x1b(7\x1b[1;1H\x1b8");
        assert_eq!(pos(&t), (0, 0), "ESC ( 7 saves nothing");
        t.advance(b"\x1b9\x1b[Qz\x1b%Gy");
        assert_eq!(text(&t, 0), "zy");
    }

    #[test]
    fn csi_s_and_u_without_parameters_act_as_decsc_and_decrc() {
        let mut t = term(10, 5);
        t.advance(b"ab\x1b[s\x1b[3;1H\x1b[u");
        assert_eq!(pos(&t), (2, 0));
    }

    #[test]
    fn csi_s_and_u_variants_with_parameters_or_intermediates_are_ignored() {
        let mut t = term(10, 5);
        t.advance(b"ab\x1b[s\x1b[3;1H\x1b[?u\x1b[>u");
        assert_eq!(pos(&t), (0, 2), "kitty keyboard forms do not restore");
        t.advance(b"\x1b[1s\x1b[5;5H\x1b[u");
        assert_eq!(pos(&t), (2, 0), "CSI 1 s did not overwrite the save");
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

    #[test]
    fn has_no_responses_by_default() {
        let mut t = term(10, 3);
        t.advance(b"hello");
        assert!(t.take_responses().is_empty());
    }

    #[test]
    fn replies_to_cursor_position_report_one_based() {
        let mut t = term(10, 5);
        t.advance(b"\x1b[3;4H\x1b[6n");
        assert_eq!(t.take_responses(), b"\x1b[3;4R");
    }

    #[test]
    fn replies_to_device_status_report() {
        let mut t = term(10, 3);
        t.advance(b"\x1b[5n");
        assert_eq!(t.take_responses(), b"\x1b[0n");
    }

    #[test]
    fn replies_to_primary_device_attributes() {
        let mut t = term(10, 3);
        t.advance(b"\x1b[c\x1b[0c");
        assert_eq!(t.take_responses(), b"\x1b[?62;4;22c\x1b[?62;4;22c");
    }

    #[test]
    fn take_responses_drains_the_buffer() {
        let mut t = term(10, 3);
        t.advance(b"\x1b[6n");
        t.take_responses();
        assert!(t.take_responses().is_empty());
    }

    #[test]
    fn replies_to_window_size_queries() {
        let mut t = sized(8, 3);
        assert_eq!(t.cell_pixels(), CellPixels::new(10, 20));
        t.advance(b"\x1b[14t\x1b[16t\x1b[18t");
        assert_eq!(t.take_responses(), b"\x1b[4;60;80t\x1b[6;20;10t\x1b[8;3;8t");
    }

    #[test]
    fn replies_to_sixel_graphics_attribute_queries() {
        let mut t = sized(8, 3);
        t.advance(b"\x1b[?1;1S\x1b[?2;1S\x1b[?3;1S");
        assert_eq!(
            t.take_responses(),
            b"\x1b[?1;0;256S\x1b[?2;0;80;60S\x1b[?3;1;0S"
        );
    }

    #[test]
    fn kitty_image_is_placed_between_surrounding_text() {
        let mut t = sized(10, 3);
        let mut input = b"ab".to_vec();
        input.extend(kitty_rgba(1, 15, 30, ""));
        input.extend_from_slice(b"cd");
        t.advance(&input);
        assert_eq!(t.take_responses(), b"\x1b_Gi=1;OK\x1b\\");
        let p = t.images().placements()[0];
        assert_eq!((p.col, p.row), (2, 0));
        // 15x30 px covers 2x2 cells: cursor moves right 2 and down 1.
        assert_eq!(text(&t, 0), "ab");
        assert_eq!(text(&t, 1), "    cd");
        assert_eq!(pos(&t), (6, 1));
    }

    #[test]
    fn kitty_sequences_split_across_reads_work() {
        let mut t = sized(10, 3);
        let input = kitty_rgba(2, 1, 1, ",C=1");
        for chunk in input.chunks(3) {
            t.advance(chunk);
        }
        assert_eq!(t.take_responses(), b"\x1b_Gi=2;OK\x1b\\");
        assert_eq!(t.images().placements().len(), 1);
        assert_eq!(pos(&t), (0, 0), "C=1 keeps the cursor");
    }

    #[test]
    fn kitty_cursor_advance_past_the_edge_waits_to_wrap() {
        let mut t = sized(4, 3);
        t.advance(&kitty_rgba(1, 40, 20, ",q=2"));
        assert_eq!(pos(&t), (3, 0));
        t.advance(b"x");
        assert_eq!(text(&t, 1), "x");
    }

    #[test]
    fn kitty_cursor_advance_past_the_edge_does_not_wait_without_autowrap() {
        let mut t = sized(4, 3);
        t.advance(b"\x1b[?7l");
        t.advance(&kitty_rgba(1, 40, 20, ",q=2"));
        assert_eq!(pos(&t), (3, 0));
        t.advance(b"x");
        assert_eq!(text(&t, 0), "   x", "the print overwrites the last column");
        assert_eq!(text(&t, 1), "");
    }

    #[test]
    fn kitty_image_at_bottom_scrolls_the_screen() {
        let mut t = sized(4, 3);
        t.advance(b"top\x1b[3;1H");
        t.advance(&kitty_rgba(1, 10, 40, ""));
        assert_eq!(text(&t, 0), "", "scrolled up one line");
        assert_eq!(t.images().placements()[0].row, 1);
        assert_eq!(pos(&t), (1, 2));
    }

    #[test]
    fn placements_scroll_with_text_and_drop_off_the_top() {
        let mut t = sized(4, 3);
        // Two rows tall, anchored at the top.
        t.advance(&kitty_rgba(1, 10, 40, ",C=1"));
        t.advance(b"\x1b[3;1H\n");
        assert_eq!(t.images().placements()[0].row, -1, "partly visible");
        t.advance(b"\n");
        assert!(t.images().placements().is_empty(), "fully scrolled off");
        assert_eq!(t.images().len(), 1, "the image data stays");
    }

    #[test]
    fn full_screen_clear_removes_placements_but_keeps_images() {
        let mut t = sized(4, 3);
        t.advance(&kitty_rgba(1, 10, 20, ""));
        t.advance(b"\x1b[J");
        assert_eq!(t.images().placements().len(), 1, "ED 0 keeps images");
        t.advance(b"\x1b[2J");
        assert!(t.images().placements().is_empty());
        assert_eq!(t.images().len(), 1);
        t.advance(b"\x1b_Ga=p,i=1\x1b\\\x1b[3J");
        assert!(t.images().placements().is_empty());
    }

    #[test]
    fn sixel_is_placed_at_cursor_and_moves_cursor_below() {
        let mut t = sized(10, 5);
        t.advance(b"\x1b[1;3H\x1bP0;1q#1;2;100;0;0!12~-!12~-!12~-!12~\x1b\\");
        let p = t.images().placements()[0];
        assert_eq!((p.col, p.row), (2, 0));
        let image = t.images().image(p.image).unwrap();
        assert_eq!((image.width, image.height), (12, 24));
        assert_eq!(image.pixel(11, 23), [255, 0, 0, 255]);
        // 24 px is 2 rows of 20 px: cursor ends on the row below.
        assert_eq!(pos(&t), (2, 2));
        assert!(t.take_responses().is_empty());
    }

    #[test]
    fn sixel_at_bottom_scrolls_the_image_into_view() {
        let mut t = sized(10, 3);
        t.advance(b"\x1b[3;1H\x1bPq#1~-~-~-~-~-~-~\x1b\\x");
        let p = t.images().placements()[0];
        // 42 px = 3 rows; starts on row 2, three line feeds scroll 3.
        assert_eq!(p.row, -1);
        assert_eq!(pos(&t), (1, 2));
        assert_eq!(text(&t, 2), "x");
    }

    #[test]
    fn sixel_display_mode_pins_image_to_origin() {
        let mut t = sized(10, 3);
        t.advance(b"\x1b[?80h\x1b[2;2H\x1bPq#1~\x1b\\");
        let p = t.images().placements()[0];
        assert_eq!((p.col, p.row), (0, 0));
        assert_eq!(pos(&t), (1, 1), "cursor does not move");
        t.advance(b"\x1b[?80l\x1bPq#1~\x1b\\");
        assert_eq!(pos(&t), (1, 2));
    }

    /// A 10x5 terminal with a 30x40 px sixel (3 cols, 2 rows) at row 1, col 2.
    fn with_sixel() -> crate::Terminal {
        let mut t = sized(10, 5);
        t.advance(b"\x1b[2;3H\x1bPq\"1;1;30;40#1~\x1b\\");
        assert_eq!(t.images().placements().len(), 1);
        t
    }

    #[test]
    fn text_over_a_sixel_removes_it() {
        let mut t = with_sixel();
        t.advance(b"\x1b[s\x1b[3;3H   \x1b[u");
        assert!(t.images().placements().is_empty());
        assert!(t.images().is_empty(), "the orphaned sixel image is freed");
    }

    #[test]
    fn text_beside_a_sixel_keeps_it() {
        let mut t = with_sixel();
        t.advance(b"\x1b[2;1Hab\x1b[2;6Hx\x1b[4;3Hyyy");
        assert_eq!(t.images().placements().len(), 1);
    }

    #[test]
    fn erase_in_line_over_a_sixel_removes_it() {
        let mut t = with_sixel();
        t.advance(b"\x1b[2;6H\x1b[K");
        assert_eq!(t.images().placements().len(), 1, "EL 0 right of it");
        t.advance(b"\x1b[3;1H\x1b[1K");
        assert_eq!(t.images().placements().len(), 1, "EL 1 left of it");
        t.advance(b"\x1b[3;5H\x1b[K");
        assert!(t.images().placements().is_empty());
    }

    #[test]
    fn erase_in_display_over_a_sixel_removes_it() {
        let mut t = with_sixel();
        t.advance(b"\x1b[4;1H\x1b[J");
        assert_eq!(t.images().placements().len(), 1, "ED 0 below it");
        t.advance(b"\x1b[1J");
        assert!(t.images().placements().is_empty());
    }

    #[test]
    fn erase_chars_over_a_sixel_removes_it() {
        let mut t = with_sixel();
        t.advance(b"\x1b[2;1H\x1b[2X");
        assert_eq!(t.images().placements().len(), 1, "ECH stops short of it");
        t.advance(b"\x1b[3X");
        assert!(t.images().placements().is_empty());
    }

    #[test]
    fn kitty_placements_survive_text_and_erase() {
        let mut t = sized(10, 5);
        t.advance(&kitty_rgba(1, 30, 40, ",C=1"));
        t.advance(b"\x1b[1;1Hxyz\x1b[2;1H\x1b[K\x1b[1;1H\x1b[3X");
        assert_eq!(t.images().placements().len(), 1);
    }

    #[test]
    fn other_dcs_strings_are_ignored() {
        let mut t = sized(10, 3);
        t.advance(b"\x1bP$qm\x1b\\\x1bP1$r\x1b\\ok");
        assert!(t.images().is_empty());
        assert_eq!(text(&t, 0), "ok");
    }

    /// Deterministic xorshift so failures reproduce.
    fn noise(seed: &mut u64) -> u64 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed
    }

    #[test]
    fn random_and_truncated_input_never_panics() {
        let mut seed = 0x9e37_79b9_7f4a_7c15;
        let alphabet: &[u8] =
            b"\x1b\x07\x18_PGq#!$-~?;=,0123456789aitTfpdsvxyzwhcrCXYIUmoS\\[]\"@^ \n\r\
              LM@DEFJK78ubl=";
        for _ in 0..300 {
            let mut t = sized(7, 4);
            let len = (noise(&mut seed) % 400) as usize;
            let bytes: Vec<u8> = (0..len)
                .map(|_| {
                    let r = noise(&mut seed);
                    if r % 4 == 0 {
                        (r >> 8) as u8
                    } else {
                        alphabet[(r >> 8) as usize % alphabet.len()]
                    }
                })
                .collect();
            t.advance(&bytes);
            t.state.assert_invariants();
            t.take_responses();
            t.resize(TermSize::new(3, 2).unwrap());
            t.state.assert_invariants();
            t.advance(b"\x1b\\\x1b[2J");
        }
        let valid = [
            kitty_rgba(1, 3, 3, ""),
            b"\x1bP0;1;0q\"1;1;8;8#0;2;0;0;0#1;1;120;50;100#1!8~-!8~\x1b\\".to_vec(),
        ];
        for input in valid {
            for cut in 0..input.len() {
                let mut t = sized(5, 3);
                t.advance(&input[..cut]);
                t.advance(b"\x1b\\");
                t.advance(&input[cut..]);
            }
        }
    }

    /// Sequences that touch screens, regions, modes and cell editing, so
    /// that random mixes of them reach states a byte alphabet rarely does.
    #[test]
    fn screen_ops_never_panic_across_resizes() {
        let mut tokens: Vec<Vec<u8>> = [
            "\x1b[?1049h",
            "\x1b[?1049l",
            "\x1b[?47h",
            "\x1b[?47l",
            "\x1b[?1047h",
            "\x1b[?1047l",
            "\x1b[?1048h",
            "\x1b[?1048l",
            "\x1b[?6h",
            "\x1b[?6l",
            "\x1b[?7h",
            "\x1b[?7l",
            "\x1b[r",
            "\x1bM",
            "\x1bD",
            "\x1bE",
            "\x1b7",
            "\x1b8",
            "\x1bc",
            "\n",
            "\r",
            "\x1b[m",
            "\x1b[44m",
            "\x1b[2J",
        ]
        .iter()
        .map(|s| s.as_bytes().to_vec())
        .collect();
        tokens.push(kitty_rgba(1, 10, 20, ""));
        tokens.push(kitty_rgba(2, 20, 40, ""));
        let counted = ["L", "M", "@", "P", "X", "b", "S", "T", "A", "B", "E", "F"];
        let mut seed = 0x2545_f491_4f6c_dd1d;
        for _ in 0..400 {
            let mut t = sized(8, 5);
            for _ in 0..60 {
                let r = noise(&mut seed);
                let n = (r >> 40) % 100;
                let token = match (r >> 8) % 6 {
                    0 => format!("\x1b[{};{}r", (r >> 16) % 8, (r >> 24) % 8).into_bytes(),
                    1 => format!("\x1b[{n}{}", counted[(r >> 16) as usize % counted.len()])
                        .into_bytes(),
                    2 => "text ".repeat(1 + (r >> 16) as usize % 4).into_bytes(),
                    _ => tokens[(r >> 16) as usize % tokens.len()].clone(),
                };
                t.advance(&token);
                t.state.assert_invariants();
                t.take_responses();
                if r % 5 == 0 {
                    // Down to 1x1, where a region cannot exist.
                    let size = TermSize::new(1 + (r >> 48) as u16 % 9, 1 + (r >> 52) as u16 % 6);
                    t.resize(size.unwrap());
                    t.state.assert_invariants();
                }
            }
        }
    }
}
