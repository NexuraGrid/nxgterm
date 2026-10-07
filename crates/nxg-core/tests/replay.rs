//! Replays captured full-screen program output through `Terminal` and checks
//! screen text, cursor and modes at labelled checkpoints. Public API only.
//!
//! Stream format: raw child output with `ESC ] nxg-checkpoint ; <label> BEL`
//! markers (vte ignores unknown OSC, so the marker is safe in-band). The
//! `expect.txt` next to it holds one `[label]` section per checkpoint; the
//! bytes after the last marker are checked against the `[end]` section.

use nxg_core::{TermSize, Terminal};

const MARK: &[u8] = b"\x1b]nxg-checkpoint;";
const END: &str = "end";

/// Bytes to feed, then the checkpoint that follows them (`None` for the tail).
struct Segment {
    data: Vec<u8>,
    label: Option<String>,
}

#[derive(Default)]
struct Section {
    label: String,
    size: Option<(u16, u16)>,
    alt: Option<bool>,
    cursor: Option<(u16, u16)>,
    visible: Option<bool>,
    rows: Vec<(u16, String)>,
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn split_stream(stream: &[u8]) -> Result<Vec<Segment>, String> {
    let mut segments = Vec::new();
    let mut rest = stream;
    while let Some(at) = find(rest, MARK) {
        let after = &rest[at + MARK.len()..];
        let end = after
            .iter()
            .position(|&b| b == 0x07)
            .ok_or("unterminated checkpoint marker")?;
        let label = String::from_utf8(after[..end].to_vec()).map_err(|e| e.to_string())?;
        segments.push(Segment {
            data: rest[..at].to_vec(),
            label: Some(label),
        });
        rest = &after[end + 1..];
    }
    segments.push(Segment {
        data: rest.to_vec(),
        label: None,
    });
    Ok(segments)
}

fn pair(text: &str, sep: char) -> Option<(u16, u16)> {
    let (a, b) = text.split_once(sep)?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

fn parse_expect(text: &str) -> Result<Vec<Section>, String> {
    let mut sections: Vec<Section> = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        let bad = |what: &str| format!("expect.txt line {}: {what}: {raw:?}", n + 1);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            let label = rest.split(']').next().unwrap_or_default();
            sections.push(Section {
                label: label.to_string(),
                ..Section::default()
            });
            continue;
        }
        let section = sections
            .last_mut()
            .ok_or_else(|| bad("directive before a [label]"))?;
        let (key, value) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        match key {
            "size" => section.size = Some(pair(value, ' ').ok_or_else(|| bad("size cols rows"))?),
            "alt" => section.alt = Some(value.trim().parse().map_err(|_| bad("alt true|false"))?),
            "cursor" => {
                section.cursor = Some(pair(value, ',').ok_or_else(|| bad("cursor col,row"))?)
            }
            "visible" => {
                section.visible = Some(
                    value
                        .trim()
                        .parse()
                        .map_err(|_| bad("visible true|false"))?,
                )
            }
            "row" => {
                let (idx, quoted) = value
                    .split_once(':')
                    .ok_or_else(|| bad("row N: \"text\""))?;
                let idx = idx.trim().parse().map_err(|_| bad("row index"))?;
                let text = quoted
                    .trim()
                    .strip_prefix('"')
                    .and_then(|q| q.strip_suffix('"'));
                // Compared trimmed-end, like the screen text.
                let text = text.ok_or_else(|| bad("row text must be quoted"))?;
                section.rows.push((idx, text.trim_end().to_string()));
            }
            _ => return Err(bad("unknown directive")),
        }
    }
    Ok(sections)
}

fn row_text(term: &Terminal, row: u16) -> String {
    let text: String = term.row(row).iter().map(|c| c.ch).collect();
    text.trim_end().to_string()
}

/// A message when `want` is set and differs from `got`.
fn differs<T: PartialEq + std::fmt::Debug>(what: &str, got: T, want: Option<T>) -> Option<String> {
    want.filter(|w| *w != got)
        .map(|w| format!("{what} is {got:?}, expected {w:?}"))
}

