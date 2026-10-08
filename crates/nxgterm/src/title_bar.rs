//! The tab bar as a whole: the tab labels plus, when it is the window's
//! title bar (`window.decorations = "integrated"`), an inset for the macOS
//! window buttons, a new-tab button and minimize, maximize and close
//! buttons. Layout, what a click hits, which window edge a press resizes
//! and the drawn row are pure functions, unit tested.

use std::fmt::Write as _;
use std::time::Instant;

use nxg_core::{TermSize, Terminal};
use winit::window::ResizeDirection;

use crate::appearance;
use crate::mouse::MULTI_CLICK;
use crate::tab_bar::{self, Label};

/// Columns of each window button.
pub const BUTTON_COLS: u16 = 5;
/// Columns of the new-tab button.
pub const NEW_TAB_COLS: u16 = 3;
/// Width of the window edges that resize the window, in logical pixels.
const RESIZE_BORDER: f64 = 5.0;
/// Width the macOS window buttons (and a margin) take on the left of the
/// title bar, in logical pixels.
const MACOS_BUTTONS: f64 = 78.0;

/// What the bar holds besides the tab labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chrome {
    /// Columns left empty before the first label.
    pub inset: u16,
    /// A `+` button after the last label.
    pub new_tab: bool,
    /// Minimize, maximize and close buttons on the right.
    pub buttons: bool,
}

impl Chrome {
    /// The tab bar below a system title bar: labels only.
    pub const NATIVE: Self = Self {
        inset: 0,
        new_tab: false,
        buttons: false,
    };
}

/// A window button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Minimize,
    Maximize,
    Close,
}

const BUTTONS: [Button; 3] = [Button::Minimize, Button::Maximize, Button::Close];

/// What is under a bar column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    /// The label of the tab at this index.
    Tab(usize),
    NewTab,
    Button(Button),
    /// Empty bar: moves the window.
    Drag,
}

/// The bar laid out on `cols` columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bar {
    pub cols: u16,
    /// Tab labels, at their bar columns.
    pub labels: Vec<Label>,
    /// First column of the new-tab button, when it shows.
    pub new_tab: Option<u16>,
    /// First column of the window buttons, when they show.
    pub buttons: Option<u16>,
}

/// Lays out the labels of `titles` (see [`tab_bar::layout`]) and the
/// `chrome` on a bar `cols` columns wide. The window buttons take the
/// right end; the labels share what is left after the inset and the
/// new-tab button, which follows the last label.
pub fn layout(titles: &[&str], active: usize, cols: u16, chrome: Chrome) -> Bar {
    let buttons = chrome.buttons.then(|| cols.saturating_sub(3 * BUTTON_COLS));
    let right = buttons.unwrap_or(cols);
    let reserved = if chrome.new_tab { NEW_TAB_COLS } else { 0 };
    let room = right.saturating_sub(chrome.inset).saturating_sub(reserved);
    let mut labels = tab_bar::layout(titles, active, room);
    for label in &mut labels {
        label.start += chrome.inset;
    }
    let end = labels
        .last()
        .map_or(chrome.inset, |label| label.start + label.width());
    let new_tab = (chrome.new_tab && end + NEW_TAB_COLS <= right).then_some(end);
    Bar {
        cols,
        labels,
        new_tab,
        buttons,
    }
}

impl Bar {
    /// What is at column `col`.
    pub fn region_at(&self, col: u16) -> Region {
        if let Some(start) = self.buttons.filter(|&start| col >= start) {
            let index = usize::from((col - start) / BUTTON_COLS).min(BUTTONS.len() - 1);
            return Region::Button(BUTTONS[index]);
        }
        if let Some(start) = self.new_tab {
            if (start..start + NEW_TAB_COLS).contains(&col) {
                return Region::NewTab;
            }
        }
        tab_bar::tab_at(&self.labels, col).map_or(Region::Drag, Region::Tab)
    }
}

