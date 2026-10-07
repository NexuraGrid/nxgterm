//! Key bindings: the actions the terminal handles itself, the chords that
//! trigger them, the built-in defaults and the `[keybindings]` overrides.
//!
//! Pure: the app translates its window system's key events into a
//! [`Chord`] and looks it up in [`Bindings`].

use std::fmt;
use std::str::FromStr;

use serde::de::{self, Deserialize, Deserializer, MapAccess, Visitor};

/// Groups actions for listing (e.g. in a command palette).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Font,
    Scrollback,
    Clipboard,
    Tabs,
    General,
}

impl Category {
    pub fn title(self) -> &'static str {
        match self {
            Self::Font => "Font",
            Self::Scrollback => "Scrollback",
            Self::Clipboard => "Clipboard",
            Self::Tabs => "Tabs",
            Self::General => "General",
        }
    }
}

/// Something the terminal does itself instead of sending the key to the
/// shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    ZoomIn,
    ZoomOut,
    ResetZoom,
    ScrollPageUp,
    ScrollPageDown,
    ScrollToTop,
    ScrollToBottom,
    Copy,
    Paste,
    SelectAll,
    NewTab,
    CloseTab,
    NextTab,
    PreviousTab,
    /// Tab number 1-9, named `goto_tab_1`..`goto_tab_9`.
    GotoTab(u8),
    CommandPalette,
    ReloadConfig,
}

/// Name, title and category of every action, in listing order.
const ACTIONS: [(Action, &str, &str, Category); 25] = [
    (Action::ZoomIn, "zoom_in", "Zoom In", Category::Font),
    (Action::ZoomOut, "zoom_out", "Zoom Out", Category::Font),
    (
        Action::ResetZoom,
        "reset_zoom",
        "Reset Zoom",
        Category::Font,
    ),
    (
        Action::ScrollPageUp,
        "scroll_page_up",
        "Scroll Up One Page",
        Category::Scrollback,
    ),
    (
        Action::ScrollPageDown,
        "scroll_page_down",
        "Scroll Down One Page",
        Category::Scrollback,
    ),
    (
        Action::ScrollToTop,
        "scroll_to_top",
        "Scroll to Top",
        Category::Scrollback,
    ),
    (
        Action::ScrollToBottom,
        "scroll_to_bottom",
        "Scroll to Bottom",
        Category::Scrollback,
    ),
    (Action::Copy, "copy", "Copy", Category::Clipboard),
    (Action::Paste, "paste", "Paste", Category::Clipboard),
    (
        Action::SelectAll,
        "select_all",
        "Select All",
        Category::Clipboard,
    ),
    (Action::NewTab, "new_tab", "New Tab", Category::Tabs),
    (Action::CloseTab, "close_tab", "Close Tab", Category::Tabs),
    (Action::NextTab, "next_tab", "Next Tab", Category::Tabs),
    (
        Action::PreviousTab,
        "previous_tab",
        "Previous Tab",
        Category::Tabs,
    ),
    (
        Action::GotoTab(1),
        "goto_tab_1",
        "Go to Tab 1",
        Category::Tabs,
    ),
    (
        Action::GotoTab(2),
        "goto_tab_2",
        "Go to Tab 2",
        Category::Tabs,
    ),
    (
        Action::GotoTab(3),
        "goto_tab_3",
        "Go to Tab 3",
        Category::Tabs,
    ),
    (
        Action::GotoTab(4),
        "goto_tab_4",
        "Go to Tab 4",
        Category::Tabs,
    ),
    (
        Action::GotoTab(5),
        "goto_tab_5",
        "Go to Tab 5",
        Category::Tabs,
    ),
    (
        Action::GotoTab(6),
        "goto_tab_6",
        "Go to Tab 6",
        Category::Tabs,
    ),
    (
        Action::GotoTab(7),
        "goto_tab_7",
        "Go to Tab 7",
        Category::Tabs,
    ),
    (
        Action::GotoTab(8),
        "goto_tab_8",
        "Go to Tab 8",
        Category::Tabs,
    ),
    (
        Action::GotoTab(9),
        "goto_tab_9",
        "Go to Tab 9",
        Category::Tabs,
    ),
    (
        Action::CommandPalette,
        "command_palette",
        "Command Palette",
        Category::General,
    ),
    (
        Action::ReloadConfig,
        "reload_config",
        "Reload Config",
        Category::General,
    ),
];

