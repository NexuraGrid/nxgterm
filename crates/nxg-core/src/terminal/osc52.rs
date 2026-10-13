//! OSC 52 (`ESC ] 52 ; Pc ; Pd ST`): the child sets the system clipboard
//! with base64 text, e.g. tmux or ngmux copying a mouse selection.
//!
//! Only writes are honored. A query (`Pd` = `?`) is refused silently: an
//! application must never read what the user copied elsewhere. Empty data
//! (xterm's "clear") is ignored too, so the user's clipboard is never
//! wiped by the child.
//!
//! `Pc` names the targets: empty or `c` is the clipboard, `p` or `s` the
//! PRIMARY selection; naming both sets both. Anything else (cut buffers
//! `0`-`7`, `q`) alone is ignored.

use crate::image::decode;
use crate::ports::ClipboardKind;

/// Longest text accepted, in decoded bytes; longer requests are dropped.
pub const MAX_CLIPBOARD_BYTES: usize = 1 << 20;

/// Longest base64 accepted, checked before decoding.
const MAX_ENCODED_BYTES: usize = MAX_CLIPBOARD_BYTES.div_ceil(3) * 4;

/// Clipboard writes waiting for the app, at most one per kind: a later
/// write replaces an earlier one, so a flood of requests cannot pile up.
#[derive(Debug, Default)]
pub(super) struct Requests(Vec<(ClipboardKind, String)>);

impl Requests {
    /// Queues the write in the `selection` and `data` params of an OSC 52;
    /// ignores queries and anything malformed or too large.
    pub fn handle(&mut self, selection: &[u8], data: &[u8]) {
        let kinds = targets(selection);
        if kinds.is_empty() {
            return;
        }
        let Some(text) = decode_text(data) else {
            return;
        };
        for kind in kinds {
            self.0.retain(|(queued, _)| *queued != kind);
            self.0.push((kind, text.clone()));
        }
    }

    pub fn take(&mut self) -> Vec<(ClipboardKind, String)> {
        std::mem::take(&mut self.0)
    }
}

/// The clipboards `selection` names, clipboard first.
fn targets(selection: &[u8]) -> Vec<ClipboardKind> {
    let clipboard = selection.is_empty() || selection.contains(&b'c');
    let primary = selection.iter().any(|c| matches!(c, b'p' | b's'));
    let clipboard = clipboard.then_some(ClipboardKind::Clipboard);
    let primary = primary.then_some(ClipboardKind::Primary);
    clipboard.into_iter().chain(primary).collect()
}

/// The UTF-8 text in base64 `data`; None for a query, empty or oversized
/// data, bad base64 or invalid UTF-8.
fn decode_text(data: &[u8]) -> Option<String> {
    if data.is_empty() || data == b"?" || data.len() > MAX_ENCODED_BYTES {
        return None;
    }
    let bytes = decode::base64(data).ok()?;
    if bytes.is_empty() || bytes.len() > MAX_CLIPBOARD_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use super::*;
    use crate::image::decode::tests::encode_base64;

    fn osc52(selection: &str, data: &str) -> Vec<u8> {
        format!("\x1b]52;{selection};{data}\x07").into_bytes()
    }

    #[test]
    fn bel_terminated_write_sets_the_clipboard() {
        let mut t = term(10, 3);
        t.advance(&osc52("c", "aGVsbG8="));
        assert_eq!(
            t.take_clipboard_requests(),
            [(ClipboardKind::Clipboard, "hello".to_string())]
        );
    }

    #[test]
    fn st_terminated_write_sets_the_clipboard() {
        let mut t = term(10, 3);
        t.advance(b"\x1b]52;c;aGVsbG8=\x1b\\");
        assert_eq!(
            t.take_clipboard_requests(),
            [(ClipboardKind::Clipboard, "hello".to_string())]
        );
    }

    #[test]
    fn empty_selection_means_the_clipboard() {
        let mut t = term(10, 3);
        t.advance(&osc52("", "aGk="));
        assert_eq!(
            t.take_clipboard_requests(),
            [(ClipboardKind::Clipboard, "hi".to_string())]
        );
    }

    #[test]
    fn p_and_s_mean_the_primary_selection() {
        let mut t = term(10, 3);
        t.advance(&osc52("p", "aGk="));
        assert_eq!(
            t.take_clipboard_requests(),
            [(ClipboardKind::Primary, "hi".to_string())]
        );
        t.advance(&osc52("s", "aGk="));
        assert_eq!(
            t.take_clipboard_requests(),
            [(ClipboardKind::Primary, "hi".to_string())]
        );
    }

    #[test]
    fn naming_both_sets_both() {
        let mut t = term(10, 3);
        t.advance(&osc52("pc", "aGk="));
        assert_eq!(
            t.take_clipboard_requests(),
            [
                (ClipboardKind::Clipboard, "hi".to_string()),
                (ClipboardKind::Primary, "hi".to_string()),
            ]
        );
    }

    #[test]
    fn unknown_selection_is_ignored() {
        let mut t = term(10, 3);
        t.advance(&osc52("0", "aGk="));
        assert!(t.take_clipboard_requests().is_empty());
    }

    #[test]
    fn query_is_refused_without_reply() {
        let mut t = term(10, 3);
        t.advance(&osc52("c", "?"));
        assert!(t.take_clipboard_requests().is_empty());
        assert!(t.take_responses().is_empty());
    }

    #[test]
    fn invalid_base64_and_utf8_are_ignored() {
        let mut t = term(10, 3);
        t.advance(&osc52("c", "not*base64"));
        t.advance(&osc52("c", &encode_base64(&[0xff, 0xfe])));
        t.advance(&osc52("c", ""));
        t.advance(b"\x1b]52;c\x07\x1b]52\x07");
        assert!(t.take_clipboard_requests().is_empty());
    }

    #[test]
    fn oversized_payload_is_ignored() {
        let mut t = term(10, 3);
        let big = encode_base64(&vec![b'a'; MAX_CLIPBOARD_BYTES + 1]);
        t.advance(&osc52("c", &big));
        assert!(t.take_clipboard_requests().is_empty());
        let max = encode_base64(&vec![b'a'; MAX_CLIPBOARD_BYTES]);
        t.advance(&osc52("c", &max));
        assert_eq!(t.take_clipboard_requests()[0].1.len(), MAX_CLIPBOARD_BYTES);
    }

    #[test]
    fn later_write_replaces_a_pending_one() {
        let mut t = term(10, 3);
        t.advance(&osc52("c", "b25l"));
        t.advance(&osc52("c", "dHdv"));
        assert_eq!(
            t.take_clipboard_requests(),
            [(ClipboardKind::Clipboard, "two".to_string())]
        );
    }

    #[test]
    fn take_drains_the_queue() {
        let mut t = term(10, 3);
        t.advance(&osc52("c", "aGk="));
        assert_eq!(t.take_clipboard_requests().len(), 1);
        assert!(t.take_clipboard_requests().is_empty());
    }

    #[test]
    fn leaves_the_screen_alone() {
        let mut t = term(10, 3);
        t.advance(&osc52("c", "aGk="));
        assert_eq!(text(&t, 0), "");
        assert_eq!(pos(&t), (0, 0));
    }
}
