# Delta for Rendering

## ADDED Requirements

### Requirement: Multi-pane drawing

`WindowRenderer` MUST draw an ordered list of panes (terminal, cell
position, focused flag) plus the header, overlay and shapes. Each pane MUST
be drawn at its cell offset, keeping window-level padding, in the same order
(backgrounds, images below, cursor/text, images above) on the CPU and GPU
renderers. Dividers MUST be 1 cell wide with a line drawn in its color and
width. A single pane at the origin MUST render exactly as before.

#### Scenario: Two panes on CPU
- GIVEN panes A | B with different content
- WHEN rendered
- THEN each pane's cells appear only inside its rect and the divider cell shows the divider line

#### Scenario: GPU matches CPU with panes
- GIVEN the same two-pane content
- WHEN rendered offscreen by both renderers
- THEN every channel differs by at most 2

#### Scenario: Single pane unchanged
- GIVEN one pane at the origin
- WHEN rendered
- THEN the output equals the pre-change single-terminal frame

### Requirement: Inactive pane treatment

An unfocused pane MUST show a hollow cursor outline instead of a solid block
(none when the cursor is hidden) and MUST be dimmed by `panes.inactive_dim`.
The focused pane MUST NOT be dimmed. A dim of 0 MUST disable dimming.

#### Scenario: Hollow cursor
- GIVEN panes A | B, B focused, A's cursor visible
- WHEN rendered
- THEN A's cursor cell is an outline with its center unfilled

#### Scenario: Dimming off
- GIVEN `inactive_dim = 0`
- WHEN rendered
- THEN inactive pane colors equal the focused rendering
