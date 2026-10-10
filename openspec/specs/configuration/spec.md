# Configuration Specification

## Purpose

The optional TOML configuration (`nxg-config`, pure and OS-free), where it
is found, how it is validated, the built-in themes, the command-line
interface, live reload, and the key bindings (`nxgterm`).

Sources: `crates/nxg-config/src/{lib,load,path,theme,color,keybindings}.rs`,
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
| `import` | list of config files merged first, in order (see Imports) | `[]` |
| `font.family` | string or list of strings, first installed one wins; blank names dropped | system monospace |
| `font.fallback` | list of family names searched per missing glyph; blank names dropped | `[]` (built-in defaults still apply) |
| `font.size` | integer or float points, clamped to 6-72; non-finite = 14 | `14.0` |
| `window.padding` | u16 logical pixels | `8` |
| `window.columns`, `window.rows` | non-zero u16 initial cells | `100`, `30` |
| `window.tab_bar` | `auto` (two or more tabs), `always`, `never` | `auto` |
| `window.decorations` | `integrated` (the tab bar is the title bar) or `native` (system title bar) | `integrated` |
| `window.opacity` | integer or float opacity of the default background, clamped to 0.0-1.0; `nan`/`inf` are errors | `1.0` |
| `window.blur` | bool; ask the system to blur behind a translucent background | `false` |
| `colors.theme` | theme file name in `themes/`, else built-in theme name (case-insensitive) | `catppuccin-mocha` |
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
selection values MUST override the theme individually. The window
SHOULD ask the system for a dark title bar when the effective background is
dark (white contrasts with it more than black) and a light one otherwise,
again whenever the colors are reloaded. ANSI colors 16-255 SHALL follow the
xterm cube and grayscale ramp regardless of theme.

#### Scenario: Partial override
- GIVEN `theme = "nord"` and `foreground = "#010203"`
- WHEN colors are resolved
- THEN the foreground is `#010203` and the background is nord's

### Requirement: Theme files

`theme = "<name>"` MUST first read `<config dir>/themes/<name>.toml`, where
the config dir is the directory of the main config file, and only then fall
back to the built-in theme of that name; names that are not plain file
names (with a path separator, `.` or `..`) MUST skip the file lookup. A
theme file MUST accept the `[colors]` override keys at its top level or
under `[colors]`, reject other keys, and take the keys it leaves out from
the default theme. The config's own `[colors]` overrides MUST apply on top.
An unknown name MUST be an error at the `theme` line of the file that set
it, listing the built-in themes and the theme files found.

#### Scenario: Custom theme
- GIVEN `themes/custom-theme.toml` with only `foreground = "#010203"`
- WHEN the config sets `theme = "custom-theme"`
- THEN the foreground is `#010203` and the other colors are catppuccin-mocha's

### Requirement: Imports

A top-level `import` list MUST merge the named files first, in order, with
the importing file's own values overriding them; tables MUST merge key by
key and any other value (lists included) MUST be replaced by the later one.
Paths MUST be relative to the importing file's directory, with a leading `~`
expanded to the home directory; absolute paths MUST work. Imported files MAY
import others; cycles and nesting deeper than 8 levels MUST be errors, and so
MUST a missing or unreadable import, naming the importing file and the
import. Every file MUST be checked alone first, so errors name the file and
line they are in.

#### Scenario: Main file wins
- GIVEN `import = ["a.toml", "b.toml"]`, both setting `font.size`, and the
  main file setting `window.padding` like `a.toml`
- WHEN the config is loaded
- THEN the size is `b.toml`'s and the padding the main file's

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

