//! Key events matched against the configured key bindings, before keys
//! reach the pty. The bindings themselves (actions, chords, defaults and
//! `[keybindings]`) live in `nxg_config::keybindings`.

pub use nxg_config::keybindings::Action;
use nxg_config::keybindings::{Bindings, Chord, ChordKey, Mods};
use winit::keyboard::{Key, ModifiersState, NamedKey};

/// The chord for a pressed `key` with `mods`; `None` for keys a chord
/// cannot name (modifiers alone, composed text, unsupported named keys).
pub fn to_chord(key: &Key, mods: ModifiersState) -> Option<Chord> {
    let key = match key {
        Key::Character(text) => {
            let mut chars = text.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else {
                return None;
            };
            if c == ' ' {
                ChordKey::Space
            } else {
                let mut lower = c.to_lowercase();
                match (lower.next(), lower.next()) {
                    (Some(lower), None) => ChordKey::Char(lower),
                    _ => ChordKey::Char(c),
                }
            }
        }
        Key::Named(named) => named_key(*named)?,
        _ => return None,
    };
    let mods = Mods {
        ctrl: mods.control_key(),
        alt: mods.alt_key(),
        shift: mods.shift_key(),
        super_key: mods.super_key(),
    };
    Some(Chord::new(mods, key))
}

fn named_key(key: NamedKey) -> Option<ChordKey> {
    Some(match key {
        NamedKey::Tab => ChordKey::Tab,
        NamedKey::Enter => ChordKey::Enter,
        NamedKey::Escape => ChordKey::Escape,
        NamedKey::Space => ChordKey::Space,
        NamedKey::Backspace => ChordKey::Backspace,
        NamedKey::Delete => ChordKey::Delete,
        NamedKey::Insert => ChordKey::Insert,
        NamedKey::Home => ChordKey::Home,
        NamedKey::End => ChordKey::End,
        NamedKey::PageUp => ChordKey::PageUp,
        NamedKey::PageDown => ChordKey::PageDown,
        NamedKey::ArrowUp => ChordKey::Up,
        NamedKey::ArrowDown => ChordKey::Down,
        NamedKey::ArrowLeft => ChordKey::Left,
        NamedKey::ArrowRight => ChordKey::Right,
        NamedKey::F1 => ChordKey::F(1),
        NamedKey::F2 => ChordKey::F(2),
        NamedKey::F3 => ChordKey::F(3),
        NamedKey::F4 => ChordKey::F(4),
        NamedKey::F5 => ChordKey::F(5),
        NamedKey::F6 => ChordKey::F(6),
        NamedKey::F7 => ChordKey::F(7),
        NamedKey::F8 => ChordKey::F(8),
        NamedKey::F9 => ChordKey::F(9),
        NamedKey::F10 => ChordKey::F(10),
        NamedKey::F11 => ChordKey::F(11),
        NamedKey::F12 => ChordKey::F(12),
        NamedKey::F13 => ChordKey::F(13),
        NamedKey::F14 => ChordKey::F(14),
        NamedKey::F15 => ChordKey::F(15),
        NamedKey::F16 => ChordKey::F(16),
        NamedKey::F17 => ChordKey::F(17),
        NamedKey::F18 => ChordKey::F(18),
        NamedKey::F19 => ChordKey::F(19),
        NamedKey::F20 => ChordKey::F(20),
        NamedKey::F21 => ChordKey::F(21),
        NamedKey::F22 => ChordKey::F(22),
        NamedKey::F23 => ChordKey::F(23),
        NamedKey::F24 => ChordKey::F(24),
        _ => return None,
    })
}

