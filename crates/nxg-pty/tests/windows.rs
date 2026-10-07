//! Spawns real processes through each Windows backend explicitly.
#![cfg(windows)]

use std::io::Read;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use nxg_core::ports::{PtyControl, PtySession};
use nxg_core::{TermSize, WinSize};
use nxg_pty::Backend;

const TIMEOUT: Duration = Duration::from_secs(60);

/// Echoes a marker, then stays alive ~2 s so the resize hits a live child.
fn spawn_marker(backend: Backend, marker: &str) -> PtySession {
    let spawned = nxg_pty::spawn_command_with(
        TermSize::default(),
        &[backend],
        "cmd.exe",
        &[
            "/c",
            "echo",
            marker,
            "&",
            "ping",
            "-n",
            "3",
            "127.0.0.1",
            ">nul",
        ],
    )
    .unwrap_or_else(|error| panic!("spawn through {}: {error}", backend.name()));
    assert_eq!(spawned.name, backend.name());
    assert!(spawned.skipped.is_empty());
    spawned.backend
}

/// Reads on a background thread until `marker` shows up or the timeout
/// passes, so a backend that never reports EOF cannot hang the test.
///
/// Answers cursor position requests (`CSI 6 n`) like the terminal does:
/// ConPTY blocks at startup until it gets the reply.
fn read_until(reader: Box<dyn Read + Send>, control: &mut dyn PtyControl, marker: &str) -> String {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        while let Ok(n @ 1..) = reader.read(&mut buf) {
            if tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + TIMEOUT;
    let mut output = Vec::new();
    while !String::from_utf8_lossy(&output).contains(marker) {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(chunk) => {
                if chunk.windows(4).any(|w| w == b"\x1b[6n") {
                    control.write_all(b"\x1b[1;1R").expect("cursor report");
                }
                output.extend(chunk);
            }
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&output).into_owned()
}

/// Waits for the child on a background thread, bounded by the timeout.
fn wait_with_timeout(mut child: Box<dyn nxg_core::ports::ChildProcess>) {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(child.wait());
    });
    rx.recv_timeout(TIMEOUT)
        .expect("child did not exit in time")
        .expect("wait failed");
}

fn round_trip(backend: Backend, marker: &str) {
    let session = spawn_marker(backend, marker);
    let mut control = session.control;
    let output = read_until(session.reader, control.as_mut(), marker);
    assert!(output.contains(marker), "output was {output:?}");

    let size = WinSize {
        cells: TermSize::new(100, 30).unwrap(),
        cell: None,
    };
    control.resize(size).expect("resize");

    wait_with_timeout(session.child);
    drop(control);
}

#[test]
fn winpty_backend_runs_a_command() {
    round_trip(Backend::Winpty, "nxg-winpty-ok");
}

#[test]
fn native_backend_runs_a_command() {
    round_trip(Backend::Native, "nxg-native-ok");
}

#[test]
fn auto_starts_a_backend() {
    let spawned = nxg_pty::spawn_command_with(
        TermSize::default(),
        nxg_pty::auto_backends(),
        "cmd.exe",
        &["/c", "echo", "nxg-auto-ok"],
    )
    .expect("auto spawn");
    // CI runners have ConPTY, so auto never needs the fallback there.
    assert_eq!(spawned.name, "native", "skipped: {:?}", spawned.skipped);
    let mut session = spawned.backend;
    let output = read_until(session.reader, session.control.as_mut(), "nxg-auto-ok");
    assert!(output.contains("nxg-auto-ok"), "output was {output:?}");
}

#[test]
fn missing_program_fails_through_winpty() {
    let error = nxg_pty::spawn_command_with(
        TermSize::default(),
        &[Backend::Winpty],
        r"C:\definitely\missing\shell.exe",
        &[],
    )
    .unwrap_err();
    assert_eq!(error.attempts.len(), 1);
    assert_eq!(error.attempts[0].name, "winpty");
}
