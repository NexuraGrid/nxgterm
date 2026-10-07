//! Helpers shared by the terminal tests.

use super::{State, Terminal};
use crate::grid::Region;
use crate::{CellPixels, TermSize};

impl State {
    /// Panics when the terminal is in a state no input should be able to
    /// reach. The fuzz tests call it after every step.
    pub(super) fn assert_invariants(&self) {
        let size = self.screen.grid.size();
        let screens = std::iter::once(&self.screen).chain(self.dormant.as_deref());
        for screen in screens {
            let (cols, rows) = (size.cols(), size.rows());
            assert_eq!(
                screen.grid.size(),
                size,
                "grid size differs between screens"
            );
            assert!(
                screen.col < cols && screen.row < rows,
                "cursor ({}, {}) outside {cols}x{rows}",
                screen.col,
                screen.row
            );
            let region = screen.region;
            assert!(
                region.is_full(rows) || (region.top < region.bottom && region.bottom < rows),
                "invalid region {region:?} for {rows} rows"
            );
            if let Some(saved) = screen.saved {
                assert!(
                    saved.col < cols && saved.row < rows,
                    "saved cursor ({}, {}) outside {cols}x{rows}",
                    saved.col,
                    saved.row
                );
            }
        }
        // Only the active screen: a kept dormant one may hold a stale flag
        // that the next switch overwrites.
        assert!(
            self.autowrap || !self.screen.wrap_pending,
            "wrap pending while autowrap is off"
        );
        let stored = |key: u64| self.images.image(key).is_some();
        for p in self.images.placements() {
            assert!(stored(p.image), "placement references a missing image");
        }
        for p in self.images.stashed() {
            assert!(
                stored(p.image),
                "stashed placement references a missing image"
            );
        }
        let _ = Region::full(0);
    }
}

pub(super) fn term(cols: u16, rows: u16) -> Terminal {
    Terminal::new(TermSize::new(cols, rows).unwrap())
}

/// A terminal that also knows its cell pixel size (needed by images).
pub(super) fn sized(cols: u16, rows: u16) -> Terminal {
    let mut t = term(cols, rows);
    t.set_cell_pixels(10, 20);
    t
}

pub(super) fn text(term: &Terminal, row: u16) -> String {
    let line: String = term.row(row).iter().map(|c| c.ch).collect();
    line.trim_end().to_owned()
}

pub(super) fn pos(term: &Terminal) -> (u16, u16) {
    let c = term.cursor();
    (c.col, c.row)
}

/// A kitty graphics command that transmits and places an RGBA image.
pub(super) fn kitty_rgba(id: u32, w: u32, h: u32, extra: &str) -> Vec<u8> {
    let data = crate::image::decode::tests::encode_base64(&[255; 4].repeat((w * h) as usize));
    format!("\x1b_Ga=T,f=32,s={w},v={h},i={id}{extra};{data}\x1b\\").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sized_builds_the_requested_size_with_cell_pixels() {
        let t = sized(5, 3);
        assert_eq!(t.size(), TermSize::new(5, 3).unwrap());
        assert_eq!(t.cell_pixels(), CellPixels::new(10, 20));
    }

    /// Runs `corrupt` on a healthy terminal that has both screens, a saved
    /// cursor and placements on each, and returns the invariant failure text.
    fn failure_after(corrupt: impl FnOnce(&mut Terminal)) -> String {
        let mut t = sized(6, 4);
        t.advance(&kitty_rgba(1, 10, 20, ""));
        t.advance(b"\x1b[2;3r\x1b7\x1b[?1049h");
        t.advance(&kitty_rgba(2, 10, 20, ""));
        t.advance(b"\x1b7");
        t.state.assert_invariants();
        corrupt(&mut t);
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            t.state.assert_invariants();
        }));
        let payload = caught.expect_err("the corruption must be detected");
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .unwrap_or_default()
    }

    #[test]
    fn invariants_hold_for_a_healthy_terminal_with_both_screens() {
        let mut t = sized(6, 4);
        t.advance(b"abc\x1b[2;3r\x1b7\x1b[?1049hxyz\x1b[?1049l");
        t.state.assert_invariants();
        t.advance(b"\x1b[?47h");
        t.state.assert_invariants();
    }

    #[test]
    fn invariants_catch_a_cursor_outside_the_grid() {
        assert!(failure_after(|t| t.state.screen.col = 6).contains("cursor"));
        assert!(
            failure_after(|t| t.state.dormant.as_mut().unwrap().row = 4).contains("cursor"),
            "the dormant screen is checked too"
        );
    }

    #[test]
    fn invariants_catch_an_invalid_region() {
        let bad = |top, bottom| {
            move |t: &mut Terminal| {
                t.state.screen.region = crate::grid::Region { top, bottom };
            }
        };
        assert!(failure_after(bad(2, 2)).contains("region"));
        assert!(failure_after(bad(1, 4)).contains("region"));
    }

    #[test]
    fn invariants_catch_a_saved_cursor_outside_the_grid() {
        let msg = failure_after(|t| t.state.screen.saved.as_mut().unwrap().col = 9);
        assert!(msg.contains("saved cursor"));
    }

    #[test]
    fn invariants_catch_a_grid_that_does_not_match_the_other_screen() {
        let msg = failure_after(|t| {
            let other = TermSize::new(5, 4).unwrap();
            t.state.dormant.as_mut().unwrap().grid.resize(other);
        });
        assert!(msg.contains("grid size"));
    }

    #[test]
    fn invariants_catch_placements_of_missing_images_on_both_lists() {
        // The alternate placement is active, the main one is stashed.
        let msg = failure_after(|t| t.state.images.drop_image_keeping_placements(2));
        assert!(msg.contains("placement"), "{msg}");
        let msg = failure_after(|t| t.state.images.drop_image_keeping_placements(1));
        assert!(msg.contains("stashed placement"), "{msg}");
    }

    #[test]
    fn invariants_catch_a_pending_wrap_with_autowrap_off() {
        let msg = failure_after(|t| {
            t.state.autowrap = false;
            t.state.screen.wrap_pending = true;
        });
        assert!(msg.contains("wrap"));
    }

    #[test]
    fn text_trims_trailing_blanks_only() {
        let mut t = term(6, 1);
        t.advance(b"  ab  ");
        assert_eq!(text(&t, 0), "  ab");
        assert_eq!(pos(&t), (5, 0));
    }
}
