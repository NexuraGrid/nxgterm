//! Rate limit for pty resizes during a divider drag: every motion event
//! changes the layout, but the shell only hears of it every 30 ms, with the
//! last size always delivered. Time is passed in, so tests need no clock.

use std::time::{Duration, Instant};

/// Shortest time between two values going out.
pub const INTERVAL: Duration = Duration::from_millis(30);

/// Lets one value through per [`INTERVAL`]; the ones in between wait, and
/// only the last of them goes out.
#[derive(Debug)]
pub struct Throttle<T> {
    sent: Option<Instant>,
    waiting: Option<T>,
}

impl<T> Default for Throttle<T> {
    fn default() -> Self {
        Self {
            sent: None,
            waiting: None,
        }
    }
}

impl<T> Throttle<T> {
    /// Offers `value` at `now`: it comes back when it may go out, else it
    /// waits (replacing the one already waiting).
    pub fn push(&mut self, value: T, now: Instant) -> Option<T> {
        if self.due(now) {
            self.waiting = None;
            self.sent = Some(now);
            Some(value)
        } else {
            self.waiting = Some(value);
            None
        }
    }

    /// The waiting value, once its turn has come.
    pub fn poll(&mut self, now: Instant) -> Option<T> {
        if self.waiting.is_some() && self.due(now) {
            self.sent = Some(now);
            return self.waiting.take();
        }
        None
    }

    /// The waiting value now, turn or not.
    pub fn flush(&mut self) -> Option<T> {
        self.waiting.take()
    }

    /// Forgets the waiting value, as when a newer size went out directly.
    pub fn clear(&mut self) {
        self.waiting = None;
    }

    /// When the waiting value may go out; `None` without one.
    pub fn deadline(&self) -> Option<Instant> {
        self.waiting.as_ref()?;
        self.sent.map(|sent| sent + INTERVAL)
    }

    fn due(&self, now: Instant) -> bool {
        self.sent
            .is_none_or(|sent| now.saturating_duration_since(sent) >= INTERVAL)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    fn ms(base: Instant, n: u64) -> Instant {
        base + Duration::from_millis(n)
    }

    #[test]
    fn the_first_value_goes_out_at_once() {
        let (mut throttle, t0) = (Throttle::default(), Instant::now());
        assert_eq!(throttle.push(1, t0), Some(1));
        assert_eq!(throttle.deadline(), None, "nothing waits");
    }

    #[test]
    fn values_inside_the_interval_coalesce_into_the_last_one() {
        let (mut throttle, t0) = (Throttle::default(), Instant::now());
        assert_eq!(throttle.push(1, t0), Some(1));
        assert_eq!(throttle.push(2, ms(t0, 5)), None);
        assert_eq!(throttle.push(3, ms(t0, 10)), None);
        assert_eq!(throttle.deadline(), Some(ms(t0, 30)));
        assert_eq!(throttle.poll(ms(t0, 29)), None, "not yet");
        assert_eq!(throttle.poll(ms(t0, 30)), Some(3), "trailing edge");
        assert_eq!(throttle.poll(ms(t0, 60)), None, "delivered once");
    }

    #[test]
    fn a_value_after_a_quiet_interval_goes_out_at_once() {
        let (mut throttle, t0) = (Throttle::default(), Instant::now());
        throttle.push(1, t0);
        assert_eq!(throttle.push(2, ms(t0, 30)), Some(2));
    }

    #[test]
    fn a_steady_drag_sends_about_thirty_three_times_a_second() {
        let (mut throttle, t0) = (Throttle::default(), Instant::now());
        let mut sent = Vec::new();
        for step in 0..=200_u64 {
            let now = ms(t0, step * 5);
            sent.extend(throttle.push(step, now));
            sent.extend(throttle.poll(now));
        }
        // One second of motion every 5 ms, then the button is released.
        sent.extend(throttle.flush());
        assert!((33..=35).contains(&sent.len()), "{} sends", sent.len());
        assert_eq!(sent.last(), Some(&200), "the final value is delivered");
    }

    #[test]
    fn flush_delivers_the_waiting_value_whatever_the_time() {
        let (mut throttle, t0) = (Throttle::default(), Instant::now());
        throttle.push(1, t0);
        throttle.push(2, ms(t0, 1));
        assert_eq!(throttle.flush(), Some(2));
        assert_eq!(throttle.flush(), None);
        assert_eq!(throttle.poll(ms(t0, 100)), None);
    }

    #[test]
    fn clear_drops_the_waiting_value() {
        let (mut throttle, t0) = (Throttle::default(), Instant::now());
        throttle.push(1, t0);
        throttle.push(2, ms(t0, 1));
        throttle.clear();
        assert_eq!(throttle.flush(), None);
    }
}