/// The action bound to `key` with `mods`, if any. The logical key is
/// already the shifted character, so when Shift produced a symbol or
/// digit (`+` is Shift+`=` on US layouts, digits need Shift on AZERTY)
/// the chord without Shift is tried too; letters keep Shift, so
/// Ctrl+Shift+T never falls back to Ctrl+T.
pub fn resolve(bindings: &Bindings, key: &Key, mods: ModifiersState) -> Option<Action> {
    let chord = to_chord(key, mods)?;
    if let Some(action) = bindings.action(&chord) {
        return Some(action);
    }
    match chord.key {
        ChordKey::Char(c) if chord.mods.shift && !c.is_uppercase() && !c.is_lowercase() => {
            let mods = Mods {
                shift: false,
                ..chord.mods
            };
            bindings.action(&Chord::new(mods, chord.key))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nxg_config::KeybindingsConfig;
    use nxg_config::keybindings::ChordKey;
    use winit::keyboard::NamedKey;

    const CTRL: ModifiersState = ModifiersState::CONTROL;
    const SHIFT: ModifiersState = ModifiersState::SHIFT;
    const ALT: ModifiersState = ModifiersState::ALT;
    const CMD: ModifiersState = ModifiersState::SUPER;

    fn ch(c: &str) -> Key {
        Key::Character(c.into())
    }

    fn named(key: NamedKey) -> Key {
        Key::Named(key)
    }

    fn linux() -> Bindings {
        Bindings::defaults(false)
    }

    #[test]
    fn chords_from_key_events() {
        let chord = |key: &Key, mods| to_chord(key, mods).unwrap();
        let c = chord(&ch("T"), CTRL | SHIFT);
        assert_eq!(c, "ctrl+shift+t".parse().unwrap(), "letters are lowercased");
        assert_eq!(chord(&named(NamedKey::Tab), CTRL).key, ChordKey::Tab);
        assert_eq!(chord(&named(NamedKey::F5), ALT).key, ChordKey::F(5));
        assert_eq!(chord(&named(NamedKey::Space), CTRL).key, ChordKey::Space);
        assert_eq!(chord(&ch(" "), CTRL).key, ChordKey::Space);
        assert_eq!(chord(&named(NamedKey::PageUp), SHIFT).key, ChordKey::PageUp);
        assert_eq!(chord(&named(NamedKey::ArrowLeft), CTRL).key, ChordKey::Left);
        assert!(to_chord(&named(NamedKey::Shift), SHIFT).is_none());
        assert!(to_chord(&ch("ab"), CTRL).is_none(), "composed text");
    }

    #[test]
    fn ctrl_zoom_off_macos() {
        let b = linux();
        assert_eq!(resolve(&b, &ch("="), CTRL), Some(Action::ZoomIn));
        assert_eq!(resolve(&b, &ch("+"), CTRL), Some(Action::ZoomIn));
        assert_eq!(resolve(&b, &ch("-"), CTRL), Some(Action::ZoomOut));
        assert_eq!(resolve(&b, &ch("0"), CTRL), Some(Action::ResetZoom));
    }

    #[test]
    fn shift_that_makes_a_symbol_is_ignored() {
        // `+` is Shift+`=` on US layouts; digits need Shift on AZERTY.
        let b = linux();
        assert_eq!(resolve(&b, &ch("+"), CTRL | SHIFT), Some(Action::ZoomIn));
        assert_eq!(resolve(&b, &ch("0"), CTRL | SHIFT), Some(Action::ResetZoom));
        // Letters keep Shift: Ctrl+Shift+T is not Ctrl+T.
        let config = "ctrl+t".parse().map(|c| KeybindingsConfig {
            entries: vec![(c, Some(Action::ReloadConfig))],
        });
        let b = Bindings::new(&config.unwrap(), false);
        assert_eq!(resolve(&b, &ch("T"), CTRL | SHIFT), Some(Action::NewTab));
        assert_eq!(resolve(&b, &ch("t"), CTRL), Some(Action::ReloadConfig));
    }

    #[test]
    fn cmd_zoom_on_macos() {
        let b = Bindings::defaults(true);
        assert_eq!(resolve(&b, &ch("="), CMD), Some(Action::ZoomIn));
        assert_eq!(resolve(&b, &ch("-"), CMD), Some(Action::ZoomOut));
        assert_eq!(resolve(&b, &ch("0"), CMD), Some(Action::ResetZoom));
        assert_eq!(resolve(&b, &ch("="), CTRL), None, "Ctrl is not primary");
        assert_eq!(resolve(&linux(), &ch("="), CMD), None, "Cmd only on macOS");
    }

    #[test]
    fn extra_modifiers_do_not_match() {
        let b = linux();
        assert_eq!(resolve(&b, &ch("="), ModifiersState::empty()), None);
        assert_eq!(resolve(&b, &ch("="), CTRL | ALT), None);
        assert_eq!(resolve(&b, &ch("="), CTRL | CMD), None);
        assert_eq!(resolve(&b, &ch("c"), CTRL), None);
        assert_eq!(resolve(&b, &named(NamedKey::Enter), CTRL), None);
    }

    #[test]
    fn tab_scroll_and_palette_defaults() {
        let b = linux();
        assert_eq!(resolve(&b, &ch("T"), CTRL | SHIFT), Some(Action::NewTab));
        assert_eq!(resolve(&b, &ch("W"), CTRL | SHIFT), Some(Action::CloseTab));
        assert_eq!(
            resolve(&b, &ch("P"), CTRL | SHIFT),
            Some(Action::CommandPalette)
        );
        let tab = named(NamedKey::Tab);
        assert_eq!(resolve(&b, &tab, CTRL), Some(Action::NextTab));
        assert_eq!(resolve(&b, &tab, CTRL | SHIFT), Some(Action::PreviousTab));
        assert_eq!(resolve(&b, &ch("3"), ALT), Some(Action::GotoTab(3)));
        let page_up = named(NamedKey::PageUp);
        assert_eq!(resolve(&b, &page_up, SHIFT), Some(Action::ScrollPageUp));
        assert_eq!(resolve(&b, &page_up, ModifiersState::empty()), None);
        let end = named(NamedKey::End);
        assert_eq!(resolve(&b, &end, SHIFT), Some(Action::ScrollToBottom));
    }
}
