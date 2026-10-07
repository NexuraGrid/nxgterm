# Exploration: Alternate screen, scroll regions and full-screen app essentials

Addresses Recommendation #3 (P0) in `openspec/RECOMMENDATIONS.md`.

## Trigger

Real-window smoke test on Linux (KDE Plasma Wayland, 2026-10-07): running
`vim` and quitting leaves vim's screen on the terminal and the previous shell
lines are lost. vim renders correctly while running.

## Current state

### Terminal (`crates/nxg-core/src/terminal.rs`)

- `State` holds a single `grid`, `cursor` (col, row, visible), `pen`,
  `wrap_pending`, `responses`, `cell`, one `ImageStore`, kitty graphics and
  sixel state.
- `impl vte::Perform for State` implements `print`, `execute`,
  `hook/put/unhook` (sixel) and `csi_dispatch` only. There is no
  `esc_dispatch` and no `osc_dispatch`, so `ESC 7/8/M/D/E/c/=/>` are no-ops.
- `csi_dispatch` implements CUU/CUD/CUF/CUB, CUP/HVP, CHA, VPA, ED, EL, SGR,
  DSR 5/6, DA1, XTWINOPS 14/16/18, XTSMGRAPHICS and private modes `?25`
  (DECTCEM) and `?80` (DECSDM). Every other private mode is ignored.
- `Terminal::resize` clamps the cursor and clears the pending wrap.

| Feature | State |
|---|---|
| `?1049` / `?47` / `?1047` / `?1048` | Ignored (`?1049h` is used as an "ignored" case in `unknown_and_malformed_sequences_are_ignored`) |
| DECSTBM `CSI r`, DECOM `?6` | Missing; LF scrolls the whole grid only at the last row |
| DECSC/DECRC (`ESC 7/8`, `CSI s/u`) | Missing |
| IND, NEL, RI (`ESC D/E/M`), RIS (`ESC c`) | Missing |
| ICH, DCH, ECH, IL, DL, REP, SU, SD | Missing |
| DECAWM `?7` | Missing; wrap is always on |
| DECCKM `?1`, DECKPAM/DECKPNM | Missing in core and in `keys.rs` |
| CNL/CPL (`CSI E/F`) | Missing; trivial |

### Grid (`crates/nxg-core/src/grid.rs`)

Flat row-major `Vec<Cell>` with `row`, `row_mut`, `scroll_up(blank)` (whole
grid, `copy_within`) and `resize` (keeps top-left). No scrollback and no
scroll-down. The vim bug is not lost history: the main grid is overwritten
because nothing swaps buffers.

### Images (`crates/nxg-core/src/image/store.rs`)

- `placements: Vec<Placement>` has no screen tag; placements are anchored to
  screen rows (`row: i32`, may be negative).
- `ImageStore::scroll_up(lines, cell)` shifts placements and drops those fully
  off-screen; called only from `State::line_feed`.
- `clear_placements` runs on ED 2/3; `remove_image` filters active placements.
- `kitty::Context` receives `cursor` by value, so kitty code is decoupled from
  screens. `advance_over_image` and `finish_sixel` call `line_feed` and will
  inherit region behavior.

### Renderers and app

- Renderers and the app only use `Terminal::row()`, `cursor()`, `images()` and
  `size()`. Swapping screens inside `Terminal` is invisible to adapters.
- `keys::encode(key, text, mods)` (`crates/nxgterm/src/keys.rs`) is pure.
  Arrows are hardcoded `ESC [ A..D`, Home/End `ESC [ H/F`; modifiers on arrows
  are ignored; numpad is not distinguished.
- `TERM=xterm-256color` (`crates/nxg-pty/src/lib.rs`), so vim/less/yazi use
  `smcup/rmcup` (`?1049`) and `smkx` (`?1h` + `ESC =`).

### Tests

- Unit tests inline in `terminal.rs` with helpers `term`, `sized`, `text`,
  `pos`, `kitty_rgba`; pattern `t.advance(b"...")` then assert.
- `random_and_truncated_input_never_panics` is a seeded fuzz that resizes
  mid-state; its alphabet should gain `L M @ P D E 7 8 u b r =`.
- `nxg-core` has no `tests/` directory and no replay fixtures.

## Affected areas

- `crates/nxg-core/src/terminal.rs`: per-screen vs global state, `esc_dispatch`,
  new CSI handlers and modes.
- `crates/nxg-core/src/grid.rs`: region scroll up/down, insert/delete cells and
  lines.
- `crates/nxg-core/src/image/store.rs`: inactive placement stash, region-aware
  scroll, `remove_image` purges the stash.
- `crates/nxg-core/src/lib.rs`: export `Modes`.
- `crates/nxgterm/src/keys.rs`, `crates/nxgterm/src/app.rs`: `encode` takes
  modes; app passes `terminal.modes()`.
