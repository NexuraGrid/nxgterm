# nxgterm

Fast, configurable, cross-platform terminal emulator written in Rust.

## Platforms

Linux (Arch, Debian, Ubuntu, Fedora), macOS, and Windows 10 / Windows Server 2016 or newer.

## Install

Packages are attached to each [GitHub Release](https://github.com/NexuraGrid/nxgterm/releases)
together with a `SHA256SUMS` file. Replace `<v>` with the version.

| OS | Install |
|---|---|
| Arch Linux (AUR) | `yay -S nxgterm-bin` (release binary) or `yay -S nxgterm` (build from source) |
| Debian 12+, Ubuntu 22.04+ | `sudo apt install ./nxgterm_<v>-1_amd64.deb` (or `_arm64.deb`) |
| Fedora | `sudo dnf install ./nxgterm-<v>-1.x86_64.rpm` (or `.aarch64.rpm`) |
| Other Linux | extract `nxgterm-<v>-x86_64-linux.tar.gz` and put `nxgterm` on your `PATH` |
| Windows (winget) | `winget install NexuraGrid.nxgterm` |
| Windows (MSI) | run `nxgterm-<v>-x86_64.msi`: Program Files, Start Menu shortcut, optional `PATH` entry |
| Windows (portable) | extract `nxgterm-<v>-x86_64-windows.zip` anywhere |
| macOS (Homebrew) | `brew install --cask nexuragrid/tap/nxgterm` |
| macOS (manual) | open `nxgterm-<v>-universal-macos.dmg` and drag `nxgterm.app` to Applications |

The macOS app is universal (Apple silicon and Intel) but not notarized yet.
If macOS says it cannot be opened, run
`xattr -dr com.apple.quarantine /Applications/nxgterm.app` once.

From source (Rust 1.87 or newer):

```sh
cargo install --locked --git https://github.com/NexuraGrid/nxgterm nxgterm
```

Building needs no system libraries: X11, Wayland, xkbcommon and the GPU
drivers are loaded at runtime. Maintainers: see [`packaging/`](packaging/README.md).

## Tools profile

nxgterm does not bundle any tools. An optional, separate installer sets up a
terminal workflow: [Yazi](https://github.com/sxyazi/yazi) (file manager),
[zoxide](https://github.com/ajeetdsouza/zoxide) (smarter `cd`),
[ngmux](https://github.com/NexuraGrid/ng_mux) (terminal multiplexer),
[Bruno CLI](https://www.usebruno.com/) (`bru`, API client) and curl, and
gives Yazi the Catppuccin Mocha flavor so it matches nxgterm's default theme.

```sh
# Linux, macOS
curl -fsSL https://raw.githubusercontent.com/NexuraGrid/nxgterm/main/profile/install.sh | sh -s -- --yes
```

```powershell
# Windows PowerShell 5.1 or PowerShell 7
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/NexuraGrid/nxgterm/main/profile/install.ps1))) -Yes
```

Without `--yes` / `-Yes` it asks before using sudo, installing Node.js or
running the ngmux installer (plain `irm ... | iex` works too, interactively).
The scripts also ship with every package: `/usr/share/nxgterm/profile/`,
`profile\` next to `nxgterm.exe`, and `nxgterm.app/Contents/Resources/profile/`.

| Option (`install.sh` / `install.ps1`) | Effect |
|---|---|
| `--dry-run` / `-DryRun` | Print what would be done; change nothing |
| `--yes` / `-Yes` | Do not ask |
| `--only yazi,zoxide` / `-Only yazi,zoxide` | Only these tools (`curl`, `zoxide`, `yazi`, `yazi-theme`, `ngmux`, `bruno`) |
| `--skip bruno` / `-Skip bruno` | Skip these tools (`--skip yazi-theme` keeps your Yazi theme setup as is) |
| `--no-shell-init` / `-NoShellInit` | Leave shell startup files alone |
| `--uninstall-shell-init` / `-UninstallShellInit` | Remove the shell integration and exit |

How each tool is installed:

| Tool | Arch | Debian, Ubuntu | Fedora | macOS | Windows |
|---|---|---|---|---|---|
| Yazi | `pacman` | release binary | release binary | `brew` | winget `sxyazi.yazi` / scoop `yazi` |
| Yazi theme (`yazi-theme`) | pinned flavor download | same | same | same | same |
| zoxide | `pacman` | `apt` | `dnf` | `brew` | winget `ajeetdsouza.zoxide` / scoop `zoxide` |
| ngmux | official `install.sh` | official `install.sh` | official `install.sh` | official `install.sh` | official `install.ps1` |
| Bruno CLI | `npm` (`@usebruno/cli`) | `npm` | `npm` | `npm` | `npm` |
| Node.js LTS (for `bru`) | `nodejs-lts`, `npm` | `nodejs`, `npm` | `nodejs`, `nodejs-npm` | `node` | winget `OpenJS.NodeJS.LTS` / scoop `nodejs-lts` |
| curl | `pacman` | `apt` | `dnf` | preinstalled | built in (`curl.exe`); winget `cURL.cURL` / scoop `curl` |

Release binaries (Yazi on Debian/Ubuntu/Fedora; zoxide and Yazi wherever no
package manager is available, such as Windows Server 2016) come from the
projects' latest GitHub release, are checked against the SHA-256 digest GitHub
publishes for the asset, and go to `~/.local/bin`
(`%LOCALAPPDATA%\Programs\nxgterm-profile\bin` on Windows). Re-running is
safe: installed tools are detected and skipped.

The shell integration adds `zoxide init`, Yazi's `y` wrapper (changes
directory when Yazi exits) and, on Linux and macOS, `~/.local/bin` on `PATH` to `~/.bashrc`, `~/.zshrc`,
`~/.config/fish/conf.d/nxgterm-profile.fish` and the PowerShell profiles,
between `# >>> nxgterm profile >>>` and `# <<< nxgterm profile <<<` markers.

`yazi-theme` is selected whenever `yazi` is. It downloads the official
[`catppuccin-mocha`](https://github.com/yazi-rs/flavors/tree/main/catppuccin-mocha.yazi)
flavor (MIT) from a pinned `yazi-rs/flavors` commit, checks every file against
a SHA-256 kept in the script, and puts it in Yazi's config directory
(`$YAZI_CONFIG_HOME`, else `~/.config/yazi` or `%AppData%\yazi\config`) under
`flavors/catppuccin-mocha.yazi`. A plain download is used instead of
`ya pkg add` so it needs no git (Windows Server 2016) and works when piped.
Then, only if there is no `theme.toml`, it creates one with:

```toml
[flavor]
dark = "catppuccin-mocha"
```

An existing `theme.toml` is never changed: the summary shows `skipped` and
the two lines to add yourself. To let `ya pkg` manage the flavor later,
delete `flavors/catppuccin-mocha.yazi` and run
`ya pkg add yazi-rs/flavors:catppuccin-mocha`.

On Windows, Yazi detects file types with `file.exe`, which Windows does not
ship. `install.ps1` points the user variable `YAZI_FILE_ONE` at the copy in
Git for Windows (`<Git>\usr\bin\file.exe`), installing Git first if it is
missing. Without it, Yazi previews fail with "Cannot find `file`". Open a
new terminal afterwards so the variable takes effect.

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

### PTY backends

| Backend | Platforms |
|---|---|
| `native` | Unix pty; ConPTY on Windows 10 1809+ / Server 2019+ (via `portable-pty`) |
| `winpty` | Windows without ConPTY, e.g. Windows Server 2016 (x86_64 only) |

On Windows the native backend first checks that `kernel32.dll` exports
`CreatePseudoConsole`; when it does not, nxgterm falls back to winpty. Set
`NXGTERM_PTY=native` or `winpty` to force one (`auto` is the default). The
chosen backend and any skipped one are printed to stderr, e.g.
`nxgterm: skipped native: ConPTY is unavailable ...` then `nxgterm: pty winpty`.

winpty 0.4.3 (`winpty.dll` and `winpty-agent.exe`, x64, MIT license) is
embedded in the Windows executable and unpacked to
`%LOCALAPPDATA%\nxgterm\winpty\<version>-<hash>\` the first time it is used.
Provenance, hashes and license: [`crates/nxg-pty/winpty`](crates/nxg-pty/winpty/README.md).

The Windows zip and MSI also ship Microsoft's standalone ConPTY
(`conpty.dll` beside `nxgterm.exe`, `x64\OpenConsole.exe`, MIT license in
`LICENSE-conpty`), which the native backend prefers over the one built into
Windows. With `nxgterm: pty native`, stderr also shows
`nxgterm: conpty bundled ...` or `nxgterm: conpty inbox ...`.
Provenance and hashes: [`packaging/windows/conpty`](packaging/windows/conpty/README.md).

## Configuration

nxgterm reads a TOML file at:

| OS | Location |
|---|---|
| Linux, macOS | `$XDG_CONFIG_HOME/nxgterm/nxgterm.toml`, or `~/.config/nxgterm/nxgterm.toml` |
| Windows | `%APPDATA%\nxgterm\nxgterm.toml` |

On the first run nxgterm writes the documented defaults there (every key
commented or set to its default), so there is a file to edit. An existing
file is never overwritten, and a location that cannot be written only
prints a warning. `nxgterm --print-config` prints the same documented
defaults.

`nxgterm --config <path>` or `NXGTERM_CONFIG=<path>` uses another file
(`--config` wins); such a file is never generated.

Every key is optional:

```toml
[font]
family = "JetBrains Mono"   # falls back to the system monospace font
fallback = ["Symbols Nerd Font Mono"]  # for glyphs the family lacks, e.g. icons
size = 14.0                 # points at 100% scale, clamped to 6-72

[window]
padding = 8                 # pixels around the grid at 100% scale
columns = 100               # initial size in cells
rows = 30
tab_bar = "auto"            # auto (2+ tabs) | always | never
decorations = "integrated"  # integrated (the tab bar is the title bar) | native
opacity = 1.0               # default background opacity, 0.0-1.0
blur = false                # blur behind a translucent background

[colors]
theme = "catppuccin-mocha"
# Optional overrides on top of the theme:
# foreground = "#c0caf5"
# background = "#1a1b26"
# cursor = "#c0caf5"
# ansi = ["#15161e", ...]   # exactly 16 colors: normal 0-7, bright 8-15
# selection_foreground = "#c0caf5"  # unset: the theme's, or inverted cells
# selection_background = "#33467c"

[shell]
program = "pwsh.exe"        # default: $SHELL on Unix; pwsh, powershell, cmd on Windows
args = ["-NoLogo"]

[renderer]
backend = "auto"            # auto | gpu | cpu

[scrollback]
lines = 10000               # history kept on the main screen; 0 disables it

[selection]
copy_on_select = true       # selected text goes to PRIMARY (Linux only)

[keybindings]
"ctrl+shift+r" = "reload_config"  # "chord" = "action", added to the defaults
"ctrl+tab" = "none"               # free a default chord for the shell
```

Built-in themes: `catppuccin-mocha` (default), `nxg-dark` (xterm colors),
`nxg-light`, `tokyo-night`, `gruvbox-dark`, `dracula`, `nord`, `one-dark`.

`opacity` below 1.0 makes the default background translucent, like
Alacritty and WezTerm: the padding and cells with the theme background let
the desktop show through, while text, the cursor, colored cells, the
selection, the tab bar, the command palette and images stay opaque. It needs
the GPU renderer and a surface that can blend: it works on macOS, on Wayland
and on X11 with a compositor (Vulkan), and on Windows with the DX12 backend
(the default there), which then presents through DirectComposition. With the
OpenGL backend, usually with Vulkan on Windows, and with the CPU renderer the
background stays opaque and a line on stderr says so. `blur = true` blurs what is behind the window on
macOS, KDE Plasma (Wayland) and Windows 11 (Acrylic).

The file is reloaded when saved. Font, colors, padding, the tab bar, opacity,
blur, the scrollback limit and key bindings apply at once; `[shell]`, `[renderer]`, the initial
`columns`/`rows`, `decorations` and lowering `opacity` from 1.0 (the window is created
transparent only then) apply on the next start. An invalid file is reported on stderr (with the line and the reason,
unknown keys included) and the previous settings stay in effect; at startup
the defaults are used instead.

### Key bindings

| Keys | Action |
|---|---|
| `Ctrl+=` / `Ctrl++` (`Cmd` on macOS) | `zoom_in`: increase font size |
| `Ctrl+-` (`Cmd` on macOS) | `zoom_out`: decrease font size |
| `Ctrl+0` (`Cmd` on macOS) | `reset_zoom`: back to the configured size |
| `Shift+PageUp` / `Shift+PageDown` | `scroll_page_up` / `scroll_page_down` |
| `Shift+Home` / `Shift+End` | `scroll_to_top` / `scroll_to_bottom` |
| `Ctrl+Shift+T` / `Ctrl+Shift+W` | `new_tab` / `close_tab` |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | `next_tab` / `previous_tab` |
| `Alt+1` ... `Alt+9` | `goto_tab_1` ... `goto_tab_9` |
| `Ctrl+Shift+P` | `command_palette`: list and run every action |
| `Ctrl+Shift+C` (`Cmd+C` on macOS) | `copy`: the selection to the clipboard |
| `Ctrl+Shift+V` (`Cmd+V` on macOS), `Shift+Insert` | `paste`: the clipboard into the shell |
| (unbound) | `select_all`, `reload_config` |

Bound keys are handled by the terminal and never reach the shell (the
scrolling keys still do in full-screen programs, which have no history).
Change them in `[keybindings]` with `"chord" = "action"`: a chord
is modifiers (`ctrl`, `alt`, `shift`, `super`/`cmd`) and one key joined
with `+` in any order and case, where the key is a character, `f1`-`f24`,
`tab`, `enter`, `escape`, `space`, `backspace`, `delete`, `insert`, `home`,
`end`, `pageup`, `pagedown`, `up`, `down`, `left`, `right`, `plus`, `minus`
or `equal`. Map a default chord to `"none"` to unbind it. Invalid chords and
unknown actions are reported like any other config error.

### Selection, copy and paste

| Input | Action |
|---|---|
| Left drag | Select characters; dragging above or below the grid scrolls the history |
| `Alt` + left drag | Select a block (rectangle) |
| Double click / triple click | Select a word / a whole line (wrapped lines included) |
| Single click | Clear the selection |
| Middle click | Paste the PRIMARY selection (Linux) |

Selected text stays selected while the view scrolls and output moves it into
the history; output that changes a selected line, a resize or switching
screens clears it. Copied text has trailing blanks trimmed and lines that
wrapped automatically joined. `Ctrl+C` stays an interrupt for the shell, and
copy without a selection does nothing. On Linux (X11 and Wayland) every
selection is also copied to the PRIMARY selection (`selection.copy_on_select`).
Words stop at spaces and ``()[]{}<>'"`,;:│``, so paths select whole.

Pasted text has its line breaks sent as Enter (`\r`) and other control
characters, escape included, removed. Programs that enable bracketed paste
(mode 2004) receive it between `ESC [200~` and `ESC [201~`, so a pasted
command does not run by itself. When a program uses the mouse, hold `Shift`
to select or middle-click paste instead.

### Command palette

`Ctrl+Shift+P` opens a box over the terminal listing every action with its
current shortcut (from the defaults and `[keybindings]`), so it doubles as
an index of the key bindings. Type to filter by title, category or action
name (fuzzy, case-insensitive); `Up`/`Down` (or `Ctrl+P`/`Ctrl+N`),
`PageUp`/`PageDown`, `Home`/`End` and the mouse wheel move the selection;
`Enter` or a click runs it. `Escape`, `Ctrl+Shift+P` again or a click
outside the box closes it. While it is open, keys and the mouse do not
reach the shell; its output keeps showing underneath.

### Tabs

`Ctrl+Shift+T` opens a tab next to the current one. Every tab runs the
configured `[shell]` (or the default shell) on its own pty, starting in the
directory nxgterm was started in (the shell's current directory is not
tracked yet), with its own history and modes; background tabs keep running.
A tab closes when its program exits or with `Ctrl+Shift+W`, and nxgterm
exits with the last one. `Alt+N` goes to tab N (`Alt+9` past the last tab
goes to the last).

With two or more tabs (see `window.tab_bar`) a bar takes the top row,
labelled ` N: program `, the current tab highlighted; click a label to
switch to it. Labels are the program name; titles set by programs (OSC 0/2)
are not shown yet.

With `window.decorations = "integrated"` (the default) the tab bar replaces
the system title bar, as in Windows Terminal or a browser. It always shows, even with
one tab, and has a `+` button after the last tab that opens a new one. Drag
its empty part to move the window and double-click it to maximize or
restore. On Windows and Linux it draws minimize, maximize/restore and close
buttons on the right (close turns red under the pointer), the window keeps a
drop shadow on Windows, and the outer 5 pixels (at 100% scale) of the window
resize it, except while maximized; right-clicking the empty bar opens the
window menu on Windows. On macOS the native window buttons stay at the left
of the bar and the system resizes the window. Without the system title bar
some desktop features are gone: the Windows 11 snap layouts shown when
hovering the maximize button (dragging to a screen edge still snaps, and
Win+arrow keys work) and, on Linux, the window manager's title bar menu.

### Scrolling and the mouse

| Input | Action |
|---|---|
| Mouse wheel | Scroll the history, 3 lines per notch |
| `Shift+PageUp` / `Shift+PageDown` | Scroll the history by a page |
| `Shift+Home` / `Shift+End` | Jump to the oldest line / back to the bottom |

New output or typing returns to the bottom. In full-screen programs (the
alternate screen) the wheel sends arrow keys instead, unless the program
turns that off (mode 1007). Programs that ask for mouse reports (modes
1000, 1002, 1003; SGR 1006 or urxvt 1015 encoding) receive clicks, motion
and the wheel; hold `Shift` to scroll the terminal instead.

### Environment

| Variable | Effect |
|---|---|
| `NXGTERM_CONFIG` | Config file path |
| `NXGTERM_RENDERER` | `auto`, `gpu` or `cpu`; overrides `[renderer] backend` |
| `NXGTERM_PTY` | `auto`, `native` or `winpty`; forces a PTY backend |

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

On Windows, images need the ConPTY bundled in the zip and MSI
(`conpty.dll` and `x64\OpenConsole.exe` next to `nxgterm.exe`). The ConPTY
built into Windows drops Kitty graphics and Sixel output and answers the
terminal queries itself, so programs such as Yazi fall back to text. A bare
`nxgterm.exe` built with cargo uses the inbox ConPTY; copy both files next to
it (see [`packaging/windows/conpty`](packaging/windows/conpty/README.md)).
winpty (Windows Server 2016) does not pass images through either.

## Roadmap

1. Workspace skeleton and CI ✅
2. Window + PTY + text (CPU renderer) ✅
3. GPU renderer with automatic fallback ✅
4. Configuration, themes, fonts ✅
5. Images: Kitty graphics protocol and Sixel ✅
6. winpty fallback for Windows Server 2016 ✅
7. Packages (AUR, deb, rpm, winget, Homebrew) and the tools profile
   (Yazi, zoxide, ngmux, Bruno CLI, curl) ✅
8. 0.2.0: scrollback, mouse wheel and mouse reporting, font fallback for
   icons, configurable key bindings, tabs, command palette ✅
9. 0.3.0: text selection, copy and paste, bracketed paste ✅
10. 0.4.0: inline images on Windows through a bundled ConPTY ✅
11. 0.5.0: Catppuccin Mocha default theme, title bar matching the theme,
    window icon ✅

Planned work is tracked in
[`openspec/RECOMMENDATIONS.md`](openspec/RECOMMENDATIONS.md).

## Development

```sh
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all
```
