# Apply progress: split-panes

## S1: Pure split tree (done, 8/8)

Branch `feat/split-panes-s1`. Mode: Strict TDD (RED run with `todo!()` stubs, then GREEN) per work unit.

| Task | Status | Notes |
|------|--------|-------|
| 1.1 | [x] | Types, `new`, `get/get_mut/focused/iter/contains`, `len`, `zoomed` |
| 1.2 | [x] | `rects`, `cut`/`bounds` geometry (relaxed minimums on tiny areas) |
| 1.3 | [x] | `split_with` (TooSmall, factory Err unchanged, focus new, unzoom) |
| 1.4 | [x] | `close` (promote sibling, nearest leaf focus, zoom cleared) |
| 1.5 | [x] | `focus_dir` (largest overlap, tree order tie), `set_focus` |
| 1.6 | [x] | `resize`, `drag`, `equalize` |
| 1.7 | [x] | `toggle_zoom`, `dividers`, `hit` |
| 1.8 | [x] | `mod panes;` with narrow `#[allow(dead_code)]` (S3 wires it) |

TDD evidence: RED counts per unit were 16/17, 15/32, 12/44 and 3/51 failing tests before implementation. Zoom behavior of `rects`/`dividers` was implemented in unit 1 (needed by `split_with` unzoom tests), so the 1.7 zoom tests were green on arrival; only `hit` was RED.

## Deviations

- `Closed::Last` carries no payload (design: `Last(T)`). Moving the only leaf out would leave the tree without a root; the tree stays intact and the caller drops the whole tab (and the `Panes` with it).
- `resize` also ends zoom when it moves a divider; `equalize` keeps zoom.
- Added `len()`, `zoomed()` and `CellRect::contains` (needed by S3/S5).
- `DividerPath` is a `Vec<bool>` alias; `Hit` is `Pane(PaneId) | Divider(DividerPath)`.

## Gates

- `cargo fmt --all --check`: ok
- `RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets`: clean
- `cargo test --workspace`: all pass (51 panes tests)
- `cargo +1.85 check`: fails because dependencies (wgpu 30) require rustc 1.87 (pre-existing); `cargo +1.87 check --workspace --all-targets`: ok

## S2: N-pane renderer (done, 8/8)

Branch `feat/split-panes-s2` (stacked on S1). Mode: Strict TDD; RED was a failing assertion (2.1) or a test referencing not-yet-existing items (compile failure) before each GREEN. Review budget: 1,189 changed lines (1,018 added, 171 removed, `git diff --shortstat feat/split-panes-s1 -- . ':!openspec'`), over the 800 budget; about 55% is tests. Needs a size decision.

| Task | Status | Notes |
|------|--------|-------|
| 2.1 | [x] | `grid_clip`/`draws` honor `layout.left`; origin-pane regression kept |
| 2.2 | [x] | `paint::dim`, plus `Look { focused, dim }`, `look_colors`, `cursor_outline` |
| 2.3 | [x] | `PaneView` (+ `single`, `look`), `draw_layers(header, panes, ...)`, `Detached` and `app.rs` call site use one `PaneView::single` |
| 2.4 | [x] | CPU pane loop in `render_layers`; two-pane test with a divider `Shape::Rect`, pane equals its solo render |
| 2.5 | [x] | Hollow cursor (1 px, 2 px at height >= 24), glyph keeps fg, dim on backgrounds and glyph fg; CPU and `instance::build_look` |
| 2.6 | [x] | `frame_quads` (pure, unit-tested) with `PaneQuads` ranges and `tail_start`; per-pane images below/above with `grid_clip` scissor |
| 2.7 | [x] | Textures keyed `(pane_id, image_key)`; `prune(&[(id, &Terminal)])` via pure `keep` |
| 2.8 | [x] | Offscreen GPU vs CPU two-pane + image scene (tolerance 2, real adapter ran: Intel UHD Vulkan); mutation check: keying by `(0, key)` fails it |

### TDD Cycle Evidence (S2)

| Task | Test file | Layer | Safety net | RED | GREEN | Triangulate | Refactor |
|------|-----------|-------|------------|-----|-------|-------------|----------|
| 2.1 | `images.rs` | Unit | 9/9 | 2 failing | 11/11 | left=0 and left>0, clip | none needed |
| 2.2 | `paint.rs` | Unit | 12/12 | compile fail (Look, dim) | 20/20 | 0, 1, .25, .5, clamp, NaN; 1 px and 2 px outline; hidden cursor | `!(a > 0.0)` -> clippy-clean form |
| 2.3-2.5 CPU | `renderer.rs` | Unit | 106+2 | compile fail (PaneView) | 118 -> all green | two panes, hollow cursor, dim 0.5 and 0 | none needed |
| 2.5 GPU | `gpu/instance.rs` | Unit | 14/14 | compile fail (`build_look`) | 20/20 | outline, hidden, glyph colour, dim | none needed |
| 2.6 | `gpu/painter.rs` | Unit | existing GPU parity | compile fail (`frame_quads`) | green | two panes + tail | none needed |
| 2.7 | `gpu/image.rs` | Unit | n/a | compile fail (`keep`) | green | same key two panes, deleted image, gone pane | none needed |
| 2.8 | `gpu/tests.rs` | Offscreen | 4 GPU tests | mutation: pane key `(0, k)` -> fails | green | CPU parity at tolerance 2 | none needed |

### Work Unit Evidence (S2)

| Evidence | Value |
|---|---|
| Focused test command | `cargo test -p nxg-render` -> 124 + 2 passed, 0 failed |
| Runtime harness | `cargo test -p nxg-render --lib gpu::tests` ran on a real Vulkan adapter; app still draws one `PaneView::single` (`cargo build --workspace` ok, 340 nxgterm tests green); GUI not launched in this headless run |
| Rollback boundary | `crates/nxg-render/src/**` and the `PaneView` call sites in `crates/nxgterm/src/app.rs` (`Detached`, `draw_layers` call) |

