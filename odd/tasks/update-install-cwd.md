# Feature: self-update, install scripts and start directory

## Objective

Make nxgterm easy to install and update from the command line, and open new
terminals in a sensible directory.

## Problem

- Users update by downloading release assets by hand from GitHub.
- There is no one-line install command.
- New windows start in the process working directory. The Windows Start Menu
  shortcut sets `WorkingDirectory='APPLICATIONFOLDER'`
  (`crates/nxgterm/wix/main.wxs:118`), so the shell opens in
  `Program Files\nxgterm` instead of the user's home. `spawn_pane`
  (`crates/nxgterm/src/app.rs:1900`) never passes a cwd to the PTY.
  - Verified in T1: with no cwd, portable-pty starts the shell in home
    (`cmdbuilder.rs` `get_home_dir`), so ConPTY and Unix never inherited the
    launching shell's directory; only winpty (Server 2016) inherited the process
    cwd, which produced the `Program Files` start.

## Decisions

- Start directory follows Ghostty (user decision, 2026-10-14): launched from a
  terminal, inherit its cwd; launched from a launcher/shortcut, open in home.
  - `[shell] working_directory` and `--working-directory <inherit|home|PATH>`
    (flag beats config). Default is `auto`.
  - `auto`: inherit the process cwd unless it is unusable, `/`, the directory
    containing the executable, or the Windows/System32 directory; then home.
  - Tradeoff: launchers that run with an arbitrary cwd (for example rofi) still
    inherit it; `working_directory = "home"` covers that.
- `nxgterm update` detects how it was installed and only self-replaces
  binaries no package manager owns:
  - portable zip/tarball: download, verify `SHA256SUMS`, replace in place
    (rename the running files on Windows);
  - MSI: download the `.msi`, verify, run `msiexec`;
  - `.deb`/`.rpm` in `/usr/bin`: download, verify, run `sudo apt install` /
    `sudo dnf install`;
  - AUR, Homebrew, other: print the package-manager command.
  - The latest release is read from
    `https://github.com/NexuraGrid/nxgterm/releases/latest/download/SHA256SUMS`
    (asset names carry the version), so no GitHub API or JSON is needed.
  - HTTP is embedded (`ureq` + `rustls`): Windows Server 2016 has no `curl.exe`.
- Install scripts: `install.sh` (Linux/macOS) and `install.ps1` (Windows),
  served from `main` via raw.githubusercontent.com, verifying checksums.

## Delivery

- Strategy: `ask-on-risk`. Forecast about 1200 authored changed lines across
  three slices, one PR per task.

## Tasks

- [x] T1 Start directory: `working_directory` config + `--working-directory`
      flag + `auto` resolution, pass the cwd to every spawned shell, document it.
      Route: delegated (writer trigger: config, cli, app, pty, docs).
- [ ] T2 `nxgterm update` and `nxgterm update --check`: release lookup, asset
      selection, checksum verification, install-kind detection, apply.
      Route: delegated.
- [ ] T3 `install.sh` / `install.ps1` and README install/update docs.
      Route: delegated.

## Acceptance criteria

- Opening nxgterm from the Windows Start Menu or a Linux desktop launcher starts
  the shell in home; running `nxgterm` from a shell starts it in that shell's
  directory.
- `nxgterm update --check` reports the current and latest versions;
  `nxgterm update` installs the latest release for the detected install kind,
  refusing to install anything whose checksum does not match.
- One command installs nxgterm on Linux, macOS and Windows.

## Checks

- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` (CI, `.github/workflows/ci.yml`).

## Progress

- T1 done: `WorkingDirectory` in `crates/nxg-config/src/working_dir.rs`, cwd
  plumbed through `nxg_pty::spawn_shell_with` (winpty passes it to
  `winpty_spawn_config_new`), resolved once at startup, `--elevate` forwards it.
  Checks: fmt, clippy (Linux + windows-gnu), `cargo test --workspace` all green;
  parent re-ran `cargo test -p nxg-pty --test spawn` (8 passed). Not run on real
  Windows yet. Config paths must be absolute or `~`; CLI paths may be relative.
- T1 review: high risk, consent granted, 4-lens native review approved and
  acknowledged (lineage review-2430ce3d46c3feff, commit 89cc602). Advisory
  follow-ups (non-blocking): lossy non-UTF-8 path round-trip in
  `working_dir.rs:64`, `~` rule repeated in `elevation.rs:112`, CLI-over-config
  precedence untested in `main.rs`, winpty cwd not exercised on real Windows.
- Next step: T2.
