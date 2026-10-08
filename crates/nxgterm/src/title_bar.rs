//! The tab bar as a whole: the tab labels plus, when it is the window's
//! title bar (`window.decorations = "integrated"`), an inset for the macOS
//! window buttons, a new-tab button and minimize, maximize and close
//! buttons. Layout, what a click hits, which window edge a press resizes,
//! the drawn row and the window buttons' geometry are pure functions, unit
//! tested.
//!
//! The labels and the new-tab button are text on a one-row terminal. The
//! window buttons are drawn as [`Shape`]s in window pixels instead: flush
//! with the top-right corner, as tall as the bar and with vector glyphs,
//! so they scale with the display and the bar rather than with the font's
//! glyphs (whose box-drawing and geometric characters vary in size, and
//! come from fallback fonts shrunk into a cell).

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Instant;

use nxg_core::{TermSize, Terminal};
use nxg_render::palette::{Rgb, rgb};
use nxg_render::{Mask, Segment, Shape};
use winit::window::ResizeDirection;

use crate::appearance;
use crate::mouse::MULTI_CLICK;
use crate::tab_bar::{self, Label};

/// Size of a window button in logical pixels on a bar this tall, as on
/// Windows 11; buttons on a taller bar grow with it.
const BUTTON_WIDTH: f64 = 46.0;
const BUTTON_HEIGHT: f64 = 32.0;
/// Side of the button glyphs in logical pixels on a [`BUTTON_HEIGHT`] bar.
const GLYPH_SIDE: f64 = 10.0;
/// The conventional close-button red, under a white glyph.
const CLOSE_HOVER: Rgb = rgb(196, 43, 28);
const CLOSE_HOVER_GLYPH: Rgb = rgb(255, 255, 255);
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
    /// The first column under the minimize, maximize and close buttons
    /// (see [`first_button_col`]), when they show.
    pub buttons: Option<u16>,
}

impl Chrome {
    /// The tab bar below a system title bar: labels only.
    pub const NATIVE: Self = Self {
        inset: 0,
        new_tab: false,
        buttons: None,
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

/// A rectangle in window pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    /// Whether window pixel `x`, `y` is inside.
    pub fn contains(self, x: f64, y: f64) -> bool {
        let (left, top) = (f64::from(self.x), f64::from(self.y));
        (left..left + f64::from(self.width)).contains(&x)
            && (top..top + f64::from(self.height)).contains(&y)
    }
}

/// What is under a bar column, or under the pointer (see [`button_at`]).
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
    /// First column under the window buttons, when they show.
    pub buttons: Option<u16>,
}

