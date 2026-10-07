# Configuration Specification

## Purpose

The optional TOML configuration (`nxg-config`, pure and OS-free), where it
is found, how it is validated, the built-in themes, the command-line
interface, live reload, and the font zoom key bindings (`nxgterm`).

Sources: `crates/nxg-config/src/{lib,path,theme,color}.rs`,
`crates/nxgterm/src/{main,cli,reload,watch,bindings,appearance}.rs`.

## Requirements

### Requirement: Config file location

The path MUST be resolved in this order: `--config <path>`, then
`NXGTERM_CONFIG` (when non-empty), then the platform default. On Linux and
macOS the default SHALL be `$XDG_CONFIG_HOME/nxgterm/nxgterm.toml` when
`XDG_CONFIG_HOME` is absolute, else `$HOME/.config/nxgterm/nxgterm.toml`. On
Windows it SHALL be `%APPDATA%\nxgterm\nxgterm.toml`. When no location can
be derived, nxgterm MUST run with defaults and without live reload.

#### Scenario: Relative XDG_CONFIG_HOME is ignored
- GIVEN `XDG_CONFIG_HOME=relative/dir` and `HOME=/home/u`
- WHEN the path is resolved on Unix
- THEN it is `/home/u/.config/nxgterm/nxgterm.toml`

#### Scenario: CLI wins over environment
- GIVEN `NXGTERM_CONFIG=/a.toml`
- WHEN nxgterm runs with `--config /b.toml`
- THEN `/b.toml` is loaded and watched

### Requirement: Schema and defaults

Every section and key MUST be optional; a missing file, an empty file or a
missing key MUST yield the defaults. Unknown keys MUST be errors. The schema:

| Key | Type | Default |
|---|---|---|
| `font.family` | string; blank = unset | system monospace |
| `font.size` | integer or float points, clamped to 6-72; non-finite = 14 | `14.0` |
| `window.padding` | u16 logical pixels | `4` |
| `window.columns`, `window.rows` | non-zero u16 initial cells | `100`, `30` |
| `colors.theme` | built-in theme name, case-insensitive | `nxg-dark` |
| `colors.foreground`/`background`/`cursor` | `#rrggbb` or `#rgb` | from theme |
| `colors.ansi` | exactly 16 colors (normal 0-7, bright 8-15) | from theme |
| `shell.program`, `shell.args` | string; blank = unset / list | platform default / `[]` |
| `renderer.backend` | `auto` \| `gpu` \| `cpu` | `auto` |

`--print-config` MUST print a documented sample that parses to exactly the
defaults.

#### Scenario: Sample config round trip
- GIVEN the output of `nxgterm --print-config`
- WHEN it is parsed
- THEN it equals `Config::default()`

#### Scenario: Font size clamped
- GIVEN `[font] size = 2`
- WHEN parsed
- THEN the size is 6.0

### Requirement: Themes and overrides

nxgterm MUST ship the themes `nxg-dark` (default, original xterm colors),
`nxg-light`, `tokyo-night`, `catppuccin-mocha`, `gruvbox-dark`, `dracula`,
`nord` and `one-dark`. Explicit `foreground`, `background`, `cursor` and
`ansi` values MUST override the theme individually. ANSI colors 16-255
SHALL follow the xterm cube and grayscale ramp regardless of theme.

#### Scenario: Partial override
- GIVEN `theme = "nord"` and `foreground = "#010203"`
- WHEN colors are resolved
- THEN the foreground is `#010203` and the background is nord's

### Requirement: Invalid config handling

Parse errors MUST name the file path, the line and the reason (unknown key,
unknown theme with the list of available names, invalid color). At startup
an invalid or unreadable file MUST be reported as
`nxgterm: <error>; using the defaults` and the terminal MUST still open.
On reload it MUST be reported as `...; keeping the previous config` and the
running settings MUST stay unchanged.

#### Scenario: Typo at startup
- GIVEN a config with `[fonts]`
- WHEN nxgterm starts
- THEN stderr names the unknown key `fonts` and the window opens with defaults

#### Scenario: Typo while running
- GIVEN a running terminal with theme `dracula`
- WHEN the file is saved with `theme = "solarized"`
- THEN stderr reports `unknown theme `solarized`` with the available names
- AND the colors stay `dracula`

### Requirement: Live reload

The parent directory of the config file MUST be watched (so editors that
save by rename are handled), events for other files, reads and access-time
updates MUST be ignored, and bursts MUST be debounced to one reload after
200 ms of quiet. Font family, font size, colors and padding MUST apply
immediately (restyle and refit the grid). Changes to `[shell]`,
`[renderer]` and `window.columns`/`rows` MUST be reported as
`nxgterm: <section> changes apply on restart`. A deleted file MUST reload as
the defaults. A failure to start watching MUST only disable live reload.

#### Scenario: Color change applies live
- GIVEN a running terminal
- WHEN `background` is changed and saved
- THEN the next frame uses it and stderr shows `nxgterm: config reloaded`

#### Scenario: Shell change deferred
- GIVEN a running terminal
- WHEN `[shell] program` changes
- THEN stderr shows `nxgterm: shell changes apply on restart` and the shell keeps running

### Requirement: Display scale

Font size (points) and padding (logical pixels) MUST be multiplied by the
window scale factor; a non-positive or non-finite scale MUST count as 1.0. A
scale factor change MUST re-rasterize the font and refit the grid.

#### Scenario: HiDPI
- GIVEN padding 4 and scale 1.5
- WHEN the style is built
- THEN padding is 6 physical pixels

### Requirement: Font zoom key bindings

The primary modifier (Ctrl; Cmd on macOS) with `=` or `+` MUST increase the
font size by 1 point, with `-` decrease it by 1, and with `0` reset it to the
configured size; results MUST be clamped to 6-72. Shift MAY also be held;
any other extra modifier MUST NOT match. Matched keys MUST NOT reach the
shell. Changing the configured size on reload MUST discard the zoom.

#### Scenario: Zoom on macOS
- GIVEN macOS
- WHEN Cmd+= is pressed
- THEN the font grows one point and nothing is written to the PTY

#### Scenario: Ctrl+Alt is not a binding
- GIVEN Linux
- WHEN Ctrl+Alt+= is pressed
- THEN no zoom happens

### Requirement: Command-line interface

nxgterm MUST accept `--config <path>` and `--config=<path>` (last wins; the
value MAY start with `-`), `--print-config`, `-V`/`--version` and
`-h`/`--help`. Informational flags MUST act as soon as they are seen. A
missing `--config` value or any unknown argument MUST print the error and
usage and exit with status 2. Non-UTF-8 paths MUST be preserved on Unix.

#### Scenario: Unknown flag
- GIVEN `nxgterm --colour`
- WHEN it runs
- THEN stderr shows ``unknown argument `--colour``` plus usage and the exit code is 2
