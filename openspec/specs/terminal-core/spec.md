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
the top-left content, blank new cells, clamp the cursor of both screens into
the new grid, cancel any pending wrap and reset the scroll region to the
full screen.

#### Scenario: Zero dimensions are rejected
- GIVEN a request for a 0x24 or 80x0 terminal
- WHEN `TermSize::new` is called
- THEN it returns `SizeError::ZeroDimension`

#### Scenario: Resize keeps top-left content
- GIVEN a 3x3 grid with `a` at (0,0) and `z` at (2,2)
- WHEN it is resized to 2x4
- THEN `a` remains at (0,0), `z` is gone and new rows are blank

#### Scenario: Resize resets region and clamps both cursors
- GIVEN region 2;4 and saved cursors at row 20 on both screens
- WHEN the terminal is resized to 80x10
- THEN the region is full screen and both cursors are within the grid

### Requirement: Printing and line discipline

Printable characters MUST be written at the cursor with the current pen
(colors and flags). With DECAWM (`CSI ? 7 h/l`, default set), printing in
the last column MUST leave the cursor there with a pending wrap; the next
print wraps to column 0 of the next line. With DECAWM reset, there is no
wrap and no pending wrap (including after an image advances the cursor): the
last column is overwritten. `CR` MUST move to column 0 and cancel
a pending wrap. `LF`, `VT` and `FF` MUST move down one row keeping the
column; at the bottom margin they MUST scroll only the scroll region (the
whole grid when no region is set), filling the new row with blanks that keep
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

#### Scenario: Wrap disabled
- GIVEN a 3-column terminal and `CSI ? 7 l`
- WHEN `abcd` is printed
- THEN row 0 reads `abd` and the cursor is at row 0, column 2

#### Scenario: LF outside the region does not scroll
- GIVEN region 1;3 on a 5-row grid and the cursor on row 5
- WHEN LF is fed
- THEN no row changes and the cursor stays on row 5

### Requirement: Cursor movement and erasing

The terminal MUST support CUU/CUD/CUF/CUB (`CSI A/B/C/D`), CUP/HVP
(`CSI row;col H`/`f`, 1-based), CHA (`CSI G`) and VPA (`CSI d`). A count or
position of 0 MUST be treated as 1 and every move MUST clamp to the grid
(to the scroll region for CUP/VPA while DECOM is set). ED (`CSI J` modes
0, 1, 2, 3) and EL (`CSI K` modes 0, 1, 2) MUST erase with blanks keeping
the pen background. ED 2 and 3 MUST also remove every image placement of the
active screen. DECTCEM (`CSI ? 25 h/l`) MUST show or hide the cursor.
Sequences whose intermediates or final byte are not defined in this
specification MUST be ignored.

#### Scenario: CUP is one-based and clamps
- GIVEN a 10x5 terminal
- WHEN `CSI 99;99 H` is fed
- THEN the cursor is at column 9, row 4

#### Scenario: Full clear removes images
- GIVEN a terminal with an image placement
- WHEN `CSI 2 J` is fed
- THEN the grid is blank and no placements remain, but the image data is kept

#### Scenario: Unknown sequences are ignored
- GIVEN any screen content
- WHEN `CSI ? 9999 h` or `CSI 1 ! z` is fed
- THEN grid, cursor and modes are unchanged

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
- `Clipboard { get_text, set_text }` over a `ClipboardKind` (`Clipboard` or
  the X11/Wayland `Primary` selection), failing with a `ClipboardError` that
  callers log; the adapter MUST live as long as copied text should stay
  pasteable.

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

### Requirement: Alternate screen

The terminal MUST keep a main and an alternate screen, each with its own grid
and saved cursor. Modes: `?47` swaps screens only; `?1047` swaps and clears
the alternate screen when leaving it; `?1048` acts as DECSC (set) / DECRC
(reset); `?1049` on entry does DECSC, switches and clears the alternate
screen, and on exit switches back and does DECRC. Entering while on the
alternate screen, or leaving while on the main screen, MUST be a no-op.
Main-screen content MUST be untouched while the alternate screen is active.

