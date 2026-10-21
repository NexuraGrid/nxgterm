//! Child input written off the UI thread: keys, pastes and terminal
//! replies go through a channel to a per-pane writer thread, so a child
//! that stops reading blocks that thread, not every tab. The queue is
//! bounded: input that does not fit is dropped whole (see [`Input::send`]).

use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

/// Pastes are queued in pieces of at most this many bytes, written (and
/// flushed) one after the other in order with any later input.
pub const CHUNK: usize = 64 * 1024;

/// The most input queued for a child and not yet written.
pub const LIMIT: usize = 8 * 1024 * 1024;

/// Input [`Input::send`] did not queue: the child has not read what was
/// queued before and the queue is full.
#[derive(Debug, PartialEq, Eq)]
pub struct Dropped;

/// The sending end of a pane's writer thread. Dropping it ends the thread
/// once everything queued is written; a failed write is reported once,
/// ends it at once and later input is dropped (the reader reports the
/// child's exit).
pub struct Input {
    queue: Sender<Vec<u8>>,
    /// Bytes queued and not yet written; only the sender adds to it.
    pending: Arc<AtomicUsize>,
    limit: usize,
    /// Detached; only tests wait for it.
    thread: JoinHandle<()>,
}

impl Input {
    /// Starts a thread called `name` writing to `writer`, holding up to
    /// [`LIMIT`] bytes; a failed write is passed to `failed`.
    pub fn spawn(
        name: String,
        writer: Box<dyn Write + Send>,
        failed: impl FnOnce(io::Error) + Send + 'static,
    ) -> io::Result<Self> {
        Self::spawn_bounded(name, writer, LIMIT, failed)
    }

    /// [`Input::spawn`] holding up to `limit` bytes.
    pub fn spawn_bounded(
        name: String,
        writer: Box<dyn Write + Send>,
        limit: usize,
        failed: impl FnOnce(io::Error) + Send + 'static,
    ) -> io::Result<Self> {
        let (queue, received) = mpsc::channel::<Vec<u8>>();
        let pending = Arc::new(AtomicUsize::new(0));
        let written = Arc::clone(&pending);
        let thread = thread::Builder::new().name(name).spawn(move || {
            let mut writer = writer;
            for bytes in received {
                let result = writer.write_all(&bytes).and_then(|()| writer.flush());
                written.fetch_sub(bytes.len(), Ordering::Relaxed);
                if let Err(error) = result {
                    failed(error);
                    break;
                }
            }
        })?;
        Ok(Self {
            queue,
            pending,
            limit,
            thread,
        })
    }

    /// Queues `bytes` for the child, after any input queued before. Never
    /// blocks: when they do not fit in the queue they are dropped whole, so
    /// a paste is never cut in the middle (its closing bracket kept) and a
    /// child that stops reading costs at most the limit in memory.
    pub fn send(&self, bytes: &[u8]) -> Result<(), Dropped> {
        // A failed write ended the thread: the child is gone and nothing
        // will be written, so the bytes still counted as pending (queued
        // behind the failure) must not fill the queue.
        if self.thread.is_finished() {
            return Ok(());
        }
        // Only this side adds, so the check holds until the add below.
        let pending = self.pending.load(Ordering::Relaxed);
        if bytes.len() > self.limit.saturating_sub(pending) {
            return Err(Dropped);
        }
        self.pending.fetch_add(bytes.len(), Ordering::Relaxed);
        for chunk in bytes.chunks(CHUNK) {
            if self.queue.send(chunk.to_vec()).is_err() {
                return Ok(()); // The thread ended: the child is gone.
            }
        }
        Ok(())
    }

    /// Stops queueing and waits until everything queued was written.
    #[cfg(test)]
    pub fn close(self) {
        let Self { queue, thread, .. } = self;
        drop(queue);
        let _ = thread.join();
    }
}

