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

/// The text pasted for a file dropped on the window: its `path` quoted so
/// a shell (or an agent's prompt) reads it as one word, followed by a space
/// so several dropped files end up separated.
///
/// On Unix shell metacharacters and spaces are escaped with a backslash,
/// as Ghostty and WezTerm do; on `windows` a path with spaces is wrapped
/// in double quotes (backslashes there are separators, not escapes).
pub fn dropped_path(path: &str, windows: bool) -> String {
    let mut out = String::with_capacity(path.len() + 3);
    if windows {
        if path.contains([' ', '&', '(', ')', '^', ';', ',']) {
            out.push('"');
            out.push_str(path);
            out.push('"');
        } else {
            out.push_str(path);
        }
    } else {
        for ch in path.chars() {
            if ch.is_ascii() && !ch.is_ascii_alphanumeric() && !"/._-+,:@%=~".contains(ch) {
                out.push('\\');
            }
            out.push(ch);
        }
    }
    out.push(' ');
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
    fn plain_dropped_paths_are_kept_and_followed_by_a_space() {
        assert_eq!(
            dropped_path("/home/u/img-1_a.png", false),
            "/home/u/img-1_a.png "
        );
        assert_eq!(dropped_path("/tmp/ñandú.jpg", false), "/tmp/ñandú.jpg ");
    }

    #[test]
    fn shell_special_characters_in_dropped_paths_are_escaped() {
        assert_eq!(
            dropped_path("/home/u/My Pics/a (1).png", false),
            r"/home/u/My\ Pics/a\ \(1\).png "
        );
        assert_eq!(
            dropped_path("/x/it's&$`\"*?;|<>!#.png", false),
            r#"/x/it\'s\&\$\`\"\*\?\;\|\<\>\!\#.png "#
        );
        assert_eq!(dropped_path(r"/x/a\b", false), r"/x/a\\b ");
    }

    #[test]
    fn windows_dropped_paths_are_double_quoted_only_when_needed() {
        assert_eq!(dropped_path(r"C:\img.png", true), r"C:\img.png ");
        assert_eq!(
            dropped_path(r"C:\My Pics\a.png", true),
            r#""C:\My Pics\a.png" "#
        );
    }

    #[test]
    fn a_pasted_end_marker_cannot_close_the_bracket() {
        let bytes = encode("a\x1b[201~; rm -rf ~\n", true);
        assert_eq!(bytes, b"\x1b[200~a[201~; rm -rf ~\r\x1b[201~");
        let body = &bytes[BRACKET_START.len()..bytes.len() - BRACKET_END.len()];
        assert!(!body.contains(&0x1b));
    }
}
