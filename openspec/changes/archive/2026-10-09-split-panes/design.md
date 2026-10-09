# Design: Native split panes

Addresses Recommendation #23 in `openspec/RECOMMENDATIONS.md`. Inputs: `proposal.md`, `exploration.md`. Verified against the code on `docs/split-panes-exploration` (file and symbol references below are real).

## Technical Approach

A tab owns a binary split tree of panes (`crates/nxgterm/src/panes.rs`, pure, generic over the leaf, no OS/GPU). The app computes cell-aligned rects from the tree, fits each pane's `Terminal` and pty to its rect, and hands the renderer a pane list. Panes are drawn with the same pipeline that already draws the overlay (`Layout::at(col, row)`), dividers are `Shape::Rect`. Reader/waiter threads are keyed by a global, never-reused `PaneId`. Order of work: behavior-preserving refactor (one leaf per tab) first, new actions after (slices S1-S5).

## ADR-1: Native binary split tree supersedes "ngmux is the answer"

- **Status**: Accepted 2026-10-09 (same ADR as `proposal.md`; this section adds the design rationale).
- **Decision**: `Panes<T>` = binary tree, `Node::Leaf | Node::Split { axis, ratio, a, b }`, one per tab. Cell-aligned rects, 1-cell divider.

| Option | Tradeoff | Decision |
|---|---|---|
| Binary split tree | Arbitrary nesting; close = promote sibling; drag-resize maps to one ratio; directional focus needs rect geometry (pure, testable). Same model as WezTerm/Ghostty/tmux. | **Chosen** |
| Kitty-style named layouts over a flat list | Different mental model ("layout" not "split here"); layout algorithms to maintain; no arbitrary nesting. | Rejected |
| Fixed 1xN / Nx1 | Tiny, but a dead end: real splits need a rewrite. | Rejected |
| Keep "use ngmux" | No native input/image story, extra process; ngmux stays optional (persistence, remote). | Superseded |

## Architecture Decisions

| # | Decision | Alternatives rejected | Why |
|---|---|---|---|
| D1 | Pane list in `WindowRenderer` (`PaneView`), one frame, shared atlas/glyph cache | Per-pane offscreen textures (resize churn, bad for CPU/translucent); N calls of the single-terminal API (needs begin/end, same as D1 in disguise) | Mirrors the overlay precedent (`Overlay`, `Layout::at`); no new render targets |
| D2 | Cell-aligned panes, divider = 1 reserved cell, thin rect drawn inside it | Pixel gap (leftover pixels, fiddly hit-test) | Integer math, exact pty sizes |
| D3 | Global `PaneId(u64)`, id allocated by the app, tree is told the id | `(TabId, PaneId)` in events (more plumbing); id counter inside the tree (ids would repeat across tabs) | Smallest thread change; stale events dropped like `TabId` |
| D4 | Dimming and hollow cursor computed in the renderer from `PaneView { focused, dim }`, colors mixed CPU-side with one shared fn (`paint::dim`) | Shader uniform for dim (CPU/GPU divergence); alpha overlay quad (shapes are opaque) | One code path for both backends, parity testable |
| D5 | GPU texture cache scoped by pane id: key `(pane_id, image_key)` | Keep `u64` key | `ImageStore::next_key` starts at 1 per store (`nxg-core/src/image/store.rs:50`), so two terminals both own key 1. Today this already risks a wrong texture after a tab switch; with panes it is certain |
| D6 | Zoom changes only which rects `rects()` returns (hidden panes keep running, keep their size) | Remove from tree while zoomed | Tree stays valid; unzoom is a flag flip |
| D7 | Throttle pty resizes during divider drag (see Resize throttling) | Resize on release only (stale shell geometry); every motion event (SIGWINCH storm) | Live feel with bounded SIGWINCH rate |

## panes.rs API (S1)

Pure module, TDD, no dependency outside `std`. Mirrors `tabs.rs` style (`add_with`, getters, `Option` for stale ids).

