# Exploration: Native split panes

Addresses Recommendation #23 (P2, "Tabs and splits: decide") in `openspec/RECOMMENDATIONS.md`. Tabs shipped in 0.2.0; the open half is "native splits vs relying on ngmux". This change records the decision for native splits. The proposal and design must carry the ADR.

## Current state

Window and tab model:
- `crates/nxgterm/src/tabs.rs`: generic `Tabs<T>` keyed by `TabId(u64)`, never reused, so late events from closed tabs are dropped. `add_with(|id| ...)` opens the tab after the active one. `Session.tabs: Tabs<Tab>`.
- `app.rs`: `struct Tab { terminal: Terminal, pty: Box<dyn PtyControl>, title: String }`. One tab is one terminal plus one pty. Every tab keeps reading output, but only the active one is drawn and gets input.
- Events: `UserEvent::{Output(TabId, Vec<u8>), Exited(TabId), ConfigChanged}`. `spawn_reader` and `spawn_waiter` are keyed by `TabId`. `Session::output` redraws only if the tab is active. `Exited` closes the tab, and the app exits when no tab is left.
- Sizing: `Session::sync_grid_size` computes ONE grid size from the window and `Layout` (padding, tab bar rows via `below`). It calls `Tab::fit` on every tab (`terminal.resize`, `set_cell_pixels`, `pty.resize`).
- Spawning: `spawn_tab` has no working-directory parameter. `nxg_pty::spawn_shell_with(size, Option<&ShellCommand>, backends)` takes no cwd, so every tab starts in nxgterm's launch cwd. The core handles no OSC at all (no OSC 7 or OSC 0/2), and focus reporting (`?1004`) is not implemented either.
- Pointer state in `Session`: `pointer` (cell), `held`, `clicks`, `drag` (selection `Drag`), `bar_pointer`, `wheel`. All of it assumes one grid. `mouse::cell_at(layout, size, x, y)` clamps to the grid.
- Selection, scrollback and mouse modes live inside `Terminal` (`selection()`, `display_offset()`, `modes()`), so they are already per terminal.
- Keys: `key_pressed` resolves `bindings::resolve` to an `Action`, and `perform` handles it, otherwise the key goes to `tabs.active_mut()`. `Session::send`, `paste`, `copy`, `select_all` and `selected_text` all target the active tab.
- Palette and title bar: `palette_rect` and `redraw` size the overlay and the bar from `tabs.active().terminal.size()` (cols of the active terminal).
- Config: `nxg-config/src/keybindings.rs` has `enum Action` (17 variants) and `const ACTIONS: [..; 25]`, with Category (Font, Scrollback, Clipboard, Tabs, General). Defaults are in the `DEFAULTS` table (`ctrl+shift+t/w`, `ctrl+tab`, `alt+1..9`, `ctrl+shift+p`). `ChordKey` already has Up/Down/Left/Right. `reload::diff` produces `Changes` flags.

Rendering (the main constraint):
- `WindowRenderer::draw_layers(header: Option<&Terminal>, terminal: &Terminal, overlay: Option<Overlay>, shapes: &[Shape])`, implemented by the GPU renderer, the CPU renderer and `Detached` in `app.rs`. All draw ONE main terminal.
- `Layout { cell, padding, left, top }`. `padding` is applied once, so `Layout::at(col,row)` gives a cell-offset layout that keeps window padding. The overlay already uses it to draw a second `Terminal` at an arbitrary cell. This is the precedent for drawing N terminals.
- CPU path (`renderer.rs::render_layers`): `paint(term, frame, layout)` per terminal. Adding a loop is trivial.
- GPU path (`gpu/painter.rs`):
  - `build` makes quads for term, then header, then overlay, then shapes. `Frame { quads, term_quads }` assumes ONE terminal's backgrounds/glyph split, because the pass draws backgrounds, images-below, text, images-above in order.
  - `image_layers` and `draw_images` take one `term` and one scissor.
  - N panes need per-pane instance ranges and a per-pane image scissor.
- Images (sixel/kitty): placements are per `Terminal`, anchored to screen rows. `images::grid_clip` and `images::draws` use `layout.padding` and `layout.top` but IGNORE `layout.left` (the overlay never has images). For a pane at col > 0 they would clip and place images wrongly. This is a real bug to fix in the rendering slice.
- Cursor: `paint_cursor` and `instance::build` draw a solid block if `cursor.visible`. There is no inactive/hollow variant.
- `Shape::Rect { x, y, width, height, color }` draws opaque pixel rects over everything on both renderers. It is a ready tool for dividers and a focus border.

