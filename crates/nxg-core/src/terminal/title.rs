//! Window titles: OSC 0 / OSC 2 set them and the XTWINOPS title stack
//! (`CSI 22 ; Ps t` / `CSI 23 ; Ps t`) saves and restores them.
//!
//! Icon names (OSC 1, and the icon half of OSC 0) are ignored: nothing
//! here draws one, so OSC 0 only sets the title.

/// Longest title kept, in bytes; longer ones are cut on a char boundary.
pub const MAX_TITLE_BYTES: usize = 1024;

/// Titles the stack holds before pushing drops the oldest (like xterm).
const STACK_DEPTH: usize = 10;

/// The title set by the child, if any, and the titles it pushed.
#[derive(Debug, Default)]
pub(super) struct Titles {
    current: Option<String>,
    stack: Vec<Option<String>>,
    /// The title changed since [`Titles::take_changed`] last ran.
    changed: bool,
}

impl Titles {
    pub fn get(&self) -> Option<&str> {
        self.current.as_deref()
    }

    /// Sets the title from the raw bytes of an OSC string (see
    /// [`sanitize`]); an empty one clears it.
    pub fn set(&mut self, raw: &[u8]) {
        self.replace(sanitize(raw));
    }

    /// XTWINOPS 22: saves the title.
    pub fn push(&mut self) {
        if self.stack.len() == STACK_DEPTH {
            self.stack.remove(0);
        }
        self.stack.push(self.current.clone());
    }

    /// XTWINOPS 23: restores the last saved title; nothing when none is.
    pub fn pop(&mut self) {
        if let Some(title) = self.stack.pop() {
            self.replace(title);
        }
    }

    /// Whether the title changed since the last call.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    fn replace(&mut self, title: Option<String>) {
        if self.current != title {
            self.current = title;
            self.changed = true;
        }
    }
}

/// A title from raw OSC bytes: invalid UTF-8 replaced, control chars
/// dropped and cut to [`MAX_TITLE_BYTES`]. None when nothing but spaces
/// is left.
fn sanitize(raw: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(raw);
    let mut title = String::new();
    for ch in text.chars().filter(|ch| !ch.is_control()) {
        if title.len() + ch.len_utf8() > MAX_TITLE_BYTES {
            break;
        }
        title.push(ch);
    }
    (!title.trim().is_empty()).then_some(title)
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use super::*;

    #[test]
    fn osc_2_sets_the_title_with_bel() {
        let mut t = term(10, 2);
        assert_eq!(t.title(), None);
        t.advance(b"\x1b]2;hello\x07");
        assert_eq!(t.title(), Some("hello"));
    }

    #[test]
    fn osc_0_sets_the_title_with_st() {
        let mut t = term(10, 2);
        t.advance(b"\x1b]0;hi\x1b\\");
        assert_eq!(t.title(), Some("hi"));
    }

    #[test]
    fn osc_1_icon_names_are_ignored() {
        let mut t = term(10, 2);
        t.advance(b"\x1b]1;icon\x07");
        assert_eq!(t.title(), None);
    }

    #[test]
    fn semicolons_stay_in_the_title() {
        let mut t = term(10, 2);
        t.advance(b"\x1b]2;~: ngmux; vim - ngmux\x07");
        assert_eq!(t.title(), Some("~: ngmux; vim - ngmux"));
    }

    #[test]
    fn an_empty_title_falls_back() {
        let mut t = term(10, 2);
        t.advance(b"\x1b]2;vim\x07");
        t.advance(b"\x1b]2;\x07");
        assert_eq!(t.title(), None);
        t.advance(b"\x1b]2;vim\x07\x1b]0\x07");
        assert_eq!(t.title(), None, "no text at all");
    }

    #[test]
    fn control_chars_are_dropped() {
        let mut t = term(10, 2);
        t.advance("\x1b]2;a\x7fb\u{9b}c\x07".as_bytes());
        assert_eq!(t.title(), Some("abc"));
    }

    #[test]
    fn long_titles_are_cut_on_a_char_boundary() {
        let mut t = term(10, 2);
        let long = "é".repeat(MAX_TITLE_BYTES);
        t.advance(format!("\x1b]2;x{long}\x07").as_bytes());
        let title = t.title().unwrap();
        assert!(title.len() <= MAX_TITLE_BYTES);
        assert_eq!(title.len(), MAX_TITLE_BYTES - 1, "x plus whole é's");
        assert!(title.starts_with("xé"));
    }

    #[test]
    fn title_text_is_not_printed() {
        let mut t = term(10, 2);
        t.advance(b"a\x1b]2;title\x07b");
        assert_eq!(text(&t, 0), "ab");
    }

    #[test]
    fn changes_are_reported_once() {
        let mut t = term(10, 2);
        assert!(!t.take_title_changed());
        t.advance(b"\x1b]2;one\x07");
        assert!(t.take_title_changed());
        assert!(!t.take_title_changed());
        t.advance(b"\x1b]2;one\x07");
        assert!(!t.take_title_changed(), "same title again");
        t.advance(b"\x1b]2;\x07");
        assert!(t.take_title_changed(), "reset");
    }

    #[test]
    fn the_title_stack_saves_and_restores() {
        let mut t = term(10, 2);
        t.advance(b"\x1b]2;shell\x07\x1b[22;0t\x1b]2;vim\x07");
        assert_eq!(t.title(), Some("vim"));
        t.advance(b"\x1b[23;0t");
        assert_eq!(t.title(), Some("shell"));
        t.advance(b"\x1b[23;0t");
        assert_eq!(t.title(), Some("shell"), "popping an empty stack");
        t.advance(b"\x1b[22;2t\x1b]2;vim\x07\x1b[23;2t");
        assert_eq!(t.title(), Some("shell"), "Ps 2 is the title");
        t.advance(b"\x1b[22;1t\x1b]2;vim\x07\x1b[23;1t");
        assert_eq!(t.title(), Some("vim"), "Ps 1 is the icon name only");
    }

    #[test]
    fn the_title_stack_is_bounded() {
        let mut titles = Titles::default();
        for n in 0..=STACK_DEPTH {
            titles.set(n.to_string().as_bytes());
            titles.push();
        }
        assert_eq!(titles.stack.len(), STACK_DEPTH);
        assert_eq!(titles.stack[0].as_deref(), Some("1"), "the oldest went");
    }
}