```rust
pub struct PaneId(u64);                       // Copy, Eq, Hash; never reused (app-owned counter)
pub enum Dir { Left, Right, Up, Down }
pub enum Axis { Right, Down }                 // split right = side by side
pub struct CellRect { pub col: u16, pub row: u16, pub cols: u16, pub rows: u16 }
pub const MIN_COLS: u16 = 4; pub const MIN_ROWS: u16 = 2;

pub struct Panes<T> { /* root: Node<T>, focus: PaneId, zoomed: Option<PaneId> */ }
enum Node<T> { Leaf(PaneId, T), Split { axis: Axis, ratio: u16 /* permille of a */, a: Box<Node<T>>, b: Box<Node<T>> } }

impl<T> Panes<T> {
    pub fn new(id: PaneId, leaf: T) -> Self;
    /// Splits the focused pane. `make` gets the new pane's rect (so the pty can be spawned
    /// at the right size) and builds the leaf; on Err nothing changes.
    /// Unzooms first. Err(TooSmall) if either child would fall under MIN_COLS x MIN_ROWS.
    pub fn split_with<E>(&mut self, axis: Axis, id: PaneId, area: (u16, u16),
        make: impl FnOnce(CellRect) -> Result<T, E>) -> Result<(), SplitError<E>>;
    /// Removes `id` (shell exit or action); the sibling subtree takes the space and focus moves
    /// into it (the leaf nearest the closed one). Clears zoom if it was the zoomed pane.
    pub fn close(&mut self, id: PaneId) -> Closed<T>;   // Closed::{Last(T), Removed(T), Unknown}
    pub fn focus_dir(&mut self, dir: Dir, area: (u16, u16)) -> bool;  // unzooms; false = no neighbour
    pub fn set_focus(&mut self, id: PaneId) -> bool;
    /// Moves the innermost ancestor divider of matching axis by `cells` toward `dir`; clamps to minimums.
    pub fn resize(&mut self, dir: Dir, cells: u16, area: (u16, u16)) -> bool;
    pub fn drag(&mut self, divider: &DividerPath, pos: u16, area: (u16, u16));  // pointer in cells
    pub fn equalize(&mut self);                   // ratio = leaves(a) / leaves(a+b) per split
    pub fn toggle_zoom(&mut self);                // no-op with one leaf
    pub fn rects(&self, area: (u16, u16)) -> Vec<(PaneId, CellRect)>;  // zoomed: only that pane, full area
    pub fn dividers(&self, area: (u16, u16)) -> Vec<Divider>;          // empty while zoomed
    pub fn hit(&self, area: (u16, u16), col: u16, row: u16) -> Option<Hit>; // Hit::{Pane(id), Divider(path)}
    pub fn get(&self, id: PaneId) -> Option<&T>; pub fn get_mut(..); pub fn focused(&self) -> (PaneId, &T);
    pub fn iter(&self) -> impl Iterator<Item = (PaneId, &T)>; pub fn contains(&self, id: PaneId) -> bool;
}
pub struct Divider { pub rect: CellRect, pub axis: Axis, pub path: DividerPath }  // path = Vec<bool> from the root
```

Geometry rule: for a split over `len` cells, `total = len - 1` (divider), `a = round(total * ratio / 1000)` clamped to `[min, total - min]`, `b = total - a`. When the area is smaller than the minimums the clamp relaxes to 1 cell and rects never panic (they may overflow a tiny window; the frame/scissor clips). Directional focus: candidates are panes whose near edge touches the focused pane's `dir` edge (across the 1-cell divider) with overlapping perpendicular span; pick largest overlap, then the lowest tree order. `DividerPath` is invalidated by any structural change; the app drops pointer capture then (see Pointer capture).

## Renderer API (S2)

`crates/nxg-render/src/style.rs`:

```rust
pub struct PaneView<'a> { pub id: u64, pub terminal: &'a Terminal, pub col: u16, pub row: u16,
                          pub focused: bool, pub dim: f32 /* 0.0..=1.0, 0 when focused */ }
fn draw_layers(&mut self, header: Option<&Terminal>, panes: &[PaneView<'_>],
               overlay: Option<Overlay<'_>>, shapes: &[Shape]) -> Result<(), RenderError>;
```

`Renderer::draw(&Terminal)` stays: one `PaneView` at (0,0), focused, dim 0. Three impls change mechanically: `GpuRenderer`, `CpuWindowRenderer`, `Detached` in `app.rs`. `col,row` are relative to the grid below `header`; each pane's layout is `grid.at(col, row)`; padding stays window-level (already how `Layout::at` works).

