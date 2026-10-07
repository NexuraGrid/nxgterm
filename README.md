# nxgterm

Fast, configurable, cross-platform terminal emulator written in Rust.

## Platforms

Linux (Arch, Debian, Ubuntu, Fedora), macOS, and Windows 10 / Windows Server 2016 or newer.

## Architecture

Hexagonal: a platform-agnostic core defines ports, and adapters implement them per platform.

| Crate | Role |
|---|---|
| `nxg-core` | Terminal state (VT parser, grid), ports (`PtySession`, `Renderer`), runtime backend fallback |
| `nxg-pty` | PTY adapters: Unix pty, ConPTY, winpty (Server 2016) |
| `nxg-render` | `Renderer` adapters: wgpu GPU (glyph atlas, instanced quads) and CPU (softbuffer); fonts via fontdb + fontdue |
| `nxg-config` | Configuration, themes, fonts, key bindings |
| `nxgterm` | Application binary wiring everything together |

Backends are chosen at runtime with `nxg_core::fallback::first_available`, so the
terminal always starts: ConPTY falls back to winpty, GPU falls back to CPU.

The GPU renderer skips software adapters (WARP, llvmpipe), where the CPU
renderer is lighter, and is replaced by the CPU renderer at runtime if the
device is lost. Set `NXGTERM_RENDERER=gpu` or `cpu` to force one; the
choice and any skipped renderer are printed to stderr.

## Configuration

nxgterm works without a config file. To customize it, create a TOML file at:

| OS | Location |
|---|---|
| Linux, macOS | `$XDG_CONFIG_HOME/nxgterm/nxgterm.toml`, or `~/.config/nxgterm/nxgterm.toml` |
| Windows | `%APPDATA%\nxgterm\nxgterm.toml` |

`nxgterm --config <path>` or `NXGTERM_CONFIG=<path>` uses another file
(`--config` wins). Start from the documented defaults:

```sh
nxgterm --print-config > ~/.config/nxgterm/nxgterm.toml
```

Every key is optional:

```toml
[font]
family = "JetBrains Mono"   # falls back to the system monospace font
size = 14.0                 # points at 100% scale, clamped to 6-72

[window]
padding = 4                 # pixels around the grid at 100% scale
columns = 100               # initial size in cells
rows = 30

[colors]
theme = "nxg-dark"
# Optional overrides on top of the theme:
# foreground = "#c0caf5"
# background = "#1a1b26"
# cursor = "#c0caf5"
# ansi = ["#15161e", ...]   # exactly 16 colors: normal 0-7, bright 8-15

[shell]
program = "pwsh.exe"        # default: $SHELL on Unix; pwsh, powershell, cmd on Windows
args = ["-NoLogo"]

[renderer]
backend = "auto"            # auto | gpu | cpu
```

Built-in themes: `nxg-dark` (default), `nxg-light`, `tokyo-night`,
`catppuccin-mocha`, `gruvbox-dark`, `dracula`, `nord`, `one-dark`.

The file is reloaded when saved. Font, colors and padding apply at once;
`[shell]`, `[renderer]` and the initial `columns`/`rows` apply on the next
start. An invalid file is reported on stderr (with the line and the reason,
unknown keys included) and the previous settings stay in effect; at startup
the defaults are used instead.

### Key bindings

| Keys (Cmd instead of Ctrl on macOS) | Action |
|---|---|
| `Ctrl+=` / `Ctrl++` | Increase font size |
| `Ctrl+-` | Decrease font size |
| `Ctrl+0` | Reset font size to the configured one |

These are handled by the terminal and never reach the shell.

### Environment

| Variable | Effect |
|---|---|
| `NXGTERM_CONFIG` | Config file path |
| `NXGTERM_RENDERER` | `auto`, `gpu` or `cpu`; overrides `[renderer] backend` |

## Images

nxgterm shows inline images from programs such as Yazi, `kitten icat`,
`chafa`, `timg` and `img2sixel`:

- Kitty graphics protocol: transmit, display, place, delete and query;
  PNG, RGB and RGBA data, sent directly (chunked, optionally zlib
  compressed), from a file or from a temporary file. Unicode placeholders
  (needed inside multiplexers) and animation are not supported yet.
- Sixel, with sixel scrolling (DECSDM) and the VT340 palette.

Child processes see `TERM_PROGRAM=nxgterm` and the pixel size of the
window (`TIOCGWINSZ`, `CSI 14/16/18 t`). Decoded images are capped at
256 MiB; the oldest are evicted first.

## Roadmap

1. Workspace skeleton and CI ✅
2. Window + PTY + text (CPU renderer) ✅
3. GPU renderer with automatic fallback ✅
4. Configuration, themes, fonts ✅
5. Images: Kitty graphics protocol and Sixel ✅
6. winpty fallback for Windows Server 2016
7. Packages (AUR, deb, rpm, winget, Homebrew) and the tools profile
   (Yazi, zoxide, ngmux, Bruno CLI, curl)

## Development

```sh
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all
```
