# Tasks: Full-screen app essentials

Addresses Recommendation #3 (P0). Strict TDD: in every task the RED test is
written and seen failing first, then the code that turns it GREEN. Test command:
`cargo test --workspace`. Each slice (S1..S5) is one PR boundary, under 800
changed lines (fixtures and other data files are excluded from the count).
The chain strategy (stacked branches vs feature branch) is not chosen yet.

Spec references: `TC` = `specs/terminal-core/spec.md`, `II` = `specs/inline-images/spec.md`,
`D#` = design decision number in `design.md`.

## Review Workload Forecast

| Slice | PR boundary | Est. changed lines (code + tests) | Ships |
|---|---|---|---|
| S1 | PR 1 | ~520 (plus a zero-count `git mv`) | Alt screen, DECSC/DECRC, `esc_dispatch`, image stash (vim-exit fix) |
| S2 | PR 2 (or 2a + 2b) | ~620 | DECSTBM, DECOM, IND/NEL/RI, SU/SD, RIS, region image scroll |
| S3 | PR 3 | ~330 | ICH/DCH/ECH, IL/DL, REP, DECAWM, CNL/CPL, fuzz alphabet |
| S4 | PR 4 | ~200 | `Modes`, DECCKM |
| S5 | PR 5 | ~420 + fixtures (<= 3 x 64 KiB, not counted) | Replay harness, capture tool, spec notes, vttest record |
| **Total** | 5 PRs | **~2090** | |

- Chained PRs recommended: Yes
- 800-line budget risk: Medium (S2 is the only slice near the limit)
- Decision needed before apply: Yes (chain strategy is not chosen; S2 split is decided at the checkpoint in 2.0)
- S2 split rule: S2 starts with a line-count checkpoint (task 2.0). The move of
  `goto`/`line_feed`/`erase*` into `edit.rs` is a pure move (about 150 moved lines
  counted twice by diff stat). If the S2 diff, measured with `git diff --stat`
  after tasks 2.1 to 2.7, would exceed ~700 lines, ship the move as S2a (its own
  PR, behaviour unchanged, all existing tests green) before S2b (the rest).
  Task 2.1 is already isolated as a commit so this split is mechanical.

---

## Slice S1: Alternate screen, save/restore cursor, ESC dispatch (PR 1)

### Phase 1.A: Module split (zero counted lines)

- [x] 1.1 `git mv crates/nxg-core/src/terminal.rs crates/nxg-core/src/terminal/mod.rs` as its own commit (100% similarity, no edits). Verify `cargo test --workspace` is still green (D11)

### Phase 1.B: Test helpers and Screen refactor

- [x] 1.2 Create `terminal/testing.rs` (`#[cfg(test)]`: `term`, `sized`, `text`, `pos`) and make existing `mod.rs` tests use it. RED: a test in `testing.rs` asserting `sized(5,3)` builds the expected size and `text(row)` trims trailing blanks
- [x] 1.3 RED: tests in `terminal/screen.rs` that `Screen::new(size)` has a blank grid, cursor (0,0), `wrap_pending == false`, `saved == None`, full region, origin off. GREEN: add `Region` (`full`, `is_full`, `contains`) to `grid.rs` with its unit tests first, then `Screen` and `SavedCursor` (D1, D6)
- [x] 1.4 Convert `State` to hold `screen: Screen`, `dormant: Option<Box<Screen>>`, `alt_active`; keep cursor visibility, `pen`, `last_char` global (D2). RED: the whole existing terminal test suite must stay green after the call-site conversion (no behaviour change); add `Terminal::cursor()` test that it still builds the same `Cursor`
- [x] 1.5 RED: `State::resize` test that both active and dormant screens are resized, cursors and saved cursors clamped, `wrap_pending` cleared (TC "Resize resets region and clamps both cursors", region part deferred to S2). GREEN: resize applies to both screens

### Phase 1.C: DECSC/DECRC and esc_dispatch

- [x] 1.6 RED: `esc_dispatch` tests: `ESC 7`/`ESC 8` act; `ESC =` and `ESC >` are accepted and ignored; any ESC with intermediates is ignored; unknown final bytes are ignored. GREEN: implement `esc_dispatch` in `mod.rs` (only `7 8` act in this slice, `D E M c` arrive in S2)
- [x] 1.7 RED: TC "Pen is restored", "Restore without save" (home, default pen, origin and wrap reset), and `wrap_pending` round trip. GREEN: `save_cursor`/`restore_cursor` in `screen.rs` (clamped on restore) (D4, D14)
- [x] 1.8 RED: `CSI s` / `CSI u` with no params and no intermediates act as DECSC/DECRC; `CSI ? u`, `CSI > u`, `CSI 1 s` stay ignored (D14). GREEN: wire into the CSI dispatch
- [x] 1.9 RED: TC "Saved cursors are per screen". GREEN: per-screen `saved` verified through the swap in 1.12

