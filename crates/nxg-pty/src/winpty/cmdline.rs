//! Windows command lines, quoted so the MSVC CRT (`CommandLineToArgvW`)
//! splits them back into the original arguments.
//!
//! Works on UTF-16 code units so arguments that are not valid Unicode
//! survive unchanged.

/// Joins `argv` (program first) into one command line.
///
/// The program is only wrapped in quotes (the CRT does not unescape it), so
/// it may not contain `"`. Arguments are quoted when empty or containing
/// whitespace or `"`. NUL is rejected anywhere since `CreateProcess` would
/// truncate the line there.
pub(crate) fn command_line<S: AsRef<[u16]>>(argv: &[S]) -> Result<Vec<u16>, String> {
    let (program, args) = argv.split_first().ok_or("empty command")?;
    let program = program.as_ref();
    if program.is_empty() {
        return Err("empty program name".into());
    }
    if program.contains(&QUOTE) {
        return Err("program name contains a double quote".into());
    }
    if argv.iter().any(|arg| arg.as_ref().contains(&0)) {
        return Err("command contains a NUL character".into());
    }

    let mut line = Vec::new();
    if program.iter().any(|&c| is_blank(c)) {
        line.push(QUOTE);
        line.extend_from_slice(program);
        line.push(QUOTE);
    } else {
        line.extend_from_slice(program);
    }
    for arg in args {
        line.push(u16::from(b' '));
        push_arg(&mut line, arg.as_ref());
    }
    Ok(line)
}

const QUOTE: u16 = b'"' as u16;
const BACKSLASH: u16 = b'\\' as u16;

fn is_blank(c: u16) -> bool {
    b" \t\n\x0b".iter().any(|&blank| u16::from(blank) == c)
}

/// Appends `arg`, quoted when needed. Inside quotes, backslashes are literal
/// except in runs that precede a `"`, which are doubled.
fn push_arg(line: &mut Vec<u16>, arg: &[u16]) {
    let needs_quotes = arg.is_empty() || arg.iter().any(|&c| is_blank(c) || c == QUOTE);
    if !needs_quotes {
        line.extend_from_slice(arg);
        return;
    }
    line.push(QUOTE);
    let mut backslashes = 0;
    for &c in arg {
        match c {
            BACKSLASH => backslashes += 1,
            QUOTE => {
                // Escape the pending backslashes and the quote itself.
                line.extend(std::iter::repeat_n(BACKSLASH, backslashes + 1));
                backslashes = 0;
            }
            _ => backslashes = 0,
        }
        line.push(c);
    }
    // Escape trailing backslashes so they do not escape the closing quote.
    line.extend(std::iter::repeat_n(BACKSLASH, backslashes));
    line.push(QUOTE);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(argv: &[&str]) -> Result<String, String> {
        let wide: Vec<Vec<u16>> = argv.iter().map(|a| a.encode_utf16().collect()).collect();
        command_line(&wide).map(|line| String::from_utf16(&line).unwrap())
    }

    #[test]
    fn plain_arguments_are_joined_with_spaces() {
        assert_eq!(
            line(&["cmd.exe", "/c", "echo", "hi"]).unwrap(),
            "cmd.exe /c echo hi"
        );
    }

    #[test]
    fn program_with_spaces_is_quoted_without_escaping() {
        assert_eq!(
            line(&[r"C:\Program Files\PowerShell\7\pwsh.exe", "-NoLogo"]).unwrap(),
            r#""C:\Program Files\PowerShell\7\pwsh.exe" -NoLogo"#
        );
        assert_eq!(line(&[r"C:\a b\"]).unwrap(), r#""C:\a b\""#);
    }

    #[test]
    fn arguments_with_whitespace_or_empty_are_quoted() {
        assert_eq!(line(&["p", "a b", ""]).unwrap(), r#"p "a b" """#);
        assert_eq!(line(&["p", "tab\there"]).unwrap(), "p \"tab\there\"");
    }

    #[test]
    fn quotes_are_escaped() {
        assert_eq!(line(&["p", r#"say "hi""#]).unwrap(), r#"p "say \"hi\"""#);
    }

    #[test]
    fn backslashes_are_literal_unless_before_a_quote() {
        assert_eq!(line(&["p", r"C:\dir\file"]).unwrap(), r"p C:\dir\file");
        // Trailing backslashes inside quotes are doubled so the closing
        // quote is not escaped.
        assert_eq!(line(&["p", r"C:\my dir\"]).unwrap(), r#"p "C:\my dir\\""#);
        // Backslashes before an embedded quote are doubled, plus one for it.
        assert_eq!(line(&["p", r#"a\"b"#]).unwrap(), r#"p "a\\\"b""#);
    }

    #[test]
    fn unrepresentable_input_is_rejected() {
        assert!(line(&[]).is_err());
        assert!(line(&[""]).is_err());
        assert!(line(&[r#"C:\odd"name.exe"#]).is_err());
        assert!(line(&["p", "nul\0byte"]).is_err());
    }

    #[test]
    fn unpaired_surrogates_pass_through() {
        let argv = [vec![u16::from(b'p')], vec![0xD800]];
        let joined = command_line(&argv).unwrap();
        assert_eq!(joined, [u16::from(b'p'), u16::from(b' '), 0xD800]);
    }
}
