# Configuration Specification

## Purpose

The optional TOML configuration (`nxg-config`, pure and OS-free), where it
is found, how it is validated, the built-in themes, the command-line
interface, live reload, and the key bindings (`nxgterm`).

Sources: `crates/nxg-config/src/{lib,path,theme,color,keybindings}.rs`,
`crates/nxgterm/src/{main,cli,reload,watch,bindings,appearance}.rs`.

## Requirements

### Requirement: Config file location

The path MUST be resolved in this order: `--config <path>`, then
`NXGTERM_CONFIG` (when non-empty), then the platform default. On Linux and
macOS the default SHALL be `$XDG_CONFIG_HOME/nxgterm/nxgterm.toml` when
`XDG_CONFIG_HOME` is absolute, else `$HOME/.config/nxgterm/nxgterm.toml`. On
Windows it SHALL be `%APPDATA%\nxgterm\nxgterm.toml`. When no location can
be derived, nxgterm MUST run with defaults and without live reload.

When neither `--config` nor `NXGTERM_CONFIG` is in effect and the default
file does not exist, nxgterm MUST create its directory and write the
`--print-config` sample there at startup, logging
`nxgterm: wrote the default config to <path>`. An existing file MUST NOT be
overwritten. A failure to write MUST only be reported as a warning; the
terminal MUST still open with the defaults.

#### Scenario: Relative XDG_CONFIG_HOME is ignored
- GIVEN `XDG_CONFIG_HOME=relative/dir` and `HOME=/home/u`
- WHEN the path is resolved on Unix
- THEN it is `/home/u/.config/nxgterm/nxgterm.toml`

#### Scenario: First run generates the file
- GIVEN no file at the default location and no override
- WHEN nxgterm starts
- THEN the documented sample is written there and parses to the defaults

#### Scenario: Read-only config directory
- GIVEN a default location that cannot be written
- WHEN nxgterm starts
- THEN stderr shows a warning and the window opens with defaults

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
| `font.fallback` | list of family names searched per missing glyph; blank names dropped | `[]` (built-in defaults still apply) |
| `font.size` | integer or float points, clamped to 6-72; non-finite = 14 | `14.0` |
| `window.padding` | u16 logical pixels | `4` |
| `window.columns`, `window.rows` | non-zero u16 initial cells | `100`, `30` |
| `window.tab_bar` | `auto` (two or more tabs), `always`, `never` | `auto` |
| `colors.theme` | built-in theme name, case-insensitive | `catppuccin-mocha` |
| `colors.foreground`/`background`/`cursor` | `#rrggbb` or `#rgb` | from theme |
| `colors.ansi` | exactly 16 colors (normal 0-7, bright 8-15) | from theme |
| `colors.selection_foreground`/`selection_background` | `#rrggbb` or `#rgb`; unset swaps the colors of selected cells | from theme (unset for most) |
| `shell.program`, `shell.args` | string; blank = unset / list | platform default / `[]` |
| `renderer.backend` | `auto` \| `gpu` \| `cpu` | `auto` |
| `scrollback.lines` | usize history lines; `0` disables the scrollback | `10000` |
| `selection.copy_on_select` | bool; copy finished selections to PRIMARY (Linux X11/Wayland only, ignored elsewhere) | `true` |
| `keybindings` | table of `"chord" = "action"` (or `"none"`), merged over the defaults | see Key bindings |

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

nxgterm MUST ship the themes `catppuccin-mocha` (default, the official
Mocha palette with selection on surface2), `nxg-dark` (original xterm
colors), `nxg-light`, `tokyo-night`, `gruvbox-dark`, `dracula`, `nord` and
`one-dark`. A theme MAY define selection colors; the others swap the colors
of selected cells. Explicit `foreground`, `background`, `cursor`, `ansi` and
selection values MUST override the theme individually. ANSI colors 16-255
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
200 ms of quiet. Font family, fallback families, font size, colors, padding and
`window.tab_bar` MUST apply immediately (restyle and refit the grid); `scrollback.lines` MUST apply
immediately, dropping the oldest lines beyond a lower limit; `[keybindings]`
MUST apply to the next key press. Changes to `[shell]`,
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

### Requirement: Key bindings

Actions MUST have stable snake_case names: `zoom_in`, `zoom_out`,
`reset_zoom`, `scroll_page_up`, `scroll_page_down`, `scroll_to_top`,
`scroll_to_bottom`, `new_tab`, `close_tab`, `next_tab`, `previous_tab`,
`goto_tab_1`..`goto_tab_9`, `command_palette`, `copy`, `paste`,
`select_all` and `reload_config`, each with a title and a category for
listing. The defaults MUST be: Ctrl (Cmd on
macOS) with `=` or `+` for `zoom_in`, `-` for `zoom_out` and `0` for
`reset_zoom`; Shift+PageUp/PageDown/Home/End for the scroll actions;
Ctrl+Shift+T/W for `new_tab`/`close_tab`; Ctrl+Tab and Ctrl+Shift+Tab for
`next_tab`/`previous_tab`; Alt+1..9 for `goto_tab_N`; Ctrl+Shift+P for
`command_palette`; Ctrl+Shift+C for `copy` and Ctrl+Shift+V for `paste`
(Cmd+C and Cmd+V on macOS), plus Shift+Insert for `paste`. `select_all` and
`reload_config` MUST be unbound by default. Ctrl+C and Ctrl+V MUST reach
the shell. `copy` without a selection MUST do nothing, and MUST NOT write to
the PTY.

`[keybindings]` entries MUST be added over the defaults; `"none"` MUST
remove a binding. Chords are modifiers (`ctrl`, `alt`, `shift`,
`super`/`cmd`) and one key joined with `+`, in any order and case. An
invalid chord, an unknown action or a chord bound twice MUST be a config
error naming it. A chord MUST match exactly the modifiers held, except that
Shift MAY be ignored when it produced a non-letter character (`+` from
Shift+`=`). Matched keys MUST NOT reach the shell, except scroll actions on
the alternate screen. Zoom results MUST be clamped to 6-72, and changing the
configured size on reload MUST discard the zoom. Every action with the label
of its shortcut (e.g. `Ctrl+Shift+T`) MUST be listable.

#### Scenario: Zoom on macOS
- GIVEN macOS
- WHEN Cmd+= is pressed
- THEN the font grows one point and nothing is written to the PTY

#### Scenario: Ctrl+Alt is not a binding
- GIVEN Linux
- WHEN Ctrl+Alt+= is pressed
- THEN no zoom happens

#### Scenario: Unbinding a default
- GIVEN `[keybindings] "ctrl+tab" = "none"`
- WHEN Ctrl+Tab is pressed
- THEN the key is sent to the shell

#### Scenario: Unknown action
- GIVEN `[keybindings] "ctrl+t" = "cut"`
- WHEN the config is loaded
- THEN the error names `unknown action `cut`` and lists the actions

#### Scenario: Copy keeps Ctrl+C for the shell
- GIVEN Linux and a selection
- WHEN Ctrl+C is pressed
- THEN the shell receives ETX and the clipboard is unchanged
- AND Ctrl+Shift+C copies the selected text

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
