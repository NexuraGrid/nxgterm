//! Child input written off the UI thread: keys, pastes and terminal
//! replies go through a channel to a per-pane writer thread, so a child
//! that stops reading blocks that thread, not every tab.

use std::io::{self, Write};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

/// Pastes are queued in pieces of at most this many bytes, written (and
/// flushed) one after the other in order with any later input.
pub const CHUNK: usize = 64 * 1024;

/// The sending end of a pane's writer thread. Dropping it ends the thread
/// once everything queued is written; a failed write ends it at once and
/// later input is dropped (the reader reports the child's exit).
pub struct Input {
    queue: Sender<Vec<u8>>,
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "detached; only tests wait for it")
    )]
    thread: JoinHandle<()>,
}

impl Input {
    /// Starts a thread called `name` writing to `writer`.
    pub fn spawn(name: String, writer: Box<dyn Write + Send>) -> io::Result<Self> {
        let (queue, received) = mpsc::channel::<Vec<u8>>();
        let thread = thread::Builder::new().name(name).spawn(move || {
            let mut writer = writer;
            for bytes in received {
                if writer
                    .write_all(&bytes)
                    .and_then(|()| writer.flush())
                    .is_err()
                {
                    break;
                }
            }
        })?;
        Ok(Self { queue, thread })
    }

    /// Queues `bytes` for the child, after any input queued before. Never
    /// blocks.
    pub fn send(&self, bytes: &[u8]) {
        for chunk in bytes.chunks(CHUNK) {
            if self.queue.send(chunk.to_vec()).is_err() {
                return; // The thread ended: the child is gone.
            }
        }
    }

    /// Stops queueing and waits until everything queued was written.
    #[cfg(test)]
    pub fn close(self) {
        let Self { queue, thread } = self;
        drop(queue);
        let _ = thread.join();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::Receiver;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use super::*;

    /// Records every write, and how long each one was.
    #[derive(Clone, Default)]
    struct Recorder {
        bytes: Arc<Mutex<Vec<u8>>>,
        writes: Arc<Mutex<Vec<usize>>>,
    }

    impl Write for Recorder {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.bytes.lock().unwrap().extend_from_slice(buf);
            self.writes.lock().unwrap().push(buf.len());
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn input_reaches_the_writer_in_order() {
        let recorder = Recorder::default();
        let input = Input::spawn("test".into(), Box::new(recorder.clone())).unwrap();
        let paste: Vec<u8> = (0..CHUNK * 3 + 7).map(|i| (i % 251) as u8).collect();
        input.send(b"echo ");
        input.send(&paste);
        input.send(b"\r");
        input.close();
        let mut expected = b"echo ".to_vec();
        expected.extend_from_slice(&paste);
        expected.push(b'\r');
        assert_eq!(*recorder.bytes.lock().unwrap(), expected);
        let writes = recorder.writes.lock().unwrap();
        assert!(writes.iter().all(|&len| len <= CHUNK), "{writes:?}");
        assert_eq!(writes.len(), 6, "the paste in four pieces");
    }

    /// Blocks every write until the test lets it through.
    struct Gate(Receiver<()>, Recorder);

    impl Write for Gate {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.recv().map_err(|_| io::ErrorKind::BrokenPipe)?;
            self.1.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_child_that_stops_reading_does_not_block_sending() {
        let (open, gate) = mpsc::channel();
        let recorder = Recorder::default();
        let input = Input::spawn("test".into(), Box::new(Gate(gate, recorder.clone()))).unwrap();
        let start = Instant::now();
        for _ in 0..100 {
            input.send(&[b'x'; 1024]);
        }
        assert!(start.elapsed() < Duration::from_secs(5), "send never waits");
        assert!(recorder.bytes.lock().unwrap().is_empty(), "still blocked");
        for _ in 0..100 {
            open.send(()).unwrap();
        }
        input.close();
        assert_eq!(recorder.bytes.lock().unwrap().len(), 100 * 1024);
    }

    /// Fails every write.
    struct Broken;

    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_failed_write_ends_the_thread_quietly() {
        let input = Input::spawn("test".into(), Box::new(Broken)).unwrap();
        input.send(b"a");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !input.thread.is_finished() {
            assert!(Instant::now() < deadline, "the thread did not end");
            thread::yield_now();
        }
        input.send(b"b"); // Dropped, no panic.
        input.close();
    }
}