## Affected areas

- NEW `crates/nxgterm/src/panes.rs`: pure split tree, generic over the leaf like `tabs.rs` (PaneId, split, close, focus by direction, resize, equalize, zoom, rects).
- `crates/nxgterm/src/app.rs`: `Tab` becomes a pane tree plus focus. Events change to `PaneId`. `output`, `Exited`, `sync_grid_size`, `open_tab` and `spawn_tab` change. `perform` gets the new actions, `key_pressed`, `send`, `paste`, `copy`, `select_all`, `selected_text`, the mouse handlers (`CursorMoved`, `MouseInput`, `MouseWheel`), `track_pointer`, `start_drag`/`drag_to`, `palette_rect` and `redraw` all change. The title bar cols should come from the content area, not a terminal.
- `crates/nxgterm/src/tabs.rs`: mostly unchanged (it stays generic). `next_id` is private, so panes need their own id counter.
- `crates/nxgterm/src/mouse.rs`: `cell_at` per pane rect, divider hit-test.
- `crates/nxgterm/src/title_bar.rs`, `tab_bar.rs`: the title comes from the focused pane. Little change.
- `crates/nxgterm/src/command_palette.rs`: new actions are listed automatically via `Action::ALL`. The overlay rect must use the content area size.
- `crates/nxg-render/src/style.rs` (`WindowRenderer`, `Overlay`), `renderer.rs` (CPU), `cpu_window.rs`, `gpu/renderer.rs`, `gpu/painter.rs` (+ `gpu/instance.rs`), `images.rs` (`grid_clip`/`draws` and `left`), `paint.rs` (inactive cursor), and the existing tests in `gpu/tests.rs` and `renderer.rs`.
- `crates/nxg-config/src/keybindings.rs`: new Actions, a `Panes` category, the `ACTIONS` array length, defaults and tests. `crates/nxg-config/src/lib.rs` plus `DEFAULT_CONFIG_TOML`: new `[panes]` keys. `crates/nxgterm/src/reload.rs`: new `Changes` flag.
- Later and optional (cwd inheritance): `crates/nxg-core/src/ports.rs`, `crates/nxg-pty/src/lib.rs` (`ShellCommand` gets cwd, or the port exposes the child pid).
- Specs: new `panes` capability, and deltas to `rendering`, `configuration`, `inline-images` (images in non-origin panes) and `terminal-core` only if focus reporting is added.

## Prior art (brief)

- WezTerm: per-tab binary split tree. Actions are SplitHorizontal/SplitVertical, ActivatePaneDirection, AdjustPaneSize, TogglePaneZoomState, CloseCurrentPane. Dividers are 1 cell. Mouse drag resizes. Inactive panes can be dimmed (`inactive_pane_hsb`). New panes inherit the cwd (via OSC 7 or the process cwd).
- Ghostty: split tree. `new_split:right|down`, `goto_split:<dir|previous|next>`, `resize_split:<dir>,<px>`, `toggle_split_zoom`, `equalize_splits`. Linux defaults are ctrl+shift+o (right) and ctrl+shift+e (down), ctrl+alt+arrows to focus and ctrl+shift+enter to zoom. Offers `unfocused-split-opacity` and `focus-follows-mouse`. A thin divider is drag-resizable.
- Kitty: named layouts (tall, fat, grid, splits, stack) with a per-tab layout state. More powerful but a different model (windows not split-tree). Not a fit for a "split here" mental model.
- Common ground: the binary tree is what users expect (nest arbitrarily, close collapses the sibling). Closing a pane gives its space to the sibling. The last pane closing closes the tab.

## Approaches

### Fork 1: layout model

1. **Binary split tree per tab (recommended).** `Node::Leaf(PaneId) | Split { dir, ratio, a, b }`. Pros: matches WezTerm/Ghostty/tmux, nesting, simple close (promote the sibling), natural directional focus and drag-resize. Cons: directional focus needs rect geometry (tractable, pure). Effort: Medium.
2. Flat list with named layouts (Kitty-style). Pros: simple addressing. Cons: no arbitrary nesting, a different mental model, layout algorithms to maintain. Effort: Medium, less flexible.
3. Fixed 1xN / Nx1 only. Pros: tiny. Cons: it is a dead end, and a rewrite is needed for real splits. Effort: Low.

### Fork 2: how N terminals reach the frame

