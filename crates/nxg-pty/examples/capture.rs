//! Dev-only capture tool for replay fixtures (never part of the product).
//!
//! Usage: `cargo run -p nxg-pty --example capture -- <script> <out.vt>`
//!
//! Script lines (one per line, `#` starts a comment):
//!   run <program> [args...]   program to spawn (required, first)
//!   env KEY=VALUE             extra environment (use HOME= to isolate config)
//!   cwd <dir>                 working directory
//!   wait <ms>                 let the program draw
//!   type <text>               keystrokes; escapes \e \r \n \t \\ \xNN
//!   mark <label>              write an `nxg-checkpoint` marker into the stream
//!
//! The child runs at 80x24 (800x384 px, so yazi enables sixel previews) with
//! TERM=xterm-256color. Its terminal queries are answered by a real
//! `nxg_core::Terminal`, so programs that probe the terminal (yazi) do not
//! stall. Output is scrubbed of HOME, user, host and
//! cwd with same-length substitutions, so cursor columns stay valid.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nxg_core::{TermSize, Terminal};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

#[derive(Debug, PartialEq)]
enum Step {
    Wait(u64),
    Type(Vec<u8>),
    Mark(String),
}

#[derive(Debug, Default)]
struct Script {
    program: Vec<String>,
    env: Vec<(String, String)>,
    cwd: Option<String>,
    steps: Vec<Step>,
}

fn unescape(text: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.extend(c.to_string().as_bytes());
            continue;
        }
        match chars.next() {
            Some('e') => out.push(0x1b),
            Some('r') => out.push(b'\r'),
            Some('n') => out.push(b'\n'),
            Some('t') => out.push(b'\t'),
            Some('\\') => out.push(b'\\'),
            Some('x') => {
                let hex: String = chars.by_ref().take(2).collect();
                // from_str_radix would accept a single digit; insist on two.
                if hex.len() != 2 {
                    return Err(format!("bad \\x{hex}"));
                }
                out.push(u8::from_str_radix(&hex, 16).map_err(|_| format!("bad \\x{hex}"))?);
            }
            other => return Err(format!("bad escape \\{}", other.unwrap_or(' '))),
        }
    }
    Ok(out)
}

fn parse_script(text: &str) -> Result<Script, String> {
    let mut script = Script::default();
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let bad = |what: &str| format!("script line {}: {what}", n + 1);
        let (key, rest) = line.split_once(' ').unwrap_or((line, ""));
        match key {
            "run" => script.program = rest.split_whitespace().map(String::from).collect(),
            "env" => {
                let (k, v) = rest.split_once('=').ok_or_else(|| bad("env KEY=VALUE"))?;
                script.env.push((k.to_string(), v.to_string()));
            }
            "cwd" => script.cwd = Some(rest.to_string()),
            "wait" => script.steps.push(Step::Wait(
                rest.trim().parse().map_err(|_| bad("wait <ms>"))?,
            )),
            "type" => script
                .steps
                .push(Step::Type(unescape(rest).map_err(|e| bad(&e))?)),
            "mark" if !rest.trim().is_empty() => {
                script.steps.push(Step::Mark(rest.trim().to_string()))
            }
            _ => return Err(bad("unknown or incomplete directive")),
        }
    }
    if script.program.is_empty() {
        return Err("script has no `run` line".to_string());
    }
    Ok(script)
}

/// Replaces every needle with `x` of the same length (a leading `/` stays, so
/// paths remain paths). Longest needles first so `/home/me` wins over `me`.
fn scrub(bytes: &[u8], needles: &[String]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let mut sorted: Vec<&String> = needles.iter().filter(|n| !n.is_empty()).collect();
    sorted.sort_by_key(|n| std::cmp::Reverse(n.len()));
    for needle in sorted {
        let n = needle.as_bytes();
        let mut fill = vec![b'x'; n.len()];
        if n[0] == b'/' {
            fill[0] = b'/';
        }
        let mut i = 0;
        while i + n.len() <= out.len() {
            if &out[i..i + n.len()] == n {
                out[i..i + n.len()].copy_from_slice(&fill);
                i += n.len();
            } else {
                i += 1;
            }
        }
    }
    out
}

