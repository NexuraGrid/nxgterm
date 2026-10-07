//! Mouse reporting: the modes an application sets and the bytes a mouse
//! event becomes under them.

/// Which mouse events the application asked for (DECSET 1000/1002/1003).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MouseTracking {
    #[default]
    Off,
    /// 1000: presses and releases.
    Click,
    /// 1002: also motion while a button is held.
    Drag,
    /// 1003: also any motion.
    Motion,
}

impl MouseTracking {
    /// Whether `action` is reported; `button_held` matters only for motion.
    pub fn reports(self, action: MouseAction, button_held: bool) -> bool {
        match (self, action) {
            (Self::Off, _) => false,
            (_, MouseAction::Press | MouseAction::Release) => true,
            (Self::Click, MouseAction::Motion) => false,
            (Self::Drag, MouseAction::Motion) => button_held,
            (Self::Motion, MouseAction::Motion) => true,
        }
    }
}

/// How reports are encoded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MouseEncoding {
    /// `CSI M` and three bytes offset by 32; cells past 223 cannot be sent.
    #[default]
    X10,
    /// 1006: `CSI < b ; x ; y M` (press) or `m` (release).
    Sgr,
    /// 1015: `CSI b ; x ; y M`, with the X10 button code in decimal.
    Urxvt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    /// Motion with no button held.
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Release,
    Motion,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MouseMods {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

/// A mouse event over cell (`col`, `row`), zero-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseEvent {
    pub button: MouseButton,
    pub action: MouseAction,
    pub col: u16,
    pub row: u16,
    pub mods: MouseMods,
}

