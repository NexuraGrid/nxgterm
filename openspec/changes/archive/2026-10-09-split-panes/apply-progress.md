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

## S4: Actions and split UX (done, 5/5)

Branch `feat/split-panes-s4` (stacked on S3). Mode: Strict TDD; RED was a compile failure (missing variants/functions) for 4.1 and 4.2-4.3, and the existing `documented_sample_lists_every_default_binding` test failing for 4.5. Review budget: 745 changed lines (720 added, 25 removed, `git diff --shortstat feat/split-panes-s3 -- . ':!openspec'`), within the pre-authorized 1,300; about 60% is tests.

| Task | Status | Notes |
|------|--------|-------|
| 4.1 | [x] | 13 `Action` variants (flat, `Dir` lives in nxgterm), `Category::Panes`, `ACTIONS` 25 -> 38, `DEFAULTS` 25 -> 37, palette entries; macOS chords decided (below) |
| 4.2 | [x] | `tab::split_active` through `Panes::split_with`; `Session::split` spawns the shell at the new rect size; TooSmall/spawn error log `nxgterm: ...` and leave the tree unchanged; `panes_changed` refits |
| 4.3 | [x] | `tab::{command, resize_step, focus_active, resize_active, close_focused}` and `App::pane_command`; last pane closes the tab, last tab exits the app (also from a palette click) |
| 4.4 | [x] | `dividers.rs`: `shapes` (1 px centred in the divider cell, clamped to the cell) and `default_color` (fg 25% into bg); emitted in `Session::redraw` under the window buttons |
| 4.5 | [x] | `DEFAULT_CONFIG_TOML` lists the 12 chords, the 13 names, the macOS variant and the KDE/GNOME `ctrl+alt+arrows` clash |

### TDD Cycle Evidence (S4)

| Task | Test file | Layer | Safety net | RED | GREEN | Triangulate | Refactor |
|------|-----------|-------|------------|-----|-------|-------------|----------|
| 4.1 | `keybindings.rs` | Unit | 83/83 nxg-config | compile fail (36 errors) | 88/88 | names/titles, Linux defaults, macOS defaults, no collisions per platform, rebind/`none` | none needed |
| 4.1 palette | `command_palette.rs` | Unit | 195/195 nxgterm | 2 existing tests failing (counts 24 -> 37, "zo" list) | 18/18 | `equalize`, `split`, `panes` queries | none needed |
| 4.5 | `lib.rs` | Unit | 88/88 | `documented_sample_lists_every_default_binding` failing | green | every default and every name documented | none needed |
| 4.2, 4.3 | `tab.rs` | Unit | 194/194 | compile fail (`command`, `split_active`, ...) | 18/18 `tab::` | action map, step 2 cols/1 row, split active tab only, TooSmall, factory Err, no tab, focus/resize no-neighbour, close rules | none needed |
| 4.4 | `dividers.rs` | Unit | n/a (new file) | tests written with the code (not RED-first); mutation check: dropping the centring fails the vertical test | 8/8 | vertical, horizontal, clamp 0/3/99, nested, no divider, colour | none needed |

### Work Unit Evidence (S4)

| Evidence | Value |
|---|---|
| Focused test command | `cargo test -p nxg-config -p nxgterm` -> 88 + 207 passed, 0 failed |
| Runtime harness | N/A for the GUI (headless, no display): split/focus/close/resize paths are covered by `tab::` tests on `Panes<u32>`; `Session::split` (pty spawn) and divider drawing are checked by the compiler and clippy only. Manual run of `nxgterm` is still pending |
| Rollback boundary | `keybindings.rs` + sample TOML in `lib.rs`; `tab.rs` helpers + `pane_command`/`split`/`panes_changed` in `app.rs`; `dividers.rs` + its call in `redraw`; palette test updates |

### Decisions and deviations (S4)

