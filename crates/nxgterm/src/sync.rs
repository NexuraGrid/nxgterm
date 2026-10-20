//! Synchronized output (mode 2026): while an app on screen is mid-frame its
//! pane holds the redraws it would cause, so a TUI never shows half an
//! update. Each pane holds on its own: output of another pane neither ends
//! nor delays it. A frame goes out anyway after [`TIMEOUT`], in case the app
//! never ends it; the caller then ends the mode on that pane's terminal, so
//! a stuck app does not keep rendering at one frame a second. Time is passed
//! in, so tests need no clock.
//!
//! A redraw caused by something else (another pane, a key, a resize) draws
//! a held pane as it is: the renderer keeps no snapshot of the last whole
//! frame.

use std::collections::HashMap;
use std::hash::Hash;
use std::time::{Duration, Instant};

/// Longest a redraw waits for an app to finish its frame.
pub const TIMEOUT: Duration = Duration::from_secs(1);

/// The redraws held back by synchronized output, per pane `K`.
#[derive(Debug)]
pub struct Holds<K> {
    /// When each held frame began.
    since: HashMap<K, Instant>,
}

impl<K> Default for Holds<K> {
    fn default() -> Self {
        Self {
            since: HashMap::new(),
        }
    }
}

impl<K: Copy + Eq + Hash> Holds<K> {
    /// Output from `pane`, on screen, was parsed at `now`, leaving its mode
    /// `synchronized`, with `frame_ended` if a frame ended during the parse
    /// (perhaps followed by the next one); returns whether to redraw now.
    /// A hold already running keeps its start, so a stream of updates
    /// cannot push the frame back; an ended frame is drawn and the next one
    /// held from `now`.
    pub fn output(&mut self, pane: K, synchronized: bool, frame_ended: bool, now: Instant) -> bool {
        if !synchronized {
            self.since.remove(&pane);
            true
        } else if frame_ended {
            self.since.insert(pane, now);
            true
        } else {
            self.since.entry(pane).or_insert(now);
            false
        }
    }

    /// Forgets the hold of `pane` (closed, or no longer on screen).
    pub fn release(&mut self, pane: K) {
        self.since.remove(&pane);
    }

    /// Forgets every hold.
    pub fn clear(&mut self) {
        self.since.clear();
    }

    /// The panes whose held frame timed out at `now`; they are released,
    /// and the caller ends their mode and draws them.
    pub fn expired(&mut self, now: Instant) -> Vec<K> {
        let expired: Vec<K> = self
            .since
            .iter()
            .filter(|(_, since)| now >= **since + TIMEOUT)
            .map(|(pane, _)| *pane)
            .collect();
        for pane in &expired {
            self.since.remove(pane);
        }
        expired
    }

    /// When the earliest held frame is drawn anyway; `None` without one.
    pub fn deadline(&self) -> Option<Instant> {
        self.since.values().min().map(|since| *since + TIMEOUT)
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
        let mut holds = Holds::default();
        assert!(holds.output(1, false, false, Instant::now()));
        assert_eq!(holds.deadline(), None);
    }

    #[test]
    fn synchronized_output_holds_until_the_mode_ends() {
        let base = Instant::now();
        let mut holds = Holds::default();
        assert!(!holds.output(1, true, false, base));
        assert!(!holds.output(1, true, false, ms(base, 10)));
        assert!(
            holds.output(1, false, true, ms(base, 20)),
            "the reset draws the frame"
        );
        assert_eq!(holds.deadline(), None);
    }

    #[test]
    fn a_frame_that_ended_mid_batch_is_drawn_and_the_next_one_held_anew() {
        let base = Instant::now();
        let mut holds = Holds::default();
        holds.output(1, true, false, base);
        assert!(
            holds.output(1, true, true, ms(base, 900)),
            "reset and set again in one batch still draws"
        );
        assert_eq!(holds.deadline(), Some(ms(base, 900) + TIMEOUT));
    }

    #[test]
    fn the_deadline_counts_from_the_first_held_output() {
        let base = Instant::now();
        let mut holds = Holds::default();
        holds.output(1, true, false, base);
        holds.output(1, true, false, ms(base, 500));
        assert_eq!(holds.deadline(), Some(base + TIMEOUT));
    }

    #[test]
    fn another_pane_neither_ends_nor_delays_a_hold() {
        let base = Instant::now();
        let mut holds = Holds::default();
        holds.output(1, true, false, base);
        assert!(holds.output(2, false, false, ms(base, 10)), "pane 2 draws");
        assert_eq!(holds.deadline(), Some(base + TIMEOUT), "pane 1 still held");
        holds.output(2, true, false, ms(base, 300));
        assert_eq!(holds.deadline(), Some(base + TIMEOUT), "the earliest");
        assert!(holds.output(1, false, true, ms(base, 400)), "its own reset");
        assert_eq!(holds.deadline(), Some(ms(base, 300) + TIMEOUT));
    }

    #[test]
    fn a_frame_never_ended_is_drawn_after_the_timeout() {
        let base = Instant::now();
        let mut holds = Holds::default();
        holds.output(1, true, false, base);
        holds.output(2, true, false, ms(base, 500));
        assert!(holds.expired(ms(base, 999)).is_empty());
        assert_eq!(holds.expired(base + TIMEOUT), [1]);
        assert_eq!(holds.deadline(), Some(ms(base, 500) + TIMEOUT));
        assert_eq!(holds.expired(ms(base, 2000)), [2]);
        assert_eq!(holds.deadline(), None, "released once drawn");
        assert!(
            !holds.output(1, true, false, ms(base, 2000)),
            "a later frame holds again"
        );
    }

    #[test]
    fn a_released_pane_holds_nothing() {
        let mut holds = Holds::default();
        holds.output(1, true, false, Instant::now());
        holds.release(1);
        assert_eq!(holds.deadline(), None);
        assert!(holds.expired(Instant::now() + TIMEOUT).is_empty());
    }
}
