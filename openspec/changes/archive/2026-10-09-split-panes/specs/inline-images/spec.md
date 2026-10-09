# Delta for Inline Images

## ADDED Requirements

### Requirement: Images in panes

Placements MUST be positioned and clipped relative to their pane's rect on
both renderers, honoring the pane's horizontal cell offset (`layout.left`)
as well as vertical. An image MUST NOT draw outside its pane, over a
divider or over the padding. Each pane's images MUST keep the existing z-order
relative to that pane's text. Placements MUST remain per pane terminal.

#### Scenario: Image in the right pane
- GIVEN panes A | B and a red image placed at B's cell (0,0)
- WHEN rendered on CPU
- THEN red pixels start at B's left edge and none appear in A

#### Scenario: Clipped at pane edge
- GIVEN an image wider than B
- WHEN rendered
- THEN pixels beyond B's rect, including the divider and padding, stay unchanged

#### Scenario: Origin pane unchanged
- GIVEN an image in a pane at column 0
- WHEN rendered
- THEN output equals the pre-change frame
