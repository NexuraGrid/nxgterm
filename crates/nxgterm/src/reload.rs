//! What a config reload changes in a running terminal.

use nxg_config::Config;

/// The effect of replacing one config with another.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    /// Font family or fallback families changed: reload the font files.
    pub font_faces: bool,
    /// Configured font size changed: drop any zoom and use the new size.
    pub font_size: bool,
    /// Font, colors, padding, the tab bar mode or the background opacity
    /// changed: give the renderer a new style and recompute the grid size.
    pub restyle: bool,
    /// `window.blur` changed: ask the system again.
    pub blur: bool,
    /// The scrollback limit changed: apply it to the terminal.
    pub scrollback: bool,
    /// `[keybindings]` changed: rebuild the bindings.
    pub keybindings: bool,
    /// Sections that changed but only apply on the next start.
    pub on_restart: Vec<&'static str>,
}

/// Compares `old` with `new`.
pub fn diff(old: &Config, new: &Config) -> Changes {
    let font_faces = old.font.family != new.font.family || old.font.fallback != new.font.fallback;
    let font_size = old.font.size != new.font.size;
    let restyle = old.font != new.font
        || old.colors != new.colors
        || old.window.padding != new.window.padding
        || old.window.tab_bar != new.window.tab_bar
        || old.window.opacity != new.window.opacity;
    let blur = old.window.blur != new.window.blur;
    let scrollback = old.scrollback != new.scrollback;
    let keybindings = old.keybindings != new.keybindings;
    let mut on_restart = Vec::new();
    if old.shell != new.shell {
        on_restart.push("shell");
    }
    if old.renderer != new.renderer {
        on_restart.push("renderer");
    }
    if (old.window.columns, old.window.rows) != (new.window.columns, new.window.rows) {
        on_restart.push("window size");
    }
    if old.window.decorations != new.window.decorations {
        on_restart.push("window decorations");
    }
    Changes {
        font_faces,
        font_size,
        restyle,
        blur,
        scrollback,
        keybindings,
        on_restart,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nxg_config::Backend;
    use std::num::NonZeroU16;

    fn with(edit: impl FnOnce(&mut Config)) -> Changes {
        let mut new = Config::default();
        edit(&mut new);
        diff(&Config::default(), &new)
    }

    #[test]
    fn identical_configs_change_nothing() {
        assert_eq!(
            diff(&Config::default(), &Config::default()),
            Changes::default()
        );
    }

    #[test]
    fn colors_and_padding_restyle_only() {
        let expected = Changes {
            restyle: true,
            ..Changes::default()
        };
        assert_eq!(
            with(|c| c.colors.theme = "dracula".parse().unwrap()),
            expected
        );
        assert_eq!(with(|c| c.window.padding = 9), expected);
        assert_eq!(
            with(|c| c.window.tab_bar = nxg_config::TabBar::Never),
            expected
        );
        assert_eq!(with(|c| c.window.opacity = 0.8), expected);
    }

    #[test]
    fn blur_applies_live_without_a_restyle() {
        let changes = with(|c| c.window.blur = true);
        assert!(changes.blur && !changes.restyle);
        assert!(changes.on_restart.is_empty());
    }

    #[test]
    fn font_changes_restyle_and_say_what_to_reload() {
        let family = with(|c| c.font.family = Some("Iosevka".into()));
        assert!(family.restyle && family.font_faces && !family.font_size);
        let size = with(|c| c.font.size = 20.0);
        assert!(size.restyle && size.font_size && !size.font_faces);
        let fallback = with(|c| c.font.fallback = vec!["Symbols Nerd Font Mono".into()]);
        assert!(fallback.restyle && fallback.font_faces && !fallback.font_size);
    }

    #[test]
    fn shell_renderer_and_initial_size_apply_on_restart() {
        let changes = with(|c| {
            c.shell.program = Some("zsh".into());
            c.renderer.backend = Backend::Cpu;
            c.window.columns = NonZeroU16::new(80).unwrap();
            c.window.decorations = nxg_config::Decorations::Native;
        });
        assert!(!changes.restyle);
        assert_eq!(
            changes.on_restart,
            ["shell", "renderer", "window size", "window decorations"]
        );
    }

    #[test]
    fn key_bindings_apply_live() {
        let chord = "ctrl+alt+n".parse().unwrap();
        let changes = with(|c| {
            c.keybindings.entries = vec![(chord, Some(nxg_config::keybindings::Action::NewTab))];
        });
        assert!(changes.keybindings && !changes.restyle);
        assert!(changes.on_restart.is_empty());
    }

    #[test]
    fn the_scrollback_limit_applies_live() {
        let changes = with(|c| c.scrollback.lines = 5);
        assert!(changes.scrollback && !changes.restyle);
        assert!(changes.on_restart.is_empty());
    }
}
