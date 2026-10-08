//! The tab bar: one row of labels above the grid, as pure functions of
//! the tab titles so layout, truncation and clicks are unit tested.
//!
//! The bar is drawn (see [`crate::title_bar::render`]) as a one-row
//! terminal that the renderers put above the grid, so it needs no drawing
//! code of its own.

use std::path::Path;

use nxg_render::Layout;

/// One tab's label, positioned on the bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    /// First column.
    pub start: u16,
    /// What is drawn, one column per char.
    pub text: String,
    pub active: bool,
}

impl Label {
    pub fn width(&self) -> u16 {
        self.text.chars().count() as u16
    }
}

/// Labels `" N: title "` for `titles` (N counts from 1), fitting `cols`
/// columns: when they do not all fit each gets an equal share, truncated
/// with `…`. With more tabs than columns the ones past the edge are left
/// out. Titles are assumed one column per char.
pub fn layout(titles: &[&str], active: usize, cols: u16) -> Vec<Label> {
    let full: Vec<String> = titles
        .iter()
        .enumerate()
        .map(|(i, title)| {
            let title: String = title.chars().filter(|c| !c.is_control()).collect();
            format!(" {}: {title} ", i + 1)
        })
        .collect();
    let total: usize = full.iter().map(|label| label.chars().count()).sum();
    let share = if total <= usize::from(cols) {
        usize::MAX
    } else {
        (usize::from(cols) / full.len().max(1)).max(1)
    };
    let mut start = 0u16;
    let mut labels = Vec::new();
    for (i, label) in full.iter().enumerate() {
        if start >= cols {
            break;
        }
        let label = Label {
            start,
            text: truncate(label, share),
            active: i == active,
        };
        start += label.width();
        labels.push(label);
    }
    labels
}

/// `text` cut to `width` chars, the last one replaced by `…` when cut.
pub(crate) fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    if width < 2 {
        return text.chars().take(width).collect();
    }
    let mut cut: String = text.chars().take(width - 1).collect();
    cut.push('…');
    cut
}

/// The tab whose label covers column `col`.
pub fn tab_at(labels: &[Label], col: u16) -> Option<usize> {
    labels
        .iter()
        .position(|label| (label.start..label.start + label.width()).contains(&col))
}

/// The bar column under the pointer at `x`, `y` window pixels, or `None`
/// when the pointer is below the bar. `layout` is the window's layout
/// without the bar (`top` 0); the padding above the bar counts as bar.
pub fn column_at(layout: Layout, cols: u16, x: f64, y: f64) -> Option<u16> {
    let bottom = f64::from(layout.padding + layout.top + layout.cell.height);
    if y >= bottom {
        return None;
    }
    let col = ((x - f64::from(layout.padding)) / f64::from(layout.cell.width.max(1))).floor();
    Some(col.clamp(0.0, f64::from(cols.max(1) - 1)) as u16)
}

/// The title of a tab running `program` (the configured shell), or the
/// default shell: the file name without directories or `.exe`. On Unix
/// the default shell is `$SHELL` (`shell_env`), falling back to `sh`; on
/// Windows it is resolved when spawning, so the title is just `shell`.
pub fn title(program: Option<&str>, shell_env: Option<&str>, windows: bool) -> String {
    let program = match program {
        Some(program) => program,
        None if windows => return "shell".into(),
        None => shell_env.filter(|shell| !shell.is_empty()).unwrap_or("sh"),
    };
    // Windows paths split on either separator wherever this runs.
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let name = Path::new(name)
        .file_stem()
        .filter(|_| name.to_ascii_lowercase().ends_with(".exe"))
        .and_then(|stem| stem.to_str())
        .unwrap_or(name);
    if name.is_empty() {
        program.to_owned()
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nxg_render::CellSize;

    fn texts(labels: &[Label]) -> Vec<&str> {
        labels.iter().map(|label| label.text.as_str()).collect()
    }

    #[test]
    fn labels_number_the_tabs_and_sit_side_by_side() {
        let labels = layout(&["zsh", "vim"], 1, 80);
        assert_eq!(texts(&labels), [" 1: zsh ", " 2: vim "]);
        assert_eq!(labels[0].start, 0);
        assert_eq!(labels[1].start, 8);
        assert_eq!(
            labels.iter().map(|l| l.active).collect::<Vec<_>>(),
            [false, true]
        );
    }

    #[test]
    fn labels_that_do_not_fit_share_the_width_and_are_truncated() {
        let labels = layout(&["bash", "longer-name", "zsh"], 0, 24);
        assert_eq!(texts(&labels), [" 1: bas…", " 2: lon…", " 3: zsh "]);
        assert_eq!(labels[2].start, 16);
        let width: usize = labels.iter().map(|l| l.text.chars().count()).sum();
        assert!(width <= 24);
    }

    #[test]
    fn tabs_past_a_tiny_bar_are_left_out() {
        let labels = layout(&["a", "b", "c", "d"], 0, 2);
        assert_eq!(texts(&labels), [" ", " "]);
        let labels = layout(&["a", "b"], 0, 5);
        assert_eq!(texts(&labels), [" …", " …"]);
    }

    #[test]
    fn control_characters_are_dropped_from_titles() {
        let labels = layout(&["a\x1b[31mb"], 0, 80);
        assert_eq!(texts(&labels), [" 1: a[31mb "]);
    }

    #[test]
    fn clicks_map_to_the_label_under_them() {
        let labels = layout(&["zsh", "vim"], 0, 80);
        assert_eq!(tab_at(&labels, 0), Some(0));
        assert_eq!(tab_at(&labels, 7), Some(0));
        assert_eq!(tab_at(&labels, 8), Some(1));
        assert_eq!(tab_at(&labels, 15), Some(1));
        assert_eq!(tab_at(&labels, 16), None, "past the last label");
    }

    #[test]
    fn only_the_top_row_and_its_padding_are_the_bar() {
        let layout = Layout {
            cell: CellSize {
                width: 10,
                height: 20,
            },
            padding: 5,
            left: 0,
            top: 0,
        };
        assert_eq!(column_at(layout, 8, 0.0, 0.0), Some(0));
        assert_eq!(column_at(layout, 8, 26.0, 24.9), Some(2));
        assert_eq!(column_at(layout, 8, 999.0, 10.0), Some(7));
        assert_eq!(column_at(layout, 8, 26.0, 25.0), None, "grid row 0");
    }

    #[test]
    fn titles_are_program_file_names() {
        assert_eq!(title(Some("/usr/bin/fish"), None, false), "fish");
        assert_eq!(title(Some("nu"), None, false), "nu");
        assert_eq!(
            title(
                Some("C:\\Program Files\\PowerShell\\7\\pwsh.exe"),
                None,
                true
            ),
            "pwsh"
        );
        assert_eq!(title(Some("cmd.EXE"), None, true), "cmd");
        assert_eq!(title(None, Some("/bin/zsh"), false), "zsh");
        assert_eq!(title(None, None, false), "sh");
        assert_eq!(title(None, Some(""), false), "sh");
        assert_eq!(title(None, Some("/bin/zsh"), true), "shell");
    }
}