/// `text` and every suffix of it at least `min` bytes long.
fn tails(text: &str, min: usize) -> Vec<String> {
    (0..text.len())
        .filter(|&i| text.is_char_boundary(i) && text.len() - i >= min)
        .map(|i| text[i..].to_string())
        .collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let (Some(script_path), Some(out_path)) = (args.next(), args.next()) else {
        return Err("usage: capture <script> <out.vt>".into());
    };
    let script = parse_script(&std::fs::read_to_string(script_path)?)?;

    let pair = native_pty_system().openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 800,
        pixel_height: 384,
    })?;
    let mut cmd = CommandBuilder::new(&script.program[0]);
    cmd.args(&script.program[1..]);
    cmd.env("TERM", "xterm-256color");
    for (k, v) in &script.env {
        cmd.env(k, v);
    }
    if let Some(cwd) = &script.cwd {
        cmd.cwd(cwd);
    }
    let mut child = pair.slave.spawn_command(cmd)?;
    drop(pair.slave);

    let writer = Arc::new(Mutex::new(pair.master.take_writer()?));
    let output = Arc::new(Mutex::new(Vec::<u8>::new()));
    let mut reader = pair.master.try_clone_reader()?;
    let (out, wr) = (Arc::clone(&output), Arc::clone(&writer));
    std::thread::spawn(move || {
        let mut term = Terminal::new(TermSize::new(80, 24).expect("80x24"));
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            out.lock().unwrap().extend_from_slice(&buf[..n]);
            term.advance(&buf[..n]);
            let replies = term.take_responses();
            if !replies.is_empty() {
                let _ = wr.lock().unwrap().write_all(&replies);
            }
        }
    });

    for step in &script.steps {
        match step {
            Step::Wait(ms) => std::thread::sleep(Duration::from_millis(*ms)),
            Step::Type(bytes) => {
                let mut w = writer.lock().unwrap();
                w.write_all(bytes)?;
                w.flush()?;
            }
            Step::Mark(label) => {
                let mut o = output.lock().unwrap();
                o.extend_from_slice(format!("\x1b]nxg-checkpoint;{label}\x07").as_bytes());
            }
        }
    }
    // Give the program a moment to exit on its own, then stop it.
    std::thread::sleep(Duration::from_millis(500));
    if child.try_wait()?.is_none() {
        eprintln!("capture: program still running after the script, killing it");
        child.kill()?;
    }
    std::thread::sleep(Duration::from_millis(100));

    let mut needles: Vec<String> = ["HOME", "USER", "HOSTNAME"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .collect();
    needles.extend(std::fs::read_to_string("/etc/hostname").map(|h| h.trim().to_string()));
    let mut paths: Vec<String> = script
        .env
        .iter()
        .filter(|(k, _)| k == "HOME")
        .map(|(_, v)| v.clone())
        .collect();
    paths.extend(script.cwd.clone());
    paths.extend(
        std::env::current_dir()
            .ok()
            .map(|p| p.display().to_string()),
    );
    // Programs such as yazi cut long paths on the left, so scrub the tails too.
    needles.extend(paths.iter().flat_map(|p| tails(p, 8)));
    // Names shorter than the guard's threshold would mangle ordinary text
    // ("dev" inside "development"), so they are left alone on both sides.
    needles.retain(|n| n.len() >= 4);
    let data = scrub(&output.lock().unwrap(), &needles);
    std::fs::write(out_path, data)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_parses_every_directive() {
        let text = "# note\nrun vim -u NONE -N\nenv HOME=/tmp/h\ncwd /tmp/w\nwait 250\ntype ihi\\e\nmark vim-open\n";
        let s = parse_script(text).unwrap();
        assert_eq!(s.program, ["vim", "-u", "NONE", "-N"]);
        assert_eq!(s.env, [("HOME".to_string(), "/tmp/h".to_string())]);
        assert_eq!(s.cwd.as_deref(), Some("/tmp/w"));
        assert_eq!(
            s.steps,
            [
                Step::Wait(250),
                Step::Type(b"ihi\x1b".to_vec()),
                Step::Mark("vim-open".to_string())
            ]
        );
    }

    #[test]
    fn script_rejects_bad_input() {
        assert!(parse_script("wait 10\n").is_err(), "missing run");
        assert!(parse_script("run a\nwait soon\n").is_err());
        assert!(parse_script("run a\nfly away\n").is_err());
        assert!(parse_script("run a\nmark\n").is_err());
    }

    #[test]
    fn unescape_handles_every_form() {
        assert_eq!(
            unescape("a\\r\\n\\t\\e\\\\\\x41").unwrap(),
            b"a\r\n\t\x1b\\A"
        );
        assert_eq!(unescape("plain").unwrap(), b"plain");
        assert!(unescape("bad\\q").is_err());
        assert!(unescape("cut\\x4").is_err());
    }

    #[test]
    fn scrub_keeps_length_and_prefers_the_longest_needle() {
        let needles = vec!["/h/me".to_string(), "me".to_string()];
        let out = scrub(b"cd /h/me; whoami=me", &needles);
        assert_eq!(out, b"cd /xxxx; whoami=xx");
        assert_eq!(out.len(), "cd /h/me; whoami=me".len());
    }

    #[test]
    fn suffixes_cover_left_truncated_paths() {
        // Programs such as yazi cut long paths on the left ("...re/work").
        let got = tails("/ab/cdefgh", 8);
        assert_eq!(got, ["/ab/cdefgh", "ab/cdefgh", "b/cdefgh"]);
        assert!(tails("short", 8).is_empty());
    }

    #[test]
    fn scrub_ignores_empty_needles_and_leaves_other_bytes() {
        assert_eq!(scrub(b"abc\x1b[1m", &[String::new()]), b"abc\x1b[1m");
        assert_eq!(scrub(b"abcabc", &["abc".to_string()]), b"xxxxxx");
    }
}
