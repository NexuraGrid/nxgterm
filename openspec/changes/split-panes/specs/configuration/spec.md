# Delta for Configuration

## ADDED Requirements

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
