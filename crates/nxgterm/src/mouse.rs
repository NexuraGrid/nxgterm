//! Mouse input: what the wheel, buttons and pointer motion do, as pure
//! functions of the terminal modes so the event loop only forwards.

use nxg_core::mouse::{self, MouseAction, MouseButton, MouseEvent, MouseMods, MouseTracking};
use nxg_core::{Modes, TermSize};
use nxg_render::{CellSize, Layout};
use winit::dpi::PhysicalPosition;
use winit::event::MouseScrollDelta;
use winit::keyboard::ModifiersState;

use crate::bindings::Action;

/// Lines one wheel notch scrolls.
pub const LINES_PER_NOTCH: u32 = 3;

/// What a wheel movement of some lines does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WheelAction {
    /// The application tracks the mouse: send `count` wheel events.
    Report { button: MouseButton, count: u32 },
    /// Alternate scroll: send these arrow keys to the application.
    Keys(Vec<u8>),
    /// Move the viewport this many lines back (negative: forward).
    Scroll(i32),
}

/// Decides what `lines` of wheel movement (positive: up, away from the
/// user) do. Mouse reporting comes first unless Shift is held; then the
/// alternate screen gets arrow keys if alternate scroll (1007) is on;
/// otherwise the main screen scrolls its history.
pub fn wheel_action(modes: Modes, shift: bool, lines: i32) -> Option<WheelAction> {
    if lines == 0 {
        return None;
    }
    let up = lines > 0;
    let count = lines.unsigned_abs();
    if modes.mouse_tracking != MouseTracking::Off && !shift {
        let button = if up {
            MouseButton::WheelUp
        } else {
            MouseButton::WheelDown
        };
        // One event per notch, as a mouse wheel sends; a partial notch
        // from a touchpad still counts as one.
        let count = count.div_ceil(LINES_PER_NOTCH);
        return Some(WheelAction::Report { button, count });
    }
    if modes.alt_screen {
        if !modes.alternate_scroll {
            return None;
        }
        let arrow: &[u8] = match (up, modes.app_cursor_keys) {
            (true, true) => b"\x1bOA",
            (true, false) => b"\x1b[A",
            (false, true) => b"\x1bOB",
            (false, false) => b"\x1b[B",
        };
        return Some(WheelAction::Keys(arrow.repeat(count as usize)));
    }
    Some(WheelAction::Scroll(lines))
}

/// Turns wheel deltas into whole lines, carrying fractions (touchpad
/// pixels, fractional notches) over to the next event.
#[derive(Debug, Default)]
pub struct Wheel {
    pending: f64,
}

impl Wheel {
    /// Whole lines for `delta`; pixels are counted in rows of
    /// `cell_height`.
    pub fn lines(&mut self, delta: MouseScrollDelta, cell_height: u32) -> i32 {
        self.pending += match delta {
            MouseScrollDelta::LineDelta(_, y) => f64::from(y) * f64::from(LINES_PER_NOTCH),
            MouseScrollDelta::PixelDelta(PhysicalPosition { y, .. }) => {
                y / f64::from(cell_height.max(1))
            }
        };
        let whole = self.pending.trunc();
        self.pending -= whole;
        whole as i32
    }
}

/// A viewport movement requested from the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewportScroll {
    /// Lines back into the history (negative: forward).
    Lines(i32),
    Top,
    Bottom,
}

/// The viewport movement for a scroll `action`, by a page of `rows` or
/// to the oldest line or the live screen. `None` for other actions, and on
/// the alternate screen, which has no history: there the key belongs to
/// the application.
pub fn viewport_scroll(action: Action, modes: Modes, rows: u16) -> Option<ViewportScroll> {
    if modes.alt_screen {
        return None;
    }
    let page = i32::from(rows);
    match action {
        Action::ScrollPageUp => Some(ViewportScroll::Lines(page)),
        Action::ScrollPageDown => Some(ViewportScroll::Lines(-page)),
        Action::ScrollToTop => Some(ViewportScroll::Top),
        Action::ScrollToBottom => Some(ViewportScroll::Bottom),
        _ => None,
    }
}

