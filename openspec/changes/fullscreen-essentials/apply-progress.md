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
