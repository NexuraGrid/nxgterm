//! Command-line parsing.

use std::ffi::OsString;
use std::path::PathBuf;

use nxg_config::WorkingDirectory;

/// Printed by `--help`.
pub const USAGE: &str = "\
Usage: nxgterm [OPTIONS]
       nxgterm update [--check] [--force]

Options:
  --config <PATH>            Use this config file instead of the default location
  --working-directory <DIR>  Start shells in DIR: auto, inherit, home or a path
                             (overrides [shell] working_directory)
  --elevate                  Relaunch as Administrator (Windows)
  --print-config             Print a documented default config and exit
  -V, --version              Print the version and exit
  -h, --help                 Print this help and exit

Commands:
  update                     Download, verify (SHA-256) and install the latest
                             release, or print the package-manager command
    --check                  Only print the current and latest versions
    --force                  Reinstall even when already up to date

Environment:
  NXGTERM_CONFIG      Config file path (overridden by --config)
  NXGTERM_RENDERER    auto, gpu or cpu (overrides [renderer] backend)
  NXGTERM_UPDATE_URL  Directory with SHA256SUMS and the assets, instead of
                      the latest GitHub release (for `update`)
";

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Open the terminal, optionally with a specific config file and
    /// start directory (which beats the config's); `elevate` asks to
    /// relaunch as Administrator first (Windows).
    Run {
        config: Option<PathBuf>,
        elevate: bool,
        working_directory: Option<WorkingDirectory>,
    },
    /// `nxgterm update`: install the latest release, or with `check` only
    /// report whether there is one; `force` reinstalls an up-to-date copy.
    Update {
        check: bool,
        force: bool,
    },
    PrintConfig,
    Version,
    Help,
}

/// Parses the arguments after the program name. A first argument of
/// `update` selects that subcommand. `--help`, `--version` and
/// `--print-config` act as soon as they are seen; `--config` and
/// `--working-directory` take the next argument or an `=` value, and the
/// last one wins.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Command, String> {
    let mut config = None;
    let mut elevate = false;
    let mut working_directory = None;
    let mut args = args.into_iter().peekable();
    if args.peek().is_some_and(|arg| arg == "update") {
        args.next();
        return parse_update(args);
    }
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy();
        match text.as_ref() {
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "--print-config" => return Ok(Command::PrintConfig),
            "--elevate" => elevate = true,
            "--config" => {
                let value = args.next().ok_or("--config needs a file path")?;
                config = Some(PathBuf::from(value));
            }
            "--working-directory" => {
                let value = args.next().ok_or(NEEDS_DIRECTORY)?;
                working_directory = Some(start_directory(value));
            }
            _ => {
                if let Some(value) = text.strip_prefix("--config=") {
                    if value.is_empty() {
                        return Err("--config needs a file path".into());
                    }
                    config = Some(PathBuf::from(value));
                } else if let Some(value) = text.strip_prefix("--working-directory=") {
                    if value.is_empty() {
                        return Err(NEEDS_DIRECTORY.into());
                    }
                    working_directory = Some(WorkingDirectory::parse(value));
                } else {
                    return Err(format!("unknown argument `{text}`"));
                }
            }
        }
    }
    Ok(Command::Run {
        config,
        elevate,
        working_directory,
    })
}

/// The arguments after `update`.
fn parse_update(args: impl Iterator<Item = OsString>) -> Result<Command, String> {
    let mut check = false;
    let mut force = false;
    for arg in args {
        match arg.to_string_lossy().as_ref() {
            "-h" | "--help" => return Ok(Command::Help),
            "--check" => check = true,
            "--force" => force = true,
            text => return Err(format!("unknown argument `{text}` for `update`")),
        }
    }
    Ok(Command::Update { check, force })
}

const NEEDS_DIRECTORY: &str = "--working-directory needs auto, inherit, home or a directory";

/// A `--working-directory` value; one that is not valid Unicode can only
/// be a path, kept as given.
fn start_directory(value: OsString) -> WorkingDirectory {
    match value.to_str() {
        Some(text) => WorkingDirectory::parse(text),
        None => WorkingDirectory::Path(PathBuf::from(value)),
    }
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
            elevate: false,
            working_directory: None,
        }
    }

    #[test]
    fn no_arguments_runs_with_the_default_config() {
        assert_eq!(
            run(&[]),
            Ok(Command::Run {
                config: None,
                elevate: false,
                working_directory: None
            })
        );
    }

    #[test]
    fn elevate_combines_with_config() {
        assert_eq!(
            run(&["--elevate"]),
            Ok(Command::Run {
                config: None,
                elevate: true,
                working_directory: None
            })
        );
        assert_eq!(
            run(&["--elevate", "--config", "a.toml"]),
            Ok(Command::Run {
                config: Some(PathBuf::from("a.toml")),
                elevate: true,
                working_directory: None
            })
        );
        assert_eq!(
            run(&["--config=a.toml", "--elevate"]),
            Ok(Command::Run {
                config: Some(PathBuf::from("a.toml")),
                elevate: true,
                working_directory: None
            })
        );
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

    fn with_working_directory(value: WorkingDirectory) -> Command {
        Command::Run {
            config: None,
            elevate: false,
            working_directory: Some(value),
        }
    }

    #[test]
    fn working_directory_takes_a_separate_or_inline_value() {
        assert_eq!(
            run(&["--working-directory", "home"]),
            Ok(with_working_directory(WorkingDirectory::Home))
        );
        assert_eq!(
            run(&["--working-directory=inherit"]),
            Ok(with_working_directory(WorkingDirectory::Inherit))
        );
        assert_eq!(
            run(&["--working-directory", "auto"]),
            Ok(with_working_directory(WorkingDirectory::Auto))
        );
        assert_eq!(
            run(&[
                "--working-directory=~/src",
                "--working-directory",
                "sub dir"
            ]),
            Ok(with_working_directory(WorkingDirectory::Path(
                "sub dir".into()
            )))
        );
        assert_eq!(
            run(&["--working-directory", "-x"]),
            Ok(with_working_directory(WorkingDirectory::Path("-x".into())))
        );
    }

    #[test]
    fn missing_working_directory_value_is_an_error() {
        let error = run(&["--working-directory"]).unwrap_err();
        assert!(error.contains("--working-directory"), "{error}");
        assert!(run(&["--working-directory="]).is_err());
    }

    #[test]
    fn update_is_a_subcommand() {
        assert_eq!(
            run(&["update"]),
            Ok(Command::Update {
                check: false,
                force: false
            })
        );
        assert_eq!(
            run(&["update", "--check"]),
            Ok(Command::Update {
                check: true,
                force: false
            })
        );
        assert_eq!(
            run(&["update", "--force", "--check"]),
            Ok(Command::Update {
                check: true,
                force: true
            })
        );
        assert_eq!(run(&["update", "--help"]), Ok(Command::Help));
    }

    #[test]
    fn update_rejects_other_arguments() {
        let error = run(&["update", "--config", "a.toml"]).unwrap_err();
        assert!(error.contains("`--config`"), "{error}");
        assert!(run(&["update", "now"]).is_err());
        // Only the first argument names a subcommand.
        assert!(run(&["--elevate", "update"]).is_err());
    }

    #[test]
    fn usage_documents_update() {
        assert!(USAGE.contains("nxgterm update [--check] [--force]"));
        assert!(USAGE.contains("NXGTERM_UPDATE_URL"));
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
                config: Some(PathBuf::from(path)),
                elevate: false,
                working_directory: None
            })
        );
    }
}