- macOS chords (open question resolved): split, close and zoom keep the `ctrl+shift` chords like `new_tab`/`close_tab` (no system clash); focus and resize use the `PRIMARY` placeholder (cmd on macOS): `cmd+alt+arrows` and `cmd+shift+alt+arrows`, so `ctrl+arrows` (Mission Control) are never bound.
- `zoom_pane` and `equalize_panes` exist as actions (palette, defaults, names) but do nothing until S5 wires them (`perform` arm returns `true`, `tab::command` maps them to `None`).
- Resize step is 2 columns / 1 row (`tab::resize_step`); the divider width is the constant 1 px until `[panes]` lands in S5.
- Every layout change (split, focus, resize) resets `held`/`drag` (`panes_changed`); close reuses `tab_switched`.
- Fix outside the plain scope: the app now also exits when a palette click closes the last tab (`palette_done` path in `window_event`).
- The module-level `#[allow(dead_code)]` on `mod panes` is gone; item-level allows remain on `CellRect::contains`, `Hit`, `leaf_count`, `equalize` (x2), `toggle_zoom`, `drag`, `hit` (S5).
- Divider drawing was written together with its tests (no separate RED run); a mutation check proved the tests bite.

### Gates (S4)

- `cargo fmt --all --check`: ok; clippy `-D warnings`: clean; `cargo test --workspace`: all pass; `cargo +1.87 check --workspace --all-targets`: ok.

## S5: Mouse, config, zoom/equalize/dim (done, 6/6)

Branch `feat/split-panes-s5` (stacked on S4). Mode: Strict TDD; RED was a compile failure (missing `panes`, `Throttle`, `under`, `Capture`, ...) for every unit, and a forced failing run (`panes` flag hard-wired to false) for 5.2. Review budget: 1,223 changed lines (1,059 added, 164 removed, `git diff --shortstat feat/split-panes-s4 -- . ':!openspec'`), within the pre-authorized 1,300; about 60% is tests.

| Task | Status | Notes |
|------|--------|-------|
| 5.1 | [x] | `PanesConfig { divider_color: Option<Rgb>, divider_width: NonZeroU16, inactive_dim: f32 }`, `clamp_dim`, documented `[panes]` in `DEFAULT_CONFIG_TOML`; color parser = the `[colors]` `Rgb` (`#rrggbb`/`#rgb`) |
| 5.2 | [x] | `Changes.panes` (`old.panes != new.panes`), redraw only |
| 5.3 | [x] | `mouse::{Capture, Drag, Pointer, divider_icon}`; `tab::{Under, under, frame, focus_pane}`; any press focuses the pane under it and is delivered to it; capture dropped on release, tab switch, close, zoom, equalize, split, focus change/resize (`panes_changed`) and window focus loss |
| 5.4 | [x] | `tab::drag_divider` + `Throttle<T>` (`throttle.rs`, 30 ms, injected `Instant`), `Pane::{fit_at, poll_resize, flush_resize}`, `ApplicationHandler::about_to_wait` with `ControlFlow::WaitUntil`; flush on release and focus loss |
| 5.5 | [x] | `PaneCommand::{Zoom, Equalize}` through `tab::{zoom_active, equalize_active}`; `tab::dim_of` feeds `PaneView.dim` from `[panes] inactive_dim`; `dividers::style` feeds width and colour. Panes hidden by zoom are refit by `sync_grid_size` when the layout returns |
| 5.6 | [x] | Gates below; GUI not launched (headless run) |

### TDD Cycle Evidence (S5)