1. **Pane list in the renderer API (recommended).** `draw_layers(header, panes: &[PaneView{terminal, col, row, focused}], overlay, shapes)`. Panes sit at `layout.at(col,row)` (cell units, padding stays window-level). The CPU path loops. The GPU path records per-pane quad ranges and image layers (backgrounds, images-below, text, images-above for each pane in order). Dividers are `Shape::Rect`. Pros: same pipeline, atlas and glyph cache, no extra textures, mirrors the existing overlay design. Cons: touches the GPU painter's single-terminal assumptions (`term_quads`, `image_layers`), plus the `left` bug in `images.rs`. Effort: Medium-High.
2. Per-pane offscreen textures, composited. Pros: isolation. Cons: new render targets, resize churn, a worse fit for translucent windows and the CPU path. Effort: High. Not recommended.
3. Scissor-per-pane with the existing single-terminal API called N times. Pros: minimal API change. Cons: would need N frames or a begin/end API anyway, and the clear/present is per frame. Effort: Medium. Effectively option 1 in disguise.

### Fork 3: geometry and divider

1. **Cell-aligned panes, divider = 1 cell column/row (recommended).** Rects in whole cells, a thin line (`Shape::Rect`, 1–2 px, centered) drawn in the divider cell. Pros: integer math, exact pty sizes, no remainder pixels, same as tmux/WezTerm. Cons: costs a cell of width per split and the divider looks thicker than a pixel line. Effort: Low.
2. Pixel gap (1–2 px) between panes with cell-aligned content. Pros: slimmer look. Cons: leftover pixels per split, and pane sizes derive from pixel rects (mouse hit-testing and layout get fiddly). Effort: Medium.

### Fork 4: ids and event routing

1. **Global `PaneId` for reader/waiter threads; tabs hold trees (recommended).** `UserEvent::Output(PaneId, ..)`/`Exited(PaneId)`. Lookup scans the (few) tabs' trees. `Tabs<Tab>` still owns tab order and `TabId`. Pros: smallest change to threads, stale events are still dropped. Cons: an O(tabs x panes) scan per chunk (negligible). Effort: Low.
2. `(TabId, PaneId)` in events. Pros: direct lookup. Cons: more plumbing for no real gain. Effort: Low-Medium.

### Fork 5: PTY sizing

Each pane's `Terminal` and pty are sized from its rect after every layout change (split, close, resize, window resize, restyle, tab bar show/hide). `sync_grid_size` becomes "compute rects per tab, `fit` each pane". Hidden tabs are also resized, as today. One `WinSize` per pane (`cell` pixels identical for all, so image queries stay correct).

## Recommendation

Fork 1.1 (binary tree), Fork 2.1 (pane list in the renderer API), Fork 3.1 (cell divider), Fork 4.1 (global `PaneId`). Put the split tree in a pure module (`nxgterm/src/panes.rs`), TDD first, with no OS or GPU dependency, like `tabs.rs`. Do a behavior-preserving refactor (one pane per tab) before adding any split action, so each PR is reviewable and the old behavior is pinned by tests.

