//! The command palette: a filterable list of every action with its
//! shortcut, opened over the grid with the `command_palette` binding.
//!
//! Pure: filtering, selection, layout, clicks and keys are functions of
//! the palette state, and it is drawn as a small [`Terminal`] that the
//! renderers put over the grid (see `nxg_render::Overlay`).

use std::ops::Range;

use nxg_core::TermSize;
use nxg_core::Terminal;
use nxg_render::palette::Rgb;
use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::bindings::Action;
use crate::tab_bar::truncate;

/// Title on the first row of the box.
const TITLE: &str = "Command Palette";
/// Widest and tallest the box gets, in cells.
const MAX_COLS: u16 = 72;
const MAX_ROWS: u16 = 20;
/// Rows above the list: the title and the filter input.
const HEADER_ROWS: u16 = 2;

/// One action the palette lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub action: Action,
    /// Label of its first shortcut; `None` when unbound.
    pub shortcut: Option<String>,
}

/// A key for the palette, already translated from the window system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    Text(String),
    Backspace,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Enter,
    Escape,
}

/// What the app does after the palette handled an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Close the palette.
    Close,
    /// Close the palette and run the action.
    Run(Action),
    /// The palette changed: draw it again.
    Redraw,
    /// Nothing changed.
    Ignore,
}

/// Where the box sits on the grid, in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub col: u16,
    pub row: u16,
    pub cols: u16,
    pub rows: u16,
}

impl Rect {
    /// Rows left for the list under the title and the input.
    pub fn list_rows(self) -> usize {
        usize::from(self.rows.saturating_sub(HEADER_ROWS))
    }

    fn contains(self, col: u16, row: u16) -> bool {
        (self.col..self.col + self.cols).contains(&col)
            && (self.row..self.row + self.rows).contains(&row)
    }
}

/// The open palette: every entry, the filter, and the selected match.
#[derive(Debug, Clone)]
pub struct CommandPalette {
    entries: Vec<Entry>,
    query: String,
    /// Indices into `entries` matching `query`, best first.
    matches: Vec<usize>,
    /// Index into `matches`.
    selected: usize,
    /// First match shown.
    scroll: usize,
}

impl CommandPalette {
    /// A palette listing `shortcuts` (see `Bindings::shortcuts`) in their
    /// order, without the palette itself.
    pub fn new(shortcuts: Vec<(Action, Option<String>)>) -> Self {
        let entries: Vec<Entry> = shortcuts
            .into_iter()
            .filter(|(action, _)| *action != Action::CommandPalette)
            .map(|(action, shortcut)| Entry { action, shortcut })
            .collect();
        let mut palette = Self {
            entries,
            query: String::new(),
            matches: Vec::new(),
            selected: 0,
            scroll: 0,
        };
        palette.filter();
        palette
    }