impl Action {
    /// Every action, in listing order.
    pub const ALL: [Action; ACTIONS.len()] = {
        let mut all = [Action::ZoomIn; ACTIONS.len()];
        let mut i = 0;
        while i < ACTIONS.len() {
            all[i] = ACTIONS[i].0;
            i += 1;
        }
        all
    };

    fn row(self) -> &'static (Action, &'static str, &'static str, Category) {
        ACTIONS
            .iter()
            .find(|row| row.0 == self)
            .expect("every action has a row (`GotoTab` takes 1-9)")
    }

    /// The stable snake_case name used in the config file.
    pub fn name(self) -> &'static str {
        self.row().1
    }

    /// A human-readable title.
    pub fn title(self) -> &'static str {
        self.row().2
    }

    pub fn category(self) -> Category {
        self.row().3
    }

    /// The action called `name` (case-insensitive).
    pub fn from_name(name: &str) -> Option<Self> {
        ACTIONS
            .iter()
            .find(|row| row.1.eq_ignore_ascii_case(name))
            .map(|row| row.0)
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Modifiers held for a chord.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// Super, the Windows key; Cmd on macOS.
    pub super_key: bool,
}

/// The non-modifier key of a chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChordKey {
    /// A character key, lowercased (`t`, `1`, `=`, `+`); never a space.
    Char(char),
    /// F1-F24.
    F(u8),
    Tab,
    Enter,
    Escape,
    Space,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
}

/// Named keys: config name and label.
const NAMED_KEYS: [(ChordKey, &str, &str); 15] = [
    (ChordKey::Tab, "tab", "Tab"),
    (ChordKey::Enter, "enter", "Enter"),
    (ChordKey::Escape, "escape", "Escape"),
    (ChordKey::Space, "space", "Space"),
    (ChordKey::Backspace, "backspace", "Backspace"),
    (ChordKey::Delete, "delete", "Delete"),
    (ChordKey::Insert, "insert", "Insert"),
    (ChordKey::Home, "home", "Home"),
    (ChordKey::End, "end", "End"),
    (ChordKey::PageUp, "pageup", "PageUp"),
    (ChordKey::PageDown, "pagedown", "PageDown"),
    (ChordKey::Up, "up", "Up"),
    (ChordKey::Down, "down", "Down"),
    (ChordKey::Left, "left", "Left"),
    (ChordKey::Right, "right", "Right"),
];

/// Characters with a name, since `+` separates modifiers.
const NAMED_CHARS: [(char, &str); 3] = [('+', "plus"), ('-', "minus"), ('=', "equal")];

impl ChordKey {
    /// `name` is already lowercase.
    fn parse(name: &str) -> Option<Self> {
        if let Some(&(key, _, _)) = NAMED_KEYS.iter().find(|k| k.1 == name) {
            return Some(key);
        }
        if let Some(&(c, _)) = NAMED_CHARS.iter().find(|c| c.1 == name) {
            return Some(Self::Char(c));
        }
        if let Some(n) = name.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
            return (1..=24).contains(&n).then_some(Self::F(n));
        }
        let mut chars = name.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) if !c.is_whitespace() => Some(Self::Char(c)),
            _ => None,
        }
    }

    fn config_name(self) -> String {
        match self {
            Self::Char(c) => match NAMED_CHARS.iter().find(|n| n.0 == c) {
                Some((_, name)) => (*name).to_owned(),
                None => c.to_string(),
            },
            Self::F(n) => format!("f{n}"),
            named => named_key(named).1.to_owned(),
        }
    }

    fn label(self) -> String {
        match self {
            Self::Char('+') => "Plus".to_owned(),
            Self::Char(c) => c.to_uppercase().collect(),
            Self::F(n) => format!("F{n}"),
            named => named_key(named).2.to_owned(),
        }
    }
}

fn named_key(key: ChordKey) -> &'static (ChordKey, &'static str, &'static str) {
    NAMED_KEYS
        .iter()
        .find(|k| k.0 == key)
        .expect("named keys are listed")
}

/// Modifiers plus one key, e.g. `ctrl+shift+t`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    pub mods: Mods,
    pub key: ChordKey,
}

impl Chord {
    pub fn new(mods: Mods, key: ChordKey) -> Self {
        Self { mods, key }
    }

