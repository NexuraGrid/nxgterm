# Verification Report: fullscreen-essentials

**Change**: fullscreen-essentials (Recommendation #3, P0)
**Branch**: `feat/fullscreen-essentials-s5` (full chain S1, S2a, S2b, S3, S4, S5 on top of `main`), working tree clean
**Date**: 2026-10-07
**Mode**: openspec, Strict TDD (runner: `cargo test --workspace`)
**Verdict**: **PASS WITH WARNINGS**

Counts: 0 CRITICAL, 4 WARNING, 4 SUGGESTION.

## Completeness

| Metric | Value |
|---|---|
| Tasks total | 77 |
| Tasks complete | 77 |
| Tasks incomplete | 0 |

Every task in `tasks.md` is ticked. The notes on tasks 1.21, 2.0, 2.15, 3.7, 4.2, 4.5, 5.6, 5.8, 5.14 match the code and the apply-progress record.

## Build and test evidence (run by this verifier)

| Command | Exit | Result |
|---|---|---|
| `cargo fmt --all --check` | 0 | clean |
| `RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets` | 0 | clean (includes the `capture` example) |
| `cargo test --workspace` | 0 | all pass |
| `cargo +1.85 check --workspace --all-targets` | 0 | clean (MSRV respected) |

Per-binary counts of `cargo test --workspace`:

| Binary | Passed |
|---|---|
| nxg-config (lib) | 31 |
| nxg-core (lib) | 262 |
| nxg-core `tests/replay.rs` | 16 |
| nxg-pty (lib) | 33 |
| nxg-pty `tests/spawn.rs` | 7 |
| nxg-pty `tests/windows.rs` | 0 (Windows-only) |
| nxg-pty `examples/capture.rs` | 6 |
| nxg-render (lib) | 66 |
| nxg-render `tests/inline_images.rs` | 2 |
| nxgterm (bin) | 40 |
| Doc-tests (4 crates) | 0 |
| **Total** | **463 passed, 0 failed, 0 ignored** |

Coverage tool: not configured (analysis skipped, informational only).

## Spec compliance matrix

Totals: 11 requirements (terminal-core 10: 7 ADDED, 3 MODIFIED; inline-images 1 MODIFIED) and 31 scenarios (terminal-core 26, inline-images 5). All 31 are covered by a passing test. Rows marked "(stmt)" below prove requirement statements that have no scenario of their own.

### terminal-core

| Scenario | Test(s) | Status |
|---|---|---|
| Quitting a full-screen app restores the shell | `terminal::modes::tests::quitting_a_full_screen_app_restores_the_shell`, `a_vim_style_session_leaves_the_shell_untouched` | covered |
| Re-entry is a no-op | `modes::tests::reentering_the_alternate_screen_is_a_no_op` (content and saved cursor) | covered |
| ?47 keeps alternate content | `modes::tests::mode_47_keeps_the_alternate_content` | covered |
| ?1047 clears on leave | `modes::tests::mode_1047_clears_the_alternate_screen_on_leave` | covered |
| (stmt) ?1048 as DECSC/DECRC; ?1049 enter/leave; leave on main no-op | `mode_1048_saves_and_restores_the_cursor`, `mode_1049_always_enters_a_blank_alternate_screen`, `leaving_while_on_main_does_not_restore_a_stale_save` | covered |
| Pen is restored | `terminal::tests::decrc_restores_the_pen`, `screen::tests::restore_returns_the_saved_pen_and_position` | covered |
| Restore without save | `terminal::tests::decrc_without_save_goes_home_with_the_default_pen`, `screen::tests::restore_without_save_homes_and_resets` | covered |
| Saved cursors are per screen | `modes::tests::saved_cursors_are_per_screen`, `the_alternate_screen_has_its_own_empty_save_slot` | covered |
| (stmt) `CSI s/u` only bare; `CSI ? u`, `CSI > u`, `CSI 1 s` ignored | `terminal::tests::csi_s_and_u_without_parameters_act_as_decsc_and_decrc`, `csi_s_and_u_variants_with_parameters_or_intermediates_are_ignored` | covered |
| (stmt) DECRC does not revive pending wrap while DECAWM reset | `screen::tests::decrc_does_not_revive_a_pending_wrap_after_autowrap_was_turned_off` | covered |
| Region scroll | `edit::tests::lf_at_the_region_bottom_scrolls_only_the_region` | covered |
| Invalid region ignored | `edit::tests::an_invalid_region_is_ignored_and_leaves_the_cursor_alone` | covered |
| Origin mode | `edit::tests::origin_mode_makes_cup_hvp_and_vpa_relative_to_the_region` | covered |
| (stmt) RI, IND, NEL, VT/FF, SU/SD, CNL/CPL | `ri_at_the_region_top_scrolls_the_region_down`, `vt_ff_and_ind_scroll_like_lf`, `nel_is_a_carriage_return_plus_index`, `su_/sd_scrolls_the_region_*`, `cnl_and_cpl_*` | covered |
| Insert characters | `edit::tests::ich_opens_blank_cells_and_pushes_the_rest_right` (`a  bc`) | covered |
| IL outside region | `edit::tests::il_and_dl_do_nothing_outside_the_region` | covered |
| Repeat | `edit::tests::rep_repeats_the_last_printed_character` (`xxxx`) | covered |
| (stmt) DCH/ECH/IL/DL pen bg, REP cap, REP reset after controls | `dch_pulls_the_rest_left_and_ech_blanks_in_place`, `il_and_dl_blank_with_the_pen_background_*`, `rep_count_is_capped_at_the_screen_size`, `controls_csi_and_esc_forget_the_last_character` | covered |
| Reset from alternate screen | `screen::tests::ris_from_the_alternate_screen_returns_to_a_blank_main` (main active, blank, full region, no placements), `ris_drops_the_stashed_main_placements_too` | covered |
| Application cursor keys | `modes::tests::decckm_follows_set_and_reset` (state), `keys::tests::application_cursor_keys_use_ss3`, `encodes_navigation_keys` (encoding), manual `od -c` probe | partially covered (see W3) |
| Replay vim session | `tests/replay.rs::vim_session_replays` (plus `less_session_replays`, `yazi_session_replays`, `mini_fixture_replays`) | covered |
| Zero dimensions are rejected | `size.rs` test asserting `TermSize::new(0, 24) == Err(ZeroDimension)` (unchanged) | covered |
| Resize keeps top-left content | `terminal::tests::resize_keeps_content_and_clamps_cursor` | covered |
| Resize resets region and clamps both cursors | `screen::tests::resize_resets_the_region_on_both_screens_and_clamps_both_cursors`, `modes::tests::resize_applies_to_the_active_and_the_dormant_screen` | covered |
| Deferred wrap | `terminal::tests::wraps_at_last_column` | covered |
| Scrolling at the bottom | `terminal::tests::lf_at_bottom_scrolls_up`, `wrap_at_bottom_scrolls_up` | covered |
| Wrap disabled | `modes::tests::autowrap_off_overwrites_the_last_column`, `disabling_autowrap_cancels_a_pending_wrap`, `kitty_cursor_advance_past_the_edge_does_not_wait_without_autowrap` | covered |
| LF outside the region does not scroll | `edit::tests::lf_inside_or_below_the_region_moves_without_scrolling` | covered |
| CUP is one-based and clamps | `terminal::tests::cup_moves_cursor_one_based_and_clamps` | covered |
| Full clear removes images (ED 2 and 3) | `terminal::tests::full_screen_clear_removes_placements_but_keeps_images` | covered |
| Unknown sequences are ignored | `terminal::tests::unknown_and_malformed_sequences_are_ignored`, `modes::tests::unknown_private_modes_change_nothing`, fuzz `random_and_truncated_input_never_panics` | covered |

### inline-images

| Scenario | Test(s) | Status |
|---|---|---|
| Image scrolls away | `terminal::tests::placements_scroll_with_text_and_drop_off_the_top`, `image::store::tests::scroll_moves_placements_and_drops_those_off_screen` | covered |
| Alternate screen hides main placements | `modes::tests::main_placements_hide_in_the_alternate_screen_and_return`, `store::tests::stash_hides_placements_and_restore_brings_them_back` | covered |
| Alternate placements dropped on leave | `modes::tests::placements_made_on_the_alternate_screen_are_dropped_on_leave` | covered |
| Deleted image leaves no stashed placement | `modes::tests::deleting_an_image_in_the_alternate_screen_leaves_no_stashed_placement`, `store::tests::remove_image_purges_stashed_placements` | covered |
| Partial region scroll | `edit::tests::region_scroll_moves_placements_inside_it_and_spares_the_rest`, `store::tests::region_scroll_up_moves_only_placements_inside_the_span` | covered |

Additional safety net: `screen_ops_never_panic_across_resizes` (400 runs x 60 tokens, `assert_invariants` after every step).

Compliance summary: 30/31 scenarios fully covered, 1 partially covered (Application cursor keys: no automated test connects `Terminal::modes()` to `keys::encode` in `app.rs`; the wiring is the composition root and is proven manually). No scenario is untested or failing.

## Strict TDD compliance

| Check | Result | Details |
|---|---|---|
| TDD evidence reported | OK | "TDD cycle evidence" table present for S1 to S5 in `apply-progress.md` |
| All tasks have tests | OK | every behaviour task cites a test file; pure-move (1.1, 2.1), composition root (4.4), docs (5.8, 5.9) are justified exceptions |
| RED confirmed | OK | all cited test files exist (`screen.rs`, `modes.rs`, `edit.rs`, `grid.rs`, `store.rs`, `testing.rs`, `keys.rs`, `replay.rs`, `capture.rs`, fixtures) |
| GREEN confirmed | OK | all pass in this run (463/463) |
| Triangulation | OK | multi-case in all slices |
| Safety net | OK | recorded per slice; new files marked N/A (new) |

RED honesty: tasks 1.15, 2.9, 3.5 (RIS part) and 4.2/3.7/2.10 are declared "not RED" in apply-progress with mutation checks or rationale (see W4).

Assertion quality audit: no tautologies, no ghost loops, no assertion without a production call in the changed tests. The replay guard tests assert that at least 8 fixture files are scanned so the leak scan cannot pass vacuously. **Assertion quality**: 0 CRITICAL, 0 WARNING.

Test layers: unit (about 300 tests in nxg-core lib, 40 nxgterm, 6 capture), fuzz/property-style (2), integration (16 replay). E2E: none (manual real-window checks below).

## Design coherence

| Check | Result |
|---|---|
| `nxg-core` free of OS/window/GPU APIs | OK: no new platform imports in `terminal/`, `grid.rs`, `image/store.rs`; `std::fs`/`std::env` appear only in `tests/replay.rs` (test code reading fixtures and the host name for the leak scan) |
| No new dependencies | OK: `git diff main` touches no `Cargo.lock`; only change to a manifest is a `[[example]] capture` entry (`test = true`) in `crates/nxg-pty/Cargo.toml`, using the already present `portable-pty` |
| MSRV 1.85 | OK: `cargo +1.85 check --workspace --all-targets` clean (let chains were removed from `replay.rs`) |
| `Screen` swap via `mem::swap`, state split (D1, D2) | OK per tests (`Screen`, `dormant`, `alt_active`) |
| Single scroll entry `State::scroll_up/scroll_down(top, bottom, n)` (D7) | OK: old `Grid::scroll_up` removed |
| `Modes` value struct, no new port, `encode` stays pure (D10) | OK: `Modes` exported from `lib.rs`; `keys::encode` takes `Modes` |
| Image stash (D8), region scroll (D9) | OK, with the documented cosmetic gap (renderers do not clip to the region; recorded in `vttest-notes.md`) |
| Fixtures small and scrubbed | OK: total 9292 bytes, each under 3.5 KiB; guard test passes |

Deviations recorded in apply-progress are reasonable: `visible` directive in replay, `[end]` as stream tail, DECAWM-off kitty advance rule (added to the spec in 5.9).

## Proposal success criteria

| Criterion | Status |
|---|---|
| vim, less, yazi render and exit cleanly: replay tests + real window | met (replay 16/16; manual 2026-10-07) |
| Quitting vim restores shell lines and cursor | met (unit, replay and manual S1 check) |
| Arrow keys honour DECCKM | met (keys tests + `od -c` probe) |
| `cargo test`, fmt, clippy `-D warnings`, MSRV 1.85 | met (this report) |
| vttest menus 1-2 recorded with known deviations | partially met: vttest not installed, not run; known deviations recorded in `vttest-notes.md` (W2) |

## Manual evidence (orchestrator, 2026-10-07, KDE Plasma Wayland, Intel UHD, Vulkan)

- S1: exiting vim restores the shell lines.
- S2: DECSTBM header and footer stay fixed.
- S3: vim Ctrl-Y rows are contiguous after IL.
- S4: user verified with an `od -c` probe that arrows send `ESC [ A..D` normally and `ESC O A..D` with DECCKM.
- S5: vim, less and yazi render and restore the shell on exit.
- yazi Nerd Font icons render as missing glyphs: Recommendation #8, out of scope.
- vttest: not installed, not run.

## Size decisions (budget counts code only, user decision)

| Slice | Changed code lines | Decision |
|---|---|---|
| S1 | about 935 | `size:exception`, user-approved 2026-10-07 |
| S2 | 847 | split into S2a (134, pure move) and S2b (about 713) |
| S3 | 725 | within budget |
| S4 | 202 | within budget |
| S5 | 690 (excluding fixtures and openspec/) | within budget |

Total about 3,400 code lines versus the ~2,090 forecast.

## Issues

### CRITICAL
None.

### WARNING
- W1. S1 exceeded the 800-line budget (about 935) and shipped as a user-approved `size:exception`; the overall change ran about 60% over the forecast. Accepted, recorded for traceability.
- W2. Success criterion "vttest menus 1-2 recorded" is only met as "recorded as skipped": vttest was never run, so deviations in `vttest-notes.md` are expected, not observed. Follow-up: run vttest when installed and append results.
- W3. Spec scenario "Application cursor keys" is split across `Modes` state tests and pure `keys::encode` tests; the call in `app.rs` is verified only manually. Acceptable at a composition root, but a regression there would not be caught by CI.
- W4. Some apply-progress rows were not RED-first (1.15, 2.9, 3.5 RIS, 5.2 hand-corrected expectation, 1.6 "written with the batch"). Each is justified or backed by a mutation check; the strict RED-before-GREEN trail is therefore not fully demonstrable for these.

### SUGGESTION
- S1. `tasks.md` header still says "The chain strategy ... is not chosen yet" and the forecast table shows the original sizes; update at archive.
- S2. Tick the proposal success-criteria checkboxes (all currently `[ ]`) at archive.
- S3. `crates/nxg-core/tests/fixtures/yazi/expect.txt` contains two private-use glyphs (U+F48A, U+E68B); consider asserting only ASCII rows so the fixture does not depend on Nerd Font codepoints.
- S4. Cosmetic gap (D9): a shifted image placement can overlap rows above a scroll region because renderers do not clip to the region; track alongside Recommendation #8 or the scrollback work.

## Final verdict

**PASS WITH WARNINGS.** All commands exit 0, all 31 spec scenarios have passing tests (one partially, with manual proof), tasks are fully ticked, design rules hold (no new deps, no OS APIs in core, MSRV 1.85). No blockers for `sdd-archive`.
