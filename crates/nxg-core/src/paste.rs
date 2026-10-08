//! Pasted text turned into the bytes sent to the child.

/// Starts and ends a bracketed paste (mode 2004).
pub const BRACKET_START: &[u8] = b"\x1b[200~";
pub const BRACKET_END: &[u8] = b"\x1b[201~";

/// The bytes for pasting `text`: line breaks become `\r` (what the Enter
/// key sends), and with `bracketed` the result is wrapped in
/// [`BRACKET_START`] and [`BRACKET_END`].
///
/// Control characters other than tab and line breaks (C0, DEL and C1) are
/// dropped in both modes, ESC included, so pasted text can neither end a
/// bracketed paste early (`ESC [ 201 ~`) nor run escape sequences or
/// control keys in the application.
pub fn encode(text: &str, bracketed: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + BRACKET_START.len() + BRACKET_END.len());
    if bracketed {
        out.extend_from_slice(BRACKET_START);
    }
    let mut chars = text.chars().peekable();
    let mut utf8 = [0; 4];
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                chars.next_if_eq(&'\n');
                out.push(b'\r');
            }
            '\n' => out.push(b'\r'),
            '\t' => out.push(b'\t'),
            ch if ch.is_control() => {}
            ch => out.extend_from_slice(ch.encode_utf8(&mut utf8).as_bytes()),
        }
    }
    if bracketed {
        out.extend_from_slice(BRACKET_END);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_sent_as_utf8() {
        assert_eq!(encode("ls -la ~/ñ", false), "ls -la ~/ñ".as_bytes());
        assert_eq!(encode("", false), b"");
    }

    #[test]
    fn line_breaks_become_carriage_returns() {
        assert_eq!(encode("a\nb\r\nc\rd\n\n", false), b"a\rb\rc\rd\r\r");
    }

    #[test]
    fn tabs_are_kept_and_other_controls_dropped() {
        assert_eq!(encode("a\tb\x03c\x7fd\u{9b}e\x00", false), b"a\tbcde");
    }

    #[test]
    fn escapes_are_dropped_even_without_bracketing() {
        assert_eq!(encode("x\x1b[31my", false), b"x[31my");
    }

    #[test]
    fn bracketed_pastes_are_wrapped() {
        assert_eq!(encode("hi\n", true), b"\x1b[200~hi\r\x1b[201~");
        assert_eq!(encode("", true), b"\x1b[200~\x1b[201~");
    }

    #[test]
    fn a_pasted_end_marker_cannot_close_the_bracket() {
        let bytes = encode("a\x1b[201~; rm -rf ~\n", true);
        assert_eq!(bytes, b"\x1b[200~a[201~; rm -rf ~\r\x1b[201~");
        let body = &bytes[BRACKET_START.len()..bytes.len() - BRACKET_END.len()];
        assert!(!body.contains(&0x1b));
    }
}
