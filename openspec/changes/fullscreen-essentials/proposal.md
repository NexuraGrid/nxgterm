# Proposal: Full-screen app essentials

Addresses **Recommendation #3 (P0)** in `openspec/RECOMMENDATIONS.md`.

## Intent

Real-window smoke test (Linux, KDE Wayland, 2026-10-07): quitting `vim` leaves
its screen behind and the previous shell lines are lost. Cause: no alternate
screen (`?1049`), no scroll regions, no `ESC` dispatch, no DECCKM. vim, less
and yazi (the daily workflow) must render and exit cleanly.

## Scope

### In Scope (chained PRs S1 to S5, each under the 800-line budget)
- **S1**: `Screen` struct swapped via `mem::swap`; alt screen `?1049/?1047/?47/?1048`; DECSC/DECRC (`ESC 7/8`, `CSI s/u`); `esc_dispatch`; image placement stash.
- **S2**: DECSTBM, DECOM, IND/NEL/RI, SU/SD, RIS, region-aware LF; `Grid` primitives; region-aware image scroll.
- **S3**: ICH/DCH/ECH, IL/DL, REP, DECAWM, CNL/CPL.
- **S4**: `Modes` via `Terminal::modes()`; DECCKM in `keys.rs` and app wiring.
- **S5**: replay harness (`crates/nxg-core/tests/replay.rs`) with vim/less/yazi fixtures; spec deltas; vttest menus 1-2 run and recorded.

### Out of Scope / Follow-ups
- Charsets (G0/G1, `ESC ( 0`, SO/SI), tab stops (HTS/TBC), DECCOLM, DECALN, DECSCNM, double-size lines.
- DECKPAM/DECKPNM (needs winit `KeyLocation` plumbing).
- Scrollback (#5), DA2/XTVERSION, OSC. vttest/htop installation.

## Capabilities

### New Capabilities
None.

### Modified Capabilities
- `terminal-core`: "Printing and line discipline" and "Cursor movement" gain alt screen, regions, origin/wrap modes, cell/line editing, save/restore cursor, `ESC` dispatch, DECCKM.
- `inline-images`: "Scrolling, clearing and resize" gains per-screen placements and region-aware scroll.

## Approach

From exploration: Fork 1 option 1 (`Screen` swap), Fork 2 option 1 (primitives in `Grid`, policy in `Terminal`), Fork 3 option A (full-screen region unchanged; with a partial region, placements inside shift and drop on leaving; outside stay). `Modes` is a value struct on `Terminal`, no new port; `encode(key, text, mods, modes)` stays pure. Single `Grid` scroll entry point to keep scrollback (#5) feasible. Fuzz alphabet extended.

## Affected Areas

| Area | Impact | Description |
|---|---|---|
| `crates/nxg-core/src/terminal.rs` | Modified | `Screen`, `esc_dispatch`, CSI handlers, modes |
| `crates/nxg-core/src/grid.rs` | Modified | region scroll, insert/delete/erase cells |
| `crates/nxg-core/src/image/store.rs` | Modified | inactive stash, region scroll, stash purge |
| `crates/nxg-core/src/lib.rs` | Modified | export `Modes` |
| `crates/nxgterm/src/{keys,app}.rs` | Modified | DECCKM |
| `crates/nxg-core/tests/` | New | replay tests, fixtures |

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Stashed placements reference freed images | Med | `remove_image` purges stash |
| Resize with alt screen/region active | Med | Reset region, clamp both cursors; fuzz |
| wrap_pending vs DECAWM, LF at region bottom, ICH/DCH edges | Med | Explicit tests |
| Fixtures leak paths/hostnames | Low | Scrub, keep small |
| vttest deviations | High | Record as known, not blocking |
| ~1700 lines in chained PRs | Med | Slices ordered, S1 ships the vim fix |

## Rollback Plan

Each slice is a separate PR; revert in reverse order (S5 to S1). S1 alone is
revertible to the v0.1.0 single-screen model. No config, data or protocol
migration. Spec deltas merge only at archive.

## Dependencies

S2 before S3 (IL/DL need regions); RIS in S2 resets S1 state. No new crates.

## Success Criteria

- [ ] vim, less, yazi render and exit cleanly: replay tests pass and manual check in a real Linux window.
- [ ] Quitting vim restores shell lines and cursor.
- [ ] Arrow keys honour DECCKM.
- [ ] `cargo test --workspace`, fmt, clippy `-D warnings` and MSRV 1.85 check pass.
- [ ] vttest menus 1-2 recorded with known deviations.