/// The cell under window position (`x`, `y`), clamped to the grid.
pub fn cell_at(layout: Layout, size: TermSize, x: f64, y: f64) -> (u16, u16) {
    let CellSize { width, height } = layout.cell;
    let index = |pos: f64, origin: u32, cell: u32, count: u16| {
        let i = ((pos - f64::from(origin)) / f64::from(cell.max(1))).floor();
        i.clamp(0.0, f64::from(count - 1)) as u16
    };
    let (left, top) = layout.origin(0, 0);
    (
        index(x, left, width, size.cols()),
        index(y, top, height, size.rows()),
    )
}

/// The bytes to send for a button or motion event, if the application
/// asked for it. Shift is kept for the terminal, as in xterm.
pub fn button_report(
    modes: Modes,
    shift: bool,
    event: MouseEvent,
    button_held: bool,
) -> Option<Vec<u8>> {
    if shift || !modes.mouse_tracking.reports(event.action, button_held) {
        return None;
    }
    mouse::encode(event, modes.mouse_encoding)
}

pub fn mouse_mods(mods: ModifiersState) -> MouseMods {
    MouseMods {
        shift: mods.shift_key(),
        alt: mods.alt_key(),
        ctrl: mods.control_key(),
    }
}

/// The reportable button for a winit one; others are not reported.
pub fn button(button: winit::event::MouseButton) -> Option<MouseButton> {
    match button {
        winit::event::MouseButton::Left => Some(MouseButton::Left),
        winit::event::MouseButton::Middle => Some(MouseButton::Middle),
        winit::event::MouseButton::Right => Some(MouseButton::Right),
        _ => None,
    }
}

