# Terminal Core Specification

## Purpose

The platform-agnostic terminal state machine in `nxg-core`: it turns child
output bytes into a fixed-size grid of styled cells, tracks the cursor, and
queues replies to terminal queries. It defines the ports that adapters
implement and the runtime fallback helper. It MUST NOT depend on window,
GPU or PTY APIs (see the known exception in `inline-images`).

Sources: `crates/nxg-core/src/{terminal,grid,cell,size,apc,ports,fallback}.rs`.

## Requirements

### Requirement: Grid and size

The terminal MUST hold a row-major grid of `cols x rows` cells, each with a
character, foreground color, background color and attribute flags. A size
MUST be at least 1x1; the default size SHALL be 80x24. Resizing MUST keep
the top-left content, blank new cells, clamp the cursor into the new grid
and cancel any pending wrap.

#### Scenario: Zero dimensions are rejected
- GIVEN a request for a 0x24 or 80x0 terminal
- WHEN `TermSize::new` is called
- THEN it returns `SizeError::ZeroDimension`

#### Scenario: Resize keeps top-left content
- GIVEN a 3x3 grid with `a` at (0,0) and `z` at (2,2)
- WHEN it is resized to 2x4
- THEN `a` remains at (0,0), `z` is gone and new rows are blank

### Requirement: Printing and line discipline

Printable characters MUST be written at the cursor with the current pen
(colors and flags). After printing in the last column the cursor MUST stay
there with a pending wrap; the next print wraps to column 0 of the next line.
`CR` MUST move to column 0 and cancel a pending wrap. `LF`, `VT` and `FF`
MUST move down one row keeping the column. A line feed on the last row MUST
scroll the whole grid up one row, filling the new row with blanks that keep
the pen background. `BS` MUST move left, stopping at column 0. `TAB` MUST
move to the next multiple of 8, clamped to the last column. `BEL` and other
C0 controls SHALL be ignored. Every character SHALL occupy exactly one cell
(wide characters are not yet supported).

#### Scenario: Deferred wrap
- GIVEN a 3-column terminal
- WHEN `abc` is printed
- THEN the cursor is at column 2 of row 0
- AND printing `d` places it at column 0 of row 1

#### Scenario: Scrolling at the bottom
- GIVEN a 5x2 terminal
- WHEN `one\r\ntwo\r\nsix` is fed
- THEN row 0 reads `two` and row 1 reads `six`

### Requirement: Cursor movement and erasing

The terminal MUST support CUU/CUD/CUF/CUB (`CSI A/B/C/D`), CUP/HVP
(`CSI row;col H`/`f`, 1-based), CHA (`CSI G`) and VPA (`CSI d`). A count or
position of 0 MUST be treated as 1 and every move MUST clamp to the grid.
ED (`CSI J` modes 0, 1, 2, 3) and EL (`CSI K` modes 0, 1, 2) MUST erase
with blanks keeping the pen background. ED 2 and 3 MUST also remove every
image placement. DECTCEM (`CSI ? 25 h/l`) MUST show or hide the cursor.
Sequences with intermediates other than `?` MUST be ignored.

#### Scenario: CUP is one-based and clamps
- GIVEN a 10x5 terminal
- WHEN `CSI 99;99 H` is fed
- THEN the cursor is at column 9, row 4

#### Scenario: Full clear removes images
- GIVEN a terminal with an image placement
- WHEN `CSI 2 J` is fed
- THEN the grid is blank and no placements remain, but the image data is kept

### Requirement: SGR attributes and colors

SGR (`CSI m`) MUST support reset (0 or no parameters), bold (1/22), italic
(3/23), underline (4/24), inverse (7/27), the 8 standard and 8 bright
foreground/background colors (30-37, 40-47, 90-97, 100-107), default colors
(39/49), 256-color (`38;5;n`, `48;5;n`) and truecolor (`38;2;r;g;b`) in both
semicolon and colon (`38:2[:cs]:r:g:b`, `38:5:n`) forms. Values above 255
MUST be clamped to 255. Unknown SGR codes MUST be ignored.