Core semantics to adopt unless the user decides otherwise:
- A tab owns a tree. Focus is one `PaneId`. Input, clipboard, paste, palette and the cursor go to the focused pane.
- Output from any pane of the active tab redraws. Output from other tabs does not.
- Closing a pane (action or shell exit) gives its space to the sibling. Closing the last pane closes the tab; closing the last tab exits (today's behavior).
- Mouse events route by hit-testing the pointer against pane rects. A press captures the pane until release, so drags and selections stay in one pane. Coordinates are pane-relative.
- Selection, scrollback and mouse reporting stay per pane (already per `Terminal`).

## Open product decisions (for the user)

1. Default keybindings (Linux/Windows; macOS uses cmd/ctrl variants). Proposal, Ghostty-like: `ctrl+shift+o` split right, `ctrl+shift+e` split down, `ctrl+alt+arrows` focus (may clash with desktop workspace shortcuts), `ctrl+shift+alt+arrows` resize, `ctrl+shift+enter` zoom, `ctrl+shift+x` close pane. Alternatives: `alt+arrows` (clashes with readline word motion) or WezTerm's `ctrl+shift+alt+"`/`%`. All must be rebindable via `[keybindings]`.
2. Divider look: 1 cell reserved, line width (1 or 2 px), color (theme-derived dim vs configurable), highlight of the focused pane's edge.
3. Mouse drag to resize dividers: yes/no, and the hover cursor (resize cursor).
4. Focus follows mouse: off by default (opt-in) or not offered at first. Click-to-focus is assumed.
5. Pane zoom (temporarily maximize one pane): include in v1 or defer.
6. Equalize panes action: include or defer.
7. Closing the last pane: closes the tab (assumed). Confirm no "keep an empty pane" behavior.
8. New pane cwd: v1 starts in nxgterm's launch cwd, same as new tabs today. Inheriting the focused pane's cwd needs `/proc/<pid>/cwd` (Linux only), or OSC 7 plus shell integration, or a pty port change. Decide whether it is a follow-up.
9. Inactive pane treatment: dim (opacity or color mix) and/or hollow/hidden cursor. A solid block in an unfocused pane is misleading. At minimum the cursor needs a focused flag.
10. Do panes get their own tab-bar entries? Assumed no: one tab, one tree, label from the focused pane.
11. Config surface: a `[panes]` section (divider color/width, dim amount, focus_follows_mouse), hot-reloadable.
12. Directional "go to next/previous pane" cycling in addition to directional focus.

## Risks

- GPU painter refactor is the riskiest part: `term_quads`/`backgrounds` ordering and image layers assume one terminal, and a wrong draw order shows as flicker or images over text. GPU tests skip without an adapter, so CI may not exercise it. Mitigation: CPU path as the reference, offscreen GPU test comparing against it.
- `images::grid_clip`/`draws` ignore `layout.left` (an existing latent bug), so images in any pane not at col 0 would clip or draw wrongly. Needs a failing test first.
- Resize storms: dragging a divider or the window resizes several ptys per frame, and shells redraw on SIGWINCH. Needs throttling or resize-on-release to avoid flooding.
- `Session` carries single-grid pointer state (`pointer`, `held`, `drag`, `clicks`). Missed cases give wrong-pane clicks or stuck drags (for example a tab switch or pane close mid-drag). `tab_switched` resets `held`/`drag`; closing or zooming a pane needs the same.
- Min-size handling: tiny panes (1x1 cells) must not panic in `Layout::grid_size` consumers, and splitting must refuse below a minimum size.
- Event ordering: `Exited` and `Output` for a pane that just closed must be ignored safely (the `TabId` precedent handles this if `PaneId` is never reused).
- The `WindowRenderer` signature change touches three impls (GPU, CPU window, `Detached`) plus tests, so the diff is wide in mechanical edits.
- Windows ConPTY/winpty and macOS: more ptys per window, but nothing platform-specific in the design. Cwd inheritance is the only platform-divergent piece, deliberately deferred.
- No focus-reporting (`?1004`) support exists, so shells and vim cannot tell when a pane is unfocused. Not blocking, but it reduces the benefit of dimming; consider a follow-up.
- Review budget: the whole feature is far over 800 lines, so it must be chained.

## Size forecast and slice plan

Forecast: roughly 2,200–2,700 changed code lines including tests (openspec docs excluded), which is about 3x the 800-line budget. Chained PRs are required, Feature Branch Chain recommended, with strict TDD (RED first) in each.

| # | Slice | Contents | Est. lines |
|---|---|---|---|
| S1 | Pure split tree | `panes.rs`: tree, split, close, directional focus, resize ratio, equalize, zoom, rect computation, min size. Tests only, no wiring | ~450–550 |
| S2 | Rendering of N panes | `WindowRenderer` takes a pane list (+ focused flag, inactive cursor), CPU and GPU multi-pane, `images` `left` fix, divider `Shape`s, renderer tests. Still one pane in the app | ~450–550 |
| S3 | App refactor (no new behavior) | `Tab` holds a tree with one leaf, `PaneId` events, per-pane `fit`, focused-pane routing for input/clipboard/palette, content-area size for the bar and palette | ~500–650 |
| S4 | Split/focus/close actions | New `Action`s, `Panes` category, defaults, palette entries, split + shell spawn, focus by direction, close/exit semantics, resize by keyboard | ~450–600 |
| S5 | Mouse + config | Click-to-focus, pane-relative mouse reports/selection, drag-resize dividers, `[panes]` config, reload flag, zoom/equalize/dimming if accepted | ~350–450 |
| S6 (optional, separate change) | cwd inheritance | `ShellCommand` cwd or pid port, `/proc` read, tests | ~150–250 |

S1 and S2 are independent and could be reviewed in parallel; S3 depends on both; S4 on S3; S5 on S4. The first user-visible result lands in S4.

## Ready for proposal

Yes, after the user answers decisions 1–5 and 8 at least. The design must include an ADR superseding the "ngmux is the answer" branch of Recommendation #23, a sequence diagram for the output/exit/resize flow, and the tree-vs-layout rationale above.

## SDD session preflight (2026-10-08)

- Execution mode: interactive
- Artifact store: openspec
- Delivery strategy: ask-on-risk
- Review budget: 800 changed lines (code only, openspec docs excluded)