/// A one-row terminal showing `bar`: the active label in inverse video,
/// the others and the new-tab button dimmed, the window buttons in the
/// foreground color. `hover` is the button under the pointer: it gets a
/// gray background, red for close. `maximized` picks the restore glyph.
/// No cursor; nothing is drawn past the last column.
pub fn render(bar: &Bar, hover: Option<Region>, maximized: bool) -> Terminal {
    let cols = bar.cols.max(1);
    let mut terminal = Terminal::new(TermSize::new(cols, 1).expect("at least one column and row"));
    let mut input = String::from("\x1b[?25l");
    let mut put = |col: u16, sgr: &str, text: &str| {
        if col >= cols {
            return;
        }
        let text: String = text.chars().take(usize::from(cols - col)).collect();
        let _ = write!(input, "\x1b[1;{}H{sgr}{text}\x1b[0m", col + 1);
    };
    // Bright black: dimmed in every theme, as the faint attribute is not
    // drawn.
    const DIM: &str = "\x1b[90m";
    const HOVER: &str = "\x1b[100m";
    for label in &bar.labels {
        let sgr = if label.active { "\x1b[7m" } else { DIM };
        put(label.start, sgr, &label.text);
    }
    if let Some(col) = bar.new_tab {
        let sgr = if hover == Some(Region::NewTab) {
            HOVER
        } else {
            DIM
        };
        put(col, sgr, " + ");
    }
    if let Some(start) = bar.buttons {
        for (i, button) in BUTTONS.into_iter().enumerate() {
            let glyph = match button {
                Button::Minimize => '─',
                // Two-square restore glyphs are missing from most
                // monospace fonts.
                Button::Maximize if maximized => '▫',
                Button::Maximize => '□',
                Button::Close => '×',
            };
            let sgr = match (hover == Some(Region::Button(button)), button) {
                (false, _) => "",
                // White on the conventional close red.
                (true, Button::Close) => "\x1b[38;2;255;255;255;48;2;196;43;28m",
                (true, _) => HOVER,
            };
            put(start + i as u16 * BUTTON_COLS, sgr, &format!("  {glyph}  "));
        }
    }
    terminal.advance(input.as_bytes());
    terminal
}

/// The window edge a press at `x`, `y` pixels resizes in a `width` x
/// `height` window, or `None` away from the edges. The edges are `border`
/// pixels wide; the corners reach twice as far along each edge, so they
/// are easier to grab.
pub fn resize_edge(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    border: f64,
) -> Option<ResizeDirection> {
    let corner = border * 2.0;
    let (left, right) = (x < border, x >= width - border);
    let (top, bottom) = (y < border, y >= height - border);
    let (near_left, near_right) = (x < corner, x >= width - corner);
    let (near_top, near_bottom) = (y < corner, y >= height - corner);
    let direction = if (top && near_left) || (left && near_top) {
        ResizeDirection::NorthWest
    } else if (top && near_right) || (right && near_top) {
        ResizeDirection::NorthEast
    } else if (bottom && near_left) || (left && near_bottom) {
        ResizeDirection::SouthWest
    } else if (bottom && near_right) || (right && near_bottom) {
        ResizeDirection::SouthEast
    } else if top {
        ResizeDirection::North
    } else if bottom {
        ResizeDirection::South
    } else if left {
        ResizeDirection::West
    } else if right {
        ResizeDirection::East
    } else {
        return None;
    };
    Some(direction)
}

/// Width of the resizing edges in physical pixels at `scale`.
pub fn resize_border(scale: f64) -> f64 {
    RESIZE_BORDER * appearance::valid_scale(scale)
}

/// Columns of `cell_width` pixels to leave empty on the left of the bar so
/// the labels clear the macOS window buttons, given the `padding` (pixels)
/// before the first column, at `scale`.
pub fn macos_inset(scale: f64, padding: u32, cell_width: u32) -> u16 {
    let needed = MACOS_BUTTONS * appearance::valid_scale(scale) - f64::from(padding);
    let cols = (needed.max(0.0) / f64::from(cell_width.max(1))).ceil();
    cols.min(f64::from(u16::MAX)) as u16
}

