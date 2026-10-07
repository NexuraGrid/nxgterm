//! Command-line parsing.

use std::ffi::OsString;
use std::path::PathBuf;

/// Printed by `--help`.
pub const USAGE: &str = "\
Usage: nxgterm [OPTIONS]

Options:
  --config <PATH>   Use this config file instead of the default location
  --print-config    Print a documented default config and exit
  -V, --version     Print the version and exit
  -h, --help        Print this help and exit

Environment:
  NXGTERM_CONFIG    Config file path (overridden by --config)
  NXGTERM_RENDERER  auto, gpu or cpu (overrides [renderer] backend)
";

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Open the terminal, optionally with a specific config file.
    Run {
        config: Option<PathBuf>,
    },
    PrintConfig,
    Version,
    Help,
}

/// Parses the arguments after the program name. `--help`, `--version` and
/// `--print-config` act as soon as they are seen; `--config` takes the
/// next argument or an `=` value, and the last one wins.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Command, String> {
    let mut config = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy();
        match text.as_ref() {
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "--print-config" => return Ok(Command::PrintConfig),
            "--config" => {
                let value = args.next().ok_or("--config needs a file path")?;
                config = Some(PathBuf::from(value));
            }
            _ => match text.strip_prefix("--config=") {
                Some("") => return Err("--config needs a file path".into()),
                Some(value) => config = Some(PathBuf::from(value)),
                None => return Err(format!("unknown argument `{text}`")),
            },
        }
    }
    Ok(Command::Run { config })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Result<Command, String> {
        parse(args.iter().map(OsString::from))
    }

    fn with_config(path: &str) -> Command {
        Command::Run {
            config: Some(PathBuf::from(path)),
        }
    }

    #[test]
    fn no_arguments_runs_with_the_default_config() {
        assert_eq!(run(&[]), Ok(Command::Run { config: None }));
    }

    #[test]
    fn informational_flags() {
        assert_eq!(run(&["--help"]), Ok(Command::Help));
        assert_eq!(run(&["-h"]), Ok(Command::Help));
        assert_eq!(run(&["--version"]), Ok(Command::Version));
        assert_eq!(run(&["-V"]), Ok(Command::Version));
        assert_eq!(run(&["--print-config"]), Ok(Command::PrintConfig));
        assert_eq!(run(&["--config", "a.toml", "--help"]), Ok(Command::Help));
    }

    #[test]
    fn config_takes_a_separate_or_inline_value() {
        assert_eq!(
            run(&["--config", "/x/a.toml"]),
            Ok(with_config("/x/a.toml"))
        );
        assert_eq!(run(&["--config=/x/b.toml"]), Ok(with_config("/x/b.toml")));
        assert_eq!(
            run(&["--config", "a.toml", "--config", "b.toml"]),
            Ok(with_config("b.toml"))
        );
    }

    #[test]
    fn config_value_may_look_like_a_flag() {
        assert_eq!(
            run(&["--config", "-odd.toml"]),
            Ok(with_config("-odd.toml"))
        );
    }

    #[test]
    fn missing_config_value_is_an_error() {
        let error = run(&["--config"]).unwrap_err();
        assert!(error.contains("--config"), "{error}");
        assert!(run(&["--config="]).is_err());
    }

    #[test]
    fn unknown_arguments_are_errors() {
        let error = run(&["--colour"]).unwrap_err();
        assert!(error.contains("`--colour`"), "{error}");
        assert!(run(&["file.toml"]).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_config_paths_are_kept() {
        use std::os::unix::ffi::OsStringExt;
        let path = OsString::from_vec(vec![b'a', 0xff]);
        let args = vec![OsString::from("--config"), path.clone()];
        assert_eq!(
            parse(args),
            Ok(Command::Run {
                config: Some(PathBuf::from(path))
            })
        );
    }
}