    /// Matches the entries against the query, best rank first and in
    /// listing order within a rank, and selects the first.
    fn filter(&mut self) {
        let query = self.query.to_lowercase();
        let mut ranked: Vec<(u8, usize)> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, entry)| Some((rank(entry, &query)?, i)))
            .collect();
        ranked.sort_by_key(|&(rank, _)| rank);
        self.matches = ranked.into_iter().map(|(_, i)| i).collect();
        self.selected = 0;
        self.scroll = 0;
    }

    /// Selects match `index` (clamped), keeping it in `rows` list rows.
    fn select(&mut self, index: usize, rows: usize) -> Outcome {
        let index = index.min(self.matches.len().saturating_sub(1));
        if index == self.selected {
            return Outcome::Ignore;
        }
        self.selected = index;
        self.scroll = self.visible(rows).start;
        Outcome::Redraw
    }

    #[cfg(test)]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// The matching entries, best first.
    #[cfg(test)]
    pub fn matches(&self) -> impl Iterator<Item = &Entry> {
        self.matches.iter().map(|&i| &self.entries[i])
    }

    /// The action of the selected match.
    pub fn selected(&self) -> Option<Action> {
        let &index = self.matches.get(self.selected)?;
        Some(self.entries[index].action)
    }

    /// Handles a key with `rows` list rows on screen (for paging).
    pub fn handle(&mut self, input: Input, rows: usize) -> Outcome {
        let page = rows.max(1);
        let selected = self.selected;
        match input {
            Input::Text(text) => {
                self.query.push_str(&text);
                self.filter();
                Outcome::Redraw
            }
            Input::Backspace => {
                if self.query.pop().is_none() {
                    return Outcome::Ignore;
                }
                self.filter();
                Outcome::Redraw
            }
            Input::Up => self.select(selected.saturating_sub(1), rows),
            Input::Down => self.select(selected + 1, rows),
            Input::PageUp => self.select(selected.saturating_sub(page), rows),
            Input::PageDown => self.select(selected + page, rows),
            Input::Home => self.select(0, rows),
            Input::End => self.select(usize::MAX, rows),
            Input::Enter => self.selected().map_or(Outcome::Ignore, Outcome::Run),
            Input::Escape => Outcome::Close,
        }
    }

    /// Moves the selection by `lines` (the mouse wheel; negative is up).
    pub fn scroll_by(&mut self, lines: i32, rows: usize) -> Outcome {
        let distance = lines.unsigned_abs() as usize;
        let index = if lines < 0 {
            self.selected.saturating_sub(distance)
        } else {
            self.selected.saturating_add(distance)
        };
        self.select(index, rows)
    }

    /// The matches shown in `rows` list rows: the selected one always is.
    pub fn visible(&self, rows: usize) -> Range<usize> {
        let len = self.matches.len();
        let mut start = self.scroll.min(self.selected);
        if self.selected >= start + rows {
            start = self.selected + 1 - rows;
        }
        let start = start.min(len.saturating_sub(rows));
        start..(start + rows).min(len)
    }

    /// The box over a grid of `grid` cells: centered, at most
    /// [`MAX_COLS`] x [`MAX_ROWS`] with a margin of two cells, tall enough
    /// for every entry when it can be.
    pub fn rect(&self, grid: TermSize) -> Rect {
        let (grid_cols, grid_rows) = (grid.cols(), grid.rows());
        let cols = grid_cols
            .saturating_sub(4)
            .min(MAX_COLS)
            .max(grid_cols.min(20));
        let wanted = HEADER_ROWS.saturating_add(self.entries.len().try_into().unwrap_or(u16::MAX));
        let rows = grid_rows
            .saturating_sub(4)
            .min(MAX_ROWS)
            .max(grid_rows.min(HEADER_ROWS + 1))
            .min(wanted);
        Rect {
            col: (grid_cols - cols) / 2,
            row: (grid_rows - rows) / 2,
            cols,
            rows,
        }
    }

    /// A click on grid cell `col`, `row` with the box at `rect`: a list
    /// row runs its action, outside the box closes it.
    pub fn click(&mut self, rect: Rect, col: u16, row: u16) -> Outcome {
        if !rect.contains(col, row) {
            return Outcome::Close;
        }
        let Some(list_row) = (row - rect.row).checked_sub(HEADER_ROWS) else {
            return Outcome::Ignore;
        };
        let rows = rect.list_rows();
        let index = self.visible(rows).start + usize::from(list_row);
        if index >= self.matches.len() {
            return Outcome::Ignore;
        }
        self.select(index, rows);
        self.selected().map_or(Outcome::Ignore, Outcome::Run)
    }

    /// The box as a terminal of `rect`'s size on `surface` (see
    /// [`surface`]): the title and match count, the filter input with the
    /// cursor, then the visible matches, the selected one in inverse
    /// video; categories and shortcuts are dimmed.
    pub fn render(&self, rect: Rect, surface: (u8, u8, u8)) -> Terminal {
        let size = TermSize::new(rect.cols.max(1), rect.rows.max(1)).expect("at least one cell");
        let mut term = Terminal::new(size);
        let width = usize::from(rect.cols);
        let inner = width.saturating_sub(2);
        let (r, g, b) = surface;
        let base = format!("\x1b[0;48;2;{r};{g};{b}m");
        let mut out = String::new();
        let mut line = |row: usize, text: String, style: &dyn Fn(usize) -> &'static str| {
            out.push_str(&format!("\x1b[{};1H", row + 1));
            let text = format!(" {text} ");
            let mut current = None;
            for (col, ch) in text.chars().take(width).enumerate() {
                let sgr = style(col);
                if current != Some(sgr) {
                    out.push_str(&base);
                    out.push_str(sgr);
                    current = Some(sgr);
                }
                out.push(ch);
            }
        };
        let plain: &dyn Fn(usize) -> &'static str = &|_| "";
        let dim: &dyn Fn(usize) -> &'static str = &|_| DIM;

        let count = format!("{}/{}", self.matches.len(), self.entries.len());
        line(0, row_text(TITLE, &count, inner), plain);
        let room = inner.saturating_sub(2);
        let query: Vec<char> = self.query.chars().collect();
        let shown: String = if query.len() > room {
            let tail = &query[query.len() - room.saturating_sub(1)..];
            std::iter::once('…').chain(tail.iter().copied()).collect()
        } else {
            self.query.clone()
        };
        let cursor = 1 + 2 + shown.chars().count();
        line(1, row_text(&format!("> {shown}"), "", inner), plain);

        let list_rows = rect.list_rows();
        let category_width = self
            .entries
            .iter()
            .map(|e| e.action.category().title().len())
            .max()
            .unwrap_or(0);
        let visible = self.visible(list_rows);
        for row in 0..list_rows {
            let screen_row = row + usize::from(HEADER_ROWS);
            let index = visible.start + row;
            let Some(&entry) = self.matches.get(index).filter(|_| index < visible.end) else {
                let text = if row == 0 && self.matches.is_empty() {
                    "No matching action"
                } else {
                    ""
                };
                line(screen_row, row_text(text, "", inner), dim);
                continue;
            };
            let entry = &self.entries[entry];
            let category = entry.action.category().title();
            let left = format!("{category:<category_width$}  {}", entry.action.title());
            let right = entry.shortcut.as_deref().unwrap_or("");
            let text = row_text(&left, right, inner);
            if index == self.selected {
                line(screen_row, text, &|_| INVERSE);
                continue;
            }
            // Margin, category; shortcut, margin.
            let shortcut_start = width.saturating_sub(right.chars().count() + 1);
            let style = |col: usize| {
                if (1..1 + category.len()).contains(&col)
                    || (!right.is_empty() && col >= shortcut_start && col + 1 < width)
                {
                    DIM
                } else {
                    ""
                }
            };
            line(screen_row, text, &style);
        }
        let cursor = cursor.min(width.saturating_sub(1));
        out.push_str(&format!("\x1b[0m\x1b[2;{}H", cursor + 1));
        term.advance(out.as_bytes());
        term
    }
}

