//! Application key bindings, resolved before keys reach the pty.
//!
//! Hardcoded for now: the primary modifier (Ctrl, or Cmd on macOS) plus
//! `=`/`+` grows the font, `-` shrinks it and `0` resets it.

use winit::keyboard::{Key, ModifiersState};

/// Something the terminal does instead of sending the key to the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Font size up one step.
    ZoomIn,
    ZoomOut,
    ResetZoom,
}

/// The action bound to `key` with `mods`, if any. `macos` selects Cmd as
/// the primary modifier instead of Ctrl. Shift is allowed (`+` is usually
/// Shift+`=`); any other extra modifier means no binding.
pub fn resolve(key: &Key, mods: ModifiersState, macos: bool) -> Option<Action> {
    let primary = if macos {
        ModifiersState::SUPER
    } else {
        ModifiersState::CONTROL
    };
    if mods - ModifiersState::SHIFT != primary {
        return None;
    }
    match key {
        Key::Character(c) => match c.as_str() {
            "=" | "+" => Some(Action::ZoomIn),
            "-" => Some(Action::ZoomOut),
            "0" => Some(Action::ResetZoom),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::NamedKey;

    const CTRL: ModifiersState = ModifiersState::CONTROL;
    const CMD: ModifiersState = ModifiersState::SUPER;

    fn ch(c: &str) -> Key {
        Key::Character(c.into())
    }

    #[test]
    fn ctrl_bindings_off_macos() {
        assert_eq!(resolve(&ch("="), CTRL, false), Some(Action::ZoomIn));
        assert_eq!(resolve(&ch("+"), CTRL, false), Some(Action::ZoomIn));
        assert_eq!(resolve(&ch("-"), CTRL, false), Some(Action::ZoomOut));
        assert_eq!(resolve(&ch("0"), CTRL, false), Some(Action::ResetZoom));
    }

    #[test]
    fn shift_is_allowed_for_plus() {
        let mods = CTRL | ModifiersState::SHIFT;
        assert_eq!(resolve(&ch("+"), mods, false), Some(Action::ZoomIn));
    }

    #[test]
    fn cmd_bindings_on_macos() {
        assert_eq!(resolve(&ch("="), CMD, true), Some(Action::ZoomIn));
        assert_eq!(resolve(&ch("-"), CMD, true), Some(Action::ZoomOut));
        assert_eq!(resolve(&ch("0"), CMD, true), Some(Action::ResetZoom));
        assert_eq!(resolve(&ch("="), CTRL, true), None, "Ctrl is not primary");
        assert_eq!(resolve(&ch("="), CMD, false), None, "Cmd only on macOS");
    }

    #[test]
    fn needs_exactly_the_primary_modifier() {
        assert_eq!(resolve(&ch("="), ModifiersState::empty(), false), None);
        assert_eq!(resolve(&ch("="), CTRL | ModifiersState::ALT, false), None);
        assert_eq!(resolve(&ch("="), CTRL | CMD, false), None);
        assert_eq!(resolve(&ch("="), CMD | CTRL, true), None);
    }

    #[test]
    fn other_keys_are_not_bound() {
        assert_eq!(resolve(&ch("c"), CTRL, false), None);
        assert_eq!(resolve(&ch("_"), CTRL, false), None);
        assert_eq!(resolve(&ch("1"), CTRL, false), None);
        assert_eq!(resolve(&Key::Named(NamedKey::Enter), CTRL, false), None);
    }
}
