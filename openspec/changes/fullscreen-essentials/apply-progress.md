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