fn check(term: &Terminal, section: &Section) -> Result<(), String> {
    let fail = |what: String| Err(format!("[{}] {what}", section.label));
    let size = term.size();
    let cursor = term.cursor();
    // Not `if let ... &&`: let chains need Rust 1.88 and the MSRV is 1.85.
    let scalars = [
        differs("size", (size.cols(), size.rows()), section.size),
        differs("alt", term.modes().alt_screen, section.alt),
        differs("cursor", (cursor.col, cursor.row), section.cursor),
        differs("visible", cursor.visible, section.visible),
    ];
    if let Some(msg) = scalars.into_iter().flatten().next() {
        return fail(msg);
    }
    for (idx, want) in &section.rows {
        if *idx >= size.rows() {
            return fail(format!("row {idx} is outside the screen"));
        }
        let got = row_text(term, *idx);
        if &got != want {
            return fail(format!("row {idx} is {got:?}, expected {want:?}"));
        }
    }
    Ok(())
}

/// Feeds `stream` into an 80x24 terminal and checks every section. A section
/// whose checkpoint never occurs is an error, so a typo cannot pass silently.
fn replay(stream: &[u8], expect: &str) -> Result<(), String> {
    let sections = parse_expect(expect)?;
    let mut term = Terminal::new(TermSize::new(80, 24).unwrap());
    let mut seen = vec![false; sections.len()];
    for seg in split_stream(stream)? {
        term.advance(&seg.data);
        let label = seg.label.as_deref().unwrap_or(END);
        for (i, section) in sections
            .iter()
            .enumerate()
            .filter(|(_, s)| s.label == label)
        {
            seen[i] = true;
            check(&term, section)?;
        }
    }
    match sections.iter().zip(&seen).find(|(_, seen)| !**seen) {
        Some((section, _)) => Err(format!("[{}] checkpoint never reached", section.label)),
        None => Ok(()),
    }
}

/// First needle found in `bytes`, if any (fixture privacy guard).
fn find_forbidden(bytes: &[u8], needles: &[String]) -> Option<String> {
    needles
        .iter()
        .find(|n| find(bytes, n.as_bytes()).is_some())
        .cloned()
}