The parent directories of the config file, its imports and its theme file
MUST be watched (so editors that save by rename are handled; directories
that do not exist yet are tried again after each reload), events for other
files, reads and access-time updates MUST be ignored, and bursts MUST be debounced to one reload after
200 ms of quiet. Font family, fallback families, font size, colors, padding,
`window.tab_bar` and `window.opacity` (on a window created transparent) MUST
apply immediately (restyle and refit the grid); `window.blur` MUST be asked of
the system again; `scrollback.lines` MUST apply
immediately, dropping the oldest lines beyond a lower limit; `[keybindings]`
MUST apply to the next key press. Changes to `[shell]`,
`[renderer]`, `window.columns`/`rows` and `window.decorations` MUST be reported as
`nxgterm: <section> changes apply on restart`; an opacity below 1.0 on a
window created opaque MUST be reported as
`nxgterm: window opacity changes apply on restart`. A deleted file MUST reload as
the defaults. A failure to start watching MUST only disable live reload.

#### Scenario: Color change applies live
- GIVEN a running terminal
- WHEN `background` is changed and saved
- THEN the next frame uses it and stderr shows `nxgterm: config reloaded`

#### Scenario: Shell change deferred
- GIVEN a running terminal
- WHEN `[shell] program` changes
- THEN stderr shows `nxgterm: shell changes apply on restart` and the shell keeps running

### Requirement: Window opacity

With `window.opacity` below 1.0 at start, the window MUST be created
transparent; at 1.0 it MUST NOT be, and drawing MUST stay as without the
option. Only the default background MUST take the opacity: the padding and
the grid cells whose background is the palette background. Text, the
cursor, other cell backgrounds, the selection, the tab bar, the command
palette and images MUST stay opaque. The GPU renderer MUST write
premultiplied colors and ask for a premultiplied (else post-multiplied)
surface alpha mode. On Windows the DX12 backend MUST present a window that
starts translucent through a DirectComposition visual (wgpu's
`DxgiFromVisual` swapchain, which offers premultiplied alpha), keep the
window-handle swapchain for opaque windows, and show a translucent window
only after its first frame is drawn (the visual is white until then). When
the surface offers no blending mode (OpenGL, most Vulkan drivers on
Windows), or the renderer is the CPU one (softbuffer has no alpha), the
background MUST stay opaque and stderr MUST show
`nxgterm: the <renderer> renderer cannot draw a translucent window here; the background stays opaque`.
The CPU renderer MUST set the alpha byte of a transparent window's pixels so
it never shows through. `window.blur` MUST ask for a blurred backdrop only on
a transparent window: winit's blur on macOS and on Wayland with KDE's blur
protocol, the Acrylic system backdrop on Windows 11.

#### Scenario: Translucent background on Wayland
- GIVEN `[window] opacity = 0.85` and the GPU renderer on Vulkan
- WHEN nxgterm starts
- THEN default-background cells and the padding are 85% opaque and colored cells are opaque

#### Scenario: Translucent background on Windows
- GIVEN `[window] opacity = 0.85` and the GPU renderer on DX12
- WHEN nxgterm starts
- THEN the surface presents through DirectComposition, the window appears after its first frame and default-background cells are 85% opaque

#### Scenario: Surface without alpha
- GIVEN `[window] opacity = 0.85` and the OpenGL backend (`WGPU_BACKEND=gl`)
- WHEN nxgterm starts
- THEN stderr reports that the gpu renderer cannot draw a translucent window and the window is opaque

#### Scenario: Opacity lowered while running
- GIVEN a terminal started with `opacity = 1.0`
- WHEN `opacity = 0.9` is saved
- THEN stderr shows `nxgterm: window opacity changes apply on restart` and the window stays opaque

### Requirement: Integrated title bar

With `window.decorations = "native"` the window MUST keep the system title
bar and borders and the tab bar MUST behave as `window.tab_bar` says. With
`integrated`, applied at start, the tab bar MUST be the window's title bar:

- It MUST always show, whatever `window.tab_bar` says, and the initial window
  size MUST include its row.
- On Windows and Linux the window MUST be created without decorations (on
  Windows with the undecorated drop shadow) and the bar MUST end with
  minimize, maximize/restore and close buttons that act when the left button
  is released over the one it was pressed on; the button under the pointer
  MUST be highlighted, close in red. The buttons MUST sit flush with the
  window's top-right corner, span the bar's height and be at least 46
  logical pixels wide (46:32 on a taller bar), with anti-aliased vector
  glyphs (a line, a square, two overlapping squares while maximized, an X)
  that scale with the display scale and the bar height, never with the
  font's glyphs.