/// Bright black: dimmed in every theme, as the faint attribute is not
/// drawn.
const DIM: &str = "\x1b[90m";
const INVERSE: &str = "\x1b[7m";

/// How well `entry` matches `query` (lowercase): 0 when the title starts
/// with it, 1 when the category, title or name contains it, 2 when they
/// contain its characters in order; `None` otherwise.
fn rank(entry: &Entry, query: &str) -> Option<u8> {
    let action = entry.action;
    let title = action.title().to_lowercase();
    if title.starts_with(query) {
        return Some(0);
    }
    let haystack = format!(
        "{} {title} {}",
        action.category().title().to_lowercase(),
        action.name()
    );
    if haystack.contains(query) {
        return Some(1);
    }
    let mut chars = haystack.chars();
    query
        .chars()
        .filter(|c| !c.is_whitespace())
        .all(|wanted| chars.any(|c| c == wanted))
        .then_some(2)
}

/// `left` and `right` on one line of exactly `width` chars, `right`
/// flush right; `left` is truncated with `…` to keep one space between
/// them. When both do not fit, `right` is dropped.
pub fn row_text(left: &str, right: &str, width: usize) -> String {
    let fit = |text: &str, width: usize| {
        let text = truncate(text, width);
        let pad = width - text.chars().count();
        format!("{text}{}", " ".repeat(pad))
    };
    let right_width = right.chars().count();
    if right.is_empty() || right_width + 2 > width {
        return fit(left, width);
    }
    format!("{} {right}", fit(left, width - right_width - 1))
}