#### Scenario: Quitting a full-screen app restores the shell
- GIVEN `$ ls` on row 0 of the main screen and the cursor at row 1, column 3
- WHEN `CSI ? 1049 h`, `hello` and `CSI ? 1049 l` are fed
- THEN row 0 reads `$ ls` and the cursor is at row 1, column 3

#### Scenario: Re-entry is a no-op
- GIVEN the alternate screen holds `X`
- WHEN `CSI ? 1049 h` is fed again
- THEN `X` is still there and the saved cursor is unchanged

#### Scenario: ?47 keeps alternate content
- GIVEN `CSI ? 47 h`, `X`, `CSI ? 47 l`, `CSI ? 47 h`
- THEN the alternate screen still shows `X`

#### Scenario: ?1047 clears on leave
- GIVEN `CSI ? 1047 h`, `X`, `CSI ? 1047 l`, `CSI ? 1047 h`
- THEN the alternate screen is blank

### Requirement: Save and restore cursor

DECSC (`ESC 7`, `CSI s`) MUST save, per screen, column, row, pen (SGR
attributes), pending-wrap flag and origin mode. DECRC (`ESC 8`, `CSI u`)
MUST restore them. DECRC without a prior save MUST move to home with the
default pen and reset origin mode. `CSI s` and `CSI u` act only with no
parameters and no intermediates; `CSI ? u`, `CSI > u` (kitty keyboard) and
`CSI 1 s` MUST be ignored. DECRC MUST NOT restore a pending wrap while DECAWM
is reset.

#### Scenario: Pen is restored
- GIVEN bold red text, then `ESC 7`, then `CSI 0 m` and `CSI 5;5 H`
- WHEN `ESC 8` and `x` are fed
- THEN `x` is bold red at the saved position

#### Scenario: Restore without save
- GIVEN a fresh terminal with the cursor at (4,4)
- WHEN `ESC 8` is fed
- THEN the cursor is at (0,0) with the default pen

#### Scenario: Saved cursors are per screen
- GIVEN `ESC 7` at (2,2) on main, then `?1049h` and `ESC 7` at (5,5)
- WHEN `ESC 8` is fed on the alternate screen
- THEN the cursor is at (5,5)

### Requirement: Scroll region and origin mode

DECSTBM (`CSI top;bottom r`, 1-based, defaults full screen) MUST set the
margins and move the cursor to home (relative to the region when DECOM is
set); if top >= bottom after clamping it MUST be ignored. LF, VT, FF, IND
(`ESC D`) and NEL (`ESC E`) at the bottom margin MUST scroll only the
region up; RI (`ESC M`) at the top margin MUST scroll it down. SU (`CSI S`)
and SD (`CSI T`) MUST scroll the region. A LF on the last row outside the
region MUST NOT scroll. DECOM (`CSI ? 6 h/l`) makes CUP/HVP/VPA relative to
the region and clamps the cursor inside it. CNL (`CSI E`) and CPL (`CSI F`)
MUST move by lines to column 0.

#### Scenario: Region scroll
- GIVEN a 5x5 terminal with rows `a`..`e` and region 2;4
- WHEN the cursor is on row 4 (1-based) and LF is fed
- THEN rows read `a`, `c`, `d`, blank, `e`

#### Scenario: Invalid region ignored
- GIVEN a region 2;4
- WHEN `CSI 3;3 r` is fed
- THEN the region stays 2;4

#### Scenario: Origin mode
- GIVEN region 2;4 and `CSI ? 6 h`
- WHEN `CSI 1;1 H` and `CSI 99;1 H` are fed
- THEN the cursor lands on rows 2 and 4 (1-based)

### Requirement: Cell and line editing

ICH (`CSI @`), DCH (`CSI P`) and ECH (`CSI X`) MUST insert, delete and erase
cells at the cursor within the line, filling with blanks that keep the pen
background. IL (`CSI L`) and DL (`CSI M`) MUST insert or delete lines at the
cursor row within the region and move the cursor to column 0; they MUST do
nothing when the cursor is outside the region. REP (`CSI b`) MUST repeat the
last printed character n times, capped at one screenful (columns x rows) so
a hostile count cannot stall the terminal; it does nothing when no character
was printed since the last control or escape sequence.