/// Lays out the labels of `titles` (see [`tab_bar::layout`]) and the
/// `chrome` on a bar `cols` columns wide. The columns under the window
/// buttons are left empty; the labels share what is left after the inset
/// and the new-tab button, which follows the last label.
pub fn layout(titles: &[&str], active: usize, cols: u16, chrome: Chrome) -> Bar {
    let buttons = chrome.buttons.map(|col| col.min(cols));
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
    /// What is at column `col`. The columns under the window buttons are
    /// [`Region::Drag`]: the buttons are found by pixel, with
    /// [`button_at`].
    pub fn region_at(&self, col: u16) -> Region {
        if self.buttons.is_some_and(|start| col >= start) {
            return Region::Drag;
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
/// the others and the new-tab button dimmed. `hover` is the region under
/// the pointer: the new-tab button gets a gray background there. The
/// window buttons are not part of it (see [`button_shapes`]). No cursor;
/// nothing is drawn past the last column.
pub fn render(bar: &Bar, hover: Option<Region>) -> Terminal {
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
    terminal.advance(input.as_bytes());
    terminal
}

/// The minimize, maximize and close buttons, left to right, of a window
/// `window_width` pixels wide whose title bar is `bar_height` pixels tall,
/// at `scale`: flush with the top-right corner and as tall as the bar.
/// They keep the 46:32 shape of the Windows 11 buttons and are never
/// narrower than 46 logical pixels, so they grow with the display scale
/// and with the bar (a larger font makes a taller bar).
pub fn button_rects(window_width: u32, bar_height: u32, scale: f64) -> [(Button, Rect); 3] {
    let scale = appearance::valid_scale(scale);
    let width = (BUTTON_WIDTH * scale)
        .max(f64::from(bar_height) * BUTTON_WIDTH / BUTTON_HEIGHT)
        .round() as u32;
    let right = i64::from(window_width);
    BUTTONS.map(|button| {
        let from_right = match button {
            Button::Minimize => 3,
            Button::Maximize => 2,
            Button::Close => 1,
        };
        let x = right - from_right * i64::from(width);
        let rect = Rect {
            x: x.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
            y: 0,
            width,
            height: bar_height,
        };
        (button, rect)
    })
}

/// The window button at window pixel `x`, `y`, if any.
pub fn button_at(buttons: &[(Button, Rect)], x: f64, y: f64) -> Option<Button> {
    buttons
        .iter()
        .find(|(_, rect)| rect.contains(x, y))
        .map(|&(button, _)| button)
}

/// The first bar column, of `cell_width` pixels after `padding`, that the
/// window `buttons` cover even in part: the bar's text stops before it.
pub fn first_button_col(buttons: &[(Button, Rect)], padding: u32, cell_width: u32) -> u16 {
    let left = buttons.iter().map(|(_, rect)| rect.x).min().unwrap_or(0);
    let room = i64::from(left) - i64::from(padding);
    let col = room.max(0) / i64::from(cell_width.max(1));
    col.min(i64::from(u16::MAX)) as u16
}

/// Side of the glyph box and stroke width, in pixels, of the glyphs on a
/// button `button_height` pixels tall at `scale`: 10 logical pixels on a
/// 32-pixel bar, growing with the bar, and a stroke a tenth of the side
/// (at least one pixel), so the glyphs keep their proportions.
pub fn glyph_metrics(button_height: u32, scale: f64) -> (u32, u32) {
    let scale = appearance::valid_scale(scale);
    let side = (GLYPH_SIDE * scale)
        .max(f64::from(button_height) * GLYPH_SIDE / BUTTON_HEIGHT)
        .round()
        .max(1.0) as u32;
    let stroke = (f64::from(side) / GLYPH_SIDE).round().max(1.0) as u32;
    (side, stroke)
}

/// The strokes of `button`'s glyph in a `side`-pixel box (`0..side` on
/// both axes), drawn `stroke` pixels wide: a line for minimize, a square
/// for maximize, two overlapping squares for restore (`maximized`) and an
/// `X` for close. Lines sit on the pixel grid (pixel centers for odd
/// strokes, pixel edges for even ones) and inside the box, so they come
/// out crisp.
pub fn glyph(button: Button, maximized: bool, side: u32, stroke: u32) -> Vec<Segment> {
    let side = side as f32;
    let half = stroke as f32 / 2.0;
    // Near and far edges of the box, at the middle of the stroke.
    let (near, far) = (half, side - half);
    let line = |from, to| Segment { from, to };
    let square = |left: f32, top: f32, right: f32, bottom: f32| {
        [
            line((left, top), (right, top)),
            line((right, top), (right, bottom)),
            line((right, bottom), (left, bottom)),
            line((left, bottom), (left, top)),
        ]
    };
    match button {
        Button::Minimize => {
            let middle = if stroke % 2 == 1 {
                (side / 2.0).floor() + 0.5
            } else {
                (side / 2.0).round()
            };
            vec![line((near, middle), (far, middle))]
        }
        Button::Maximize if maximized => {
            // The back square peeks out above and right of the front one.
            let offset = (side * 0.2).round().max(stroke as f32 + 1.0);
            let mut lines = square(near, near + offset, far - offset, far).to_vec();
            lines.extend([
                line((near + offset, near), (far, near)),
                line((far, near), (far, far - offset)),
                line((near + offset, near), (near + offset, near + offset)),
                line((far - offset, far - offset), (far, far - offset)),
            ]);
            lines
        }
        Button::Maximize => square(near, near, far, far).to_vec(),
        Button::Close => vec![
            line((near, near), (far, far)),
            line((far, near), (near, far)),
        ],
    }
}

/// Colors of the window buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonColors {
    /// The glyphs.
    pub glyph: Rgb,
    /// The background of a hovered button (close turns red instead).
    pub hover: Rgb,
}

/// The window `buttons` (see [`button_rects`]) as shapes: a background for
/// the `hover`ed one (red under a white glyph for close) and each glyph
/// (see [`glyph`]) centered in its button. `maximized` picks the restore
/// glyph.
pub fn button_shapes(
    buttons: &[(Button, Rect)],
    hover: Option<Button>,
    maximized: bool,
    scale: f64,
    colors: ButtonColors,
) -> Vec<Shape> {
    let mut shapes = Vec::new();
    for &(button, rect) in buttons {
        let hovered = hover == Some(button);
        let (background, color) = match button {
            Button::Close if hovered => (Some(CLOSE_HOVER), CLOSE_HOVER_GLYPH),
            _ if hovered => (Some(colors.hover), colors.glyph),
            _ => (None, colors.glyph),
        };
        if let Some(color) = background {
            shapes.push(Shape::Rect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
                color,
            });
        }
        let (side, stroke) = glyph_metrics(rect.height, scale);
        // A margin as wide as the stroke keeps the caps of the `X` inside.
        let margin = stroke as f32;
        let segments: Vec<Segment> = glyph(button, maximized, side, stroke)
            .into_iter()
            .map(|Segment { from, to }| Segment {
                from: (from.0 + margin, from.1 + margin),
                to: (to.0 + margin, to.1 + margin),
            })
            .collect();
        let size = side + 2 * stroke;
        let mask = Mask::stroke(size, size, &segments, stroke as f32);
        let (box_x, box_y) = (
            rect.x + rect.width.saturating_sub(side) as i32 / 2,
            rect.y + rect.height.saturating_sub(side) as i32 / 2,
        );
        shapes.push(Shape::Mask {
            x: box_x - stroke as i32,
            y: box_y - stroke as i32,
            mask: Arc::new(mask),
            color,
        });
    }
    shapes
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

    /// Window buttons over the last 15 columns of a 40-column bar.
    const INTEGRATED: Chrome = Chrome {
        inset: 0,
        new_tab: true,
        buttons: Some(25),
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
    fn integrated_buttons_keep_the_right_end_and_new_tab_follows_the_labels() {
        let bar = layout(&["zsh", "vim"], 0, 40, INTEGRATED);
        assert_eq!(bar.buttons, Some(25));
        assert_eq!(bar.new_tab, Some(16));
        assert_eq!(bar.region_at(7), Region::Tab(0));
        assert_eq!(bar.region_at(8), Region::Tab(1));
        assert_eq!(bar.region_at(16), Region::NewTab);
        assert_eq!(bar.region_at(18), Region::NewTab);
        assert_eq!(bar.region_at(19), Region::Drag);
        // Under the buttons, found by pixel instead.
        assert_eq!(bar.region_at(25), Region::Drag);
        assert_eq!(bar.region_at(u16::MAX), Region::Drag);
        let wide = Chrome {
            buttons: Some(500),
            ..INTEGRATED
        };
        assert_eq!(layout(&["zsh"], 0, 40, wide).buttons, Some(40), "clamped");
    }

    #[test]
    fn labels_share_what_the_inset_and_the_buttons_leave() {
        let chrome = Chrome {
            inset: 4,
            ..INTEGRATED
        };
        // 30 columns: 15 under the buttons, 4 inset, 3 for `+`: 8 left.
        let chrome = Chrome {
            buttons: Some(15),
            ..chrome
        };
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
        let chrome = Chrome {
            buttons: Some(0),
            ..INTEGRATED
        };
        let bar = layout(&["zsh"], 0, 10, chrome);
        assert!(bar.labels.is_empty());
        assert_eq!(bar.new_tab, None);
        assert_eq!(bar.buttons, Some(0));
        assert_eq!(text(&render(&bar, None)), " ".repeat(10));
    }

    #[test]
    fn the_bar_shows_the_active_label_inverse_and_the_others_dimmed() {
        let bar = layout(&["a", "b"], 1, 12, Chrome::NATIVE);
        let drawn = render(&bar, None);
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
        assert_eq!(text(&render(&bar, None)), " 1: ab ");
    }

    #[test]
    fn the_new_tab_button_is_highlighted_under_the_pointer() {
        let bar = layout(&["sh"], 0, 40, INTEGRATED);
        let plain = render(&bar, None);
        assert_eq!(text(&plain), format!(" 1: sh  + {}", " ".repeat(30)));
        assert_eq!(plain.display_row(0)[8].fg, Color::Indexed(8), "dim +");
        let new_tab = render(&bar, Some(Region::NewTab));
        assert_eq!(new_tab.display_row(0)[7].bg, Color::Indexed(8));
        let button = render(&bar, Some(Region::Button(Button::Close)));
        assert_eq!(text(&button), text(&plain), "buttons are not text");
    }

    fn rects(buttons: &[(Button, Rect)]) -> Vec<(i32, i32, u32, u32)> {
        buttons
            .iter()
            .map(|(_, r)| (r.x, r.y, r.width, r.height))
            .collect()
    }

    #[test]
    fn buttons_sit_flush_with_the_top_right_corner_at_any_scale() {
        let buttons = button_rects(1000, 32, 1.0);
        let order: Vec<Button> = buttons.iter().map(|&(b, _)| b).collect();
        assert_eq!(order, BUTTONS);
        assert_eq!(
            rects(&buttons),
            [(862, 0, 46, 32), (908, 0, 46, 32), (954, 0, 46, 32)]
        );
        // HiDPI: twice the pixels for a bar twice as tall.
        assert_eq!(
            rects(&button_rects(2000, 64, 2.0)),
            [(1724, 0, 92, 64), (1816, 0, 92, 64), (1908, 0, 92, 64)]
        );
        // A bar shorter than 32 logical pixels keeps 46-pixel-wide buttons.
        assert_eq!(rects(&button_rects(1000, 26, 1.0))[2], (954, 0, 46, 26));
        // A taller bar (larger font) makes the buttons wider too.
        assert_eq!(rects(&button_rects(1000, 48, 1.0))[2], (931, 0, 69, 48));
        assert_eq!(rects(&button_rects(1000, 32, f64::NAN))[2].2, 46);
    }

    #[test]
    fn buttons_are_found_by_pixel() {
        let buttons = button_rects(1000, 32, 1.0);
        assert_eq!(button_at(&buttons, 861.9, 10.0), None);
        assert_eq!(button_at(&buttons, 862.0, 0.0), Some(Button::Minimize));
        assert_eq!(button_at(&buttons, 930.0, 31.9), Some(Button::Maximize));
        assert_eq!(button_at(&buttons, 999.9, 5.0), Some(Button::Close));
        assert_eq!(button_at(&buttons, 970.0, 32.0), None, "below the bar");
    }

    #[test]
    fn bar_text_stops_at_the_first_column_under_a_button() {
        let buttons = button_rects(1000, 32, 1.0);
        // Minimize starts at 862: (862 - 8) / 10 = column 85, in part.
        assert_eq!(first_button_col(&buttons, 8, 10), 85);
        assert_eq!(first_button_col(&button_rects(100, 32, 1.0), 8, 10), 0);
        assert_eq!(first_button_col(&buttons, 8, 0), 854);
    }

    #[test]
    fn glyphs_grow_with_the_scale_and_the_bar() {
        assert_eq!(glyph_metrics(32, 1.0), (10, 1));
        assert_eq!(glyph_metrics(64, 2.0), (20, 2));
        assert_eq!(glyph_metrics(24, 1.0), (10, 1), "at least 10 logical");
        assert_eq!(glyph_metrics(26, 2.0), (20, 2));
        assert_eq!(glyph_metrics(96, 1.0), (30, 3), "a tall bar");
        assert_eq!(glyph_metrics(48, 1.5), (15, 2));
    }

    /// The glyph rasterized at `side` and `stroke`, as rows of `#` (full
    /// coverage), `+` (partial) and `.` (none).
    fn draw(button: Button, maximized: bool, side: u32, stroke: u32) -> Vec<String> {
        let mask = Mask::stroke(
            side,
            side,
            &glyph(button, maximized, side, stroke),
            stroke as f32,
        );
        mask.coverage()
            .chunks(side as usize)
            .map(|row| {
                row.iter()
                    .map(|&a| match a {
                        0 => '.',
                        255 => '#',
                        _ => '+',
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn minimize_and_maximize_are_crisp_at_scale_1() {
        let minimize = draw(Button::Minimize, false, 10, 1);
        assert_eq!(minimize[5], "##########");
        assert!(
            minimize
                .iter()
                .enumerate()
                .all(|(y, row)| y == 5 || row == "..........")
        );
        assert_eq!(
            draw(Button::Maximize, false, 10, 1),
            [
                "##########",
                "#........#",
                "#........#",
                "#........#",
                "#........#",
                "#........#",
                "#........#",
                "#........#",
                "#........#",
                "##########",
            ]
        );
    }

    #[test]
    fn restore_is_two_overlapping_squares() {
        assert_eq!(
            draw(Button::Maximize, true, 10, 1),
            [
                "..########",
                "..#......#",
                "########.#",
                "#......#.#",
                "#......#.#",
                "#......#.#",
                "#......#.#",
                "#......###",
                "#......#..",
                "########..",
            ]
        );
    }

    #[test]
    fn glyphs_at_scale_2_keep_their_shape_with_double_strokes() {
        let maximize = draw(Button::Maximize, false, 20, 2);
        assert_eq!(maximize[0], "#".repeat(20));
        assert_eq!(maximize[1], "#".repeat(20));
        assert_eq!(maximize[2], format!("##{}##", ".".repeat(16)));
        assert_eq!(maximize[19], "#".repeat(20));
        let minimize = draw(Button::Minimize, false, 20, 2);
        let inked: Vec<usize> = (0..20).filter(|&y| minimize[y].contains('#')).collect();
        assert_eq!(inked, [9, 10]);
        assert!(minimize.iter().all(|row| !row.contains('+')), "crisp");
    }

    #[test]
    fn close_is_an_anti_aliased_x() {
        let close = draw(Button::Close, false, 10, 1);
        assert!(close.iter().any(|row| row.contains('+')), "{close:?}");
        for i in 0..10 {
            let row = close[i].as_bytes();
            assert_ne!(row[i], b'.', "falling diagonal at {i}");
            assert_ne!(row[9 - i], b'.', "rising diagonal at {i}");
        }
        assert_eq!(&close[0][4..6], "..", "open top");
    }

    const COLORS: ButtonColors = ButtonColors {
        glyph: rgb(200, 200, 200),
        hover: rgb(60, 60, 60),
    };

    #[test]
    fn button_shapes_center_the_glyphs_and_show_the_hover() {
        let buttons = button_rects(1000, 32, 1.0);
        let shapes = button_shapes(&buttons, None, false, 1.0, COLORS);
        assert_eq!(shapes.len(), 3, "glyphs only");
        let Shape::Mask { x, y, mask, color } = &shapes[2] else {
            panic!("close glyph: {shapes:?}");
        };
        // The 10-pixel box at 954 + 18, 11, with a 1-pixel margin.
        assert_eq!((*x, *y, mask.width(), mask.height()), (971, 10, 12, 12));
        assert_eq!(*color, COLORS.glyph);

        let hovered = button_shapes(&buttons, Some(Button::Close), false, 1.0, COLORS);
        assert_eq!(
            hovered[2],
            Shape::Rect {
                x: 954,
                y: 0,
                width: 46,
                height: 32,
                color: CLOSE_HOVER
            }
        );
        let Shape::Mask { color, .. } = &hovered[3] else {
            panic!("close glyph: {hovered:?}");
        };
        assert_eq!(*color, CLOSE_HOVER_GLYPH);
        let minimize = button_shapes(&buttons, Some(Button::Minimize), false, 1.0, COLORS);
        assert!(matches!(minimize[0], Shape::Rect { x: 862, color, .. } if color == COLORS.hover));
    }

    #[test]
    fn button_shapes_scale_with_the_display() {
        let at = |scale: f64, bar: u32, maximized: bool| {
            let buttons = button_rects(2000, bar, scale);
            let shapes = button_shapes(&buttons, None, maximized, scale, COLORS);
            match &shapes[1] {
                Shape::Mask { x, y, mask, .. } => (*x, *y, mask.width()),
                other => panic!("maximize glyph: {other:?}"),
            }
        };
        // 20-pixel box with a 2-pixel margin, centered in 92 x 64 at 1816.
        assert_eq!(at(2.0, 64, false), (1816 + 36 - 2, 22 - 2, 24));
        assert_eq!(at(1.0, 32, false).2, 12);
        let (normal, restore) = (at(2.0, 64, false), at(2.0, 64, true));
        assert_eq!(normal, restore, "same box when maximized");
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