- On macOS the window MUST keep its frame with a transparent, hidden title
  and full-size content; the bar MUST leave room on its left for the native
  window buttons and MUST NOT draw its own.
- A `+` button after the last tab, when it fits, MUST open a new tab.
- A left press on the empty bar MUST move the window; a second one within
  the double-click time MUST maximize or restore it instead. A right press
  there SHOULD open the system window menu (Windows).
- Outside macOS and while not maximized or fullscreen, a left press within
  5 logical pixels of a window edge (corners twice as far along the edges)
  MUST resize the window from that edge or corner, and the pointer MUST
  show the matching resize cursor there. These presses MUST NOT reach the
  selection, the command palette or terminal mouse reporting.
- Clicking a tab label MUST still switch to it, and key bindings MUST NOT
  change.

#### Scenario: One tab with an integrated title bar on Windows
- GIVEN `[window] decorations = "integrated"` on Windows
- WHEN nxgterm starts with one tab
- THEN the window has no system title bar, the bar shows ` 1: pwsh `, `+` and the three window buttons, and the grid is below it

#### Scenario: Resizing from an edge
- GIVEN an integrated title bar on Linux and a program with mouse reporting on
- WHEN the left button is pressed on the right window edge and dragged
- THEN the window resizes and the program receives no button report

#### Scenario: Decorations changed while running
- GIVEN a running terminal with integrated decorations
- WHEN `decorations = "native"` is saved
- THEN stderr shows `nxgterm: window decorations changes apply on restart`

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

### Requirement: Panes section

An optional `[panes]` section MUST accept `divider_color` (`#rrggbb` or
`#rgb`), `divider_width` (non-zero integer pixels, clamped to the cell) and
`inactive_dim` (0.0-1.0, `nan`/`inf` are errors). Every key is optional and
unknown keys MUST be errors. `--print-config` MUST document the keys and
still parse to exactly the defaults.

#### Scenario: Invalid dim
- GIVEN `[panes] inactive_dim = 2`
- WHEN parsed
- THEN the value is clamped to 1.0

#### Scenario: Unknown key
- GIVEN `[panes] gap = 1`
- WHEN parsed
- THEN the error names `gap` and the file line

### Requirement: Pane key bindings

New actions MUST have stable names and a `Panes` category: `split_right`,
`split_down`, `focus_pane_left`, `focus_pane_right`, `focus_pane_up`, `focus_pane_down`,
`resize_pane_left`, `resize_pane_right`, `resize_pane_up`, `resize_pane_down`, `close_pane`,
`zoom_pane`, `equalize_panes`. Defaults on Linux and Windows MUST be
Ctrl+Shift+O (`split_right`), Ctrl+Shift+E (`split_down`), Ctrl+Alt+arrows
(`focus_*`), Ctrl+Shift+Alt+arrows (`resize_*`), Ctrl+Shift+X (`close_pane`)
and Ctrl+Shift+Enter (`zoom_pane`); `equalize_panes` MUST be unbound. macOS
MUST follow the existing Cmd/Ctrl convention. All MUST be rebindable or
removable with `"none"` and listed in the command palette.

#### Scenario: Default split
- GIVEN default bindings on Linux
- WHEN Ctrl+Shift+O is pressed
- THEN `split_right` runs and nothing reaches the shell

#### Scenario: Rebinding
- GIVEN `"ctrl+shift+o" = "none"`
- WHEN Ctrl+Shift+O is pressed
- THEN the key goes to the shell

#### Scenario: Palette entry
- GIVEN the command palette is open
- WHEN `equalize` is typed
- THEN `equalize_panes` is listed

### Requirement: Panes live reload

Changes to `[panes]` MUST apply on the next frame without restart.

#### Scenario: Dim changed live
- GIVEN a running split tab
- WHEN `inactive_dim` is saved as 0.5
- THEN the next frame dims inactive panes by 0.5 and stderr shows `nxgterm: config reloaded`