| Task | Test file | Layer | Safety net | RED | GREEN | Triangulate | Refactor |
|------|-----------|-------|------------|-----|-------|-------------|----------|
| 5.1 | `nxg-config/src/lib.rs` | Unit | 88/88 | compile fail (`Config.panes`) | 92/92 | defaults, all keys, dim 2/-0.5/0/nan/inf/-inf, width 0/-1, bad color, unknown key `gap` with line, sample documents the keys | none needed |
| 5.2 | `reload.rs` | Unit | 7/7 | flag hard-wired false: 1 failing | 8/8 | dim, width, color change; scrollback change leaves it false | none needed |
| 5.5 dim, divider style | `tab.rs`, `dividers.rs` | Unit | 207/207 | compile fail (`dim_of`, `style`) | 210/210 | focused/unfocused/0; default vs configured colour and width | removed `DEFAULT_WIDTH` |
| 5.5 zoom/equalize | `tab.rs` | Unit | 210/210 | compile fail (`Zoom`, `zoom_active`, `equalize_active`) | 213/213 | zoom twice restores sizes, single pane, no tab; resize then equalize, already equal | removed item-level `dead_code` allows |
| 5.4 throttle | `throttle.rs` | Unit (fake clock) | n/a (new file) | compile fail (`Throttle`) | 6/6 | first push, coalescing + trailing edge, quiet interval, 1 s of 5 ms motion = 33-35 sends with the last value delivered, flush, clear | none needed |
| 5.4 pane | `app.rs` | Unit (fake pty) | 6/6 | compile fail (`fit_at`, `resize`) | 8/8 | terminal at once vs pty every 30 ms; release flush; direct fit supersedes | `fit` folded into `fit_at(.., None)` |
| 5.3, 5.4 mouse | `tab.rs`, `mouse.rs` | Unit | 213/213 | compile fail (`under`, `frame`, `focus_pane`, `drag_divider`, `Capture`, `Pointer`) | 229/229 | pane/divider/zoomed hit, click focuses once, pane-relative clamped cells, drag 2 cells right + minimum clamp + same column, stacked divider by row, capture ends per button, reset, cursor icon | none needed |

### Work Unit Evidence (S5)

| Evidence | Value |
|---|---|
| Focused test command | `cargo test --workspace` -> nxg-config 92, nxgterm 229 (plus nxg-render 124+2, others unchanged), 0 failed |
| Runtime harness | N/A for the GUI (headless, no display): hit-testing, focus, drag, zoom, equalize, dim and the throttle are covered by pure tests on `Panes<u32>`/fake pty/fake clock; the `window_event` glue, `about_to_wait` wake-ups and the resize cursor are checked by the compiler and clippy only. Manual run (click focus, drag a divider, edit `[panes]` live) is still pending |
| Rollback boundary | `throttle.rs`; `mouse.rs` capture types; `tab.rs` pointer/zoom helpers; the mouse arms, `Pane::fit_at` and `about_to_wait` in `app.rs`; `PanesConfig` in `nxg-config/src/lib.rs`; `Changes.panes` |

### Decisions and deviations (S5)

- `inactive_dim` clamps to 0.0..=1.0 as the spec says (the design table said 0.0..=0.9); `divider_width` is a `NonZeroU16` (0 is an error, the renderer clamps it to the cell) instead of a `u8`.
- `Capture` is `Selection { pane, drag } | Report { pane, button } | Divider(path)` held in `Pointer { capture, over }` and replaces `held`, `drag` and the cached `pointer` cell; positions are stored as window pixels (`Session.cursor`) and re-hit-tested on every event.
- Any button press (not only the left) focuses the pane under it, so a middle-click paste lands in the pane that was clicked. Wheel scrolls/reports to the pane under the pointer without focus change.
- The unconditional reset (`Pointer::reset` in `panes_changed`, `tab_switched`, palette toggle, focus loss) implements "dropped on close, zoom, equalize, split, tab switch"; there is no separate "is my pane still alive" check.
- Selection auto-scroll while dragging now uses the captured pane's own top and bottom edges.
- Throttle: the first resize goes out at once, later ones inside 30 ms coalesce into one trailing send; a direct `fit` supersedes a waiting size. A waiting size is also flushed when the window loses focus.
- Moving the pointer over the tab bar never reports to a pane (as before); a release over the bar is reported at the captured pane's nearest cell.
- Divider width and colour come from `dividers::style`; images stay undimmed (documented limitation).

### Gates (S5)

- `cargo fmt --all --check`: ok; `RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets`: clean; `cargo test --workspace`: all pass; `cargo +1.87 check --workspace --all-targets`: ok.

