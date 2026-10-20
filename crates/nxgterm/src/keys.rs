//! Keyboard input encoding: winit keys to the bytes a shell expects, in
//! xterm's default (PC-style) form, as WezTerm and Ghostty send them.

use nxg_core::Modes;
use winit::keyboard::{Key, ModifiersState, NamedKey};

const ESC: u8 = 0x1b;

/// Bytes to send to the pty for a key press, or `None` if it sends nothing.
///
/// `text` is the text the key produced (winit's `KeyEvent::text`) and
/// `text_all` the text with every modifier applied, Ctrl included
/// (`KeyEventExtModifierSupplement::text_with_all_modifiers`). `modes` is
/// the terminal's current state: full-screen programs switch cursor keys to
/// `ESC O x` and would misread the normal form.
///
/// Windows reports AltGr as Ctrl+Alt: there, when both are held and
/// `text_all` is printable, the key typed that text (`@` from AltGr+Q on a
/// German layout) and it is sent as is, without an ESC prefix or a Ctrl
/// code (see [`AltGr`]).
pub fn encode(
    key: &Key,
    text: Option<&str>,
    text_all: Option<&str>,
    mods: ModifiersState,
    modes: Modes,
) -> Option<Vec<u8>> {
    match key {
        Key::Named(named) => encode_named(*named, mods, modes),
        Key::Character(chars) => encode_char(chars, text, text_all, mods, ALTGR),
        _ => None,
    }
}

/// How the platform reports AltGr.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AltGr {
    /// As Ctrl+Alt (Windows): Ctrl+Alt with printable text is AltGr.
    CtrlAlt,
    /// As a modifier of its own, so Ctrl+Alt is always Ctrl and Alt: X11
    /// and Wayland (ISO Level 3 shift, never reported as Ctrl or Alt) and
    /// macOS (Option is Alt; its text is not trusted with Ctrl held).
    Distinct,
}

/// How this platform reports AltGr.
const ALTGR: AltGr = if cfg!(windows) {
    AltGr::CtrlAlt
} else {
    AltGr::Distinct
};

/// xterm's modifier parameter: 1 plus Shift 1, Alt 2 and Ctrl 4, or
/// `None` without any of them.
fn modifier_param(mods: ModifiersState) -> Option<u8> {
    let param = u8::from(mods.shift_key())
        | u8::from(mods.alt_key()) << 1
        | u8::from(mods.control_key()) << 2;
    (param != 0).then_some(param + 1)
}

fn encode_named(key: NamedKey, mods: ModifiersState, modes: Modes) -> Option<Vec<u8>> {
    let param = modifier_param(mods);
    // Cursor-style keys: `CSI 1 ; m X` when modified, else SS3 or CSI.
    let cursor = |last: u8, ss3: bool| match param {
        Some(m) => format!("\x1b[1;{m}{}", char::from(last)).into_bytes(),
        None if ss3 => vec![ESC, b'O', last],
        None => vec![ESC, b'[', last],
    };
    // Tilde keys: `CSI n ~` or `CSI n ; m ~`.
    let tilde = |n: u8| match param {
        Some(m) => format!("\x1b[{n};{m}~").into_bytes(),
        None => format!("\x1b[{n}~").into_bytes(),
    };
    let app = modes.app_cursor_keys;
    // Keys that send one byte, with ESC first when Alt is held.
    let alt = |byte: u8| {
        if mods.alt_key() {
            vec![ESC, byte]
        } else {
            vec![byte]
        }
    };
    let ctrl = mods.control_key();
    Some(match key {
        NamedKey::Enter => alt(b'\r'),
        NamedKey::Backspace => alt(if ctrl { 0x08 } else { 0x7f }),
        NamedKey::Tab if mods.shift_key() => b"\x1b[Z".to_vec(),
        NamedKey::Tab => alt(b'\t'),
        NamedKey::Escape => alt(ESC),
        NamedKey::Space => alt(if ctrl { 0 } else { b' ' }),
        NamedKey::ArrowUp => cursor(b'A', app),
        NamedKey::ArrowDown => cursor(b'B', app),
        NamedKey::ArrowRight => cursor(b'C', app),
        NamedKey::ArrowLeft => cursor(b'D', app),
        NamedKey::Home => cursor(b'H', app),
        NamedKey::End => cursor(b'F', app),
        NamedKey::F1 => cursor(b'P', true),
        NamedKey::F2 => cursor(b'Q', true),
        NamedKey::F3 => cursor(b'R', true),
        NamedKey::F4 => cursor(b'S', true),
        NamedKey::Insert => tilde(2),
        NamedKey::Delete => tilde(3),
        NamedKey::PageUp => tilde(5),
        NamedKey::PageDown => tilde(6),
        NamedKey::F5 => tilde(15),
        NamedKey::F6 => tilde(17),
        NamedKey::F7 => tilde(18),
        NamedKey::F8 => tilde(19),
        NamedKey::F9 => tilde(20),
        NamedKey::F10 => tilde(21),
        NamedKey::F11 => tilde(23),
        NamedKey::F12 => tilde(24),
        _ => return None,
    })
}