    /// A label for menus, e.g. `Ctrl+Shift+T`; `macos` names Super `Cmd`.
    pub fn label(&self, macos: bool) -> String {
        let super_name = if macos { "Cmd" } else { "Super" };
        let mut parts = self.mod_names(["Ctrl", "Alt", "Shift", super_name]);
        parts.push(self.key.label());
        parts.join("+")
    }

    /// The names of the held modifiers, in the order of `names`
    /// (ctrl, alt, shift, super).
    fn mod_names(&self, names: [&str; 4]) -> Vec<String> {
        let Mods {
            ctrl,
            alt,
            shift,
            super_key,
        } = self.mods;
        [ctrl, alt, shift, super_key]
            .into_iter()
            .zip(names)
            .filter(|(held, _)| *held)
            .map(|(_, name)| name.to_owned())
            .collect()
    }
}

impl FromStr for Chord {
    type Err = String;

    /// Modifiers (`ctrl`/`control`, `alt`/`option`, `shift`,
    /// `super`/`cmd`) and one key, joined with `+` in any order and case.
    /// The key may itself be `+` when it comes last (`ctrl++`).
    fn from_str(text: &str) -> Result<Self, String> {
        let text = text.trim().to_lowercase();
        if text.is_empty() {
            return Err("empty key binding".into());
        }
        let (rest, mut key) = match text.strip_suffix("++") {
            Some(rest) => (rest, Some(ChordKey::Char('+'))),
            None if text == "+" => ("", Some(ChordKey::Char('+'))),
            None => (text.as_str(), None),
        };
        let mut mods = Mods::default();
        let names: Vec<&str> = rest.split('+').map(str::trim).collect();
        for (i, &name) in names.iter().enumerate() {
            let last = i + 1 == names.len();
            if name.is_empty() {
                if rest.is_empty() {
                    break;
                }
                return Err("no key after the modifiers".into());
            }
            let held = match name {
                "ctrl" | "control" => Some(&mut mods.ctrl),
                "alt" | "option" => Some(&mut mods.alt),
                "shift" => Some(&mut mods.shift),
                "super" | "cmd" => Some(&mut mods.super_key),
                _ => None,
            };
            match held {
                Some(held) if *held => return Err(format!("modifier `{name}` twice")),
                Some(held) => *held = true,
                None => match (ChordKey::parse(name), key) {
                    (Some(parsed), None) => key = Some(parsed),
                    (Some(_), Some(_)) => return Err(format!("more than one key (`{name}`)")),
                    (None, _) if last => return Err(unknown_key(name)),
                    (None, _) => return Err(format!("unknown modifier `{name}`")),
                },
            }
        }
        let key = key.ok_or_else(|| unknown_key(names.last().copied().unwrap_or_default()))?;
        Ok(Self { mods, key })
    }
}

fn unknown_key(name: &str) -> String {
    format!(
        "unknown key `{name}` (expected a character, f1-f24, plus, minus, equal, {})",
        NAMED_KEYS.map(|k| k.1).join(", ")
    )
}

/// The config form, e.g. `ctrl+shift+t`, which parses back to the chord.
impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = self.mod_names(["ctrl", "alt", "shift", "super"]);
        parts.push(self.key.config_name());
        f.write_str(&parts.join("+"))
    }
}

/// `[keybindings]`: chords mapped to an action, or to `none` to remove a
/// default binding, in file order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeybindingsConfig {
    pub entries: Vec<(Chord, Option<Action>)>,
}

impl<'de> Deserialize<'de> for KeybindingsConfig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(EntriesVisitor)
    }
}

struct EntriesVisitor;

impl<'de> Visitor<'de> for EntriesVisitor {
    type Value = KeybindingsConfig;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a table of \"chord\" = \"action\"")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut entries: Vec<(Chord, Option<Action>)> = Vec::new();
        while let Some((text, name)) = map.next_entry::<String, String>()? {
            let chord: Chord = text
                .parse()
                .map_err(|e| de::Error::custom(format!("invalid key binding `{text}`: {e}")))?;
            if entries.iter().any(|(c, _)| *c == chord) {
                return Err(de::Error::custom(format!("`{chord}` is bound twice")));
            }
            let action = if name.trim().eq_ignore_ascii_case("none") {
                None
            } else {
                let action = Action::from_name(name.trim()).ok_or_else(|| {
                    let names: Vec<&str> = Action::ALL.iter().map(|a| a.name()).collect();
                    de::Error::custom(format!(
                        "unknown action `{name}` for `{text}`; available: none, {}",
                        names.join(", ")
                    ))
                })?;
                Some(action)
            };
            entries.push((chord, action));
        }
        Ok(KeybindingsConfig { entries })
    }
}

