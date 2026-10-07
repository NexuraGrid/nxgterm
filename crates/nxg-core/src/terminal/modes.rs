//! Private (DEC) modes: `CSI ? Pm h` / `CSI ? Pm l`.

use super::State;

impl State {
    pub fn set_private_mode(&mut self, mode: u16, on: bool) {
        match mode {
            6 => self.set_origin_mode(on),
            25 => self.cursor_visible = on,
            // DECSDM: set disables sixel scrolling.
            80 => self.sixel_scrolling = !on,
            47 | 1047 if on => self.enter_alt(false),
            47 => self.leave_alt(true),
            1047 => self.leave_alt(false),
            1048 if on => self.save_cursor(),
            1048 => self.restore_cursor(),
            1049 if on => {
                // Only the first entry saves; a repeat would overwrite the
                // main cursor with a position from the alternate screen.
                if !self.alt_active {
                    self.save_cursor();
                }
                self.enter_alt(true);
            }
            1049 if self.alt_active => {
                self.leave_alt(false);
                self.restore_cursor();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use crate::TermSize;
    use crate::grid::Region;

    #[test]
    fn unknown_private_modes_change_nothing() {
        let mut t = term(5, 2);
        t.advance(b"ab\x1b[?9999h\x1b[?9999lc");
        assert_eq!(text(&t, 0), "abc");
        assert!(!t.state.alt_active);
    }

    #[test]
    fn quitting_a_full_screen_app_restores_the_shell() {
        let mut t = term(10, 4);
        t.advance(b"$ ls\r\nabc");
        t.advance(b"\x1b[?1049hhello");
        assert_eq!(text(&t, 0), "", "the alternate screen starts blank");
        assert_eq!(text(&t, 1), "   hello");
        t.advance(b"\x1b[?1049l");
        assert_eq!(text(&t, 0), "$ ls");
        assert_eq!(text(&t, 1), "abc");
        assert_eq!(pos(&t), (3, 1));
    }

    #[test]
    fn reentering_the_alternate_screen_is_a_no_op() {
        let mut t = term(10, 4);
        t.advance(b"abc\x1b[?1049h\x1b[2;1HX\x1b[?1049h");
        assert_eq!(text(&t, 1), "X", "content survives the second h");
        t.advance(b"\x1b[?1049l");
        assert_eq!(
            pos(&t),
            (3, 0),
            "the save was not redone from the alt cursor"
        );
    }

    #[test]
    fn leaving_while_on_main_does_not_restore_a_stale_save() {
        let mut t = term(10, 4);
        t.advance(b"\x1b[3;3H\x1b7\x1b[1;1H\x1b[?1049l");
        assert_eq!(pos(&t), (0, 0));
    }

    #[test]
    fn mode_47_keeps_the_alternate_content() {
        let mut t = term(5, 2);
        t.advance(b"\x1b[?47hX\x1b[?47l");
        assert_eq!(text(&t, 0), "");
        t.advance(b"\x1b[?47h");
        assert_eq!(text(&t, 0), "X");
    }

    #[test]
    fn mode_1047_clears_the_alternate_screen_on_leave() {
        let mut t = term(5, 2);
        t.advance(b"main\x1b[?1047h\x1b[HX\x1b[?1047l");
        assert_eq!(text(&t, 0), "main");
        t.advance(b"\x1b[?1047h");
        assert_eq!(text(&t, 0), "");
    }

    #[test]
    fn mode_1049_always_enters_a_blank_alternate_screen() {
        let mut t = term(5, 2);
        t.advance(b"\x1b[?1049hX\x1b[?1049l\x1b[?1049h");
        assert_eq!(text(&t, 0), "");
    }

    #[test]
    fn modes_47_and_1047_carry_the_cursor_across_the_switch() {
        for mode in ["47", "1047"] {
            let mut t = term(10, 5);
            t.advance(format!("ab\x1b[?{mode}h").as_bytes());
            assert_eq!(pos(&t), (2, 0), "{mode}: position follows into alt");
            t.advance(format!("\x1b[4;5H\x1b[?{mode}l").as_bytes());
            assert_eq!(pos(&t), (4, 3), "{mode}: and back to main");
        }
    }

    #[test]
    fn modes_47_and_1047_carry_the_pending_wrap() {
        let mut t = term(3, 2);
        t.advance(b"abc\x1b[?47hd");
        assert_eq!(
            text(&t, 1),
            "d",
            "the alt screen wrapped on the first print"
        );
        assert_eq!(text(&t, 0), "");
    }

    #[test]
    fn mode_1048_saves_and_restores_the_cursor() {
        let mut t = term(10, 5);
        t.advance(b"\x1b[2;3H\x1b[?1048h\x1b[1;1H");
        t.advance(b"\x1b[?1048l");
        assert_eq!(pos(&t), (2, 1));
    }

    #[test]
    fn saved_cursors_are_per_screen() {
        let mut t = term(10, 8);
        t.advance(b"\x1b[3;3H\x1b7\x1b[?1049h\x1b[6;6H\x1b7\x1b[1;1H\x1b8");
        assert_eq!(pos(&t), (5, 5));
        t.advance(b"\x1b[?1049l");
        assert_eq!(pos(&t), (2, 2));
    }

    #[test]
    fn the_alternate_screen_has_its_own_empty_save_slot() {
        let mut t = term(10, 8);
        t.advance(b"\x1b[3;3H\x1b7\x1b[?47h\x1b[6;6H\x1b8");
        assert_eq!(pos(&t), (0, 0), "main's save is not visible from alt");
        t.advance(b"\x1b[?47l\x1b8");
        assert_eq!(pos(&t), (2, 2));
    }

    #[test]
    fn cursor_visibility_is_shared_between_screens() {
        let mut t = term(5, 2);
        t.advance(b"\x1b[?1049h\x1b[?25l\x1b[?1049l");
        assert!(!t.cursor().visible);
    }

    #[test]
    fn the_arriving_alternate_screen_starts_with_full_region_and_origin_off() {
        let mut t = term(5, 4);
        t.advance(b"\x1b[?47h");
        t.state.screen.origin = true;
        t.state.screen.region = Region { top: 1, bottom: 2 };
        t.advance(b"\x1b[?47l");
        t.state.screen.origin = true;
        t.advance(b"\x1b[?47h");
        assert!(!t.state.screen.origin);
        assert!(t.state.screen.region.is_full(4));
        t.advance(b"\x1b[?47l");
        assert!(t.state.screen.origin, "main's origin is back");
    }

    #[test]
    fn resize_applies_to_the_active_and_the_dormant_screen() {
        let mut t = term(10, 5);
        t.advance(b"hello\x1b[5;10H\x1b[?1049h\x1b[5;10H");
        let small = TermSize::new(3, 2).unwrap();
        t.resize(small);
        assert_eq!(t.state.screen.grid.size(), small);
        assert_eq!(t.state.dormant.as_ref().unwrap().grid.size(), small);
        assert_eq!(pos(&t), (2, 1));
        t.advance(b"\x1b[?1049l");
        assert_eq!(text(&t, 0), "hel");
        assert_eq!(pos(&t), (2, 1), "the saved cursor was clamped too");
    }

    #[test]
    fn main_placements_hide_in_the_alternate_screen_and_return() {
        let mut t = sized(10, 5);
        t.advance(&kitty_rgba(1, 10, 20, ""));
        assert_eq!(t.images().placements().len(), 1);
        t.advance(b"\x1b[?1049h");
        assert!(t.images().placements().is_empty());
        t.advance(b"\x1b[?1049l");
        assert_eq!(t.images().placements().len(), 1);
        assert_eq!(t.images().placements()[0].row, 0);
    }

    #[test]
    fn placements_made_on_the_alternate_screen_are_dropped_on_leave() {
        let mut t = sized(10, 5);
        t.advance(&kitty_rgba(1, 10, 20, ""));
        t.advance(b"\x1b[?1049h");
        t.advance(&kitty_rgba(2, 10, 20, ""));
        assert_eq!(t.images().placements().len(), 1);
        t.advance(b"\x1b[?1049l");
        let ids: Vec<u32> = t
            .images()
            .placements()
            .iter()
            .filter_map(|p| t.images().image(p.image).map(|i| i.id))
            .collect();
        assert_eq!(ids, [1]);
    }

    #[test]
    fn deleting_an_image_in_the_alternate_screen_leaves_no_stashed_placement() {
        let mut t = sized(10, 5);
        t.advance(&kitty_rgba(5, 10, 20, ""));
        t.advance(b"\x1b[?1049h\x1b_Ga=d,d=I,i=5\x1b\\\x1b[?1049l");
        assert!(t.images().placements().is_empty());
        assert!(t.images().by_id(5).is_none());
    }

    #[test]
    fn a_vim_style_session_leaves_the_shell_untouched() {
        let mut t = term(20, 5);
        t.advance(b"$ vim file\r\n$ ");
        t.advance(b"\x1b[?1049h\x1b[?25l\x1b[2J\x1b[H~\r\n~\r\n~\x1b[1;1H\x1b[?25h");
        assert_eq!(text(&t, 0), "~");
        t.advance(b"\x1b[?1049l");
        assert_eq!(text(&t, 0), "$ vim file");
        assert_eq!(text(&t, 1), "$");
        assert_eq!(pos(&t), (2, 1));
        assert!(t.cursor().visible);
    }
}