### Phase 1.D: Image stash

- [x] 1.10 RED (`image/store.rs`): `stash_placements` moves active placements to `inactive`; `restore_placements` brings them back and drops placements added in between; `remove_image` purges both lists; `remove_placements(free)` frees an image only when neither list references it; `clear_placements` touches the active list only (D8). GREEN: implement `inactive: Vec<Placement>` and the four behaviours (II "Alternate screen hides main placements", "Alternate placements dropped on leave", "Deleted image leaves no stashed placement")

### Phase 1.E: Mode switching

- [x] 1.11 RED: `modes.rs` tests that `set_private_mode(mode, on)` routes `47`, `1047`, `1048`, `1049` and leaves unknown modes (`CSI ? 9999 h`) as no-ops. GREEN: create `terminal/modes.rs` and move the existing private-mode handling there
- [x] 1.12 RED: the xterm switch table rows in TC "Alternate screen": quit-restores-shell, `?1049` re-entry no-op, `?1049l` on main no-op (no stale DECRC) (D12), `?47` keeps alt content, `?1047` clears on leave, `?1048` as DECSC/DECRC, alt starts with full region and origin off, `?47/?1047` carry cursor position and `wrap_pending` (D5), main grid untouched while alt is active. GREEN: `enter_alt`/`leave_alt` in `screen.rs` with `mem::swap` (D1, D4, D5)
- [x] 1.13 RED: terminal-level image tests: placement hidden in alt and back after leave; alt placement dropped on leave; delete during alt leaves no stashed placement (II scenarios). GREEN: call `stash_placements`/`restore_placements` from `enter_alt`/`leave_alt`
- [x] 1.14 Update existing test `unknown_and_malformed_sequences_are_ignored` in `mod.rs`: remove or re-target the cases that now have meaning (`ESC 7`, `ESC 8`, `CSI s`, `CSI u`, `?47`, `?1047`, `?1048`, `?1049`) and keep truly unknown ones (`CSI ? 9999 h`, `CSI 1 ! z`, `ESC` with intermediates). RED first: change the expectations, see the test fail against the old behaviour, then confirm GREEN with 1.6 to 1.12
- [x] 1.15 RED: minimal hand-written vim-style stream test (`ESC[?1049h`, draw, `ESC[?1049l`) asserting shell lines and cursor restored. GREEN: already satisfied by 1.12, kept as the slice regression test

### Phase 1.F: Verification (S1)

- [x] 1.16 Run `cargo fmt --all --check`
- [x] 1.17 Run `RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets`
- [x] 1.18 Run `cargo test --workspace`
- [x] 1.19 Run `cargo +1.85 check --workspace --all-targets`
- [x] 1.20 Manual real-window check on Linux: open nxgterm, print a few shell lines, run `vim -u NONE`, quit with `:q`; the shell lines and cursor are restored. Repeat with `less`. Record the result in the PR description
- [x] 1.21 Confirm S1 diff is under 800 changed lines (`git diff --stat`, rename counted as 0) — exceeded (~935); user accepted size:exception on 2026-10-07

**PR boundary: S1 ends here.**

---

## Slice S2: Scroll regions, origin mode, index operations, RIS (PR 2)

### Phase 2.A: Checkpoint and pure move

- [ ] 2.0 Line-count checkpoint: after finishing 2.1 to 2.7, run `git diff --stat main`. If above ~700 lines, split: PR 2a = tasks 2.1 and 2.2 only (pure move plus `Region`-aware helper stubs removed), PR 2b = the rest. Record the decision in the PR description
- [ ] 2.1 Pure move: create `terminal/edit.rs` and move `goto`, `line_feed` and the `erase*` helpers out of `mod.rs` with no behaviour change. RED/guard: the existing suite must be green before and after; no new test code needed beyond a compile-only `edit.rs` smoke test calling the moved functions via `Terminal`

### Phase 2.B: Grid primitives and image region scroll

