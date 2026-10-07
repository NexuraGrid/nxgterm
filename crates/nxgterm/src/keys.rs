//! Keyboard input encoding: winit keys to the bytes a shell expects.

use nxg_core::Modes;
use winit::keyboard::{Key, ModifiersState, NamedKey};

/// Bytes to send to the pty for a key press, or `None` if it sends nothing.
///
/// `text` is the text the key produced (winit's `KeyEvent::text`). `modes` is
/// the terminal's current state: full-screen programs switch cursor keys to
/// `ESC O x` and would misread the normal form.
pub fn encode(
    key: &Key,
    text: Option<&str>,
    mods: ModifiersState,
    modes: Modes,
) -> Option<Vec<u8>> {
    match key {
        Key::Named(named) => encode_named(*named, mods, modes).map(|bytes| bytes.to_vec()),
        Key::Character(chars) => encode_char(chars, text, mods),
        _ => None,
    }
}

fn encode_named(key: NamedKey, mods: ModifiersState, modes: Modes) -> Option<&'static [u8]> {
    Some(match key {
        NamedKey::Enter => b"\r",
        NamedKey::Backspace => b"\x7f",
        NamedKey::Tab if mods.shift_key() => b"\x1b[Z",
        NamedKey::Tab => b"\t",
        NamedKey::Escape => b"\x1b",
        NamedKey::Space if mods.control_key() => b"\0",
        NamedKey::Space => b" ",
        NamedKey::ArrowUp if modes.app_cursor_keys => b"\x1bOA",
        NamedKey::ArrowUp => b"\x1b[A",
        NamedKey::ArrowDown if modes.app_cursor_keys => b"\x1bOB",
        NamedKey::ArrowDown => b"\x1b[B",
        NamedKey::ArrowRight if modes.app_cursor_keys => b"\x1bOC",
        NamedKey::ArrowRight => b"\x1b[C",
        NamedKey::ArrowLeft if modes.app_cursor_keys => b"\x1bOD",
        NamedKey::ArrowLeft => b"\x1b[D",
        NamedKey::Home if modes.app_cursor_keys => b"\x1bOH",
        NamedKey::Home => b"\x1b[H",
        NamedKey::End if modes.app_cursor_keys => b"\x1bOF",
        NamedKey::End => b"\x1b[F",
        NamedKey::Insert => b"\x1b[2~",
        NamedKey::Delete => b"\x1b[3~",
        NamedKey::PageUp => b"\x1b[5~",
        NamedKey::PageDown => b"\x1b[6~",
        _ => return None,
    })
}

fn encode_char(chars: &str, text: Option<&str>, mods: ModifiersState) -> Option<Vec<u8>> {
    if mods.super_key() {
        return None; // Reserved for application shortcuts.
    }
    let mut bytes = Vec::new();
    if mods.alt_key() {
        bytes.push(0x1b);
    }
    match single_ascii_letter(chars) {
        Some(letter) if mods.control_key() => bytes.push(letter - b'a' + 1),
        _ => bytes.extend_from_slice(text.unwrap_or(chars).as_bytes()),
    }
    Some(bytes)
}