/// The box background: the theme `background` moved a little towards
/// the `foreground`, so it stands out in light and dark themes alike.
pub fn surface(background: Rgb, foreground: Rgb) -> (u8, u8, u8) {
    let mix = |shift: u32| {
        let (bg, fg) = (
            f64::from((background >> shift) & 0xff),
            f64::from((foreground >> shift) & 0xff),
        );
        (bg + (fg - bg) * 0.15).round() as u8
    };
    (mix(16), mix(8), mix(0))
}

/// The palette input for a key press: `key` with `mods`, typing `text`.
/// Ctrl+P and Ctrl+N move up and down; other Ctrl chords are ignored.
pub fn input(key: &Key, text: Option<&str>, mods: ModifiersState) -> Option<Input> {
    if mods.control_key() {
        return match key {
            Key::Character(c) if c.eq_ignore_ascii_case("p") => Some(Input::Up),
            Key::Character(c) if c.eq_ignore_ascii_case("n") => Some(Input::Down),
            _ => None,
        };
    }
    match key {
        Key::Named(NamedKey::Escape) => return Some(Input::Escape),
        Key::Named(NamedKey::Enter) => return Some(Input::Enter),
        Key::Named(NamedKey::Backspace) => return Some(Input::Backspace),
        Key::Named(NamedKey::ArrowUp) => return Some(Input::Up),
        Key::Named(NamedKey::ArrowDown) => return Some(Input::Down),
        Key::Named(NamedKey::PageUp) => return Some(Input::PageUp),
        Key::Named(NamedKey::PageDown) => return Some(Input::PageDown),
        Key::Named(NamedKey::Home) => return Some(Input::Home),
        Key::Named(NamedKey::End) => return Some(Input::End),
        _ => {}
    }
    let text: String = text?.chars().filter(|c| !c.is_control()).collect();
    (!text.is_empty()).then_some(Input::Text(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nxg_config::Bindings;
    use nxg_core::{Color, Flags};

    fn palette() -> CommandPalette {
        CommandPalette::new(Bindings::defaults(false).shortcuts())
    }

    fn titles(palette: &CommandPalette) -> Vec<&'static str> {
        palette.matches().map(|e| e.action.title()).collect()
    }

    fn typed(text: &str) -> CommandPalette {
        let mut palette = palette();
        palette.handle(Input::Text(text.into()), 5);
        palette
    }

    #[test]
    fn lists_every_action_but_itself_in_order_with_shortcuts() {
        let palette = palette();
        let actions: Vec<Action> = palette.matches().map(|e| e.action).collect();
        let expected: Vec<Action> = Action::ALL
            .into_iter()
            .filter(|&a| a != Action::CommandPalette)
            .collect();
        assert_eq!(actions, expected);
        let new_tab = palette.matches().find(|e| e.action == Action::NewTab);
        assert_eq!(new_tab.unwrap().shortcut.as_deref(), Some("Ctrl+Shift+T"));
        assert!(titles(&palette).contains(&"Go to Tab 3"));
        assert_eq!(palette.selected(), Some(Action::ZoomIn));
    }

    #[test]
    fn unbound_actions_have_no_shortcut() {
        let palette = CommandPalette::new(vec![(Action::ReloadConfig, None)]);
        assert_eq!(palette.matches().next().unwrap().shortcut, None);
    }

    #[test]
    fn prefix_matches_come_before_substring_matches() {
        assert_eq!(
            titles(&typed("zo")),
            [
                "Zoom In",
                "Zoom Out",
                "Zoom Pane",
                "Reset Zoom",
                "Resize Pane Down"
            ]
        );
        assert_eq!(titles(&typed("ZOOM")), titles(&typed("zoom")), "any case");
    }

    #[test]
    fn pane_actions_are_listed_and_found_by_name() {
        assert_eq!(titles(&typed("equalize")), ["Equalize Panes"]);
        assert_eq!(titles(&typed("split"))[..2], ["Split Right", "Split Down"]);
        assert!(titles(&typed("panes")).contains(&"Close Pane"));
    }

    #[test]
    fn subsequence_matches_come_last_in_listing_order() {
        let palette = typed("nt");
        let titles = titles(&palette);
        // "Font" contains "nt": the font entries match as substrings.
        assert_eq!(titles[..3], ["Zoom In", "Zoom Out", "Reset Zoom"]);
        let new_tab = titles.iter().position(|&t| t == "New Tab").unwrap();
        let next_tab = titles.iter().position(|&t| t == "Next Tab").unwrap();
        assert!(3 <= new_tab && new_tab < next_tab);
        assert!(!titles.contains(&"Scroll to Top"));
    }

    #[test]
    fn categories_and_names_match_too() {
        assert_eq!(titles(&typed("scrollback")).len(), 4);
        assert_eq!(titles(&typed("goto_tab_2")), ["Go to Tab 2"]);
        assert_eq!(titles(&typed("tab 9")), ["Go to Tab 9"]);
        assert!(titles(&typed("xyz")).is_empty());
    }

    #[test]
    fn backspace_edits_the_filter_and_resets_the_selection() {
        let mut palette = typed("zom");
        assert_eq!(palette.handle(Input::Down, 5), Outcome::Redraw);
        assert_eq!(palette.handle(Input::Backspace, 5), Outcome::Redraw);
        assert_eq!(palette.query(), "zo");
        assert_eq!(palette.selected(), Some(Action::ZoomIn));
        palette.handle(Input::Backspace, 5);
        palette.handle(Input::Backspace, 5);
        assert_eq!(palette.handle(Input::Backspace, 5), Outcome::Ignore);
        assert_eq!(palette.matches().count(), Action::ALL.len() - 1);
    }

    #[test]
    fn selection_moves_and_clamps() {
        let mut palette = palette();
        let last = *Action::ALL.last().unwrap();
        assert_eq!(palette.handle(Input::Up, 5), Outcome::Ignore);
        palette.handle(Input::Down, 5);
        assert_eq!(palette.selected(), Some(Action::ZoomOut));
        palette.handle(Input::PageDown, 5);
        assert_eq!(palette.selected(), Some(Action::ScrollToBottom));
        palette.handle(Input::End, 5);
        assert_eq!(palette.selected(), Some(last));
        assert_eq!(palette.handle(Input::Down, 5), Outcome::Ignore);
        assert_eq!(palette.handle(Input::PageDown, 5), Outcome::Ignore);
        palette.handle(Input::PageUp, 5);
        palette.handle(Input::Home, 5);
        assert_eq!(palette.selected(), Some(Action::ZoomIn));
        assert_eq!(palette.scroll_by(2, 5), Outcome::Redraw);
        assert_eq!(palette.selected(), Some(Action::ResetZoom));
        assert_eq!(palette.scroll_by(-9, 5), Outcome::Redraw);
        assert_eq!(palette.selected(), Some(Action::ZoomIn));
    }

    #[test]
    fn the_visible_window_follows_the_selection() {
        let mut palette = palette();
        assert_eq!(palette.visible(5), 0..5);
        for _ in 0..6 {
            palette.handle(Input::Down, 5);
        }
        assert_eq!(palette.visible(5), 2..7, "selection on the last row");
        palette.handle(Input::Up, 5);
        assert_eq!(palette.visible(5), 2..7, "the window stays put");
        palette.handle(Input::End, 5);
        let len = palette.matches().count();
        assert_eq!(palette.visible(5), len - 5..len);
        assert_eq!(palette.visible(100), 0..len, "short lists");
        assert_eq!(typed("xyz").visible(5), 0..0);
    }

    #[test]
    fn enter_runs_the_selection_and_escape_closes() {
        let mut palette = typed("new");
        assert_eq!(
            palette.handle(Input::Enter, 5),
            Outcome::Run(Action::NewTab)
        );
        assert_eq!(palette.handle(Input::Escape, 5), Outcome::Close);
        let mut none = typed("xyz");
        assert_eq!(none.selected(), None);
        assert_eq!(none.handle(Input::Enter, 5), Outcome::Ignore);
    }

    #[test]
    fn the_box_is_centered_with_a_margin_and_capped() {
        let palette = palette();
        let entries = palette.matches().count() as u16;
        let rect = palette.rect(TermSize::new(80, 24).unwrap());
        assert_eq!(
            rect,
            Rect {
                col: 4,
                row: 2,
                cols: 72,
                rows: 20
            }
        );
        assert_eq!(rect.list_rows(), 18);
        let rect = palette.rect(TermSize::new(200, 60).unwrap());
        assert_eq!((rect.cols, rect.rows), (72, (entries + 2).min(20)));
        assert_eq!((rect.col, rect.row), (64, 20));
        let rect = palette.rect(TermSize::new(40, 10).unwrap());
        assert_eq!(
            rect,
            Rect {
                col: 2,
                row: 2,
                cols: 36,
                rows: 6
            }
        );
        // Tiny grids: the whole grid, never more.
        let rect = palette.rect(TermSize::new(10, 2).unwrap());
        assert_eq!(
            rect,
            Rect {
                col: 0,
                row: 0,
                cols: 10,
                rows: 2
            }
        );
        assert_eq!(rect.list_rows(), 0);
        let few = CommandPalette::new(vec![(Action::NewTab, None)]);
        assert_eq!(few.rect(TermSize::new(80, 24).unwrap()).rows, 3);
    }

    #[test]
    fn rows_put_the_shortcut_flush_right_and_truncate_the_rest() {
        assert_eq!(row_text("New Tab", "Ctrl+T", 16), "New Tab   Ctrl+T");
        assert_eq!(row_text("New Tab", "", 9), "New Tab  ");
        assert_eq!(row_text("Previous Tab", "Ctrl+T", 14), "Previo… Ctrl+T");
        assert_eq!(
            row_text("Previous Tab", "Ctrl+Shift+T", 13),
            "Previous Tab "
        );
        assert_eq!(row_text("Previous Tab", "", 5), "Prev…");
        assert_eq!(row_text("ab", "cd", 0), "");
    }

    #[test]
    fn clicks_run_list_rows_and_close_outside() {
        let mut palette = palette();
        let rect = Rect {
            col: 4,
            row: 2,
            cols: 30,
            rows: 7,
        };
        assert_eq!(palette.click(rect, 3, 5), Outcome::Close, "left of it");
        assert_eq!(palette.click(rect, 10, 9), Outcome::Close, "below it");
        assert_eq!(palette.click(rect, 10, 2), Outcome::Ignore, "title");
        assert_eq!(palette.click(rect, 10, 3), Outcome::Ignore, "input");
        assert_eq!(palette.click(rect, 4, 4), Outcome::Run(Action::ZoomIn));
        assert_eq!(palette.click(rect, 33, 6), Outcome::Run(Action::ResetZoom));
        assert_eq!(palette.selected(), Some(Action::ResetZoom));
        // Rows past the end of a short list do nothing.
        let mut short = typed("zoom in");
        assert_eq!(short.click(rect, 10, 5), Outcome::Ignore);
    }

    #[test]
    fn clicks_follow_the_scrolled_list() {
        let mut palette = palette();
        let rect = Rect {
            col: 0,
            row: 0,
            cols: 30,
            rows: 7,
        };
        palette.handle(Input::End, rect.list_rows());
        let last = *Action::ALL.last().unwrap();
        assert_eq!(palette.click(rect, 1, 6), Outcome::Run(last));
    }

    fn text(term: &Terminal, row: u16) -> String {
        term.display_row(row).iter().map(|c| c.ch).collect()
    }

    #[test]
    fn renders_title_input_and_rows_on_the_surface() {
        let mut palette = typed("zo");
        palette.handle(Input::Down, 3);
        let rect = Rect {
            col: 0,
            row: 0,
            cols: 40,
            rows: 5,
        };
        let term = palette.render(rect, (1, 2, 3));
        assert_eq!(term.size(), TermSize::new(40, 5).unwrap());
        assert_eq!(text(&term, 0), " Command Palette                   5/37 ");
        assert_eq!(text(&term, 1), " > zo                                   ");
        assert_eq!(text(&term, 2), " Font        Zoom In             Ctrl+= ");
        assert_eq!(text(&term, 3), " Font        Zoom Out            Ctrl+- ");
        assert_eq!(text(&term, 4), " Panes       Zoom Pane Ctrl+Shift+Enter ");
        let cursor = term.display_cursor();
        assert!(cursor.visible);
        assert_eq!((cursor.col, cursor.row), (5, 1));
        let surface = Color::Rgb(1, 2, 3);
        for row in 0..5 {
            assert!(term.display_row(row).iter().all(|c| c.bg == surface));
        }
        let plain = &term.display_row(2);
        assert_eq!(plain[1].fg, Color::Indexed(8), "dim category");
        assert_eq!(plain[13].fg, Color::Default, "title");
        assert_eq!(plain[33].fg, Color::Indexed(8), "dim shortcut");
        let selected = &term.display_row(3);
        assert!(selected.iter().all(|c| c.flags.contains(Flags::INVERSE)));
        assert!(!plain.iter().any(|c| c.flags.contains(Flags::INVERSE)));
    }

    #[test]
    fn renders_an_empty_list_and_a_long_query() {
        let palette = typed("a long query that does not match");
        let rect = Rect {
            col: 0,
            row: 0,
            cols: 20,
            rows: 4,
        };
        let term = palette.render(rect, (0, 0, 0));
        assert_eq!(text(&term, 1), " > … does not match ");
        assert_eq!(text(&term, 2), " No matching action ");
        assert_eq!(term.display_cursor().col, 19);
        assert_eq!(text(&term, 3), " ".repeat(20));
    }

    #[test]
    fn surface_leans_towards_the_foreground() {
        use nxg_render::palette::rgb;
        assert_eq!(surface(rgb(0, 0, 0), rgb(255, 255, 255)), (38, 38, 38));
        assert_eq!(surface(rgb(255, 255, 255), rgb(0, 0, 0)), (217, 217, 217));
        assert_eq!(surface(rgb(10, 20, 30), rgb(10, 20, 30)), (10, 20, 30));
    }

    #[test]
    fn keys_become_palette_inputs() {
        let none = ModifiersState::empty();
        let ctrl = ModifiersState::CONTROL;
        let named = |key| Key::Named(key);
        let ch = |c: &str| Key::Character(c.into());
        assert_eq!(
            input(&ch("a"), Some("a"), none),
            Some(Input::Text("a".into()))
        );
        assert_eq!(
            input(&ch("A"), Some("A"), ModifiersState::SHIFT),
            Some(Input::Text("A".into()))
        );
        assert_eq!(
            input(&named(NamedKey::Space), Some(" "), none),
            Some(Input::Text(" ".into()))
        );
        assert_eq!(input(&ch("p"), None, ctrl), Some(Input::Up));
        assert_eq!(input(&ch("n"), None, ctrl), Some(Input::Down));
        assert_eq!(input(&ch("c"), Some("\x03"), ctrl), None);
        assert_eq!(
            input(&named(NamedKey::ArrowUp), None, none),
            Some(Input::Up)
        );
        assert_eq!(
            input(&named(NamedKey::ArrowDown), None, none),
            Some(Input::Down)
        );
        assert_eq!(
            input(&named(NamedKey::PageUp), None, none),
            Some(Input::PageUp)
        );
        assert_eq!(
            input(&named(NamedKey::PageDown), None, none),
            Some(Input::PageDown)
        );
        assert_eq!(input(&named(NamedKey::Home), None, none), Some(Input::Home));
        assert_eq!(input(&named(NamedKey::End), None, none), Some(Input::End));
        assert_eq!(
            input(&named(NamedKey::Enter), Some("\r"), none),
            Some(Input::Enter)
        );
        assert_eq!(
            input(&named(NamedKey::Escape), None, none),
            Some(Input::Escape)
        );
        assert_eq!(
            input(&named(NamedKey::Backspace), Some("\x08"), none),
            Some(Input::Backspace)
        );
        assert_eq!(input(&named(NamedKey::Shift), None, none), None);
        assert_eq!(input(&named(NamedKey::Tab), Some("\t"), none), None);
    }
}