#### Scenario: Insert characters
- GIVEN `abcd` on a 5-column row, cursor at column 1
- WHEN `CSI 2 @` is fed
- THEN the row reads `a  bc`

#### Scenario: IL outside region
- GIVEN region 2;4 and the cursor on row 5
- WHEN `CSI L` is fed
- THEN the grid is unchanged

#### Scenario: Repeat
- GIVEN `x` was printed
- WHEN `CSI 3 b` is fed
- THEN `xxxx` is on the row

### Requirement: Full reset

RIS (`ESC c`) MUST reset both screens, modes, scroll region, pen, saved
cursors and image placements, and return to the main screen with the cursor
at home. Image data MAY be kept.

#### Scenario: Reset from alternate screen
- GIVEN the alternate screen active with a region and a placement
- WHEN `ESC c` is fed
- THEN the main screen is active, blank, with a full-screen region and no placements

### Requirement: Cursor key mode

`Terminal::modes()` MUST expose the DECCKM state (`CSI ? 1 h/l`, default
reset). When set, the application MUST encode arrows and Home/End as
`ESC O A..D` and `ESC O H`/`ESC O F`; otherwise as `ESC [ ...`. DECKPAM and
DECKPNM MAY be accepted and ignored.

#### Scenario: Application cursor keys
- GIVEN `CSI ? 1 h` was fed
- WHEN Up is pressed
- THEN `ESC O A` is sent; after `CSI ? 1 l` it is `ESC [ A`

### Requirement: Bracketed paste mode

`Terminal::modes()` MUST expose bracketed paste (`CSI ? 2004 h/l`, default
reset), global across both screens and reset by RIS. Pasted text MUST be
encoded by `nxg_core::paste::encode`: `\r\n` and `\n` become `\r`, tab is
kept, every other control character (C0, DEL, C1, ESC included) is dropped
in both modes, and with the mode set the result is wrapped in `ESC [ 200 ~`
and `ESC [ 201 ~`.

#### Scenario: Paste cannot close the bracket
- GIVEN bracketed paste on
- WHEN `a ESC [201~ b` is pasted
- THEN the child receives `ESC [200~ a[201~ b ESC [201~`

### Requirement: Soft wraps

When autowrap moves printing to the next row, the last cell of the row left
MUST carry the `WRAPLINE` flag; it moves with the row through scrolling and
into the history, and is lost when that cell is rewritten or erased.

#### Scenario: Wrapped row
- GIVEN a 3-column terminal
- WHEN `abcd` is fed
- THEN row 0 is soft-wrapped and row 1 is not

### Requirement: Selection

A selection MUST be kept in absolute line coordinates: line `n` is the `n`th
line that ever entered the history (lines dropped from a full or disabled
history still count), so it survives viewport scrolling and lines moving
into the history. Kinds: simple (cells in reading order), word (separators
are whitespace and ``()[]{}<>'"`,;:│``; words continue across soft wraps),
line (the whole logical line, soft wraps included) and block (the same
columns of every line). Its text MUST trim the trailing blanks of each row
and join rows with `\n`, except that a soft-wrapped row joins the next one
directly. A blank selection has no text.

Output that changes a selected line MUST clear the selection: printing,
erasing, inserting or deleting cells, and scrolls that move lines without
saving them (region scrolls, IL, DL, RI, SD, and saving scrolls of a region
smaller than the screen). Switching screens, resizing, RIS and ED 3 MUST
clear it too.

#### Scenario: Selection follows its line into the history
- GIVEN line `b` selected on the screen
- WHEN more lines are printed below and scroll it into the history
- THEN the selected text is still `b`

#### Scenario: Output over the selection
- GIVEN a selection on row 0
- WHEN text is printed on row 0
- THEN there is no selection

### Requirement: Replay of full-screen programs

Captured output of vim, less and yazi replayed through the terminal MUST
leave the main screen with its prior lines and cursor intact.

#### Scenario: Replay vim session
- GIVEN shell lines on the main screen and a captured vim session
- WHEN the capture is fed
- THEN the main screen shows the same lines as before
