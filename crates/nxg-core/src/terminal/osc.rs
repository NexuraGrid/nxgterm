//! OSC 10/11/12 color queries and OSC 7 working directory reports.

/// The colors OSC 10, 11 and 12 report: the window's default foreground,
/// background and cursor, which the terminal itself does not know (the
/// window sets them, see [`super::Terminal::set_reported_colors`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportedColors {
    pub foreground: [u8; 3],
    pub background: [u8; 3],
    pub cursor: [u8; 3],
}

impl Default for ReportedColors {
    /// White on black, until the window says otherwise.
    fn default() -> Self {
        Self {
            foreground: [0xff; 3],
            background: [0; 3],
            cursor: [0xff; 3],
        }
    }
}

impl ReportedColors {
    /// The color of OSC `code` (10, 11 or 12).
    fn get(&self, code: u16) -> Option<[u8; 3]> {
        match code {
            10 => Some(self.foreground),
            11 => Some(self.background),
            12 => Some(self.cursor),
            _ => None,
        }
    }

    /// Replies to OSC `code` with `specs`: each `?` asks for the color of
    /// its position (`OSC 10 ; ? ; ?` asks for 10 then 11, as in xterm),
    /// answered as `OSC n ; rgb:rrrr/gggg/bbbb` ended like the query (BEL
    /// or ST). Anything else (setting a color) is not supported and gets
    /// no reply.
    pub fn replies(&self, code: u16, specs: &[&[u8]], bell: bool) -> Vec<u8> {
        let end: &[u8] = if bell { b"\x07" } else { b"\x1b\\" };
        let mut out = Vec::new();
        for (n, spec) in (code..).zip(specs) {
            let Some([r, g, b]) = self.get(n).filter(|_| *spec == b"?") else {
                continue;
            };
            out.extend_from_slice(
                format!("\x1b]{n};rgb:{r:02x}{r:02x}/{g:02x}{g:02x}/{b:02x}{b:02x}").as_bytes(),
            );
            out.extend_from_slice(end);
        }
        out
    }
}

/// The local path of an OSC 7 `file://host/path` URL, percent-decoded;
/// a Windows drive path (`file:///C:/x`) loses its leading slash. `None`
/// for another scheme, a bad escape or a path that is not UTF-8.
pub fn file_url_path(url: &[u8]) -> Option<String> {
    let rest = url.strip_prefix(b"file://")?;
    let path = &rest[rest.iter().position(|&b| b == b'/')?..];
    let mut bytes = Vec::with_capacity(path.len());
    let mut i = 0;
    while i < path.len() {
        if path[i] == b'%' {
            let hex = std::str::from_utf8(path.get(i + 1..i + 3)?).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            bytes.push(path[i]);
            i += 1;
        }
    }
    let path = String::from_utf8(bytes).ok()?;
    let drive = path.as_bytes();
    if drive.len() >= 3 && drive[0] == b'/' && drive[1].is_ascii_alphabetic() && drive[2] == b':' {
        return Some(path[1..].to_owned());
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_decode_to_paths() {
        assert_eq!(file_url_path(b"file://h/a%2Fb/c"), Some("/a/b/c".into()));
        assert_eq!(file_url_path(b"file:///D:/w"), Some("D:/w".into()));
        assert_eq!(file_url_path(b"file://hostonly"), None);
        assert_eq!(file_url_path(b"file://h/x%4"), None, "cut escape");
        assert_eq!(file_url_path(b"file://h/%ff"), None, "not UTF-8");
    }

    #[test]
    fn unknown_codes_and_set_requests_get_no_reply() {
        let colors = ReportedColors::default();
        assert_eq!(
            colors.replies(12, &[b"?", b"?"], true),
            b"\x1b]12;rgb:ffff/ffff/ffff\x07",
            "13 is not answered"
        );
        assert!(colors.replies(10, &[b"red"], true).is_empty());
    }
}