/// Home-directory prefixes plus the current user and host, so a capture made
/// on this machine cannot leak them even if the capture tool missed a spot.
fn forbidden_needles() -> Vec<String> {
    let mut needles: Vec<String> = ["/home/", "/Users/", "C:\\Users"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let host = std::fs::read_to_string("/etc/hostname").ok();
    for name in [
        std::env::var("USER").ok(),
        std::env::var("HOSTNAME").ok(),
        host,
    ] {
        // Very short names would match ordinary screen text.
        if let Some(name) = name.map(|n| n.trim().to_string()).filter(|n| n.len() >= 4) {
            needles.push(name);
        }
    }
    needles
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marked(label: &str) -> Vec<u8> {
        [MARK, label.as_bytes(), b"\x07"].concat()
    }

    #[test]
    fn stream_splits_on_checkpoint_markers() {
        let stream = [
            b"ab".as_slice(),
            &marked("one"),
            b"cd",
            &marked("two"),
            b"ef",
        ]
        .concat();
        let segs = split_stream(&stream).unwrap();
        let got: Vec<(&[u8], Option<&str>)> = segs
            .iter()
            .map(|s| (s.data.as_slice(), s.label.as_deref()))
            .collect();
        assert_eq!(
            got,
            vec![
                (b"ab".as_slice(), Some("one")),
                (b"cd".as_slice(), Some("two")),
                (b"ef".as_slice(), None),
            ]
        );
    }

    #[test]
    fn stream_without_markers_is_one_tail_segment() {
        let segs = split_stream(b"plain").unwrap();
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].data, b"plain");
        assert_eq!(segs[0].label, None);
    }

    #[test]
    fn unterminated_marker_is_an_error() {
        assert!(split_stream(b"x\x1b]nxg-checkpoint;oops").is_err());
    }

    #[test]
    fn expect_parses_every_directive() {
        let text = "# comment\n[a]\nsize 5 3\nalt true\ncursor 2,1\nvisible false\nrow 0: \"hi  \"\nrow 2: \"\"\n\n[end]\nalt false\n";
        let sections = parse_expect(text).unwrap();
        assert_eq!(sections.len(), 2);
        let a = &sections[0];
        assert_eq!(a.label, "a");
        assert_eq!(a.size, Some((5, 3)));
        assert_eq!(a.alt, Some(true));
        assert_eq!(a.cursor, Some((2, 1)));
        assert_eq!(a.visible, Some(false));
        // Trailing blanks in the expectation are trimmed like the screen text.
        assert_eq!(a.rows, vec![(0, "hi".to_string()), (2, String::new())]);
        assert_eq!(sections[1].label, "end");
        assert_eq!(sections[1].alt, Some(false));
        assert_eq!(sections[1].size, None);
    }

    #[test]
    fn expect_rejects_garbage() {
        assert!(
            parse_expect("size 5 3\n").is_err(),
            "directive before a section"
        );
        assert!(parse_expect("[a]\nbogus 1\n").is_err());
        assert!(parse_expect("[a]\nrow x: \"y\"\n").is_err());
    }

    #[test]
    fn matching_stream_passes() {
        let stream = [b"hi\r\nyo".as_slice(), &marked("m")].concat();
        let expect = "[m]\nsize 80 24\nalt false\ncursor 2,1\nrow 0: \"hi\"\nrow 1: \"yo\"\n";
        replay(&stream, expect).unwrap();
    }

    #[test]
    fn wrong_expectations_fail_with_the_label() {
        let stream = [b"hi".as_slice(), &marked("m")].concat();
        for bad in [
            "[m]\nrow 0: \"ho\"\n",
            "[m]\ncursor 0,0\n",
            "[m]\nalt true\n",
            "[m]\nvisible false\n",
            "[m]\nsize 81 24\n",
        ] {
            let err = replay(&stream, bad).unwrap_err();
            assert!(err.contains("[m]"), "{bad:?} -> {err}");
        }
    }

    #[test]
    fn unlisted_rows_are_not_checked() {
        let stream = [b"hi\r\nyo".as_slice(), &marked("m")].concat();
        replay(&stream, "[m]\nrow 1: \"yo\"\n").unwrap();
    }

    #[test]
    fn a_section_that_never_runs_is_an_error() {
        let err = replay(b"x", "[typo]\nalt false\n").unwrap_err();
        assert!(err.contains("typo"), "{err}");
    }

    #[test]
    fn end_section_checks_the_tail_of_the_stream() {
        let stream = [b"a".as_slice(), &marked("m"), b"\x1b[?1049hb"].concat();
        replay(&stream, "[m]\nalt false\n[end]\nalt true\nrow 0: \" b\"\n").unwrap();
        assert!(replay(&stream, "[end]\nalt false\n").is_err());
    }

    fn fixtures_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn replay_fixture(name: &str) {
        let dir = fixtures_root().join(name);
        let stream = std::fs::read(dir.join("stream.vt")).unwrap();
        let expect = std::fs::read_to_string(dir.join("expect.txt")).unwrap();
        if let Err(e) = replay(&stream, &expect) {
            panic!("fixture {name}: {e}");
        }
    }

    #[test]
    fn mini_fixture_replays() {
        replay_fixture("mini");
    }

    #[test]
    fn vim_session_replays() {
        replay_fixture("vim");
    }

    #[test]
    fn less_session_replays() {
        replay_fixture("less");
    }

    #[test]
    fn yazi_session_replays() {
        replay_fixture("yazi");
    }

    #[test]
    fn fixtures_are_small_and_clean() {
        let forbidden = forbidden_needles();
        let mut checked = 0;
        for dir in std::fs::read_dir(fixtures_root()).unwrap() {
            for file in std::fs::read_dir(dir.unwrap().path()).unwrap() {
                let path = file.unwrap().path();
                let bytes = std::fs::read(&path).unwrap();
                assert!(
                    bytes.len() < 64 * 1024,
                    "{} is {} bytes",
                    path.display(),
                    bytes.len()
                );
                if let Some(hit) = find_forbidden(&bytes, &forbidden) {
                    panic!("{} contains {hit:?}", path.display());
                }
                checked += 1;
            }
        }
        assert!(
            checked >= 8,
            "no fixtures found: the guard would check nothing"
        );
    }

    #[test]
    fn scanner_flags_planted_leaks_and_passes_clean_data() {
        let needles = forbidden_needles();
        for leak in [
            b"cd /home/someone/work".as_slice(),
            b"/Users/someone",
            b"C:\\Users\\someone",
        ] {
            assert!(find_forbidden(leak, &needles).is_some(), "{leak:?}");
        }
        let custom = vec!["myhost".to_string()];
        assert_eq!(
            find_forbidden(b"prompt@myhost:~$", &custom),
            Some("myhost".to_string())
        );
        assert_eq!(find_forbidden(b"plain screen text", &needles), None);
    }
}