/// The one-line notice written into a pane whose input was dropped.
pub fn dropped_notice(len: usize) -> String {
    let kib = len.div_ceil(1024);
    format!("\r\n[nxgterm: {kib} KiB of input dropped: the program is not reading its input]\r\n")
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
        let input = Input::spawn("test".into(), Box::new(recorder.clone()), drop).unwrap();
        let paste: Vec<u8> = (0..CHUNK * 3 + 7).map(|i| (i % 251) as u8).collect();
        input.send(b"echo ").unwrap();
        input.send(&paste).unwrap();
        input.send(b"\r").unwrap();
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
        let input =
            Input::spawn("test".into(), Box::new(Gate(gate, recorder.clone())), drop).unwrap();
        let start = Instant::now();
        for _ in 0..100 {
            input.send(&[b'x'; 1024]).unwrap();
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
    fn a_failed_write_is_reported_once_and_ends_the_thread() {
        let failures = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&failures);
        let failed = move |error: io::Error| seen.lock().unwrap().push(error.kind());
        let input = Input::spawn("test".into(), Box::new(Broken), failed).unwrap();
        input.send(b"a").unwrap();
        input.send(b"b").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !input.thread.is_finished() {
            assert!(Instant::now() < deadline, "the thread did not end");
            thread::yield_now();
        }
        let _ = input.send(b"c"); // Dropped, no panic.
        input.close();
        assert_eq!(*failures.lock().unwrap(), [io::ErrorKind::BrokenPipe]);
    }

    /// Fails every write once the test lets it through.
    struct FailingGate(Receiver<()>);

    impl Write for FailingGate {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            let _ = self.0.recv();
            Err(io::ErrorKind::BrokenPipe.into())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn input_queued_behind_a_failed_write_never_fills_the_queue() {
        let (open, gate) = mpsc::channel();
        let writer = Box::new(FailingGate(gate));
        let input = Input::spawn_bounded("test".into(), writer, 10, drop).unwrap();
        input.send(b"12345678").unwrap();
        input.send(b"ab").unwrap(); // Queued behind the failing write.
        open.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !input.thread.is_finished() {
            assert!(Instant::now() < deadline, "the thread did not end");
            thread::yield_now();
        }
        // The child is gone: input is discarded quietly, never "dropped"
        // with a notice because of bytes nobody will write.
        assert_eq!(input.send(b"123456789"), Ok(()));
        assert_eq!(input.send(b"x"), Ok(()));
        input.close();
    }

    #[test]
    fn input_that_does_not_fit_is_dropped_whole() {
        let (open, gate) = mpsc::channel();
        let recorder = Recorder::default();
        let writer = Box::new(Gate(gate, recorder.clone()));
        let input = Input::spawn_bounded("test".into(), writer, 10, drop).unwrap();
        input.send(b"12345678").unwrap();
        assert_eq!(input.send(b"abc"), Err(Dropped), "8 + 3 > 10");
        input.send(b"xy").unwrap(); // A key still fits.
        assert_eq!(input.send(b"z"), Err(Dropped), "full");
        open.send(()).unwrap(); // The child reads the first piece.
        let deadline = Instant::now() + Duration::from_secs(5);
        while input.pending.load(Ordering::Relaxed) > 2 {
            assert!(Instant::now() < deadline, "the write did not finish");
            thread::yield_now();
        }
        input.send(b"z").unwrap();
        open.send(()).unwrap();
        open.send(()).unwrap();
        input.close();
        assert_eq!(*recorder.bytes.lock().unwrap(), b"12345678xyz");
    }

    #[test]
    fn a_paste_larger_than_the_limit_is_dropped() {
        let input = Input::spawn_bounded("test".into(), Box::new(io::sink()), 4, drop).unwrap();
        assert_eq!(input.send(b"12345"), Err(Dropped));
        input.send(b"1234").unwrap();
        input.close();
    }

    #[test]
    fn the_notice_names_the_dropped_size() {
        assert!(dropped_notice(1).contains("1 KiB of input dropped"));
        assert!(dropped_notice(3 * 1024 + 1).contains("4 KiB"));
    }
}
