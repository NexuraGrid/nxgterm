//! Synchronized output (mode 2026): while an app on screen is mid-frame the
//! window holds its redraws, so a TUI never shows half an update. A frame
//! goes out anyway after [`TIMEOUT`], in case the app never ends it. Time is
//! passed in, so tests need no clock.

use std::time::{Duration, Instant};

/// Longest a redraw waits for an app to finish its frame.
pub const TIMEOUT: Duration = Duration::from_secs(1);

/// The redraw held back by synchronized output, if any.
#[derive(Debug, Default)]
pub struct Hold {
    /// When the held frame began.
    since: Option<Instant>,
}

impl Hold {
    /// Output from a pane on screen was parsed at `now`, leaving its mode
    /// `synchronized`; returns whether to redraw now. A hold already running
    /// keeps its start, so a stream of updates cannot push the frame back.
    pub fn output(&mut self, synchronized: bool, now: Instant) -> bool {
        if synchronized {
            self.since.get_or_insert(now);
            false
        } else {
            self.since = None;
            true
        }
    }

    /// Whether the held frame timed out at `now`; it is then released, and
    /// the caller draws it.
    pub fn poll(&mut self, now: Instant) -> bool {
        let expired = self.deadline().is_some_and(|due| now >= due);
        if expired {
            self.since = None;
        }
        expired
    }

    /// When the held frame is drawn anyway; `None` without one.
    pub fn deadline(&self) -> Option<Instant> {
        self.since.map(|since| since + TIMEOUT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(base: Instant, n: u64) -> Instant {
        base + Duration::from_millis(n)
    }

    #[test]
    fn plain_output_redraws_and_holds_nothing() {
        let mut hold = Hold::default();
        assert!(hold.output(false, Instant::now()));
        assert_eq!(hold.deadline(), None);
    }

    #[test]
    fn synchronized_output_holds_until_the_mode_ends() {
        let base = Instant::now();
        let mut hold = Hold::default();
        assert!(!hold.output(true, base));
        assert!(!hold.output(true, ms(base, 10)));
        assert!(
            hold.output(false, ms(base, 20)),
            "the reset draws the frame"
        );
        assert_eq!(hold.deadline(), None);
    }

    #[test]
    fn the_deadline_counts_from_the_first_held_output() {
        let base = Instant::now();
        let mut hold = Hold::default();
        hold.output(true, base);
        hold.output(true, ms(base, 500));
        assert_eq!(hold.deadline(), Some(base + TIMEOUT));
    }

    #[test]
    fn a_frame_never_ended_is_drawn_after_the_timeout() {
        let base = Instant::now();
        let mut hold = Hold::default();
        hold.output(true, base);
        assert!(!hold.poll(ms(base, 999)));
        assert!(hold.poll(base + TIMEOUT));
        assert_eq!(hold.deadline(), None, "released once drawn");
        assert!(!hold.poll(ms(base, 2000)));
        assert!(
            !hold.output(true, ms(base, 2000)),
            "a later frame holds again"
        );
    }

    #[test]
    fn polling_without_a_hold_draws_nothing() {
        assert!(!Hold::default().poll(Instant::now()));
    }
}
