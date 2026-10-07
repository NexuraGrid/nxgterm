# Design: Full-screen app essentials

Addresses Recommendation #3 (P0). Implements `proposal.md` (forks: `Screen`
swap, primitives in `Grid`, region-aware image scroll). Specs
(`terminal-core`, `inline-images` deltas) are written in parallel; this design
names the behaviours they must pin down.

## Technical Approach

`nxg-core` stays pure. The terminal's per-screen state moves into a `Screen`
value; the active one lives in `State.screen`, the other in
`State.dormant: Option<Box<Screen>>`, swapped with `mem::swap`. `Grid` gains
region scroll and cell edit primitives (pure, no policy). `Terminal` (in
`State`) owns region, origin, autowrap and cursor policy. `ImageStore` gains an
inactive placement stash and region-aware scroll. `Modes` is a `Copy` value
returned by `Terminal::modes()`; `keys::encode` takes it and stays pure. No new
crates, no new ports.

## Architecture Decisions

| # | Decision | Rejected | Rationale |
|---|---|---|---|
| 1 | `Screen { grid, col, row, wrap_pending, saved: Option<SavedCursor>, region: Region, origin: bool }` swapped with `mem::swap` | Flat fields + `SavedScreen` copy; virtual second page | Every new per-screen field is written once; RIS/resize loop over two `Screen`s instead of duplicating field lists. |
| 2 | Cursor **visibility** (DECTCEM), `pen`, DECAWM, DECCKM, `sixel_scrolling`, `last_char` stay global in `State` | Per-screen everything | xterm keeps these global. A vim that hides the cursor in alt must not leave the shell cursor hidden. `pen` travels inside `SavedCursor` only. |
| 3 | **DECAWM is global**, not per screen. `wrap_pending` is per screen (it is cursor state, saved by DECSC) | Per-screen autowrap | In xterm `WRAPAROUND` is a terminal flag, not saved by DECSC or swapped by `?1049`. Disabling `?7` clears `wrap_pending`. |
| 4 | **DECOM lives in `Screen`** (confirmed) and is saved/restored by DECSC/DECRC. Alt starts with origin off and a full region | Global origin | Exiting alt always restores main's origin and region, so alt-screen apps cannot leak a region or origin mode into the shell (stricter than xterm, which shares them). |
| 5 | On `?47/?1047` switches the cursor **position and `wrap_pending` are carried** to the arriving screen (xterm has one cursor). `?1049` exit then overrides via DECRC | Independent per-screen positions | Keeps xterm behaviour for apps relying on `?47`; the swap still isolates region, origin and saved cursor. |
| 6 | `Region { top, bottom }` inclusive, invariant `top < bottom < rows`, else full screen. Defined in `grid.rs` (shared with `ImageStore`) | `Option<Region>` | One representation; `is_full(rows)` replaces `None`. |
| 7 | **One scroll entry point**: `State::scroll_up/scroll_down(top, bottom, n)` calls `Grid::scroll_{up,down}_in` and the matching `ImageStore` call. The old `Grid::scroll_up(blank)` is removed | Keep both | Scrollback (#5) hooks exactly one place: `Grid::scroll_up_in` with `top == 0`, full width, main screen only. |
| 8 | `ImageStore.inactive: Vec<Placement>` stash. `stash_placements()` on entering alt, `restore_placements()` on leaving (alt placements dropped). `remove_image` purges both lists; `remove_placements(free)` frees an image only when neither list references it. `clear_placements` (ED 2/3) touches active only | Tag each placement with a screen id | No change to `Placement`/renderers; images stay shared under one byte budget. |
| 9 | Partial region image scroll (Fork 3A): full-screen region keeps `scroll_up`; partial uses `scroll_region_up/down`. A placement is "inside" if its anchor row is in the span. Inside ones shift; dropped when fully outside the span (up: bottom edge `<= top`; down: anchor `> bottom`). Outside ones never move | Anchor-leaves-region drop | Matches the confirmed rule. Known cosmetic gap: a shifted placement may overlap rows above the region until fully out (renderers do not know the region; clipping is a follow-up). |
| 10 | `Modes { app_cursor_keys, alt_screen }` (`Copy`, `Default`) via `Terminal::modes()`; `alt_screen` is derived from `State`, not stored twice. DECKPAM/DECKPNM (`ESC =`/`ESC >`) are parsed and ignored | `Modes` port/trait; storing mode copies | Terminal is a domain object the composition root owns; a port adds nothing. |
| 11 | `terminal.rs` becomes the `terminal/` module (see Module split) | One 1500-line file | Reviewability: each slice touches mostly new files. |
| 12 | `?1049l` while on main is a no-op (no stale DECRC). `?1049h` while already in alt is a no-op | xterm-style unconditional restore | Avoids teleporting the cursor from repeated/unbalanced sequences. |
| 13 | RIS resets *everything but* `cell`, `responses`, and image data: single fresh `Screen`, dormant `None`, pen, visibility, autowrap, DECCKM, `sixel_scrolling`, `last_char`, sixel/graphics state, placements and stash | Partial reset | Spec'd as "RIS resets all state"; pending replies must still reach the child. |
| 14 | CSI `s`/`u` act as DECSC/DECRC only with no params and no intermediates (`CSI ? u`, `CSI > u` keep being ignored) | Always | DECSLRM (`s` with params) is out of scope; kitty keyboard `u` forms must not restore the cursor. |

### xterm switch semantics

| Mode | `h` (enter) | `l` (leave) |
|---|---|---|
| `?47` | swap; alt content kept | swap back |
| `?1047` | swap | clear alt, swap back |
| `?1048` | DECSC | DECRC |
| `?1049` | DECSC on main, swap, clear alt (fresh `Screen`) | swap back, DECRC on main |

Swap always starts the arriving alt with full region and origin off.
Images: enter stashes main placements; leave drops alt placements and restores.

## Data Flow and Sequence Diagrams

### `?1049h` ... `?1049l` with image stash

```
child        vte/State        Screen(active)   dormant        ImageStore
 | ESC[?1049h  |                  |  (main)       None            |
 |------------>| enter_alt(true)  |                               |
 |             | alt_active? no   |                               |
 |             | main.saved=cursor|                               |
 |             | carry=(col,row,wp)                               |
 |             | dormant.take() or Screen::new(size)               |
 |             | mem::swap(active, other); arrive.pos=carry        |
 |             |  active=alt(blank, full region), dormant=Some(main)|
 |             | stash_placements() ----------------------------->| placements -> inactive
 | (vim draws, kitty/sixel place into the empty active list)       |
 | ESC[?1049l  |                  |  (alt)                        |
 |------------>| leave_alt(clear)                                 |
 |             | alt_active? yes                                   |
 |             | mem::swap(active, dormant)  active=main           |
 |             | dormant=None  (alt grid freed; ?47 keeps it)      |
 |             | restore_placements() --------------------------->| placements = inactive,
 |             | restore_cursor() from main.saved                  |   alt ones dropped
 v             v
```

`remove_image(key)` during alt (budget eviction) filters both lists, so
`restore_placements` can never reference a freed image.

### LF/IND at a region bottom (region rows 2..=5, 80x24)

```
child     State::index                 State::scroll_up        Grid / ImageStore
 | "\n"      |                              |                       |
 |---------->| row == region.bottom?        |                       |
 |           |  yes -> scroll_up(2,5,1)---->| blank=pen bg          |
 |           |                              | Grid::scroll_up_in(2,5,1,blank)
 |           |                              | region full? no ----->| ImageStore::scroll_region_up(2,5,1,cell)
 |           |                              |   (full -> ImageStore::scroll_up)
 |           | wrap_pending=false; row unchanged
 |           |  no  -> row < last_row? row+=1 ; row == last_row (below region): stay
```

RI mirrors it: at `region.top` call `scroll_down(top, bottom, 1)`, else
`row = row.saturating_sub(1)`. NEL = `col=0` + `index`. `advance_over_image`
and `finish_sixel` already call `line_feed` and inherit region behaviour.

## Behaviour rules (inputs to specs and RED tests)

- **DECSTBM** `CSI t;b r`: 0/absent = default (1, rows). Valid only if
  `top < bottom` after clamping to rows; invalid is ignored. Valid sets region
  and homes the cursor (to region top-left if DECOM). Cursor does not move
  otherwise.
- **DECOM** (`?6`): CUP/HVP/VPA rows are relative to `region.top` and clamped
  to the region; CPR reports row relative to `region.top`. Toggling homes the
  cursor.
- **CUU/CUD/CNL/CPL** clamp to the region edge when the cursor starts inside
  the region, else to the screen edge.
- **SU/SD** `n` (clamped to region height) scroll the region, cursor unmoved.
- **IL/DL** act only when the cursor row is inside the region: scroll
  `[row, bottom]`, cursor to column 0, wrap cleared; images shift via the same
  region calls with `top = row`.
- **ICH/DCH/ECH** operate on the cursor row from the cursor column to the last
  column (no left/right margins), blank cells use the pen background, cursor
  unmoved, `wrap_pending` cleared, `n` clamped.
- **REP** repeats `last_char` `n` times (cap `cols * rows`); `last_char` is
  cleared by any control, CSI (except REP) or ESC.
- **DECAWM off**: printing at the last column overwrites it and never sets
  `wrap_pending`.
- **DECSC** saves col, row, pen, `wrap_pending`, origin into `screen.saved`;
  **DECRC** restores (clamped); with nothing saved it homes the cursor and
  resets pen, origin and wrap.
- **Resize** (`State::resize`, applied to active and dormant): `Grid::resize`,
  region reset to full, cursor and saved cursor clamped, `wrap_pending` false.

## Data Structures and Signatures

```rust
// grid.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region { pub top: u16, pub bottom: u16 }   // inclusive
impl Region { pub fn full(rows: u16) -> Self; pub fn is_full(self, rows: u16) -> bool;
              pub fn contains(self, row: u16) -> bool; }
impl Grid {
    pub fn scroll_up_in(&mut self, top: u16, bottom: u16, n: u16, blank: Cell);   // S2
    pub fn scroll_down_in(&mut self, top: u16, bottom: u16, n: u16, blank: Cell); // S2
    pub fn insert_cells(&mut self, row: u16, col: u16, n: u16, blank: Cell);      // S3
    pub fn delete_cells(&mut self, row: u16, col: u16, n: u16, blank: Cell);      // S3
    pub fn erase_cells(&mut self, row: u16, cols: Range<u16>, blank: Cell);       // S3
}   // all clamp n, no-op on empty, assert row bounds like row()

// terminal/screen.rs
struct SavedCursor { col: u16, row: u16, pen: Cell, wrap_pending: bool, origin: bool }
struct Screen { grid: Grid, col: u16, row: u16, wrap_pending: bool,
                saved: Option<SavedCursor>, region: Region, origin: bool }
// State: screen: Screen, dormant: Option<Box<Screen>>, alt_active: bool,
//        cursor_visible, autowrap, app_cursor_keys, last_char: Option<char>
// Terminal::cursor() builds Cursor { col, row, visible } (public type unchanged)

// terminal/modes.rs
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modes { pub app_cursor_keys: bool, pub alt_screen: bool }
impl Terminal { pub fn modes(&self) -> Modes; }      // re-exported in lib.rs (S4)

// image/store.rs
pub fn stash_placements(&mut self);
pub fn restore_placements(&mut self);
pub fn scroll_region_up(&mut self, top: u16, bottom: u16, lines: u32, cell: CellPixels);
pub fn scroll_region_down(&mut self, top: u16, bottom: u16, lines: u32);

// nxgterm/src/keys.rs
pub fn encode(key: &Key, text: Option<&str>, mods: ModifiersState, modes: Modes) -> Option<Vec<u8>>;
```

DECCKM: with `app_cursor_keys`, arrows `ESC O A..D`, Home/End `ESC O H/F`;
otherwise unchanged. Modifier-on-arrow encodings stay out of scope.
`app.rs` line ~246 passes `session.terminal.modes()`.

`esc_dispatch(intermediates, ignore, byte)`: only empty intermediates; `7 8 D E M c`
act, `= >` are accepted and ignored.

## Module split (`terminal.rs` is ~500 lines of code + ~500 of tests)

First commit of S1 is a pure `git mv terminal.rs terminal/mod.rs` (100%
similarity, zero counted lines). Child modules can read the parent's private
`State`, so no visibility churn.

| File | Holds | Slice |
|---|---|---|
| `terminal/mod.rs` | `Terminal`, `State`, `Perform` impl (dispatch tables), SGR, graphics glue, legacy tests, fuzz | S1+ |
| `terminal/screen.rs` | `Screen`, `SavedCursor`, `enter_alt/leave_alt`, DECSC/DECRC, resize, RIS | S1 (RIS S2) |
| `terminal/modes.rs` | `set_private_mode(mode, on)`, `Modes`, `Terminal::modes` | S1 (`Modes` S4) |
| `terminal/edit.rs` | `goto`, origin math, `index/reverse_index/line_feed`, region ops, erase, IL/DL/ICH/DCH/ECH/REP | S2 (moved `goto/line_feed/erase*`), S3 |
| `terminal/testing.rs` | `#[cfg(test)]` helpers `term, sized, text, pos` | S1 |

New tests live beside the code they cover (`#[cfg(test)] mod tests` per file);
existing tests stay in `mod.rs`. SGR extraction (~100 lines) is a possible
later cleanup, deliberately not done here.

## Testing Strategy (strict TDD: RED test first in every task)

| Layer | What | Approach |
|---|---|---|
| Unit `Grid` | scroll up/down in region, n clamp, empty, insert/delete/erase edges | inline tests, direct cells |
| Unit `ImageStore` | stash/restore, `remove_image` purges stash, free-with-stash, region up/down keep/shift/drop | inline tests |
| Unit `Terminal` | each switch mode table row; DECSC/DECRC per screen; wrap_pending with DECAWM off; LF at region bottom and below region; DECOM + CPR; IL/DL outside region; resize with alt+region active; RIS | `t.advance(...)` then assert text/pos |
| Unit `keys` | DECCKM on/off for arrows, Home, End | extend `encodes_navigation_keys` |
| Replay | vim, less, yazi end-state and mid-run screens | below |
| Manual | quit vim restores shell; vttest menus 1-2 recorded as known deviations | S5 checklist |

### Replay harness (S5)

- `crates/nxg-core/tests/replay.rs`, fixtures in
  `crates/nxg-core/tests/fixtures/<app>/{stream.vt,expect.txt}`. Uses only the
  public API (`advance`, `row`, `cursor`, `modes`).
- Stream format: raw child output with checkpoint markers
  `ESC ] nxg-checkpoint ; <label> BEL`. The harness splits on the marker (vte
  would ignore it anyway), advances chunk by chunk, and checks the `expect.txt`
  section for that label:

  ```
  [vim-open]            # label
  size 80 24            # first section only
  alt true              # modes().alt_screen
  cursor 0,23
  row 0: "hello"        # trimmed-end text; unlisted rows unchecked
  ```
- Assertions are on screen text, cursor and modes, never on byte streams.
  `[end]` asserts the shell lines before the app are restored.
- Capture: dev-only `crates/nxg-pty/examples/capture.rs` (reuses the existing
  `portable-pty` dependency, no new crate). Spawns the program at 80x24 with
  `TERM=xterm-256color`, plays a scripted keystroke/delay file, writes the
  stream, inserting markers at `mark` script lines. It scrubs `$HOME`, `$USER`,
  hostname and cwd by same-length substitution so cursor columns stay valid.
  Arguments come from the developer's script only; it is never built into the
  product.
- Guard tests in `replay.rs`: every fixture is under 64 KiB and contains none
  of `/home/`, `/Users/`, `C:\Users`, the capture user or hostname.
- Programs: `vim -u NONE`, `less`, `yazi`; htop not installed (skipped).
  Hand-made minimal streams cover the same flows in S1-S4 inline tests so no
  slice depends on a real capture.

### Fuzz extension

- Extend the `random_and_truncated_input_never_panics` alphabet with
  `L M @ D E F J K 7 8 u b l =`.
- Add `screen_ops_never_panic_across_resizes`: concatenates random tokens from
  `ESC[?1049h/l`, `?47`, `?1047`, `?1048`, `ESC[t;br`, `?6h/l`, `?7h/l`,
  `ESC M/D/E/7/8/c`, `ESC[nL/M/@/P/X/b/S/T`, `\n`, text, with random `n` up to
  99, and resizes (including shrinking to 1 row/col where `TermSize` allows)
  between tokens.
- After every step a `#[cfg(test)] State::assert_invariants()` checks, for both
  screens: grid size equals terminal size, cursor in bounds, region valid
  (`top < bottom < rows` or full), saved cursor in bounds, and every active and
  stashed placement references a stored image.

## Slice boundaries (each PR under 800 changed lines)

| Slice | Files | Est. lines | Ships |
|---|---|---|---|
| S1 | `terminal.rs -> terminal/mod.rs` (rename commit), `terminal/{screen,modes,testing}.rs`, `mod.rs` call-site conversion + `esc_dispatch` (`7 8`), `image/store.rs` stash + purge | ~500 | vim-exit fix: `?47/1047/1048/1049`, DECSC/DECRC |
| S2 | `grid.rs` (`Region`, `scroll_{up,down}_in`, remove `scroll_up`), `image/store.rs` region scroll, new `terminal/edit.rs` (moved `goto/line_feed/erase*`), `screen.rs` RIS/resize, `mod.rs` | ~550 | DECSTBM, DECOM, IND/NEL/RI, SU/SD, RIS |
| S3 | `grid.rs` cell primitives, `edit.rs`, `mod.rs` (autowrap, `last_char`), fuzz alphabet | ~300 | ICH/DCH/ECH, IL/DL, REP, DECAWM, CNL/CPL |
| S4 | `terminal/modes.rs`, `lib.rs`, `nxgterm/src/keys.rs`, `nxgterm/src/app.rs` | ~200 | DECCKM |
| S5 | `tests/replay.rs`, fixtures, `nxg-pty/examples/capture.rs`, spec deltas, vttest notes | ~400 + fixtures | Regression net |

If S2 trends above 700 lines, split the `move` of `goto/line_feed/erase*` into
a separate preceding commit/PR (pure move). Fixtures are data; ask reviewers
to exclude them from the 800-line count (capped at 3 x 64 KiB).

## File Changes

| File | Action |
|---|---|
| `crates/nxg-core/src/terminal.rs` | Rename to `terminal/mod.rs`, then Modify |
| `crates/nxg-core/src/terminal/{screen,modes,edit,testing}.rs` | Create |
| `crates/nxg-core/src/grid.rs`, `image/store.rs`, `lib.rs` | Modify |
| `crates/nxgterm/src/{keys,app}.rs` | Modify |
| `crates/nxg-core/tests/replay.rs`, `tests/fixtures/**` | Create |
| `crates/nxg-pty/examples/capture.rs` | Create |

## Threat Matrix

N/A for the product: no routing, shell, subprocess, VCS/PR or executable-file
boundary is added. The dev-only capture example spawns a developer-chosen
program from a developer-written script; no untrusted input reaches it.

## Migration / Rollout

No migration. Chained PRs S1 to S5, revert in reverse order; S1 alone reverts
to the v0.1.0 single-screen model.

## Open Questions

- [ ] Confirm `nxg-pty` exposes env/size control for the capture example; else
      capture via a small `portable-pty` use inside the example itself.
- [ ] Should reviewers exclude fixtures from the 800-line budget (assumed yes)?
- [ ] Minimum `TermSize` (1x1?) for the resize fuzz; region needs `rows >= 2`.
- [ ] Renderer clipping of shifted placements to the scroll region (follow-up).

## Key Learnings

- xterm keeps cursor visibility, pen, DECAWM and DECCKM global; only the
  saved cursor is per buffer. Putting origin and region in `Screen` is safe
  because exit restores main's values, and it blocks region leaks from alt.
- `ImageStore::remove_placements(free)` must check the stash too, or it can
  free an image still referenced by stashed main placements.
- `CSI ? u`/`CSI > u` (kitty keyboard) share the final byte with DECRC; only
  the bare `CSI u` may restore the cursor.
- A pure `git mv` to `terminal/mod.rs` costs zero counted lines and lets later
  slices add files instead of growing a 1000-line module.
- vte ignores unknown OSC, so an OSC checkpoint marker is a safe in-band
  separator for replay fixtures.
