# Proposal: Native split panes

Addresses Recommendation #23 (P2, "Tabs and splits: decide") in `openspec/RECOMMENDATIONS.md`. Source: `exploration.md`.

## Intent

Tabs shipped in 0.2.0, but one tab is still one shell. Users who want side-by-side shells must run ngmux. Decide for native splits and deliver them: split a tab into a nested layout of panes, each with its own shell.

## ADR: native splits supersede "ngmux is the answer"

- **Status**: Accepted 2026-10-09. Supersedes the "no tabs/splits, ngmux is the answer" branch of Recommendation #23 (the tabs half is already native).
- **Decision**: Native binary split tree per tab (`crates/nxgterm/src/panes.rs`, pure and generic like `tabs.rs`). The renderer API takes a pane list. Panes are cell-aligned with a 1-cell divider. A global `PaneId` (never reused) routes reader/waiter events.
- **Rationale**: Matches WezTerm/Ghostty/tmux mental model; splits work with images and native input without a multiplexer; tab precedent already proves the id/stale-event pattern.
- **Rejected**: Kitty-style named layouts (different model), fixed 1xN (dead end), per-pane offscreen textures (resize churn, bad for CPU/translucent), pixel-gap dividers (leftover pixels, fiddly hit-testing).
- **Consequences**: wider `WindowRenderer` signature (3 impls), GPU painter refactor, ngmux remains optional (persistence, remote).

## Scope

### In Scope
- Pure split tree: split, close, directional focus, resize, equalize, zoom, rects, min size.
- Multi-pane rendering (CPU and GPU), dividers, inactive-pane hollow cursor and dimming, `images` `layout.left` fix.
- Actions (all rebindable): `ctrl+shift+o` split right, `ctrl+shift+e` split down, `ctrl+alt+arrows` focus, `ctrl+shift+alt+arrows` resize, `ctrl+shift+x` close pane, `ctrl+shift+enter` zoom, equalize via palette. macOS follows the existing cmd/ctrl convention.
- Mouse: click-to-focus, drag divider to resize.
- Hot-reloadable `[panes]` config (divider color/width, dim amount).
- Closing last pane closes tab; last tab exits. Tab label from focused pane.

### Out of Scope
- cwd inheritance (new panes use launch cwd; separate follow-up).
- Focus-follows-mouse, per-pane tab-bar entries, previous/next cycling, focus reporting (`?1004`).
- Release v0.7.0 (delivery follow-up after merge; includes PRs #33-#35 already on main).

## Capabilities

### New Capabilities
- `panes`: split tree, focus, resize, zoom, equalize, close semantics, mouse routing.

### Modified Capabilities
- `rendering`: renders N panes, dividers, inactive cursor/dimming.
- `configuration`: `[panes]` section, new actions in `[keybindings]`, reload flag.
- `inline-images`: images placed and clipped correctly in non-origin panes.

## Approach

Behavior-preserving refactor first (one pane per tab, pinned by tests), then add actions. Strict TDD, CPU renderer as the reference for GPU tests. Resize storms mitigated by throttling pty resizes during divider drag.

## Slice plan (Feature Branch Chain, each PR under 800 code lines)

| # | Slice | Est. lines |
|---|-------|-----------|
| S1 | Pure `panes.rs` tree + tests | 450-550 |
| S2 | N-pane renderer API, CPU/GPU, divider, inactive cursor, images fix | 450-550 |
| S3 | App refactor, one leaf per tab, `PaneId` events | 500-650 |
| S4 | Split/focus/close/resize actions, defaults, palette | 450-600 |
| S5 | Mouse, `[panes]` config, zoom/equalize/dimming wiring | 350-450 |

S1 and S2 are independent; S3 needs both; S4 after S3; S5 after S4. First visible result in S4. Total about 2,200-2,700 lines.

## Affected Areas

| Area | Impact |
|------|--------|
| `crates/nxgterm/src/panes.rs` | New |
| `crates/nxgterm/src/app.rs`, `mouse.rs`, `reload.rs`, `command_palette.rs` | Modified |
| `crates/nxg-render/src/{style,renderer,images,paint}.rs`, `gpu/{renderer,painter,instance}.rs` | Modified |
| `crates/nxg-config/src/{keybindings,lib}.rs` | Modified |

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| GPU painter draw order/flicker; CI skips GPU without adapter | Med | CPU reference, offscreen comparison test |
| `ctrl+alt+arrows` clashes with KDE desktop switching | High | Rebindable; document; reconsider default if confirmed |
| Stale pointer/drag state on close, zoom, tab switch | Med | Reset like `tab_switched`; tests |
| Resize storms (SIGWINCH flood) | Med | Throttle during drag |
| Tiny panes panic | Low | Refuse split below minimum size |
| Wide mechanical diff in `WindowRenderer` impls | High | Isolate in S2 |

## Rollback Plan

Slices merge in order into the feature branch; revert the PR chain from the tip back. S1-S3 are behavior-preserving, so reverting S4/S5 alone restores single-pane UX. Unreleased until the chain lands on main.

## Dependencies

- None external. No new crates expected (MSRV 1.87 unaffected).

## Success Criteria

- [ ] A tab splits, nests, resizes, zooms, equalizes and closes by keyboard and mouse, with correct pty sizes.
- [ ] Images render and clip correctly in any pane.
- [ ] Single-pane behavior is unchanged (pinned by S3 tests).
- [ ] All new bindings rebindable; `[panes]` hot-reloads.
- [ ] fmt, clippy `-D warnings`, `cargo test --workspace`, MSRV 1.87 pass on all OS.
