//! Spawns real processes in a pty; Unix only for deterministic commands.
#![cfg(unix)]

use std::io::Read;

use nxg_core::TermSize;

fn read_all(mut reader: impl Read) -> String {
    let mut out = Vec::new();
    let mut buf = [0u8; 1024];
    // A Unix pty reports EIO instead of EOF once the child exits. Commands
    // sleep briefly after printing so no platform drops unread output.
    while let Ok(n @ 1..) = reader.read(&mut buf) {
        out.extend_from_slice(&buf[..n]);
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[test]
fn reads_child_output() {
    let session = nxg_pty::spawn_command(
        TermSize::default(),
        "/bin/sh",
        &["-c", "echo hi; sleep 0.3"],
    )
    .expect("spawn /bin/sh");
    let mut child = session.child;
    let output = read_all(session.reader);
    child.wait().unwrap();
    assert!(output.contains("hi"), "output was {output:?}");
}

#[test]
fn sets_term_and_size() {
    let size = TermSize::new(100, 30).unwrap();
    let session =
        nxg_pty::spawn_command(size, "/bin/sh", &["-c", "echo $TERM; stty size; sleep 0.3"])
            .expect("spawn /bin/sh");
    let output = read_all(session.reader);
    assert!(output.contains("xterm-256color"), "output was {output:?}");
    assert!(output.contains("30 100"), "output was {output:?}");
}

#[test]
fn writes_input_to_child() {
    let mut session = nxg_pty::spawn_command(
        TermSize::default(),
        "/bin/sh",
        &["-c", "read x; echo got:$x"],
    )
    .expect("spawn /bin/sh");
    session.control.write_all(b"ping\r").unwrap();
    let output = read_all(session.reader);
    assert!(output.contains("got:ping"), "output was {output:?}");
}