- **CPU** (`renderer.rs::render_layers`): clear once, paint header, loop `paint(term, frame, grid.at(col,row), focused, dim)`, then overlay, then shapes. Per-pane order is unchanged (backgrounds, images below, cursor, glyphs, images above), so a pane's image never covers a neighbour (clipped to its grid by `images::paint`).
- **GPU** (`gpu/painter.rs`): `Frame { quads, term_quads }` becomes `Frame { quads: Vec<Instance>, panes: Vec<PaneQuads>, tail_start }` with `PaneQuads { backgrounds: Range<u32>, text: Range<u32> }`. `build` calls `instance::build` once per pane and concatenates (offsets recorded), then header, overlay, shapes form the tail range. `image_layers` returns per-pane `(below, above)` layers over one shared image instance buffer. `render` draws, per pane in order: backgrounds, images below (scissor = pane grid rect), text, images above; then resets the scissor and draws the tail. `draw_images` already takes a scissor; `grid_clip(term, layout)` gives it per pane. `textures.prune` becomes `prune(&[(id, &Terminal)])` and keys are `(pane_id, image_key)` (D5).
- **Images `layout.left` fix** (`images.rs`): `grid_clip` x = `padding + left`; `draws` dest.x adds `layout.left`. RED test first: a pane at `col > 0` places and clips an image at the right pixel (currently shifted by `left`, a latent bug since the overlay never has images).
- **Inactive cursor**: `paint_cursor(term, frame, layout, palette, focused)`; unfocused draws a 1 px (2 px when cell height >= 24) outline in `palette.cursor`, glyph under it keeps its normal foreground (no fg=bg swap in `renderer.rs` / `instance::build`). `instance::build` gains `focused` and `dim` parameters; outline = 4 `KIND_SOLID` instances with explicit `size`.
- **Dimming**: `paint::dim(color, background, amount)` mixes cell colors (backgrounds and glyph foregrounds) toward `palette.background`; used by both backends. Images are not dimmed (documented limitation).
- **Dividers**: the app emits `Shape::Rect` per `Divider` (color, `divider_width` px centered in the divider cell, clamped to the cell). Shapes draw last, over everything, on both backends: no renderer change.

## App wiring (S3, S4)

```
Session { tabs: Tabs<Tab>, next_pane: u64, capture: Option<Capture>, ... }
Tab     { panes: Panes<Pane> }                       // title now read from the focused Pane
Pane    { terminal: Terminal, pty: Box<dyn PtyControl>, title: String, pending_resize: Option<(WinSize, Instant)> }
UserEvent::{ Output(PaneId, Vec<u8>), Exited(PaneId), ConfigChanged }
```