/// Whether a press `now` on the empty bar makes a double click with the
/// `previous` one.
pub fn double_click(previous: Option<Instant>, now: Instant) -> bool {
    previous.is_some_and(|then| now.saturating_duration_since(then) <= MULTI_CLICK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nxg_core::{Color, Flags};
    use std::time::Duration;

    const INTEGRATED: Chrome = Chrome {
        inset: 0,
        new_tab: true,
        buttons: true,
    };

    fn text(terminal: &Terminal) -> String {
        terminal.display_row(0).iter().map(|c| c.ch).collect()
    }

    #[test]
    fn a_native_bar_holds_only_the_labels() {
        let bar = layout(&["zsh", "vim"], 1, 40, Chrome::NATIVE);
        assert_eq!(bar.labels, tab_bar::layout(&["zsh", "vim"], 1, 40));
        assert_eq!((bar.new_tab, bar.buttons), (None, None));
        assert_eq!(bar.region_at(0), Region::Tab(0));
        assert_eq!(bar.region_at(39), Region::Drag);
    }

    #[test]
    fn integrated_buttons_take_the_right_end_and_new_tab_follows_the_labels() {
        let bar = layout(&["zsh", "vim"], 0, 40, INTEGRATED);
        assert_eq!(bar.buttons, Some(25));
        assert_eq!(bar.new_tab, Some(16));
        assert_eq!(bar.region_at(7), Region::Tab(0));
        assert_eq!(bar.region_at(8), Region::Tab(1));
        assert_eq!(bar.region_at(16), Region::NewTab);
        assert_eq!(bar.region_at(18), Region::NewTab);
        assert_eq!(bar.region_at(19), Region::Drag);
        assert_eq!(bar.region_at(24), Region::Drag);
        assert_eq!(bar.region_at(25), Region::Button(Button::Minimize));
        assert_eq!(bar.region_at(30), Region::Button(Button::Maximize));
        assert_eq!(bar.region_at(35), Region::Button(Button::Close));
        assert_eq!(bar.region_at(39), Region::Button(Button::Close));
        assert_eq!(bar.region_at(u16::MAX), Region::Button(Button::Close));
    }

    #[test]
    fn labels_share_what_the_inset_and_the_buttons_leave() {
        let chrome = Chrome {
            inset: 4,
            ..INTEGRATED
        };
        // 30 columns: 15 for the buttons, 4 inset, 3 for `+`: 8 left.
        let bar = layout(&["bash", "zsh"], 0, 30, chrome);
        let texts: Vec<&str> = bar.labels.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, [" 1:…", " 2:…"]);
        assert_eq!(bar.labels[0].start, 4);
        assert_eq!(bar.new_tab, Some(12));
        assert_eq!(bar.region_at(3), Region::Drag, "inset");
        assert_eq!(bar.region_at(4), Region::Tab(0));
    }

    #[test]
    fn a_bar_too_narrow_for_everything_drops_labels_and_new_tab() {
        let bar = layout(&["zsh"], 0, 10, INTEGRATED);
        assert!(bar.labels.is_empty());
        assert_eq!(bar.new_tab, None);
        assert_eq!(bar.buttons, Some(0));
        let drawn = render(&bar, None, false);
        assert_eq!(text(&drawn), "  ─    □  ");
    }

    #[test]
    fn the_bar_shows_the_active_label_inverse_and_the_others_dimmed() {
        let bar = layout(&["a", "b"], 1, 12, Chrome::NATIVE);
        let drawn = render(&bar, None, false);
        assert_eq!(drawn.size(), TermSize::new(12, 1).unwrap());
        assert!(!drawn.display_cursor().visible);
        let row = drawn.display_row(0);
        assert_eq!(text(&drawn), " 1: a  2: b ");
        assert_eq!(row[1].fg, Color::Indexed(8));
        assert!(!row[1].flags.contains(Flags::INVERSE));
        assert!(row[7].flags.contains(Flags::INVERSE));
        assert!(row[11].flags.contains(Flags::INVERSE));
    }

    #[test]
    fn a_full_bar_does_not_scroll_its_only_row() {
        let bar = layout(&["ab"], 0, 7, Chrome::NATIVE);
        assert_eq!(text(&render(&bar, None, false)), " 1: ab ");
    }

    #[test]
    fn window_buttons_are_drawn_with_hover_feedback() {
        let bar = layout(&["sh"], 0, 30, INTEGRATED);
        let plain = render(&bar, None, false);
        assert_eq!(text(&plain), " 1: sh  +        ─    □    ×  ");
        assert_eq!(plain.display_row(0)[8].fg, Color::Indexed(8), "dim +");
        assert_eq!(plain.display_row(0)[17].bg, Color::Default);

        let close = render(&bar, Some(Region::Button(Button::Close)), false);
        let row = close.display_row(0);
        assert_eq!(row[25].bg, Color::Rgb(196, 43, 28));
        assert_eq!(row[27].fg, Color::Rgb(255, 255, 255));
        assert_eq!(row[24].bg, Color::Default, "maximize stays plain");

        let minimize = render(&bar, Some(Region::Button(Button::Minimize)), false);
        assert_eq!(minimize.display_row(0)[15].bg, Color::Indexed(8));
        let new_tab = render(&bar, Some(Region::NewTab), false);
        assert_eq!(new_tab.display_row(0)[7].bg, Color::Indexed(8));

        let maximized = render(&bar, None, true);
        assert_eq!(maximized.display_row(0)[22].ch, '▫');
    }

    #[test]
    fn edges_and_larger_corners_resize() {
        let edge = |x, y| resize_edge(x, y, 100.0, 80.0, 5.0);
        assert_eq!(edge(50.0, 40.0), None);
        assert_eq!(edge(50.0, 0.0), Some(ResizeDirection::North));
        assert_eq!(edge(50.0, 4.9), Some(ResizeDirection::North));
        assert_eq!(edge(50.0, 5.0), None);
        assert_eq!(edge(50.0, 79.0), Some(ResizeDirection::South));
        assert_eq!(edge(50.0, 75.0), Some(ResizeDirection::South));
        assert_eq!(edge(0.0, 40.0), Some(ResizeDirection::West));
        assert_eq!(edge(99.5, 40.0), Some(ResizeDirection::East));
        assert_eq!(edge(0.0, 0.0), Some(ResizeDirection::NorthWest));
        assert_eq!(edge(9.0, 1.0), Some(ResizeDirection::NorthWest));
        assert_eq!(edge(10.0, 1.0), Some(ResizeDirection::North));
        assert_eq!(edge(1.0, 9.0), Some(ResizeDirection::NorthWest));
        assert_eq!(edge(99.0, 1.0), Some(ResizeDirection::NorthEast));
        assert_eq!(edge(1.0, 79.0), Some(ResizeDirection::SouthWest));
        assert_eq!(edge(99.0, 72.0), Some(ResizeDirection::SouthEast));
    }

    #[test]
    fn resize_borders_follow_the_scale() {
        assert_eq!(resize_border(1.0), 5.0);
        assert_eq!(resize_border(2.0), 10.0);
        assert_eq!(resize_border(f64::NAN), 5.0);
    }

    #[test]
    fn the_macos_inset_clears_the_window_buttons() {
        // 78 - 8 padding = 70 pixels: 8 columns of 9.
        assert_eq!(macos_inset(1.0, 8, 9), 8);
        assert_eq!(macos_inset(2.0, 16, 18), 8);
        assert_eq!(macos_inset(1.0, 100, 9), 0);
        assert_eq!(macos_inset(1.0, 0, 0), 78);
    }

    #[test]
    fn double_clicks_are_two_quick_presses() {
        let t0 = Instant::now();
        assert!(!double_click(None, t0));
        assert!(double_click(Some(t0), t0 + Duration::from_millis(300)));
        assert!(!double_click(
            Some(t0),
            t0 + MULTI_CLICK + Duration::from_millis(1)
        ));
    }
}