- [ ] 2.2 RED (`grid.rs`): `scroll_up_in`/`scroll_down_in` shift only rows `[top, bottom]`, blank new rows with the given cell, clamp `n` to region height, no-op on `n == 0`, handle `top == bottom` and 1-row grids. GREEN: implement both and remove the old `Grid::scroll_up(blank)` after migrating callers to the single entry point `State::scroll_up/scroll_down(top, bottom, n)` (D7)
- [ ] 2.3 RED (`image/store.rs`): `scroll_region_up/down` per design D9: placements anchored inside the span shift, dropped when fully outside the span (up: bottom edge `<= top`; down: anchor `> bottom`), outside placements never move; full-screen region keeps the existing `scroll_up`. GREEN: implement (II "Partial region scroll" and "Image scrolls away")

### Phase 2.C: Region, origin, index operations

- [ ] 2.4 RED: DECSTBM tests: defaults (0/absent), clamping, invalid `top >= bottom` ignored (TC "Invalid region ignored"), valid region homes the cursor (region top-left under DECOM). GREEN: `CSI t;b r` in `edit.rs` (D6)
- [ ] 2.5 RED: DECOM (`?6`) tests: CUP/HVP/VPA relative to `region.top` and clamped to the region, CPR relative to `region.top`, toggling homes the cursor, DECOM saved/restored by DECSC/DECRC (TC "Origin mode"). GREEN: origin math in `edit.rs`; add `?6` to `modes.rs`
- [ ] 2.6 RED: region-aware LF/VT/FF/IND/NEL/RI: region scroll scenario (TC "Region scroll"), LF below the region does not scroll, LF at last row outside region (TC "LF outside the region does not scroll"), RI at `region.top` scrolls down, NEL = CR + IND, `wrap_pending` cleared, `advance_over_image` and `finish_sixel` inherit region behaviour. GREEN: `index`, `reverse_index`, `line_feed` in `edit.rs`; add `D E M` to `esc_dispatch`
- [ ] 2.7 RED: CUU/CUD clamp to the region edge when the cursor starts inside it, else to the screen edge; SU/SD scroll the region by `n` (clamped), cursor unmoved. GREEN: implement in `edit.rs`; add `S`/`T` to CSI dispatch

### Phase 2.D: RIS and resize

- [ ] 2.8 RED: RIS test "Reset from alternate screen" (TC): main active and blank, full region, dormant `None`, pen, visibility, autowrap, DECCKM, `sixel_scrolling`, `last_char`, graphics state reset, `cell` and `responses` kept, placements and stash cleared, image data kept (D13, II). GREEN: `ESC c` in `esc_dispatch` and `reset` in `screen.rs`
- [ ] 2.9 RED: resize with alt and region active resets the region on both screens to full, clamps cursors and saved cursors (TC "Resize resets region and clamps both cursors"). GREEN: extend `State::resize`
- [ ] 2.10 Update `unknown_and_malformed_sequences_are_ignored` again: drop `ESC D/E/M/c`, `CSI r`, `CSI S/T`, `?6` from the "ignored" list. RED first (expectations changed), GREEN after 2.4 to 2.8

### Phase 2.E: Verification (S2)

- [ ] 2.11 Run `cargo fmt --all --check`
- [ ] 2.12 Run `RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets`
- [ ] 2.13 Run `cargo test --workspace`
- [ ] 2.14 Run `cargo +1.85 check --workspace --all-targets`
- [ ] 2.15 Confirm the S2 (or S2b) diff is under 800 changed lines

**PR boundary: S2 ends here (or S2a and S2b are two PR boundaries if split in 2.0).**

---

## Slice S3: Cell and line editing, REP, DECAWM, CNL/CPL (PR 3)

### Phase 3.A: Grid primitives

- [ ] 3.1 RED (`grid.rs`): `insert_cells`, `delete_cells`, `erase_cells` tests: shift within the row only, blank cells use the given cell, `n` clamped to the remaining width, no-op on empty range or `n == 0`, cursor at last column, row bounds assert like `row()`. GREEN: implement the three primitives

### Phase 3.B: Terminal policy

