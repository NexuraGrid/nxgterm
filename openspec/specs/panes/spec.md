# Panes Specification

## Purpose

Split a tab into a nested layout of panes, each with its own terminal and
shell: the pure split tree, focus, resize, zoom, close semantics and mouse
routing.

Sources: `crates/nxgterm/src/{panes,app,mouse}.rs`.

## Requirements

### Requirement: Split tree per tab

Each tab MUST own one binary split tree whose leaves are panes. A new tab MUST
have exactly one pane. A split MUST replace the focused leaf with a split of
that pane and a new pane (own terminal and shell, launch cwd), and MUST focus
the new pane. Pane ids (`PaneId`) MUST be global and never reused. Splitting
below the minimum pane size MUST be refused and leave the tree unchanged.

#### Scenario: Split right
- GIVEN one pane in a tab
- WHEN `split_right` runs
- THEN two panes sit side by side and the new one has focus

#### Scenario: Split refused when too small
- GIVEN a pane whose halves would fall under the minimum size
- WHEN `split_down` runs
- THEN the tree is unchanged and nothing panics

### Requirement: Cell-aligned layout

Pane rects MUST be whole cells, separated by a 1-cell divider. Each pane's
terminal and pty MUST be sized to its rect after every split, close, resize,
zoom, window resize and restyle, including hidden tabs.

#### Scenario: Pty sizes follow rects
- GIVEN a 100x30 content area split right
- WHEN the layout is computed
- THEN the pane widths plus the divider cell equal 100 and both ptys get their pane's size

### Requirement: Focus

Exactly one pane per tab MUST have focus. Keyboard input, paste, copy, select
all and the command palette MUST target the focused pane. `focus_*` actions
(left, right, up, down) MUST move focus to the adjacent pane in that
direction and do nothing when none exists. The tab label MUST come from the
focused pane.

#### Scenario: Directional focus
- GIVEN panes A | B and A focused
- WHEN `focus_pane_right` runs
- THEN B has focus and typed keys reach B's shell

### Requirement: Resize, equalize and zoom

`resize_*` actions MUST move the nearest divider in that direction by one
step (2 columns horizontally, 1 row vertically), keeping both panes at least the minimum size. `equalize_panes` MUST
reset every split to equal halves. `zoom_pane` MUST toggle the focused pane to
fill the content area while the others keep their shells running; any split,
close or focus change MUST end zoom.

#### Scenario: Zoom toggle
- GIVEN panes A | B with A focused
- WHEN `zoom_pane` runs twice
- THEN A fills the area, then the A | B layout returns with the same sizes

### Requirement: Closing panes

`close_pane` or a shell exit MUST remove that pane and give its space to the
sibling, which MUST take focus if the closed pane had it. Closing the last
pane MUST close the tab; closing the last tab MUST exit the application.
Output or exit events for a closed pane MUST be ignored.

#### Scenario: Shell exits in a split
- GIVEN panes A | B
- WHEN B's shell exits
- THEN A fills the area and has focus

#### Scenario: Late event
- GIVEN pane B was closed
- WHEN an `Output` event for B's id arrives
- THEN it is dropped

### Requirement: Output routing

Output from any pane of the active tab MUST redraw; output from other tabs
MUST NOT.

#### Scenario: Background tab output
- GIVEN two tabs
- WHEN the inactive tab's pane prints
- THEN no redraw is requested

### Requirement: Mouse routing

A left press inside a pane MUST focus it. Pointer events MUST be hit-tested
against pane rects and reported in pane-relative cells; a press MUST capture
its pane until release so selections and drags stay within it. Dragging a
divider MUST resize the adjacent panes, with pty resizes throttled. Pointer
and drag state MUST reset on close, zoom and tab switch.

#### Scenario: Click focuses
- GIVEN panes A | B and A focused
- WHEN the left button is pressed inside B
- THEN B has focus

#### Scenario: Divider drag
- GIVEN panes A | B
- WHEN the divider is dragged two cells right
- THEN A is two cells wider and B two cells narrower

#### Scenario: Close during drag
- GIVEN a selection drag started in B
- WHEN B is closed
- THEN the drag state is cleared
