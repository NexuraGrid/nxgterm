//! The output a pane's reader thread hands to the UI thread. Reads pile up
//! in one shared buffer and only the first read after a drain wakes the UI,
//! so a flood (`yes`, `cat bigfile`) costs one event per drain instead of
//! one per read. Past [`CAP`] pending bytes the reader waits, which blocks
//! the child on its writes; a drain parses at most [`BUDGET`] bytes, so keys
//! and redraws get their turn between drains.

use std::mem;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

/// Pending bytes at which the reader waits for the UI to drain.
pub const CAP: usize = 4 << 20;

/// Most bytes one drain hands out for parsing.
pub const BUDGET: usize = 1 << 20;

/// What a [`Producer::push`] asks of the reader.
#[derive(Debug, PartialEq, Eq)]
pub enum Push {
    /// First bytes since the last full drain: post one wakeup to the UI.
    Wake,
    /// A wakeup is already on its way; nothing to post.
    Queued,
    /// The pane is gone: stop reading.
    Closed,
}

/// The reader thread's end.
pub struct Producer(Arc<Shared>);

/// The UI thread's end; dropping it (with its pane) releases the reader.
pub struct Consumer {
    shared: Arc<Shared>,
    /// The bytes of the last drain; swapped with the pending buffer, so the
    /// two allocations take turns instead of growing anew.
    drained: Vec<u8>,
}

struct Shared {
    state: Mutex<State>,
    /// Signalled when bytes are drained or the consumer goes away.
    space: Condvar,
    cap: usize,
}

struct State {
    pending: Vec<u8>,
    /// A wakeup was posted and its drain has not emptied the buffer yet.
    wakeup: bool,
    /// The consumer was dropped.
    closed: bool,
}

/// A connected pair whose reader waits past `cap` pending bytes.
pub fn channel(cap: usize) -> (Producer, Consumer) {
    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            pending: Vec::new(),
            wakeup: false,
            closed: false,
        }),
        space: Condvar::new(),
        cap,
    });
    let consumer = Consumer {
        shared: shared.clone(),
        drained: Vec::new(),
    };
    (Producer(shared), consumer)
}

impl Shared {
    /// The state, even if a panic poisoned the lock: every update to it is
    /// a single step, so it is never left half done.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Producer {
    /// Queues `bytes`, first waiting while the buffer is full (one read
    /// may overshoot the cap, so pending stays under cap + one read).
    pub fn push(&self, bytes: &[u8]) -> Push {
        let mut state = self.0.lock();
        while state.pending.len() >= self.0.cap && !state.closed {
            state = self
                .0
                .space
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        if state.closed {
            return Push::Closed;
        }
        state.pending.extend_from_slice(bytes);
        if mem::replace(&mut state.wakeup, true) {
            Push::Queued
        } else {
            Push::Wake
        }
    }
}

impl Consumer {
    /// Takes up to `budget` pending bytes and whether more remain. While
    /// more remain the wakeup stays pending: the reader posts none, and the
    /// caller must post one itself to come back for the rest.
    pub fn drain(&mut self, budget: usize) -> (&[u8], bool) {
        self.drained.clear();
        // One huge read may have grown a buffer; do not keep it forever.
        if self.drained.capacity() > 2 * self.shared.cap {
            self.drained.shrink_to(self.shared.cap);
        }
        let more = {
            let mut state = self.shared.lock();
            let more = state.pending.len() > budget;
            if more {
                self.drained.extend_from_slice(&state.pending[..budget]);
                state.pending.drain(..budget);
            } else {
                mem::swap(&mut self.drained, &mut state.pending);
                state.wakeup = false;
            }
            more
        };
        self.shared.space.notify_all();
        (&self.drained, more)
    }