/// The built-in bindings as config text; `PRIMARY` is Ctrl, or Cmd on
/// macOS, and `CLIPBOARD` is Ctrl+Shift, or Cmd on macOS (Ctrl+C and
/// Ctrl+V belong to the shell elsewhere).
const DEFAULTS: [(&str, Action); 25] = [
    ("PRIMARY+equal", Action::ZoomIn),
    ("PRIMARY+plus", Action::ZoomIn),
    ("PRIMARY+minus", Action::ZoomOut),
    ("PRIMARY+0", Action::ResetZoom),
    ("shift+pageup", Action::ScrollPageUp),
    ("shift+pagedown", Action::ScrollPageDown),
    ("shift+home", Action::ScrollToTop),
    ("shift+end", Action::ScrollToBottom),
    ("ctrl+shift+t", Action::NewTab),
    ("ctrl+shift+w", Action::CloseTab),
    ("ctrl+tab", Action::NextTab),
    ("ctrl+shift+tab", Action::PreviousTab),
    ("alt+1", Action::GotoTab(1)),
    ("alt+2", Action::GotoTab(2)),
    ("alt+3", Action::GotoTab(3)),
    ("alt+4", Action::GotoTab(4)),
    ("alt+5", Action::GotoTab(5)),
    ("alt+6", Action::GotoTab(6)),
    ("alt+7", Action::GotoTab(7)),
    ("alt+8", Action::GotoTab(8)),
    ("alt+9", Action::GotoTab(9)),
    ("ctrl+shift+p", Action::CommandPalette),
    ("CLIPBOARD+c", Action::Copy),
    ("CLIPBOARD+v", Action::Paste),
    ("shift+insert", Action::Paste),
];

/// The effective bindings: the configured ones, then the defaults they
/// do not replace or remove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bindings {
    macos: bool,
    entries: Vec<(Chord, Action)>,
}

impl Bindings {
    /// The built-in bindings; `macos` uses Cmd instead of Ctrl for zoom.
    pub fn defaults(macos: bool) -> Self {
        let primary = if macos { "cmd" } else { "ctrl" };
        let clipboard = if macos { "cmd" } else { "ctrl+shift" };
        let entries = DEFAULTS
            .iter()
            .map(|&(text, action)| {
                let text = text
                    .replace("PRIMARY", primary)
                    .replace("CLIPBOARD", clipboard);
                let chord = text.parse().expect("default chords are valid");
                (chord, action)
            })
            .collect();
        Self { macos, entries }
    }

    /// The defaults with `config` merged over them.
    pub fn new(config: &KeybindingsConfig, macos: bool) -> Self {
        let defaults = Self::defaults(macos).entries;
        let configured = |chord: &Chord| config.entries.iter().any(|(c, _)| c == chord);
        let entries = config
            .entries
            .iter()
            .filter_map(|&(chord, action)| Some((chord, action?)))
            .chain(defaults.into_iter().filter(|(chord, _)| !configured(chord)))
            .collect();
        Self { macos, entries }
    }

    /// The action bound to `chord`, if any.
    pub fn action(&self, chord: &Chord) -> Option<Action> {
        self.entries
            .iter()
            .find(|(c, _)| c == chord)
            .map(|&(_, action)| action)
    }

    /// Every binding, configured ones first.
    pub fn entries(&self) -> &[(Chord, Action)] {
        &self.entries
    }