/// Press or release.
pub fn action(state: winit::event::ElementState) -> MouseAction {
    match state {
        winit::event::ElementState::Pressed => MouseAction::Press,
        winit::event::ElementState::Released => MouseAction::Release,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nxg_core::mouse::{MouseEncoding, MouseTracking};

    fn main() -> Modes {
        Modes::default()
    }

    fn alt() -> Modes {
        Modes {
            alt_screen: true,
            ..Modes::default()
        }
    }

    fn reporting() -> Modes {
        Modes {
            mouse_tracking: MouseTracking::Click,
            mouse_encoding: MouseEncoding::Sgr,
            ..Modes::default()
        }
    }

    #[test]
    fn the_main_screen_wheel_scrolls_the_viewport() {
        assert_eq!(wheel_action(main(), false, 3), Some(WheelAction::Scroll(3)));
        assert_eq!(
            wheel_action(main(), false, -6),
            Some(WheelAction::Scroll(-6))
        );
        assert_eq!(wheel_action(main(), false, 0), None);
    }

    #[test]
    fn mouse_reporting_gets_one_wheel_event_per_notch() {
        let up = wheel_action(reporting(), false, 3);
        assert_eq!(
            up,
            Some(WheelAction::Report {
                button: MouseButton::WheelUp,
                count: 1
            })
        );
        let down = wheel_action(reporting(), false, -4);
        assert_eq!(
            down,
            Some(WheelAction::Report {
                button: MouseButton::WheelDown,
                count: 2
            }),
            "a partial notch still reports"
        );
    }

    #[test]
    fn shift_bypasses_mouse_reporting() {
        assert_eq!(
            wheel_action(reporting(), true, 3),
            Some(WheelAction::Scroll(3))
        );
    }

    #[test]
    fn the_alternate_screen_wheel_sends_arrows_per_line() {
        let up = wheel_action(alt(), false, 3);
        assert_eq!(up, Some(WheelAction::Keys(b"\x1b[A\x1b[A\x1b[A".to_vec())));
        let app = Modes {
            app_cursor_keys: true,
            ..alt()
        };
        let down = wheel_action(app, false, -2);
        assert_eq!(down, Some(WheelAction::Keys(b"\x1bOB\x1bOB".to_vec())));
    }

    #[test]
    fn the_alternate_screen_without_alternate_scroll_ignores_the_wheel() {
        let modes = Modes {
            alternate_scroll: false,
            ..alt()
        };
        assert_eq!(wheel_action(modes, false, 3), None);
    }

    #[test]
    fn reporting_wins_over_alternate_scroll() {
        let modes = Modes {
            alt_screen: true,
            ..reporting()
        };
        assert!(matches!(
            wheel_action(modes, false, 3),
            Some(WheelAction::Report { .. })
        ));
    }

    #[test]
    fn line_deltas_are_three_lines_per_notch() {
        let mut wheel = Wheel::default();
        assert_eq!(wheel.lines(MouseScrollDelta::LineDelta(0.0, 1.0), 20), 3);
        assert_eq!(wheel.lines(MouseScrollDelta::LineDelta(0.0, -2.0), 20), -6);
    }

    #[test]
    fn pixel_deltas_accumulate_into_whole_lines() {
        let mut wheel = Wheel::default();
        let px = |y| MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, y));
        assert_eq!(wheel.lines(px(15.0), 20), 0);
        assert_eq!(wheel.lines(px(15.0), 20), 1, "30 px is one 20 px line");
        assert_eq!(wheel.lines(px(-50.0), 20), -2, "the 10 px left count");
    }

    #[test]
    fn scroll_actions_move_the_main_screen_viewport() {
        let scroll = |action| viewport_scroll(action, main(), 24);
        assert_eq!(
            scroll(Action::ScrollPageUp),
            Some(ViewportScroll::Lines(24))
        );
        assert_eq!(
            scroll(Action::ScrollPageDown),
            Some(ViewportScroll::Lines(-24))
        );
        assert_eq!(scroll(Action::ScrollToTop), Some(ViewportScroll::Top));
        assert_eq!(scroll(Action::ScrollToBottom), Some(ViewportScroll::Bottom));
        assert_eq!(scroll(Action::ZoomIn), None, "not a scroll action");
        let on_alt = viewport_scroll(Action::ScrollPageUp, alt(), 24);
        assert_eq!(on_alt, None, "the alternate screen has no history");
    }

    #[test]
    fn pointer_positions_map_to_clamped_cells() {
        let layout = Layout {
            cell: CellSize {
                width: 10,
                height: 20,
            },
            padding: 5,
            left: 0,
            top: 0,
        };
        let size = TermSize::new(4, 3).unwrap();
        assert_eq!(cell_at(layout, size, 5.0, 5.0), (0, 0));
        assert_eq!(cell_at(layout, size, 24.9, 45.0), (1, 2));
        assert_eq!(cell_at(layout, size, -3.0, 999.0), (0, 2), "clamped");
        assert_eq!(cell_at(layout, size, 999.0, 0.0), (3, 0));
        // Below a one-row tab bar the grid starts a row lower.
        let below = layout.below(1);
        assert_eq!(cell_at(below, size, 5.0, 25.0), (0, 0));
        assert_eq!(cell_at(below, size, 5.0, 45.0), (0, 1));
    }

    fn click(action: MouseAction) -> MouseEvent {
        MouseEvent {
            button: MouseButton::Left,
            action,
            col: 1,
            row: 2,
            mods: MouseMods::default(),
        }
    }

    #[test]
    fn button_reports_follow_the_tracking_mode() {
        let press = click(MouseAction::Press);
        assert_eq!(
            button_report(reporting(), false, press, true).unwrap(),
            b"\x1b[<0;2;3M"
        );
        assert_eq!(
            button_report(main(), false, press, true),
            None,
            "no tracking"
        );
        assert_eq!(
            button_report(reporting(), true, press, true),
            None,
            "shift is kept for the terminal"
        );
        let drag = click(MouseAction::Motion);
        assert_eq!(
            button_report(reporting(), false, drag, true),
            None,
            "1000 ignores motion"
        );
        let drag_mode = Modes {
            mouse_tracking: MouseTracking::Drag,
            ..reporting()
        };
        assert_eq!(
            button_report(drag_mode, false, drag, true).unwrap(),
            b"\x1b[<32;2;3M"
        );
    }

    #[test]
    fn modifiers_translate_to_mouse_modifiers() {
        let mods = ModifiersState::SHIFT | ModifiersState::CONTROL;
        assert_eq!(
            mouse_mods(mods),
            MouseMods {
                shift: true,
                alt: false,
                ctrl: true
            }
        );
    }
}