- [ ] 3.2 RED: ICH/DCH/ECH tests (TC "Insert characters"): blanks keep the pen background, cursor unmoved, `wrap_pending` cleared, `n` clamped, `n == 0` treated as 1. GREEN: `CSI @`, `CSI P`, `CSI X` in `edit.rs`
- [ ] 3.3 RED: IL/DL tests: act only when the cursor row is inside the region (TC "IL outside region"), scroll `[row, bottom]`, cursor to column 0, wrap cleared, images shift through `scroll_region_*` with `top = row`. GREEN: `CSI L`, `CSI M` in `edit.rs`
- [ ] 3.4 RED: REP tests (TC "Repeat"): repeats `last_char` n times, capped at `cols * rows`, no-op without `last_char`, `last_char` cleared by any control, CSI (except REP) or ESC. GREEN: `last_char` bookkeeping in `mod.rs`, `CSI b` in `edit.rs`
- [ ] 3.5 RED: DECAWM tests (TC "Wrap disabled"): `?7 l` makes the last column overwrite and never sets `wrap_pending`; disabling clears `wrap_pending`; DECAWM global and not swapped by `?1049` (D3). GREEN: global `autowrap` in `State`, `?7` in `modes.rs`, print path updated
- [ ] 3.6 RED: CNL/CPL tests: move by lines to column 0, clamped to the region edge when the cursor starts inside it. GREEN: `CSI E`, `CSI F`
- [ ] 3.7 Update `unknown_and_malformed_sequences_are_ignored` for `CSI @ P X L M b E F` and `?7`. RED first, GREEN after 3.2 to 3.6

### Phase 3.C: Fuzz

- [ ] 3.8 RED: extend the `random_and_truncated_input_never_panics` alphabet with `L M @ D E F J K 7 8 u b l =` (the new bytes must run without panics; fix any panic found, with a minimal regression test per bug)
- [ ] 3.9 Add `State::assert_invariants()` (`#[cfg(test)]`): for both screens, grid size equals terminal size, cursor in bounds, region valid (`top < bottom < rows` or full), saved cursor in bounds; every active and stashed placement references a stored image. RED: a deliberately corrupted state makes it fail (test with `#[should_panic]`)
- [ ] 3.10 Add `screen_ops_never_panic_across_resizes`: random tokens from `ESC[?1049h/l`, `?47`, `?1047`, `?1048`, `ESC[t;br`, `?6h/l`, `?7h/l`, `ESC M/D/E/7/8/c`, `ESC[nL/M/@/P/X/b/S/T`, `\n`, text (n up to 99), random resizes down to the minimum `TermSize` between tokens, calling `assert_invariants()` after every step. Resolve the open question on the minimum size (1x1; region needs `rows >= 2`) by letting invalid regions be ignored

### Phase 3.D: Verification (S3)

- [ ] 3.11 Run `cargo fmt --all --check`
- [ ] 3.12 Run `RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets`
- [ ] 3.13 Run `cargo test --workspace`
- [ ] 3.14 Run `cargo +1.85 check --workspace --all-targets`
- [ ] 3.15 Confirm the S3 diff is under 800 changed lines

**PR boundary: S3 ends here.**

---

## Slice S4: Cursor key mode (PR 4)

### Phase 4.A: Modes in core

- [ ] 4.1 RED (`terminal/modes.rs`): `Terminal::modes()` returns default `Modes { app_cursor_keys: false, alt_screen: false }`; `CSI ? 1 h` sets `app_cursor_keys`, `CSI ? 1 l` clears it; `alt_screen` follows `?1049h/l` and is derived from `State`, not stored twice; `ESC =` and `ESC >` leave modes unchanged; RIS clears `app_cursor_keys` (D10, D13). GREEN: add `Modes` (`Debug, Clone, Copy, Default, PartialEq, Eq`), `Terminal::modes()`, and `?1` handling; export `Modes` from `crates/nxg-core/src/lib.rs`
- [ ] 4.2 Update `unknown_and_malformed_sequences_are_ignored` for `?1`. RED first, GREEN after 4.1

### Phase 4.B: Key encoding and wiring

- [ ] 4.3 RED (`crates/nxgterm/src/keys.rs`): extend `encodes_navigation_keys`: with `app_cursor_keys` arrows encode `ESC O A..D` and Home/End `ESC O H`/`ESC O F`; without it unchanged (`ESC [ ...`); other keys unaffected (TC "Application cursor keys"). GREEN: change `encode` to take a `Modes` parameter and update all existing test call sites
- [ ] 4.4 Wire `app.rs` (line ~246) to pass `session.terminal.modes()` into `keys::encode`. RED: compile failure at the call site after 4.3 signature change; GREEN: pass the value. No extra unit test (composition root); covered by 4.3 plus the manual check below
- [ ] 4.5 Manual check on Linux: in `vim -u NONE` and `less`, arrow keys scroll/move correctly (DECCKM on), and after exit arrows in the shell prompt (history) still work

### Phase 4.C: Verification (S4)

- [ ] 4.6 Run `cargo fmt --all --check`
- [ ] 4.7 Run `RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets`
- [ ] 4.8 Run `cargo test --workspace`
- [ ] 4.9 Run `cargo +1.85 check --workspace --all-targets`
- [ ] 4.10 Confirm the S4 diff is under 800 changed lines