    /// Bytes waiting to be drained.
    #[cfg(test)]
    fn pending(&self) -> usize {
        self.shared.lock().pending.len()
    }
}

impl Drop for Consumer {
    fn drop(&mut self) {
        self.shared.lock().closed = true;
        self.shared.space.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use super::*;

    /// Long enough to tell a blocked producer from a slow one.
    const BLOCKED: Duration = Duration::from_millis(100);
    /// Generous bound for a producer that was released.
    const RELEASED: Duration = Duration::from_secs(5);

    #[test]
    fn only_the_first_push_after_a_drain_wakes() {
        let (producer, mut consumer) = channel(1024);
        assert_eq!(producer.push(b"ab"), Push::Wake);
        assert_eq!(producer.push(b"cd"), Push::Queued);
        assert_eq!(consumer.drain(BUDGET), (&b"abcd"[..], false));
        assert_eq!(producer.push(b"e"), Push::Wake, "the drain emptied it");
        assert_eq!(producer.push(b"f"), Push::Queued);
    }

    #[test]
    fn a_drain_stops_at_the_budget_and_keeps_the_wakeup() {
        let (producer, mut consumer) = channel(1024);
        producer.push(b"abcdefg");
        assert_eq!(consumer.drain(3), (&b"abc"[..], true));
        assert_eq!(producer.push(b"h"), Push::Queued, "the caller re-posts");
        assert_eq!(consumer.drain(3), (&b"def"[..], true));
        assert_eq!(consumer.drain(3), (&b"gh"[..], false));
        assert_eq!(consumer.drain(3), (&b""[..], false));
        assert_eq!(producer.push(b"i"), Push::Wake);
    }

    #[test]
    fn a_full_buffer_blocks_the_producer_until_drained() {
        let (producer, mut consumer) = channel(4);
        assert_eq!(producer.push(b"abcd"), Push::Wake);
        let (tx, rx) = mpsc::channel();
        let reader = thread::spawn(move || {
            tx.send(producer.push(b"e")).unwrap();
            producer
        });
        assert!(rx.recv_timeout(BLOCKED).is_err(), "waits at the cap");
        assert_eq!(consumer.drain(2), (&b"ab"[..], true));
        assert_eq!(rx.recv_timeout(RELEASED), Ok(Push::Queued));
        let producer = reader.join().unwrap();
        assert_eq!(consumer.drain(BUDGET), (&b"cde"[..], false));
        assert_eq!(producer.push(b"f"), Push::Wake);
    }

    #[test]
    fn dropping_the_consumer_releases_a_waiting_producer() {
        let (producer, consumer) = channel(2);
        producer.push(b"ab");
        let (tx, rx) = mpsc::channel();
        let reader = thread::spawn(move || {
            tx.send(producer.push(b"c")).unwrap();
            producer
        });
        assert!(rx.recv_timeout(BLOCKED).is_err(), "waits at the cap");
        drop(consumer);
        assert_eq!(rx.recv_timeout(RELEASED), Ok(Push::Closed));
        let producer = reader.join().unwrap();
        assert_eq!(producer.push(b"d"), Push::Closed, "even below the cap");
    }

    #[test]
    fn the_two_buffers_take_turns() {
        let (producer, mut consumer) = channel(1024);
        let mut pointers = Vec::new();
        for _ in 0..4 {
            producer.push(b"abc");
            pointers.push(consumer.drain(BUDGET).0.as_ptr());
        }
        assert_ne!(pointers[0], pointers[1]);
        assert_eq!(pointers[0], pointers[2]);
        assert_eq!(pointers[1], pointers[3]);
    }

    #[test]
    fn an_oversized_buffer_is_shrunk() {
        let (producer, mut consumer) = channel(8);
        producer.push(&[0; 100]);
        assert_eq!(consumer.drain(BUDGET).0.len(), 100);
        // Its buffer goes back as the pending one, shrunk.
        producer.push(b"a");
        consumer.drain(BUDGET);
        producer.push(b"b");
        consumer.drain(BUDGET);
        assert!(consumer.drained.capacity() <= 16);
    }

    /// A flood through a small cap arrives whole and in order, with pending
    /// bytes bounded by the cap plus one read. The channel stands in for the
    /// event loop: `Some` is a wakeup, `None` the exit that follows EOF.
    #[test]
    fn a_flood_arrives_whole_with_bounded_pending() {
        const CHUNK: usize = 64 * 1024;
        const TOTAL: usize = 50 << 20;
        const CAP: usize = 256 * 1024;
        let (producer, mut consumer) = channel(CAP);
        let (events, rx) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut sent = 0;
            while sent < TOTAL {
                let chunk: Vec<u8> = (sent..sent + CHUNK).map(|i| (i % 251) as u8).collect();
                if producer.push(&chunk) == Push::Wake {
                    events.send(Some(())).unwrap();
                }
                sent += CHUNK;
            }
            events.send(None).unwrap();
        });
        let mut received = 0;
        let mut max_pending = 0;
        while let Some(()) = rx.recv_timeout(RELEASED).unwrap() {
            loop {
                max_pending = max_pending.max(consumer.pending());
                let (bytes, more) = consumer.drain(BUDGET / 8);
                assert!(bytes.len() <= BUDGET / 8);
                for (i, &byte) in bytes.iter().enumerate() {
                    assert_eq!(byte, ((received + i) % 251) as u8);
                }
                received += bytes.len();
                if !more {
                    break;
                }
            }
        }
        reader.join().unwrap();
        assert_eq!(received, TOTAL);
        assert!(max_pending <= CAP + CHUNK, "pending reached {max_pending}");
    }
}