    /// Every action in [`Action::ALL`] order with the label of its first
    /// chord (`None` when unbound), for listing shortcuts.
    pub fn shortcuts(&self) -> Vec<(Action, Option<String>)> {
        Action::ALL
            .iter()
            .map(|&action| {
                let chord = self.entries.iter().find(|(_, a)| *a == action);
                (action, chord.map(|(c, _)| c.label(self.macos)))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(text: &str) -> Chord {
        text.parse().unwrap_or_else(|e| panic!("{text}: {e}"))
    }

    fn mods(ctrl: bool, alt: bool, shift: bool, super_key: bool) -> Mods {
        Mods {
            ctrl,
            alt,
            shift,
            super_key,
        }
    }

    fn ctrl() -> Mods {
        mods(true, false, false, false)
    }

    fn ctrl_shift() -> Mods {
        mods(true, false, true, false)
    }

    #[derive(Debug, serde::Deserialize)]
    struct Wrapper {
        keybindings: KeybindingsConfig,
    }

    fn config(text: &str) -> Result<KeybindingsConfig, String> {
        toml::from_str::<Wrapper>(&format!("[keybindings]\n{text}"))
            .map(|w| w.keybindings)
            .map_err(|e| e.to_string())
    }

    #[test]
    fn action_names_are_unique_and_round_trip() {
        let mut names: Vec<&str> = Action::ALL.iter().map(|a| a.name()).collect();
        for action in Action::ALL {
            assert_eq!(Action::from_name(action.name()), Some(action));
            assert!(!action.title().is_empty());
        }
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Action::ALL.len());
    }

    #[test]
    fn action_names_are_snake_case() {
        assert_eq!(Action::ZoomIn.name(), "zoom_in");
        assert_eq!(Action::ScrollPageUp.name(), "scroll_page_up");
        assert_eq!(Action::ScrollToBottom.name(), "scroll_to_bottom");
        assert_eq!(Action::PreviousTab.name(), "previous_tab");
        assert_eq!(Action::GotoTab(3).name(), "goto_tab_3");
        assert_eq!(Action::CommandPalette.name(), "command_palette");
        assert_eq!(Action::from_name("New_Tab"), Some(Action::NewTab));
        assert_eq!(Action::from_name("goto_tab_0"), None);
        assert_eq!(Action::from_name("goto_tab_10"), None);
        assert_eq!(Action::from_name("copy"), Some(Action::Copy));
        assert_eq!(Action::Paste.name(), "paste");
        assert_eq!(Action::SelectAll.name(), "select_all");
        assert_eq!(Action::Copy.category(), Category::Clipboard);
        assert_eq!(Action::from_name("cut"), None);
    }

    #[test]
    fn actions_have_titles_and_categories() {
        assert_eq!(Action::NewTab.title(), "New Tab");
        assert_eq!(Action::GotoTab(4).title(), "Go to Tab 4");
        assert_eq!(Action::ZoomIn.category(), Category::Font);
        assert_eq!(Action::ScrollToTop.category(), Category::Scrollback);
        assert_eq!(Action::GotoTab(9).category(), Category::Tabs);
        assert_eq!(Action::CommandPalette.category(), Category::General);
        assert_eq!(Category::Tabs.title(), "Tabs");
    }

    #[test]
    fn chords_parse_modifiers_in_any_order_and_case() {
        let expected = Chord::new(ctrl_shift(), ChordKey::Char('t'));
        assert_eq!(chord("ctrl+shift+t"), expected);
        assert_eq!(chord("Shift+Ctrl+T"), expected);
        assert_eq!(chord(" control + shift + t "), expected);
        let all = chord("super+alt+shift+ctrl+x");
        assert_eq!(all.mods, mods(true, true, true, true));
        assert_eq!(chord("cmd+n").mods, mods(false, false, false, true));
        assert_eq!(chord("t+ctrl+shift"), expected, "the key may come first");
        assert_eq!(chord("ctrl+0").mods, ctrl());
        assert_eq!(chord("q"), Chord::new(Mods::default(), ChordKey::Char('q')));
    }

    #[test]
    fn chords_parse_named_keys() {
        let key = |text: &str| chord(text).key;
        assert_eq!(key("ctrl+plus"), ChordKey::Char('+'));
        assert_eq!(key("ctrl++"), ChordKey::Char('+'));
        assert_eq!(key("ctrl+minus"), ChordKey::Char('-'));
        assert_eq!(key("ctrl+-"), ChordKey::Char('-'));
        assert_eq!(key("ctrl+equal"), ChordKey::Char('='));
        assert_eq!(key("ctrl+="), ChordKey::Char('='));
        assert_eq!(key("alt+1"), ChordKey::Char('1'));
        assert_eq!(key("f1"), ChordKey::F(1));
        assert_eq!(key("F24"), ChordKey::F(24));
        assert_eq!(key("ctrl+tab"), ChordKey::Tab);
        assert_eq!(key("enter"), ChordKey::Enter);
        assert_eq!(key("escape"), ChordKey::Escape);
        assert_eq!(key("space"), ChordKey::Space);
        assert_eq!(key("backspace"), ChordKey::Backspace);
        assert_eq!(key("delete"), ChordKey::Delete);
        assert_eq!(key("insert"), ChordKey::Insert);
        assert_eq!(key("shift+home"), ChordKey::Home);
        assert_eq!(key("shift+end"), ChordKey::End);
        assert_eq!(key("shift+pageup"), ChordKey::PageUp);
        assert_eq!(key("shift+PageDown"), ChordKey::PageDown);
        assert_eq!(key("up"), ChordKey::Up);
        assert_eq!(key("down"), ChordKey::Down);
        assert_eq!(key("left"), ChordKey::Left);
        assert_eq!(key("right"), ChordKey::Right);
    }

    #[test]
    fn invalid_chords_say_why() {
        let error = |text: &str| text.parse::<Chord>().unwrap_err();
        assert!(error("").contains("empty"), "{}", error(""));
        assert!(error("ctrl+").contains("no key"), "{}", error("ctrl+"));
        assert!(error("ctrl+a+b").contains("more than one key"));
        assert!(error("ctrl+foo").contains("unknown key `foo`"));
        assert!(error("hyper+t").contains("unknown modifier `hyper`"));
        assert!(error("ctrl+ctrl+t").contains("`ctrl` twice"));
        assert!(error("f0").contains("unknown key `f0`"));
        assert!(error("f25").contains("unknown key `f25`"));
        assert!(error("ctrl+shift").contains("unknown key `shift`"));
    }

    #[test]
    fn chords_display_in_config_form_and_round_trip() {
        for text in [
            "ctrl+shift+t",
            "ctrl+plus",
            "ctrl+minus",
            "ctrl+equal",
            "alt+1",
            "shift+pageup",
            "ctrl+alt+shift+super+f12",
            "space",
        ] {
            assert_eq!(chord(text).to_string(), text);
            assert_eq!(chord(&chord(text).to_string()), chord(text));
        }
    }

    #[test]
    fn chord_labels_are_title_case() {
        assert_eq!(chord("shift+ctrl+t").label(false), "Ctrl+Shift+T");
        assert_eq!(chord("ctrl+plus").label(false), "Ctrl+Plus");
        assert_eq!(chord("ctrl+minus").label(false), "Ctrl+-");
        assert_eq!(chord("ctrl+equal").label(false), "Ctrl+=");
        assert_eq!(chord("shift+pageup").label(false), "Shift+PageUp");
        assert_eq!(chord("ctrl+shift+tab").label(false), "Ctrl+Shift+Tab");
        assert_eq!(chord("super+f5").label(false), "Super+F5");
        assert_eq!(chord("super+equal").label(true), "Cmd+=");
    }

    #[test]
    fn defaults_cover_the_documented_shortcuts() {
        let b = Bindings::defaults(false);
        let on = |text: &str| b.action(&chord(text));
        assert_eq!(on("ctrl+equal"), Some(Action::ZoomIn));
        assert_eq!(on("ctrl+plus"), Some(Action::ZoomIn));
        assert_eq!(on("ctrl+minus"), Some(Action::ZoomOut));
        assert_eq!(on("ctrl+0"), Some(Action::ResetZoom));
        assert_eq!(on("shift+pageup"), Some(Action::ScrollPageUp));
        assert_eq!(on("shift+pagedown"), Some(Action::ScrollPageDown));
        assert_eq!(on("shift+home"), Some(Action::ScrollToTop));
        assert_eq!(on("shift+end"), Some(Action::ScrollToBottom));
        assert_eq!(on("ctrl+shift+t"), Some(Action::NewTab));
        assert_eq!(on("ctrl+shift+w"), Some(Action::CloseTab));
        assert_eq!(on("ctrl+tab"), Some(Action::NextTab));
        assert_eq!(on("ctrl+shift+tab"), Some(Action::PreviousTab));
        for n in 1..=9 {
            assert_eq!(on(&format!("alt+{n}")), Some(Action::GotoTab(n)));
        }
        assert_eq!(on("ctrl+shift+p"), Some(Action::CommandPalette));
        assert_eq!(on("ctrl+shift+c"), Some(Action::Copy));
        assert_eq!(on("ctrl+shift+v"), Some(Action::Paste));
        assert_eq!(on("shift+insert"), Some(Action::Paste));
        assert_eq!(on("ctrl+c"), None, "ctrl+c stays an interrupt");
        assert_eq!(on("ctrl+v"), None);
        assert_eq!(on("ctrl+t"), None);
        assert_eq!(on("ctrl+alt+equal"), None);
    }

    #[test]
    fn macos_zooms_with_cmd() {
        let b = Bindings::defaults(true);
        assert_eq!(b.action(&chord("cmd+equal")), Some(Action::ZoomIn));
        assert_eq!(b.action(&chord("cmd+plus")), Some(Action::ZoomIn));
        assert_eq!(b.action(&chord("cmd+minus")), Some(Action::ZoomOut));
        assert_eq!(b.action(&chord("cmd+0")), Some(Action::ResetZoom));
        assert_eq!(b.action(&chord("ctrl+equal")), None);
        assert_eq!(b.action(&chord("ctrl+shift+t")), Some(Action::NewTab));
        assert_eq!(b.action(&chord("cmd+c")), Some(Action::Copy));
        assert_eq!(b.action(&chord("cmd+v")), Some(Action::Paste));
        assert_eq!(b.action(&chord("shift+insert")), Some(Action::Paste));
        assert_eq!(b.action(&chord("ctrl+shift+c")), None);
    }

    #[test]
    fn config_entries_parse_in_order() {
        let parsed = config("\"ctrl+alt+n\" = \"new_tab\"\n\"ctrl+shift+w\" = \"none\"\n").unwrap();
        assert_eq!(
            parsed.entries,
            [
                (chord("ctrl+alt+n"), Some(Action::NewTab)),
                (chord("ctrl+shift+w"), None),
            ]
        );
        assert_eq!(config("").unwrap(), KeybindingsConfig::default());
    }

    #[test]
    fn config_errors_name_the_chord_or_action() {
        let error = config("\"ctrl+foo\" = \"new_tab\"").unwrap_err();
        assert!(error.contains("invalid key binding `ctrl+foo`"), "{error}");
        assert!(error.contains("unknown key `foo`"), "{error}");
        let error = config("\"ctrl+t\" = \"cut\"").unwrap_err();
        assert!(error.contains("unknown action `cut`"), "{error}");
        assert!(error.contains("zoom_in"), "lists the actions: {error}");
        let error = config("\"ctrl+t\" = \"new_tab\"\n\"T+Ctrl\" = \"close_tab\"").unwrap_err();
        assert!(error.contains("`ctrl+t` is bound twice"), "{error}");
        assert!(config("\"ctrl+t\" = 1").is_err());
    }

    #[test]
    fn config_merges_over_and_unbinds_defaults() {
        let parsed = config(
            "\"ctrl+alt+n\" = \"new_tab\"\n\"ctrl+shift+w\" = \"none\"\n\"ctrl+tab\" = \"command_palette\"\n",
        )
        .unwrap();
        let b = Bindings::new(&parsed, false);
        assert_eq!(b.action(&chord("ctrl+alt+n")), Some(Action::NewTab));
        assert_eq!(
            b.action(&chord("ctrl+shift+t")),
            Some(Action::NewTab),
            "default kept"
        );
        assert_eq!(b.action(&chord("ctrl+shift+w")), None, "unbound");
        assert_eq!(b.action(&chord("ctrl+tab")), Some(Action::CommandPalette));
        assert_eq!(b.action(&chord("ctrl+equal")), Some(Action::ZoomIn));
        assert_eq!(
            Bindings::new(&KeybindingsConfig::default(), true),
            Bindings::defaults(true)
        );
    }

    #[test]
    fn shortcuts_list_every_action_preferring_configured_chords() {
        let parsed = config("\"ctrl+alt+n\" = \"new_tab\"\n\"ctrl+shift+w\" = \"none\"\n").unwrap();
        let shortcuts = Bindings::new(&parsed, false).shortcuts();
        assert_eq!(shortcuts.len(), Action::ALL.len());
        assert_eq!(shortcuts[0], (Action::ZoomIn, Some("Ctrl+=".into())));
        let find = |action| {
            shortcuts
                .iter()
                .find(|(a, _)| *a == action)
                .unwrap()
                .1
                .clone()
        };
        assert_eq!(find(Action::NewTab).as_deref(), Some("Ctrl+Alt+N"));
        assert_eq!(find(Action::CloseTab), None);
        assert_eq!(find(Action::ReloadConfig), None, "unbound by default");
        assert_eq!(find(Action::GotoTab(2)).as_deref(), Some("Alt+2"));
        let mac = Bindings::defaults(true).shortcuts();
        assert_eq!(mac[2], (Action::ResetZoom, Some("Cmd+0".into())));
    }
}