fn encode_char(
    chars: &str,
    text: Option<&str>,
    text_all: Option<&str>,
    mods: ModifiersState,
    altgr: AltGr,
) -> Option<Vec<u8>> {
    if mods.super_key() {
        return None; // Reserved for application shortcuts.
    }
    if altgr == AltGr::CtrlAlt
        && mods.control_key()
        && mods.alt_key()
        && let Some(typed) = text_all.filter(|typed| is_printable(typed))
    {
        return Some(typed.as_bytes().to_vec()); // AltGr.
    }
    let mut bytes = Vec::new();
    if mods.alt_key() {
        bytes.push(ESC);
    }
    match single_char(chars).and_then(ctrl_code) {
        Some(code) if mods.control_key() => bytes.push(code),
        _ => bytes.extend_from_slice(text.unwrap_or(chars).as_bytes()),
    }
    Some(bytes)
}

/// Whether `text` is non-empty and has no control chars.
fn is_printable(text: &str) -> bool {
    !text.is_empty() && !text.chars().any(char::is_control)
}

fn single_char(chars: &str) -> Option<char> {
    let mut iter = chars.chars();
    match (iter.next(), iter.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

/// The byte Ctrl+`c` sends in xterm: letters and `@ [ \ ] ^ _` to their
/// C0 codes, Space like `@` (NUL), `/` like `_` (US), `?` to DEL, and the
/// digits 2 to 8 to the VT220 codes NUL, ESC, FS, GS, RS, US and DEL (only
/// 2 and 6 match the US-layout symbols above them).
fn ctrl_code(c: char) -> Option<u8> {
    Some(match c {
        'a'..='z' | 'A'..='Z' => c.to_ascii_lowercase() as u8 - b'a' + 1,
        '@' | ' ' | '2' => 0x00,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '/' | '7' => 0x1f,
        '?' | '8' => 0x7f,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nxg_core::Modes;

    fn named(key: NamedKey) -> Option<Vec<u8>> {
        encode(
            &Key::Named(key),
            None,
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
        encode(&Key::Named(key), None, None, ModifiersState::empty(), modes)
    }

    fn ch(c: &str, mods: ModifiersState) -> Option<Vec<u8>> {
        encode(
            &Key::Character(c.into()),
            Some(c),
            None,
            mods,
            Modes::default(),
        )
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
            ..Modes::default()
        };
        let bytes = encode(
            &Key::Character("a".into()),
            Some("a"),
            None,
            ModifiersState::empty(),
            modes,
        );
        assert_eq!(bytes.unwrap(), b"a");
        // alt_screen alone does not change arrows.
        let alt_only = Modes {
            app_cursor_keys: false,
            alt_screen: true,
            ..Modes::default()
        };
        let up = encode(
            &Key::Named(NamedKey::ArrowUp),
            None,
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
                None,
                ModifiersState::SUPER,
                Modes::default()
            ),
            None
        );
    }

    const SHIFT: ModifiersState = ModifiersState::SHIFT;
    const ALT: ModifiersState = ModifiersState::ALT;
    const CTRL: ModifiersState = ModifiersState::CONTROL;

    fn key_with(key: NamedKey, mods: ModifiersState) -> Vec<u8> {
        encode(&Key::Named(key), None, None, mods, Modes::default()).unwrap()
    }

    fn app_key_with(key: NamedKey, mods: ModifiersState) -> Vec<u8> {
        let modes = Modes {
            app_cursor_keys: true,
            ..Modes::default()
        };
        encode(&Key::Named(key), None, None, mods, modes).unwrap()
    }

    #[test]
    fn modified_cursor_keys_send_csi_1_with_the_modifier_param() {
        let none = ModifiersState::empty();
        let rows: &[(NamedKey, ModifiersState, &[u8])] = &[
            (NamedKey::ArrowUp, SHIFT, b"\x1b[1;2A"),
            (NamedKey::ArrowDown, ALT, b"\x1b[1;3B"),
            (NamedKey::ArrowRight, SHIFT | ALT, b"\x1b[1;4C"),
            (NamedKey::ArrowLeft, CTRL, b"\x1b[1;5D"),
            (NamedKey::Home, CTRL | SHIFT, b"\x1b[1;6H"),
            (NamedKey::End, CTRL | ALT, b"\x1b[1;7F"),
            (NamedKey::ArrowUp, CTRL | ALT | SHIFT, b"\x1b[1;8A"),
            (NamedKey::ArrowUp, none, b"\x1b[A"),
        ];
        for &(key, mods, bytes) in rows {
            assert_eq!(key_with(key, mods), bytes, "{key:?} {mods:?}");
            if !mods.is_empty() {
                // DECCKM only changes the unmodified form.
                assert_eq!(app_key_with(key, mods), bytes, "app {key:?} {mods:?}");
            }
        }
    }

    #[test]
    fn modified_editing_keys_send_the_modifier_param_before_the_tilde() {
        let rows: &[(NamedKey, ModifiersState, &[u8])] = &[
            (NamedKey::Insert, ModifiersState::empty(), b"\x1b[2~"),
            (NamedKey::Insert, SHIFT, b"\x1b[2;2~"),
            (NamedKey::Delete, CTRL, b"\x1b[3;5~"),
            (NamedKey::PageUp, ALT, b"\x1b[5;3~"),
            (NamedKey::PageDown, CTRL | SHIFT, b"\x1b[6;6~"),
        ];
        for &(key, mods, bytes) in rows {
            assert_eq!(key_with(key, mods), bytes, "{key:?} {mods:?}");
        }
    }

    #[test]
    fn function_keys_follow_xterm() {
        let none = ModifiersState::empty();
        let rows: &[(NamedKey, ModifiersState, &[u8])] = &[
            (NamedKey::F1, none, b"\x1bOP"),
            (NamedKey::F2, none, b"\x1bOQ"),
            (NamedKey::F3, none, b"\x1bOR"),
            (NamedKey::F4, none, b"\x1bOS"),
            (NamedKey::F1, SHIFT, b"\x1b[1;2P"),
            (NamedKey::F4, CTRL, b"\x1b[1;5S"),
            (NamedKey::F5, none, b"\x1b[15~"),
            (NamedKey::F6, none, b"\x1b[17~"),
            (NamedKey::F7, none, b"\x1b[18~"),
            (NamedKey::F8, none, b"\x1b[19~"),
            (NamedKey::F9, none, b"\x1b[20~"),
            (NamedKey::F10, none, b"\x1b[21~"),
            (NamedKey::F11, none, b"\x1b[23~"),
            (NamedKey::F12, none, b"\x1b[24~"),
            (NamedKey::F5, SHIFT, b"\x1b[15;2~"),
            (NamedKey::F12, CTRL | ALT, b"\x1b[24;7~"),
        ];
        for &(key, mods, bytes) in rows {
            assert_eq!(key_with(key, mods), bytes, "{key:?} {mods:?}");
        }
    }

    #[test]
    fn backspace_tab_enter_and_escape_with_modifiers() {
        let rows: &[(NamedKey, ModifiersState, &[u8])] = &[
            (NamedKey::Backspace, CTRL, b"\x08"),
            (NamedKey::Backspace, ALT, b"\x1b\x7f"),
            (NamedKey::Backspace, CTRL | ALT, b"\x1b\x08"),
            (NamedKey::Tab, SHIFT, b"\x1b[Z"),
            (NamedKey::Tab, ALT, b"\x1b\t"),
            (NamedKey::Enter, ALT, b"\x1b\r"),
            (NamedKey::Escape, ALT, b"\x1b\x1b"),
            (NamedKey::Space, CTRL, b"\0"),
            (NamedKey::Space, ALT, b"\x1b "),
            (NamedKey::Space, CTRL | ALT, b"\x1b\0"),
        ];
        for &(key, mods, bytes) in rows {
            assert_eq!(key_with(key, mods), bytes, "{key:?} {mods:?}");
        }
    }

    #[test]
    fn ctrl_punctuation_and_digits_map_like_xterm() {
        let rows: &[(&str, u8)] = &[
            ("@", 0x00),
            (" ", 0x00),
            ("[", 0x1b),
            ("\\", 0x1c),
            ("]", 0x1d),
            ("^", 0x1e),
            ("_", 0x1f),
            ("/", 0x1f),
            ("?", 0x7f),
            ("2", 0x00),
            ("3", 0x1b),
            ("4", 0x1c),
            ("5", 0x1d),
            ("6", 0x1e),
            ("7", 0x1f),
            ("8", 0x7f),
        ];
        for &(c, byte) in rows {
            assert_eq!(ch(c, CTRL).unwrap(), [byte], "ctrl {c:?}");
            assert_eq!(ch(c, CTRL | ALT).unwrap(), [0x1b, byte], "ctrl alt {c:?}");
        }
        assert_eq!(ch("1", CTRL).unwrap(), b"1", "unmapped digits stay");
        assert_eq!(ch("9", CTRL).unwrap(), b"9");
    }

    /// A key that typed `text` with Ctrl+Alt held on Windows, where
    /// `text_all` is what the layout made of it with every modifier.
    fn ctrl_alt(c: &str, text_all: Option<&str>) -> Option<Vec<u8>> {
        encode_char(c, Some(c), text_all, CTRL | ALT, AltGr::CtrlAlt)
    }

    #[test]
    fn ctrl_alt_is_no_altgr_off_windows() {
        // X11, Wayland and macOS report AltGr (Option) as a modifier of
        // its own: Ctrl+Alt+1 is ESC 1 there, even though "1" prints.
        let bytes = encode_char("1", Some("1"), Some("1"), CTRL | ALT, AltGr::Distinct);
        assert_eq!(bytes.unwrap(), b"\x1b1");
        let bytes = encode_char("a", Some("a"), Some("a"), CTRL | ALT, AltGr::Distinct);
        assert_eq!(bytes.unwrap(), b"\x1b\x01");
        assert_eq!(
            ALTGR,
            if cfg!(windows) {
                AltGr::CtrlAlt
            } else {
                AltGr::Distinct
            }
        );
    }

    #[test]
    fn altgr_text_is_sent_as_typed() {
        // AltGr+Q on a German layout arrives as Ctrl+Alt with "@".
        for c in ["@", "{", "[", "\\", "€", "~"] {
            assert_eq!(ctrl_alt(c, Some(c)).unwrap(), c.as_bytes(), "{c:?}");
        }
    }

    #[test]
    fn ctrl_alt_without_printable_text_is_escape_and_a_control_code() {
        assert_eq!(ctrl_alt("a", Some("\x01")).unwrap(), b"\x1b\x01");
        assert_eq!(ctrl_alt("a", None).unwrap(), b"\x1b\x01");
        assert_eq!(ctrl_alt("[", Some("\x1b")).unwrap(), b"\x1b\x1b");
    }

    #[test]
    fn alt_text_keeps_the_escape_prefix() {
        let bytes = encode(
            &Key::Character("x".into()),
            Some("x"),
            Some("x"),
            ALT,
            Modes::default(),
        );
        assert_eq!(bytes.unwrap(), b"\x1bx");
    }
}