**PR boundary: S4 ends here.**

---

## Slice S5: Replay harness, capture tool, specs record, vttest (PR 5)

### Phase 5.A: Replay harness

- [ ] 5.1 RED: create `crates/nxg-core/tests/replay.rs` with a parser test over a tiny inline stream: splits on `ESC ] nxg-checkpoint ; <label> BEL`, parses `expect.txt` sections (`size`, `alt`, `cursor`, `row N: "text"` trimmed-end, unlisted rows unchecked), and fails on a deliberately wrong expectation. GREEN: implement the harness using only the public API (`advance`, `row`, `cursor`, `modes`)
- [ ] 5.2 RED: add a hand-made fixture `tests/fixtures/mini/{stream.vt,expect.txt}` (shell lines, `?1049h`, draw, `?1049l`, `[end]` asserting shell lines restored). GREEN: harness passes it
- [ ] 5.3 RED: guard tests in `replay.rs`: every fixture file under 64 KiB; none contains `/home/`, `/Users/`, `C:\Users`, the capture user or hostname. GREEN: add the scan (the scan must fail on a planted bad fixture in a unit test of the scanner itself)

### Phase 5.B: Capture tool

- [ ] 5.4 Resolve the open question: confirm whether `nxg-pty` exposes env and size control; if not, use `portable-pty` directly inside the example (no new crate, builds on 1.85)
- [ ] 5.5 RED: unit tests (inside the example, `#[cfg(test)]`) for the pure parts: script parsing (keystrokes, delays, `mark <label>`) and same-length scrubbing of `$HOME`, `$USER`, hostname and cwd (column positions preserved). GREEN: implement `crates/nxg-pty/examples/capture.rs` (spawns the developer-chosen program at 80x24 with `TERM=xterm-256color`, writes `stream.vt` with checkpoint markers). Dev-only, never built into the product
- [ ] 5.6 Capture fixtures (manual, Linux): `vim -u NONE`, `less`, `yazi` with scripted keystrokes into `tests/fixtures/{vim,less,yazi}/{stream.vt,expect.txt}`; htop skipped (not installed). Review captured bytes for leaked paths or hostnames
- [ ] 5.7 RED: replay tests for vim, less and yazi asserting mid-run checkpoints (alt true, cursor, listed rows) and `[end]` (main screen restored, alt false) (TC "Replay vim session"). GREEN: fix any terminal deviations found, each with a minimal regression test in the owning module; if a deviation is out of scope, record it instead

### Phase 5.C: Specs record and vttest

- [ ] 5.8 Run vttest menus 1 and 2 if available on the dev machine (installation is out of scope; if absent, record that and skip). Write results and known deviations into `openspec/changes/fullscreen-essentials/vttest-notes.md` (deviations are recorded, not blocking)
- [ ] 5.9 Reconcile spec deltas with observed behaviour: adjust `specs/terminal-core/spec.md` and `specs/inline-images/spec.md` only where tests proved a difference (spec changes merge at archive)

### Phase 5.D: Verification (S5)

- [ ] 5.10 Run `cargo fmt --all --check`
- [ ] 5.11 Run `RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets` (includes the capture example)
- [ ] 5.12 Run `cargo test --workspace`
- [ ] 5.13 Run `cargo +1.85 check --workspace --all-targets`
- [ ] 5.14 Manual real-window check on Linux (final): run `vim -u NONE`, `less` and `yazi` in nxgterm; render correctly, arrows work, and after quitting the previous shell lines and cursor are intact. Record the result in the PR description
- [ ] 5.15 Confirm the S5 diff is under 800 changed lines excluding fixtures

**PR boundary: S5 ends here.**

---

## Dependency summary

| Task group | Depends on |
|---|---|
| S1 (1.x) | none; 1.1 first |
| S2 (2.x) | S1 merged (RIS resets S1 state; `Screen` swap) |
| S3 (3.x) | S2 (IL/DL need regions, `edit.rs`) |
| S4 (4.x) | S1 (`alt_screen` derived from `State`); independent of S2/S3 code but ordered after them in the chain |
| S5 (5.x) | S1 to S4 (fixtures exercise all features) |

Within a slice, tasks are sequential in the order listed unless noted; within
S3 the primitives (3.1) precede policy (3.2 to 3.6), and 3.2 to 3.6 are
independent of each other after 3.1. Tasks 5.4 and 5.1 to 5.3 can run in
parallel before capture (5.6).
