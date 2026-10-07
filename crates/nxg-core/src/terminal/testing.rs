//! Helpers shared by the terminal tests.

use super::Terminal;
use crate::{CellPixels, TermSize};

pub(super) fn term(cols: u16, rows: u16) -> Terminal {
    Terminal::new(TermSize::new(cols, rows).unwrap())
}

/// A terminal that also knows its cell pixel size (needed by images).
pub(super) fn sized(cols: u16, rows: u16) -> Terminal {
    let mut t = term(cols, rows);
    t.set_cell_pixels(10, 20);
    t
}

pub(super) fn text(term: &Terminal, row: u16) -> String {
    let line: String = term.row(row).iter().map(|c| c.ch).collect();
    line.trim_end().to_owned()
}

pub(super) fn pos(term: &Terminal) -> (u16, u16) {
    let c = term.cursor();
    (c.col, c.row)
}

/// A kitty graphics command that transmits and places an RGBA image.
pub(super) fn kitty_rgba(id: u32, w: u32, h: u32, extra: &str) -> Vec<u8> {
    let data = crate::image::decode::tests::encode_base64(&[255; 4].repeat((w * h) as usize));
    format!("\x1b_Ga=T,f=32,s={w},v={h},i={id}{extra};{data}\x1b\\").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sized_builds_the_requested_size_with_cell_pixels() {
        let t = sized(5, 3);
        assert_eq!(t.size(), TermSize::new(5, 3).unwrap());
        assert_eq!(t.cell_pixels(), CellPixels::new(10, 20));
    }

    #[test]
    fn text_trims_trailing_blanks_only() {
        let mut t = term(6, 1);
        t.advance(b"  ab  ");
        assert_eq!(text(&t, 0), "  ab");
        assert_eq!(pos(&t), (5, 0));
    }
}