- **Content area** = `grid_layout().grid_size(window)`; title bar cols and `palette_rect` use it instead of `tabs.active().terminal.size()`.
- **Event routing**: `find_pane(PaneId) -> Option<(tab_index, &mut Pane)>` scans tabs (few). Unknown id: drop. `output`: advance the terminal; redraw only if the tab is active (any pane of it, also hidden-by-zoom ones is skipped: redraw only if pane is visible). `Exited(id)`: `Panes::close`; `Closed::Last` closes the tab; last tab exits (today's behavior).
- **sync_grid_size** becomes: per tab, `rects(area)` then `Pane::fit(rect size, cell)` (existing `Tab::fit` logic moved to `Pane`). Hidden tabs are resized too, as today. Called on window resize, restyle, tab bar show/hide, split, close, resize, zoom, equalize.
- **Input**: `key_pressed` fallthrough, `send`, `paste`, `copy`, `select_all`, `selected_text`, `scroll_viewport` target the focused pane of the active tab.
- **Split**: `perform(SplitRight|SplitDown)` calls `split_with(axis, new_id, area, |rect| spawn_pane(new_id, config, WinSize::from(rect), proxy))`; a spawn error logs `nxgterm: ...` and leaves the tree unchanged. New panes start in the launch cwd (cwd inheritance out of scope).

### Pointer capture semantics (S5)

`Capture { pane: PaneId, kind: Selection | Report | Divider(DividerPath) }` replaces the single-grid `held`/`drag` pair.

1. Press: `Panes::hit` on the pointer cell. Divider: start `Divider` capture (no event to any shell). Pane: `set_focus` if different, then deliver the press to that pane (selection start, or mouse report if its modes ask) and capture it.
2. Move/release while captured: delivered to the captured pane only; coordinates from `cell_at(layout.at(pane rect), pane size, x, y)` which already clamps to the pane, so a drag never leaks into a neighbour.
3. Wheel and hover without a button: pane under the pointer, no capture, no focus change. Hover on a divider sets a resize cursor.
4. Capture is dropped (like `tab_switched` resets `held`/`drag` today) on: release, tab switch, close of the captured pane, zoom toggle, equalize, any split, and window focus loss.
5. Pointer positions are stored in window pixels and re-hit-tested; no per-pane cell cache to go stale.

### Resize throttling (S5)

Keyboard/structural resizes and window resizes apply at once (discrete). During a divider drag: `Panes::drag` updates the ratio on every motion; the affected `Terminal`s resize immediately (rendering stays consistent with the rects), but `pty.resize` is coalesced into `Pane.pending_resize` and flushed at most every 30 ms per pane (trailing edge, using `ControlFlow::WaitUntil` from `about_to_wait`), and always on release. Result: at most ~33 SIGWINCH/s per pane, final size always delivered.

## Configuration (S4, S5)

`[panes]` in `crates/nxg-config/src/lib.rs` (`Config.panes: PanesConfig`, `deny_unknown_fields`, `DEFAULT_CONFIG_TOML` documented):

| Key | Type | Default | Notes |
|---|---|---|---|
| `divider_color` | color (same type as `[colors]`) or unset | unset = foreground mixed 25% into background | |
| `divider_width` | u8 px, clamped 1..=cell size | 1 | |
| `inactive_dim` | f32 0.0..=0.9 | 0.25 | 0 disables |

`reload::Changes` gets `panes: bool` (`old.panes != new.panes`): redraw only, no restyle, no restart (the divider cell is always 1 cell, so no grid change).

New `Action`s (13; `ACTIONS` 25 -> 38; new `Category::Panes`, title "Panes"; names `snake_case`, listed automatically in the palette via `Action::ALL`):

| Action | Name | Default chord (Linux/Windows) |
|---|---|---|
| `SplitRight` / `SplitDown` | `split_right` / `split_down` | `ctrl+shift+o` / `ctrl+shift+e` |
| `FocusPane(Dir)` x4 | `focus_pane_{left,right,up,down}` | `ctrl+alt+{left,right,up,down}` |
| `ResizePane(Dir)` x4 | `resize_pane_{left,right,up,down}` | `ctrl+shift+alt+{left,right,up,down}` |
| `ClosePane` | `close_pane` | `ctrl+shift+x` |
| `ZoomPane` | `zoom_pane` | `ctrl+shift+enter` |
| `EqualizePanes` | `equalize_panes` | none (palette) |

Resize step: 2 columns / 1 row per press. macOS follows the `DEFAULTS` placeholder convention (`PRIMARY`/`CLIPBOARD`), picked in S4 to match `new_tab`/`close_tab`. `ctrl+alt+arrows` may clash with KDE/GNOME workspace switching: rebindable, documented.

## Data Flow

```mermaid
sequenceDiagram
    participant R as reader thread (PaneId)
    participant W as waiter thread (PaneId)
    participant L as winit loop (Session)
    participant T as Tabs/Panes
    participant P as Pane (Terminal+pty)
    participant G as WindowRenderer

    R->>L: UserEvent::Output(PaneId, bytes)
    L->>T: find_pane(PaneId)
    alt unknown PaneId (closed)
        T-->>L: None, event dropped
    else known
        T->>P: terminal.advance(bytes)
        opt tab active and pane visible
            L->>L: request_redraw
        end
    end
    W->>L: UserEvent::Exited(PaneId)
    L->>T: Panes::close(PaneId)
    alt Closed::Removed
        T-->>L: sibling takes space, focus moves
        L->>L: sync_grid_size (rects -> fit each pane)
    else Closed::Last
        L->>T: Tabs::close(tab); exit if no tab left
    end
    Note over L,P: window resize / split / divider drag
    L->>T: rects(area)
    L->>P: terminal.resize now, pty.resize now (or coalesced 30 ms during drag)
    L->>G: draw_layers(header, panes, overlay, dividers as Shape::Rect)
```

## File Changes

| File | Action | Slice |
|---|---|---|
| `crates/nxgterm/src/panes.rs` | Create (tree, rects, hit-test, tests) | S1 |
| `crates/nxg-render/src/style.rs` | Modify (`PaneView`, trait signature) | S2 |
| `crates/nxg-render/src/{renderer,cpu_window,paint,images}.rs` | Modify (pane loop, hollow cursor, `dim`, `left` fix) | S2 |
| `crates/nxg-render/src/gpu/{renderer,painter,instance,image}.rs`, `gpu/tests.rs` | Modify (per-pane ranges, scissor, cache scope, offscreen test) | S2 |
| `crates/nxgterm/src/app.rs` | Modify (`Tab`/`Pane`, `PaneId` events, fit, routing, actions, capture, throttle) | S3, S4, S5 |
| `crates/nxgterm/src/mouse.rs` | Modify (pane-relative `cell_at`, divider hit) | S5 |
| `crates/nxgterm/src/{title_bar,tab_bar,command_palette}.rs` | Modify (content-area size, focused-pane title) | S3 |
| `crates/nxg-config/src/keybindings.rs` | Modify (13 actions, `Panes`, defaults, tests) | S4 |
| `crates/nxg-config/src/lib.rs` | Modify (`PanesConfig`, default TOML) | S5 |
| `crates/nxgterm/src/reload.rs` | Modify (`Changes.panes`) | S5 |
| `crates/nxgterm/src/tabs.rs` | Unchanged | - |

## Slice map

| Slice | Design parts |
|---|---|
| S1 | panes.rs API, geometry, ADR-1 tests (split/close/focus/resize/equalize/zoom/rects/min size/hit) |
| S2 | Renderer API, CPU/GPU multi-pane, per-pane ranges and scissor, cache scope D5, `left` fix, hollow cursor, `paint::dim`; app uses one pane |
| S3 | `Tab`/`Pane` refactor, `PaneId` events, per-pane fit, focused routing, content-area size; one leaf per tab, behavior pinned |
| S4 | 13 actions, `Panes` category, defaults, palette, `split_with` + spawn, focus/resize/close, divider `Shape::Rect` drawing, `[panes]`-less defaults |
| S5 | Pointer capture, click-to-focus, divider drag + throttle, `[panes]` config + `Changes.panes`, zoom/equalize/dimming wiring |

## Testing Strategy (strict TDD, RED first)

| Layer | What | Approach |
|---|---|---|
| Unit (S1) | Every `Panes` op, rect sums (`a + 1 + b == len`), min-size refusal, close promotes sibling, focus after close, directional focus on asymmetric nests, zoom, equalize weights | Plain `#[test]`, `Panes<u32>` leaves, no OS/GPU |
| Unit (S2) | `images::draws`/`grid_clip` at `left > 0`; `paint::dim`; hollow vs block cursor on `Frame`; `Layout::at` regression | CPU path is the **reference** |
| Integration (S2) | N panes on CPU: backgrounds, glyphs, image clipped to its pane, divider rect over both | Pixel asserts in `renderer.rs` / `tests/inline_images.rs` |
| Offscreen GPU (S2) | Same 2-pane + image scene via `Painter` to a texture, read back, compare with the CPU frame (tolerance for glyph AA); also two panes each with image key 1 (D5) | `gpu/tests.rs`; skips with a message without an adapter (CI may skip: CPU tests carry the contract) |
| Unit (S3) | One-pane tab behaves as before: sizes, `Output` redraw only when active, stale `PaneId` dropped, `Exited` last pane -> tab -> app exit | Pure helpers extracted from `Session` where possible |
| Unit (S4) | Action names/titles/`ALL`, defaults parse and do not collide, `Bindings::new` override/remove, palette lists panes actions | `keybindings.rs` tests |
| Unit (S5) | Capture lifecycle (dropped on tab switch/close/zoom/split), throttle coalescing and release flush (injected clock), `PanesConfig` parse/clamp/unknown key, `reload::diff` flag | Pure state machines with fake `Instant` |

## Threat Matrix

N/A. New ptys are spawned through the existing `spawn_tab` path (`nxg_pty::spawn_shell_with`, configured shell, no new command construction from user input), no routing, VCS/PR automation or executable-file classification.

## Migration / Rollout

No migration. Behavior-preserving until S4; old configs load unchanged (`[panes]` optional). Revert the chain from the tip: S4/S5 alone restore single-pane UX. Unreleased until the chain lands on main (v0.7.0 is a separate follow-up).

## Open Questions

- [ ] Click on an unfocused pane: deliver the press to its program too (chosen) or consume it for focus only? Revisit if full-screen apps misbehave.
- [ ] macOS default chords for focus/resize (`ctrl+arrows` belong to Mission Control); decide in S4 with a macOS check.
- [ ] `divider_color` type: reuse the `[colors]` color type; confirm its parser in S5.
- [ ] Tiny-window behavior: rects may overflow below the summed minimums; consider `set_min_inner_size` as a follow-up.
- [ ] Images are not dimmed in inactive panes (limitation, acceptable for v1).