fn single_ascii_letter(chars: &str) -> Option<u8> {
    match chars.as_bytes() {
        [b] if b.is_ascii_alphabetic() => Some(b.to_ascii_lowercase()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nxg_core::Modes;

    fn named(key: NamedKey) -> Option<Vec<u8>> {
        encode(
            &Key::Named(key),
            None,
            ModifiersState::empty(),
            Modes::default(),
        )
    }

    fn named_app(key: NamedKey) -> Option<Vec<u8>> {
        let modes = Modes {
            app_cursor_keys: true,
            ..Modes::default()
        };
        encode(&Key::Named(key), None, ModifiersState::empty(), modes)
    }

    fn ch(c: &str, mods: ModifiersState) -> Option<Vec<u8>> {
        encode(&Key::Character(c.into()), Some(c), mods, Modes::default())
    }

    #[test]
    fn encodes_editing_keys() {
        assert_eq!(named(NamedKey::Enter).unwrap(), b"\r");
        assert_eq!(named(NamedKey::Backspace).unwrap(), b"\x7f");
        assert_eq!(named(NamedKey::Tab).unwrap(), b"\t");
        assert_eq!(named(NamedKey::Escape).unwrap(), b"\x1b");
        assert_eq!(named(NamedKey::Delete).unwrap(), b"\x1b[3~");
        assert_eq!(named(NamedKey::Space).unwrap(), b" ");
    }

    #[test]
    fn encodes_navigation_keys() {
        assert_eq!(named(NamedKey::ArrowUp).unwrap(), b"\x1b[A");
        assert_eq!(named(NamedKey::ArrowDown).unwrap(), b"\x1b[B");
        assert_eq!(named(NamedKey::ArrowRight).unwrap(), b"\x1b[C");
        assert_eq!(named(NamedKey::ArrowLeft).unwrap(), b"\x1b[D");
        assert_eq!(named(NamedKey::Home).unwrap(), b"\x1b[H");
        assert_eq!(named(NamedKey::End).unwrap(), b"\x1b[F");
        assert_eq!(named(NamedKey::PageUp).unwrap(), b"\x1b[5~");
        assert_eq!(named(NamedKey::PageDown).unwrap(), b"\x1b[6~");
    }

    #[test]
    fn application_cursor_keys_use_ss3() {
        assert_eq!(named_app(NamedKey::ArrowUp).unwrap(), b"\x1bOA");
        assert_eq!(named_app(NamedKey::ArrowDown).unwrap(), b"\x1bOB");
        assert_eq!(named_app(NamedKey::ArrowRight).unwrap(), b"\x1bOC");
        assert_eq!(named_app(NamedKey::ArrowLeft).unwrap(), b"\x1bOD");
        assert_eq!(named_app(NamedKey::Home).unwrap(), b"\x1bOH");
        assert_eq!(named_app(NamedKey::End).unwrap(), b"\x1bOF");
    }

    #[test]
    fn application_cursor_keys_leave_other_keys_alone() {
        assert_eq!(named_app(NamedKey::PageUp).unwrap(), b"\x1b[5~");
        assert_eq!(named_app(NamedKey::Delete).unwrap(), b"\x1b[3~");
        assert_eq!(named_app(NamedKey::Enter).unwrap(), b"\r");
        let modes = Modes {
            app_cursor_keys: true,
            alt_screen: true,
        };
        let bytes = encode(
            &Key::Character("a".into()),
            Some("a"),
            ModifiersState::empty(),
            modes,
        );
        assert_eq!(bytes.unwrap(), b"a");
        // alt_screen alone does not change arrows.
        let alt_only = Modes {
            app_cursor_keys: false,
            alt_screen: true,
        };
        let up = encode(
            &Key::Named(NamedKey::ArrowUp),
            None,
            ModifiersState::empty(),
            alt_only,
        );
        assert_eq!(up.unwrap(), b"\x1b[A");
    }

    #[test]
    fn shift_tab_is_back_tab() {
        let bytes = encode(
            &Key::Named(NamedKey::Tab),
            None,
            ModifiersState::SHIFT,
            Modes::default(),
        );
        assert_eq!(bytes.unwrap(), b"\x1b[Z");
    }

    #[test]
    fn encodes_text_as_utf8() {
        assert_eq!(ch("a", ModifiersState::empty()).unwrap(), b"a");
        assert_eq!(ch("A", ModifiersState::SHIFT).unwrap(), b"A");
        assert_eq!(ch("ñ", ModifiersState::empty()).unwrap(), "ñ".as_bytes());
    }

    #[test]
    fn ctrl_letter_maps_to_control_codes() {
        assert_eq!(ch("a", ModifiersState::CONTROL).unwrap(), [0x01]);
        assert_eq!(ch("c", ModifiersState::CONTROL).unwrap(), [0x03]);
        assert_eq!(ch("Z", ModifiersState::CONTROL).unwrap(), [0x1a]);
    }

    #[test]
    fn alt_prefixes_escape() {
        assert_eq!(ch("b", ModifiersState::ALT).unwrap(), b"\x1bb");
    }

    #[test]
    fn modifier_only_and_unknown_keys_send_nothing() {
        assert_eq!(named(NamedKey::Shift), None);
        assert_eq!(named(NamedKey::F24), None);
        assert_eq!(
            encode(
                &Key::Character("x".into()),
                None,
                ModifiersState::SUPER,
                Modes::default()
            ),
            None
        );
    }
}
