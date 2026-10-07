# Delta for Inline Images

## MODIFIED Requirements

### Requirement: Scrolling, clearing and resize

Placements MUST belong to a screen. Entering the alternate screen MUST stash
the main placements and start with none; leaving MUST drop the alternate
placements and restore the main ones. Deleting an image MUST also remove its
stashed placements. When the whole grid scrolls (no partial region),
placements MUST move up with it and be dropped once entirely off screen.
When a partial scroll region scrolls, only placements anchored inside the
region MUST move, and they MUST be dropped once entirely outside it;
placements outside the region MUST NOT move. ED 2/3 and RIS MUST remove all
placements of the active screen (RIS also the stashed ones) but keep image
data (except as removed by deletion).
(Previously: a single placement set; scrolling always moved every placement.)

#### Scenario: Image scrolls away
- GIVEN a one-row image placed on row 0
- WHEN the screen scrolls up one line
- THEN the placement is removed

#### Scenario: Alternate screen hides main placements
- GIVEN an image placed on the main screen
- WHEN `CSI ? 1049 h` is fed
- THEN no placements are visible
- AND after `CSI ? 1049 l` the original placement is back

#### Scenario: Alternate placements dropped on leave
- GIVEN an image placed while on the alternate screen
- WHEN `CSI ? 1049 l` is fed
- THEN that placement is gone

#### Scenario: Deleted image leaves no stashed placement
- GIVEN image 5 placed on main, then `?1049h`
- WHEN `a=d,d=I,i=5` is handled and `?1049l` is fed
- THEN no placement of image 5 exists

#### Scenario: Partial region scroll
- GIVEN region rows 3-6, placements on row 1 and row 4, and a LF at row 6
- WHEN the region scrolls up one line
- THEN the row-1 placement is unmoved and the row-4 placement is on row 3