- `crates/nxg-core/tests/replay.rs` and fixtures (new).
- Specs: `terminal-core` ("Printing and line discipline", "Cursor movement")
  and `inline-images` ("Scrolling, clearing and resize") need deltas.

## Approaches

### Fork 1: two screens

1. **`Screen` struct swapped with `mem::swap`** (recommended). Group grid,
   cursor position, wrap_pending, saved cursor, scroll region and origin mode
   in `Screen`; `State` holds the active screen and a dormant
   `Option<Box<Screen>>`. One-time refactor of ~40 call sites; natural home
   for per-screen state; alt grid freed on exit.
2. Flat fields plus a `SavedScreen` copy. Smallest first diff, but every new
   per-screen field needs copy code twice; bug-prone for 1047/1049 and RIS.
3. Single grid with a virtual second page. Rejected: fights resize and
   rendering.

### Fork 2: scroll-region logic

1. **Primitives in `Grid`, policy in `Terminal`** (recommended). `Grid` gets
   `scroll_up_in`, `scroll_down_in`, `insert_cells`, `delete_cells`,
   `erase_cells` via `copy_within`; `Terminal` owns region, origin mode and
   cursor rules. Pure, testable, and a single entry point for future
   scrollback (#5).
2. Everything in `Terminal` via `row_mut`. Rejected: duplicated copying, slower.

### Fork 3: images with a partial scroll region

- **A** (recommended): full-screen region keeps today's behavior; with a
  partial region, placements anchored inside it shift and are dropped when
  they leave it; placements outside do not move.
- **B**: partial regions never move placements. Simpler but wrong for
  less/yazi-style status lines.

### Screen-switch semantics (xterm)

- `?47`: swap only. `?1047`: swap, clear alt on leave. `?1048`: DECSC/DECRC.
  `?1049`: DECSC, switch, clear alt; DECRC on exit.
- Saved cursor is per screen; DECSC saves col, row, pen, wrap_pending and
  origin mode.
- Images: entering alt stashes main placements in `ImageStore.inactive`; the
  alt starts empty. Leaving drops alt placements and restores the main ones.
  Image data stays shared under the existing byte budget.

### Modes for DECCKM

A `Modes` value struct in `nxg-core` (`app_cursor_keys`, `app_keypad`,
`alt_screen`; later bracketed paste and mouse for #6/#9) returned by
`Terminal::modes()`. No new port: `Terminal` is a domain object the
composition root already owns. `encode(key, text, mods, modes)` stays pure.

## Recommendation

Fork 1 option 1, Fork 2 option 1, Fork 3 option A.

| # | Slice | Est. changed lines |
|---|---|---|
| S1 | `Screen` extraction, DECSC/DECRC, `ESC` dispatch, alt screen (`?1049/1047/47/1048`), image stash | ~450 |
| S2 | IND/NEL/RI, DECSTBM, DECOM, SU/SD, region-aware LF, `Grid` primitives, region image scroll, RIS | ~500 |
| S3 | ICH/DCH/ECH, IL/DL, REP, DECAWM, CNL/CPL | ~300 |
| S4 | `Modes`, DECCKM in `keys.rs` and app wiring (DECKPAM optional: needs winit `KeyLocation::Numpad`) | ~200 |
| S5 | Replay harness, fixtures, spec deltas, vttest record | ~250 + fixtures |

Total ~1700 lines against an 800-line budget: chained PRs required. S1 first
delivers the vim-exit fix; S2 must precede S3 (IL/DL need regions); RIS lands
in S2 to reset the state S1 introduces.

### Replay tests

Feasible in `crates/nxg-core/tests/replay.rs`: capture with a scripted pty at
fixed 80x24 and `TERM=xterm-256color` (vim `-u NONE`, less, yazi), scrub
prompts/hostnames/paths, keep fixtures small, and assert end-state and
mid-run screen text rather than exact bytes. htop is not installed.

### vttest

Manual only and wider than this scope: menu 1 needs DECCOLM, DECALN, DECSCNM;
menu 2 needs HTS/TBC, G0/G1 charsets with `ESC ( 0`, SO/SI and double-size
lines. "Menus 1-2 pass" is not reachable with the listed families; restate as
"run and record, with known deviations", and move charsets/tab stops to a
follow-up.

## Risks

- vttest acceptance is not reachable with the listed scope.
- No `ESC ( 0` line drawing (low risk: htop and yazi use Unicode).
- DA2 and XTVERSION remain unanswered; out of scope.
- Stashed placements may reference freed images unless `remove_image` purges
  the stash.
- Resize with alt screen or region active: reset the region and clamp both
  screens' cursors.
- DECKPAM needs winit `KeyLocation` plumbed into `encode`.
- wrap_pending with DECAWM off, LF at region bottom and ICH/DCH need explicit
  tests.
- Scrollback (#5) must only be fed by full-width scrolls on the main screen;
  keep a single `Grid` scroll entry point.