/// The report for `event`, or `None` when it has none: wheel releases, and
/// X10 cells that do not fit in a byte.
pub fn encode(event: MouseEvent, encoding: MouseEncoding) -> Option<Vec<u8>> {
    let wheel = matches!(event.button, MouseButton::WheelUp | MouseButton::WheelDown);
    if wheel && event.action == MouseAction::Release {
        return None;
    }
    let mut button: u32 = match event.button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::None => 3,
        MouseButton::WheelUp => 64,
        MouseButton::WheelDown => 65,
    };
    // Only SGR says which button was released.
    if event.action == MouseAction::Release && encoding != MouseEncoding::Sgr {
        button = 3;
    }
    if event.action == MouseAction::Motion {
        button += 32;
    }
    let mods = event.mods;
    button += 4 * u32::from(mods.shift) + 8 * u32::from(mods.alt) + 16 * u32::from(mods.ctrl);
    let (x, y) = (u32::from(event.col) + 1, u32::from(event.row) + 1);
    match encoding {
        MouseEncoding::Sgr => {
            let end = if event.action == MouseAction::Release {
                'm'
            } else {
                'M'
            };
            Some(format!("\x1b[<{button};{x};{y}{end}").into_bytes())
        }
        MouseEncoding::Urxvt => Some(format!("\x1b[{};{x};{y}M", button + 32).into_bytes()),
        MouseEncoding::X10 => {
            let byte = |v: u32| u8::try_from(v + 32).ok();
            Some(vec![0x1b, b'[', b'M', byte(button)?, byte(x)?, byte(y)?])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(button: MouseButton, action: MouseAction, col: u16, row: u16) -> MouseEvent {
        MouseEvent {
            button,
            action,
            col,
            row,
            mods: MouseMods::default(),
        }
    }

    fn sgr(e: MouseEvent) -> String {
        String::from_utf8(encode(e, MouseEncoding::Sgr).unwrap()).unwrap()
    }

    fn x10(e: MouseEvent) -> Option<Vec<u8>> {
        encode(e, MouseEncoding::X10)
    }

    #[test]
    fn sgr_reports_presses_and_releases_with_one_based_cells() {
        let press = event(MouseButton::Left, MouseAction::Press, 0, 0);
        assert_eq!(sgr(press), "\x1b[<0;1;1M");
        let release = event(MouseButton::Right, MouseAction::Release, 9, 4);
        assert_eq!(sgr(release), "\x1b[<2;10;5m", "SGR keeps the button");
        let middle = event(MouseButton::Middle, MouseAction::Press, 299, 0);
        assert_eq!(sgr(middle), "\x1b[<1;300;1M", "no coordinate limit");
    }

    #[test]
    fn wheel_buttons_are_64_and_65_and_never_release() {
        let up = event(MouseButton::WheelUp, MouseAction::Press, 2, 3);
        assert_eq!(sgr(up), "\x1b[<64;3;4M");
        let down = event(MouseButton::WheelDown, MouseAction::Press, 2, 3);
        assert_eq!(sgr(down), "\x1b[<65;3;4M");
        let release = event(MouseButton::WheelUp, MouseAction::Release, 2, 3);
        assert_eq!(encode(release, MouseEncoding::Sgr), None);
    }

    #[test]
    fn modifiers_add_4_8_and_16() {
        let mut e = event(MouseButton::Left, MouseAction::Press, 0, 0);
        e.mods.shift = true;
        assert_eq!(sgr(e), "\x1b[<4;1;1M");
        e.mods.alt = true;
        assert_eq!(sgr(e), "\x1b[<12;1;1M");
        e.mods.ctrl = true;
        assert_eq!(sgr(e), "\x1b[<28;1;1M");
    }

    #[test]
    fn motion_adds_32_and_no_button_is_3() {
        let drag = event(MouseButton::Left, MouseAction::Motion, 1, 1);
        assert_eq!(sgr(drag), "\x1b[<32;2;2M");
        let hover = event(MouseButton::None, MouseAction::Motion, 1, 1);
        assert_eq!(sgr(hover), "\x1b[<35;2;2M");
    }

    #[test]
    fn x10_offsets_everything_by_32_and_releases_as_button_3() {
        let press = event(MouseButton::Left, MouseAction::Press, 0, 0);
        assert_eq!(x10(press).unwrap(), b"\x1b[M\x20\x21\x21");
        let release = event(MouseButton::Left, MouseAction::Release, 9, 4);
        assert_eq!(x10(release).unwrap(), b"\x1b[M\x23\x2a\x25");
        let wheel = event(MouseButton::WheelDown, MouseAction::Press, 0, 0);
        assert_eq!(x10(wheel).unwrap(), b"\x1b[M\x61\x21\x21");
    }

    #[test]
    fn x10_cannot_encode_cells_past_223() {
        let last = event(MouseButton::Left, MouseAction::Press, 222, 222);
        assert_eq!(x10(last).unwrap(), b"\x1b[M\x20\xff\xff");
        let beyond = event(MouseButton::Left, MouseAction::Press, 223, 0);
        assert_eq!(x10(beyond), None);
        let below = event(MouseButton::Left, MouseAction::Press, 0, 223);
        assert_eq!(x10(below), None);
    }

    #[test]
    fn urxvt_is_decimal_with_the_x10_button_code() {
        let release = event(MouseButton::Left, MouseAction::Release, 299, 0);
        let bytes = encode(release, MouseEncoding::Urxvt).unwrap();
        assert_eq!(bytes, b"\x1b[35;300;1M");
    }

    #[test]
    fn tracking_modes_decide_which_events_are_reported() {
        use MouseAction::*;
        let off = MouseTracking::Off;
        assert!(!off.reports(Press, false));
        let click = MouseTracking::Click;
        assert!(click.reports(Press, false) && click.reports(Release, false));
        assert!(!click.reports(Motion, true));
        let drag = MouseTracking::Drag;
        assert!(drag.reports(Motion, true));
        assert!(!drag.reports(Motion, false), "1002 needs a held button");
        let motion = MouseTracking::Motion;
        assert!(motion.reports(Motion, false));
    }
}
