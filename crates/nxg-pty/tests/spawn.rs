//! Spawns real processes in a pty; Unix only for deterministic commands.
#![cfg(unix)]

use std::io::Read;

use nxg_core::{CellPixels, TermSize, WinSize};

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

#[test]
fn shell_override_runs_the_given_program_and_args() {
    let shell = nxg_pty::ShellCommand {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), "echo override:$TERM; sleep 0.3".into()],
    };
    let session =
        nxg_pty::spawn_shell_with(TermSize::default(), Some(&shell)).expect("spawn override");
    let output = read_all(session.reader);
    assert!(
        output.contains("override:xterm-256color"),
        "output was {output:?}"
    );
}

#[test]
fn missing_override_program_is_an_error() {
    let shell = nxg_pty::ShellCommand {
        program: "/definitely/missing/shell".into(),
        args: Vec::new(),
    };
    let error = nxg_pty::spawn_shell_with(TermSize::default(), Some(&shell)).unwrap_err();
    assert!(!error.attempts.is_empty());
}

#[test]
fn advertises_term_program() {
    let session = nxg_pty::spawn_command(
        TermSize::default(),
        "/bin/sh",
        &[
            "-c",
            "echo prog:$TERM_PROGRAM:$TERM_PROGRAM_VERSION; sleep 0.3",
        ],
    )
    .expect("spawn /bin/sh");
    let output = read_all(session.reader);
    let expected = format!("prog:nxgterm:{}", env!("CARGO_PKG_VERSION"));
    assert!(output.contains(&expected), "output was {output:?}");
}

#[test]
fn reports_pixel_size_through_the_window_size() {
    let probe = "import fcntl, struct, termios; \
                 r, c, w, h = struct.unpack('HHHH', fcntl.ioctl(0, termios.TIOCGWINSZ, bytes(8))); \
                 print(f'px:{w}x{h}')";
    let has_python = std::process::Command::new("python3")
        .arg("-c")
        .arg("pass")
        .status()
        .is_ok_and(|s| s.success());
    if !has_python {
        eprintln!("skipping: python3 not available");
        return;
    }
    let size = WinSize {
        cells: TermSize::new(80, 24).unwrap(),
        cell: Some(CellPixels::new(9, 18)),
    };
    let script = format!("python3 -c \"{probe}\"; sleep 0.3");
    let mut session =
        nxg_pty::spawn_command(size, "/bin/sh", &["-c", &script]).expect("spawn /bin/sh");
    let output = read_all(&mut session.reader);
    assert!(output.contains("px:720x432"), "output was {output:?}");
    let resized = WinSize {
        cells: TermSize::new(10, 2).unwrap(),
        cell: Some(CellPixels::new(9, 18)),
    };
    // Resizing after exit may fail on some platforms; only check it is callable.
    let _ = session.control.resize(resized);
}
