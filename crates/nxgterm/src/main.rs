//! nxgterm: window, shell in a pty, GPU-rendered text with a CPU fallback.

mod app;
mod appearance;
mod bindings;
mod choice;
mod cli;
mod keys;
mod reload;
mod watch;

use std::env;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nxg_config::{Config, DEFAULT_CONFIG_TOML, Platform};
use winit::event_loop::{EventLoop, EventLoopProxy};

use crate::app::{App, UserEvent};
use crate::cli::Command;

fn main() -> ExitCode {
    let command = match cli::parse(env::args_os().skip(1)) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("nxgterm: {error}\n\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };
    match command {
        Command::Help => print!("{}", cli::USAGE),
        Command::Version => println!("nxgterm {}", env!("CARGO_PKG_VERSION")),
        Command::PrintConfig => print!("{DEFAULT_CONFIG_TOML}"),
        Command::Run { config } => {
            if let Err(error) = run(config) {
                eprintln!("nxgterm: {error}");
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}

fn run(config_arg: Option<PathBuf>) -> Result<(), Box<dyn Error>> {
    let path = config_arg
        .or_else(|| nxg_config::config_path(Platform::current(), |name| env::var_os(name)));
    let config = path.as_deref().map(load_config).unwrap_or_default();

    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    // Kept alive for the whole run; dropping it stops watching.
    let _watcher = path
        .as_deref()
        .and_then(|path| start_watcher(path, proxy.clone()));
    let mut app = App::new(proxy, config, path);
    event_loop.run_app(&mut app)?;
    match app.take_error() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// The config at `path`; defaults when it is missing or invalid, so a
/// typo never stops the terminal from opening.
fn load_config(path: &Path) -> Config {
    match Config::load(path) {
        Ok(config) => config.unwrap_or_default(),
        Err(error) => {
            eprintln!("nxgterm: {error}; using the defaults");
            Config::default()
        }
    }
}

/// Watches `path` for live reload; failing to watch is not fatal.
fn start_watcher(
    path: &Path,
    proxy: EventLoopProxy<UserEvent>,
) -> Option<notify::RecommendedWatcher> {
    let watcher = watch::watch(path, move || {
        let _ = proxy.send_event(UserEvent::ConfigChanged);
    });
    watcher
        .map_err(|error| eprintln!("nxgterm: config live reload disabled: {error}"))
        .ok()
}