### Deviations (S2)

- Added `paint::Look` (focused + dim) and `instance::build_look`; `build` stays as the focused wrapper (header, overlay) to avoid touching 14 tests. `paint_backgrounds` and `paint_cursor` take a `Look`.
- GPU `frame_quads` is a free function (pure, testable without a device) instead of a method on `Painter`.
- `Renderer::draw` on the GPU/CPU windows wraps `PaneView::single`; `Detached::draw_layers` returns the same fatal error `draw` did.
- Images are not dimmed (documented limitation); the hollow cursor outline is not dimmed.

### Gates (S2)

- `cargo fmt --all --check`: ok; clippy `-D warnings`: clean; `cargo test --workspace`: all pass; `cargo +1.87 check --workspace --all-targets`: ok.

## S3: App refactor, one leaf per tab (done, 6/6)

Branch `feat/split-panes-s3` (stacked on S2). Mode: Strict TDD; RED was a `todo!()` stub run (10/10 failing) for `tab.rs` and a compile failure (missing `Pane`, `focused`, `focused_mut`) for the rest. Review budget: 726 changed lines (598 added, 128 removed, `git diff --shortstat feat/split-panes-s2 -- . ':!openspec'`), within the pre-authorized 1,300; about 55% is tests.

| Task | Status | Notes |
|------|--------|-------|
| 3.1 | [x] | `tab.rs`: `PaneIds` (global counter), `pane_mut` (stale id -> `None`), `shows` (active tab and not hidden by zoom), `exit` -> `Exit::{Stale, Pane, Tab}` (last tab closed leaves `Tabs` empty so the app exits) |
| 3.2 | [x] | `Pane { terminal, pty, title }`, `Tab<T> { panes: Panes<T> }`; `Tab::fit` moved to `Pane::fit` (approval tests with a fake pty) |
| 3.3 | [x] | `UserEvent::{Output(PaneId,_), Exited(PaneId)}`, reader/waiter and `spawn_pane` keyed by `PaneId`; routing through `tab::pane_mut`/`tab::exit` (no separate `find_pane`) |
| 3.4 | [x] | `sync_grid_size` fits each pane to its `tab::placements` rect; title bar, tab bar hit-test and palette sized from `Session::content_size()` |
| 3.5 | [x] | Input, paste, copy, select_all, selected_text, scroll, wheel, pointer use `tab::focused(_mut)`; tab label from the focused pane; `redraw` builds the `PaneView` list from the placements |
| 3.6 | [x] | Single-pane behavior pinned: one pane fills the content area with focus, redraw rule, exit flow, `Pane::fit` rules. GUI not launched (headless run) |

### TDD Cycle Evidence (S3)

| Task | Test file | Layer | Safety net | RED | GREEN | Triangulate | Refactor |
|------|-----------|-------|------------|-----|-------|-------------|----------|
| 3.1, 3.4 | `tab.rs` | Unit | 187/187 | 10/10 failing (`todo!()`) | 10/10 | two tabs, zoom hidden, stale, last pane vs one of two | none needed |
| 3.2 | `app.rs` | Unit (fake pty) | n/a (new tests) | compile fail (`Pane`) | 4/4 | new size, same size no-op, cell-only change, `send` | none needed |
| 3.5 | `tab.rs` | Unit | 187/187 | compile fail (`focused`, `focused_mut`, `each_mut`) | 12/12 | no tab, per-tab focus, 3 panes | none needed |
| 3.3 | n/a | Wiring | 194/194 | n/a | green | Triangulation skipped: type change (`TabId` -> `PaneId`) checked by the compiler | none needed |
| `PaneId::raw` | `panes.rs` | Unit | 51/51 | compile fail | green | 0, 41, distinct | none needed |

### Work Unit Evidence (S3)

| Evidence | Value |
|---|---|
| Focused test command | `cargo test -p nxgterm` -> 194 passed, 0 failed (12 `tab::`, 4 `app::`, 1 `panes::raw`) |
| Runtime harness | N/A for the GUI (headless, no display); `cargo build --workspace` and `cargo clippy --workspace --all-targets` clean; pty/threads path covered by the compiler plus `Pane::fit`/`send` fake-pty tests |
| Rollback boundary | `crates/nxgterm/src/{app,tab}.rs`, `PaneId::raw` in `panes.rs`, the removed `Tabs::index_of/get_mut` in `tabs.rs`, `mod tab;` in `main.rs` |

### Deviations (S3)

- `Pane` has no `pending_resize` yet (a field nothing reads fails `-D warnings`); S5 adds it with the throttle.
- No `find_pane`: `tab::pane_mut`, `tab::shows` and `tab::exit` are free functions over `Tabs<Tab<T>>` (generic, testable without a window). Focus helpers are free functions too, so callers keep borrowing other `Session` fields.
- Removed the now-unused `Tabs::index_of` and `Tabs::get_mut(TabId)` (and their assertions); `tabs.rs` is no longer unchanged.
- `mod panes` keeps `#[allow(dead_code)]` (module level, comment updated): split, close, focus, resize, zoom and hit-test are wired in S4/S5. `mod tab` needs none.
- Pointer mapping still uses the focused pane's terminal size at the origin; pane-relative `cell_at` is S5.
- Panes hidden by zoom are not refit on a cell size change (S5 zoom wiring should refit them on unzoom).

### Gates (S3)

- `cargo fmt --all --check`: ok; clippy `-D warnings`: clean; `cargo test --workspace`: all pass; `cargo +1.87 check --workspace --all-targets`: ok.
