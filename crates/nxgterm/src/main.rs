//! nxgterm: window, shell in a pty, GPU-rendered text with a CPU fallback.

// Release builds on Windows are GUI programs so launching them does not open
// a console window; see `attach_parent_console` for CLI output.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod appearance;
mod bindings;
mod choice;
mod cli;
mod clipboard;
mod command_palette;
mod dividers;
mod icon;
mod keys;
mod mouse;
mod panes;
mod reload;
mod tab;
mod tab_bar;
mod tabs;
mod throttle;
mod title_bar;
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
    attach_parent_console();
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
    let env_var = |name: &str| env::var_os(name);
    let default_location = config_arg.is_none() && !nxg_config::has_env_override(env_var);
    let path = config_arg.or_else(|| nxg_config::config_path(Platform::current(), env_var));
    if default_location {
        if let Some(path) = &path {
            generate_config(path);
        }
    }
    let (config, files) = path.as_deref().map(load_config).unwrap_or_default();

    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    // Kept by the app for the whole run; dropping it stops watching.
    let watcher = path
        .as_deref()
        .and_then(|path| start_watcher(path, &files, proxy.clone()));
    let mut app = App::new(proxy, config, path, watcher);
    event_loop.run_app(&mut app)?;
    match app.take_error() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// The config at `path` and the files it was built from (its imports and
/// theme file); defaults when it is missing or invalid, so a typo never
/// stops the terminal from opening.
fn load_config(path: &Path) -> (Config, Vec<PathBuf>) {
    match Config::load_with_files(path) {
        Ok(Some(loaded)) => (loaded.config, loaded.files),
        Ok(None) => Default::default(),
        Err(error) => {
            eprintln!("nxgterm: {error}; using the defaults");
            Default::default()
        }
    }
}

/// Writes the documented defaults to the default config location on first
/// run, so there is a file to edit. Failing (e.g. a read-only home) only
/// warns: the terminal runs with the defaults either way.
fn generate_config(path: &Path) {
    match nxg_config::write_default(path) {
        Ok(true) => eprintln!("nxgterm: wrote the default config to {}", path.display()),
        Ok(false) => {}
        Err(error) => eprintln!(
            "nxgterm: cannot write the default config to {}: {error}",
            path.display()
        ),
    }
}

/// Watches `path` and the other config `files` for live reload; failing
/// to watch is not fatal.
fn start_watcher(
    path: &Path,
    files: &[PathBuf],
    proxy: EventLoopProxy<UserEvent>,
) -> Option<watch::ConfigWatcher> {
    let watcher = watch::ConfigWatcher::new(path, files, move || {
        let _ = proxy.send_event(UserEvent::ConfigChanged);
    });
    watcher
        .map_err(|error| eprintln!("nxgterm: config live reload disabled: {error}"))
        .ok()
}

/// Reconnects stdout/stderr to the launching console (e.g. `nxgterm --version`
/// from PowerShell), since GUI-subsystem programs start without one. No-op
/// when started from Explorer or with redirected output.
#[cfg(windows)]
fn attach_parent_console() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: plain Win32 call without pointers; failure just means there is
    // no parent console to attach to.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
fn attach_parent_console() {}