#### Scenario: Truecolor in colon form
- GIVEN a terminal
- WHEN `CSI 38:2::10:20:30 m X` is fed
- THEN the cell `X` has foreground `Rgb(10, 20, 30)`

### Requirement: Query replies

Replies MUST be queued in input order and drained with `take_responses`;
the application MUST write them back to the PTY. The terminal SHALL answer:

| Query | Reply |
|---|---|
| DSR status `CSI 5 n` | `CSI 0 n` |
| DSR cursor `CSI 6 n` | `CSI row;col R` (1-based) |
| DA1 `CSI c` / `CSI 0 c` | `CSI ? 62;4;22 c` (VT220, sixel, ANSI color) |
| XTWINOPS `CSI 14 t` | `CSI 4;height;width t` (text area pixels) |
| XTWINOPS `CSI 16 t` | `CSI 6;cellHeight;cellWidth t` |
| XTWINOPS `CSI 18 t` | `CSI 8;rows;cols t` |
| XTSMGRAPHICS `CSI ? 1 S` | `CSI ? 1;0;256 S` |
| XTSMGRAPHICS `CSI ? 2 S` | `CSI ? 2;0;w;h S` (text area, capped at 4096) |
| XTSMGRAPHICS other item n | `CSI ? n;1;0 S` (error) |

#### Scenario: Cursor position report unblocks ConPTY
- GIVEN the cursor at column 4, row 2 (zero-based)
- WHEN `CSI 6 n` is fed
- THEN `take_responses` returns `ESC [ 3 ; 5 R`
- AND a second call returns nothing

### Requirement: APC pre-filter

Because vte 0.15 discards APC strings, the terminal MUST pass child output
through a streaming `ApcFilter` that splits it into text runs (for vte) and
complete APC payloads (`ESC _ ... ESC \`, also terminated by BEL), preserving
order. Sequences split across `advance` calls MUST be reassembled. `CAN` or
`SUB` MUST abort an APC. Payloads longer than 4 MiB (`MAX_APC`) MUST be
dropped whole. Before handling an APC the terminal MUST feed `ESC \` to vte
so any string it was inside ends, and only payloads starting with `G` (kitty
graphics) SHALL be acted on.

#### Scenario: APC split across reads
- GIVEN an APC `ESC _ Gi=1;... ESC \` delivered in two chunks
- WHEN both chunks are fed through `advance`
- THEN exactly one kitty command is executed and surrounding text prints normally

### Requirement: Ports

`nxg-core` MUST define:
- `PtySession { reader, control, child }`: a `Read + Send` reader drained on a
  background thread; a `PtyControl` (`Write + Send` plus `resize(WinSize)`)
  owned by the UI thread; a `ChildProcess` with a blocking `wait()`.
  Dropping the control is expected to terminate the child.
- `Renderer { name, cell_size, resize, draw }` returning
  `RenderError::Transient` (frame skipped, renderer still usable) or
  `RenderError::Fatal` (renderer must be replaced).
- `WinSize { cells, cell: Option<CellPixels> }` whose `pixels()` is the text
  area clamped to `u16`, or `(0, 0)` when the cell size is unknown.

#### Scenario: Pixel size unknown
- GIVEN a `WinSize` built from cells only
- WHEN `pixels()` is called
- THEN it returns `(0, 0)`

### Requirement: Runtime backend fallback

`fallback::first_available` MUST try named, lazily constructed candidates in
order and return the first success together with every skipped attempt
(name and error). Candidates after the selected one MUST NOT be constructed.
When every candidate fails it MUST return all attempts in order; an empty
list MUST fail with no attempts.

#### Scenario: GPU fails, CPU is selected
- GIVEN candidates `gpu` (fails with "no adapter") and `cpu` (succeeds)
- WHEN `first_available` runs
- THEN `cpu` is selected and `skipped` holds one attempt named `gpu`
