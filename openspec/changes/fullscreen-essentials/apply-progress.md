# Apply progress: fullscreen-essentials

## S1 (alternate screen, DECSC/DECRC, ESC dispatch) - implemented, pending manual check

Done: 1.1 to 1.19. Open: 1.20 (manual real-window vim/less check, owned by the
orchestrator) and 1.21 (diff size: ~935 changed lines, above the 800 budget; see below).

### TDD cycle evidence

| Task | Test file | Layer | Safety net | RED | GREEN | Triangulate | Refactor |
|---|---|---|---|---|---|---|---|
| 1.1 | n/a (staged `git mv`, done by orchestrator) | - | 133 core tests green | - | - | - | - |
| 1.2 | `terminal/testing.rs` | Unit | 133/133 | compile error (helpers missing) | 135 pass | 2 cases | helpers moved out of `mod.rs` tests |
| 1.3 | `grid.rs`, `terminal/screen.rs` | Unit | 135/135 | compile error (`Region`, `Screen`) | 144 pass | 2 Region cases, 7 Screen cases | clean |
| 1.4 | `terminal/mod.rs` (full legacy suite) | Approval | 144/144 | compile/behaviour gate | all legacy tests green after conversion | `cursor()` approval test | clean |
| 1.5 | `terminal/screen.rs`, `modes.rs` | Unit | green | compile error | pass | Screen-level + terminal-level (active and dormant) | clean |
| 1.6 | `terminal/mod.rs` | Unit | green | written with the batch | pass | `ESC 7/8`, `= >`, intermediates, unknown finals | clean |
| 1.7 | `screen.rs`, `mod.rs` | Unit | green | compile error | pass | pen, no-save, wrap round trip, clamp | clean |
| 1.8 | `terminal/mod.rs` | Unit | green | runtime failure (2 tests) | pass after `bare` fix (vte reports a missing param as one 0) | `?u`, `>u`, `1s` ignored | clean |
| 1.9 | `terminal/modes.rs` | Unit | green | compile error | pass | two scenarios (1049 and 47) | clean |
| 1.10 | `image/store.rs` | Unit | 15/15 | compile error (9 errors) | 20 pass | 5 tests | clean |
| 1.11-1.13 | `terminal/modes.rs` | Unit | green | compile error | pass after fixing a bug found by the origin test (`arrive_from` reset main's origin on leave) | full xterm table, carry, images | clean |
| 1.14 | `terminal/mod.rs` | Unit | green | runtime failure on the new asserts | pass after re-targeting the stream | - | clean |
| 1.15 | `terminal/modes.rs` | Unit | green | - | pass (satisfied by 1.12) | - | - |

Test totals (nxg-core lib): 133 before, 177 after (+44).

### Verification

- `cargo fmt --all --check`: clean
- `RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets`: clean (one `collapsible_match` fixed)
- `cargo test --workspace`: all pass (nxg-core 177)
- `cargo +1.85 check --workspace --all-targets`: clean

### Size

Tracked diff vs HEAD: 305 insertions, 91 deletions (rename counted as a rename).
New untracked files: `screen.rs` 248, `modes.rs` 240, `testing.rs` 51 = 539 lines.
Total ~935 changed lines, about 135 over the 800 budget (forecast was ~520). About
60% is test code. Candidate cut for a split: move the image stash (`store.rs` +
terminal image tests) into its own PR.

### Deviations

- `Screen.region` carries `#[allow(dead_code)]` until S2 reads it.
- `State` methods live in `screen.rs`/`modes.rs` as child-module `impl State`.
- `kitty_rgba` helper also moved into `testing.rs`.
- 1.21 left unticked because the budget is exceeded.

## S2 (scroll regions, origin mode, index operations, RIS) - implemented, size decision pending

Branch `feat/fullscreen-essentials-s2`, stacked on S1. Tasks 2.0 and 2.15 stay open
until the orchestrator decides on the size (see Size). The pure move (2.1) is staged
as its own commit.

### TDD cycle evidence

| Task | Test file | Layer | Safety net | RED | GREEN | Triangulate | Refactor |
|---|---|---|---|---|---|---|---|
| 2.1 | `terminal/edit.rs` | Approval (pure move) | 177/177 | guard test only (no new behaviour) | 178 pass | - | move only |
| 2.2 | `grid.rs` | Unit | green | compile error (`scroll_up_in/down_in`) | 13 grid tests pass | up/down, n=1/2/99/0, 1-row region, 1-row grid, out-of-bounds panic | old `Grid::scroll_up` and its test removed after migration to `State::scroll_up` |
| 2.3 | `image/store.rs` | Unit | green | compile error (`scroll_region_*`) | 29 pass | up (shift, drop at 2 lines, partly visible stays), down (drop past bottom), n=0 | clean; one expectation fixed (a shifted placement is outside the span afterwards, per D9) |
| 2.4 | `terminal/edit.rs` | Unit | green | 20 runtime failures with 2.5-2.7 (sequences ignored) | pass | defaults, zeros, missing bound, clamp, 3 invalid forms, homing, DECOM homing | clean |
| 2.5 | `terminal/edit.rs` | Unit | green | runtime failure | pass | CUP/HVP/VPA, clamp, DECOM off, CPR both modes, toggle homes, DECSC/DECRC | CPR test rewritten: first version passed by coincidence |
| 2.6 | `terminal/edit.rs` | Unit | green | runtime failure | pass | LF/VT/FF/IND/NEL/RI, inside, below, last row outside, wrap cleared, print wrap, kitty placements inside/outside, full-screen unchanged | `line_feed` folded into `index` (callers updated) |
| 2.7 | `terminal/edit.rs` | Unit | green | runtime failure | pass | CUU/CUD inside vs outside, SU/SD clamp, no-region SU | clean |
| 2.8 | `terminal/screen.rs`, `image/store.rs` | Unit | green | runtime failure (4 tests) | pass | alt reset, stash dropped, pen/visibility/sixel kept replies, saved cursor, `reset_placements` | clean |
| 2.9 | `terminal/screen.rs` | Unit | green | not RED: already satisfied by S1 `Screen::resize` | pass | both screens, saved cursors | kept as regression test |
| 2.10 | `terminal/mod.rs` | - | - | nothing to change: S1 already removed `D/E/M/c`, `r`, `S/T`, `?6` from the ignored list (none were in it) | - | - | - |

Test totals (nxg-core lib): 177 after S1, 218 after S2 (+41, includes the 2.1 guard).

### Verification

- `cargo fmt --all --check`: clean
- `RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets`: clean
- `cargo test --workspace`: all pass (nxg-core 218)
- `cargo +1.85 check --workspace --all-targets`: clean

### Size

`git diff --stat feat/fullscreen-essentials-s1 -- . ':!openspec'` (staged + unstaged,
`edit.rs` included): 765 insertions, 82 deletions = 847 changed lines, over the 800 budget
and over the ~700 split threshold. The pure move accounts for about 134 of them (56 deleted
from `mod.rs`, 77 lines in `edit.rs` of which about 63 moved), so S2b without the move is about 713.
Roughly 480 lines are tests.

### Deviations

- Missing in this slice by design: `last_char`, autowrap and DECCKM do not exist yet (S3/S4), so RIS does not reset them.
- `State::scroll_up/scroll_down(top, bottom, n)` plus `scroll_region_up/down` (SU/SD wrappers) live in `edit.rs`.
- Under a full-screen region SD uses `scroll_region_down` over the whole span; placements partly scrolled off the top (negative anchor) are not shifted down.
- The 1-row grid case is covered at `Grid` level; `set_region` ignores any region on a 1-row terminal.

## S3 (cell and line editing, REP, DECAWM, CNL/CPL, fuzz) - implemented

Branch `feat/fullscreen-essentials-s3`, stacked on S2b. Tasks 3.1 to 3.15 done.
Manual window check (vim Ctrl-Y) is left to the orchestrator.

### TDD cycle evidence

| Task | Test file | Layer | Safety net | RED | GREEN | Triangulate | Refactor |
|---|---|---|---|---|---|---|---|
| 3.1 | `grid.rs` | Unit | 218/218 | compile error (3 missing methods) | 20 grid tests pass | insert/delete/erase, clamp, last column, empty and reversed ranges, row bounds panic | rotate-based, clean |
| 3.2 | `terminal/edit.rs` | Unit | green | runtime failure (ignored sequences) | pass | ICH/DCH/ECH, n 0/absent/99, pen background, pending wrap | clean |
| 3.3 | `terminal/edit.rs` | Unit | green | runtime failure | pass | IL/DL inside, outside (cursor untouched), no region, pen bg, placements, vim Ctrl-Y (IL at region top) | clean |
| 3.4 | `terminal/edit.rs` | Unit | green | runtime failure | pass | REP chain, n 0/absent, no last char, cleared by control/CSI/ESC, cap (absolute cursor 1,23) | clean |
| 3.5 | `terminal/modes.rs`, `screen.rs` | Unit | green | runtime failure (3 tests) | pass | off overwrites, off cancels pending wrap, global across `?1049`, RIS restores | RIS test was not RED (ESC already cleared state); mutation check: removing the reset makes it fail |
| 3.6 | `terminal/edit.rs` | Unit | green | runtime failure | pass | CNL/CPL, zero, inside vs outside region | clean |
| 3.7 | `terminal/mod.rs` | - | - | nothing to change: none of these sequences were in the ignored list | - | - | - |
| 3.8 | `terminal/mod.rs` | Fuzz | green | alphabet extended | pass (no panic found) | - | - |
| 3.9 | `terminal/testing.rs` | Unit | green | compile error | pass | 7 corruption cases (cursor, dormant cursor, region, saved cursor, grid size, active and stashed dangling placement, wrap with autowrap off) | clean |
| 3.10 | `terminal/mod.rs` | Fuzz | green | runtime failure: invariant "wrap pending while autowrap is off" (found 2 real bugs, below) | pass | 400 runs x 60 tokens, resizes to 1x1; mutation check (removing region reset in resize) fails it | clean |

Bugs found by the invariant fuzz, each fixed with a minimal regression test:
- DECRC restored a pending wrap saved before `?7l`, so the next print wrapped with autowrap off
  (`screen.rs`, test `decrc_does_not_revive_a_pending_wrap_after_autowrap_was_turned_off`).
- Kitty cursor advance past the right edge set a pending wrap with autowrap off
  (`mod.rs`, test `kitty_cursor_advance_past_the_edge_does_not_wait_without_autowrap`).

Test totals (nxg-core lib): 218 after S2, 256 after S3 (+38).

### Verification

- `cargo fmt --all --check`: clean
- `RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets`: clean (one `reversed_empty_ranges` in a test fixed)
- `cargo test --workspace`: all pass (nxg-core 256)
- `cargo +1.85 check --workspace --all-targets`: clean

### Size

`git diff --shortstat feat/fullscreen-essentials-s2b -- . ':!openspec'`: 7 files, 719 insertions,
6 deletions = 725 changed lines (budget 800). Roughly 520 are tests.

### Deviations

- Added test-only `ImageStore::stashed()` and `drop_image_keeping_placements(id)` for the invariants.
- `assert_invariants` lives in `terminal/testing.rs` (already `#[cfg(test)]`), not in `screen.rs`.
- DECAWM off also stops the kitty cursor advance from setting a pending wrap (not in the spec; needed for the invariant).
- Wrap-pending invariant checks the active screen only; a kept dormant screen may hold a stale flag that the next switch overwrites.
- 3.7 and 3.15: see task notes. Open for later: IL/DL at a region with DECOM are by absolute row (correct per xterm); `advance_over_image` ignores autowrap only for the pending flag.

## S4 (cursor key mode, DECCKM) - implemented

Branch `feat/fullscreen-essentials-s4`, stacked on S3. Tasks 4.1 to 4.4 and 4.6 to 4.10
done. 4.5 (manual arrow-key check in vim, less and the shell) is left to the orchestrator.

### TDD cycle evidence

| Task | Test file | Layer | Safety net | RED | GREEN | Triangulate | Refactor |
|---|---|---|---|---|---|---|---|
| 4.1 | `terminal/modes.rs` | Unit | 256/256 core | compile error (`Modes`, `modes()`) | 262 pass | defaults, set/reset, combined `?1;25h`, global across `?1049`, `alt_screen` via `?1049` and `?47`, `ESC =`/`ESC >`, RIS | mutation check: removing the RIS reset fails the test |
| 4.2 | `terminal/mod.rs` | - | - | nothing to change: `?1` was never in the ignored list | - | - | - |
| 4.3 | `nxgterm/src/keys.rs` | Unit | 33/33 | compile error (4th argument) | pass | arrows and Home/End on, other keys unaffected, `alt_screen` alone changes nothing, normal mode unchanged | clean |
| 4.4 | `nxgterm/src/app.rs` | - (composition root) | - | compile error covered by 4.3 | builds | manual check 4.5 | - |

Test totals: nxg-core lib 256 after S3, 262 after S4 (+6); nxgterm 33 to 40 (+2 keys tests, plus the existing ones updated for the new signature).

### Verification

- `cargo fmt --all --check`: clean
- `RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets`: clean
- `cargo test --workspace`: all pass
- `cargo +1.85 check --workspace --all-targets`: clean

### Size

`git diff --shortstat feat/fullscreen-essentials-s3 -- . ':!openspec'`: 6 files, 191 insertions,
11 deletions = 202 changed lines (budget 800).

### Deviations

- `Modes` is defined in `modes.rs` and re-exported from `terminal/mod.rs` and `lib.rs`.
- `Modes` includes `alt_screen`, as the design says (D10); it is derived from `alt_active`.
- DECKPAM/DECKPNM stay accepted and ignored (spec allows it; S1 behaviour unchanged, now guarded by a modes test).
- Modifier combinations with arrows (e.g. Ctrl+Up) are out of scope and unchanged.
