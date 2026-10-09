# Tasks: Native split panes

Strict TDD: every pair is RED (failing test) then GREEN (minimal code). Commits are work units (tests with code). Review budget for this project: 800 code lines per PR (`git diff --shortstat <base> -- . ':!openspec'`).

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | 2,200-2,700 total (S1 450-550, S2 450-550, S3 500-650, S4 450-600, S5 350-450) |
| 800-line budget risk | Medium (S3 and S4 upper bounds approach 650/600; none expected over 800) |
| Chained PRs recommended | Yes |
| Suggested split | S1 -> S2 -> S3 -> S4 -> S5 (S1 and S2 are independent; S3 needs both) |
| Delivery strategy | ask-on-risk |
| Chain strategy | pending (Feature Branch Chain recommended) |

Decision needed before apply: Yes
Chained PRs recommended: Yes
Chain strategy: pending
400-line budget risk: High (every slice exceeds 400; the project budget is 800, where risk is Medium)

Decision needed: confirm Feature Branch Chain (tracker `feat/split-panes` draft/no-merge; PR1 base = tracker; PR2 base = PR1 branch; each later PR base = previous PR branch; only the tracker merges to main).

### Suggested Work Units

| Unit | Goal | Likely PR | Focused test command | Runtime harness | Rollback boundary |
|------|------|-----------|----------------------|-----------------|-------------------|
| S1 | Pure `Panes<T>` tree | PR 1 (base tracker) | `cargo test -p nxgterm panes::` | N/A: pure module, unused | delete `panes.rs` + `mod` line |
| S2 | N-pane renderer, CPU+GPU, images fix, cache scope, hollow cursor, dim | PR 2 (base PR 1) | `cargo test -p nxg-render` | `cargo test -p nxg-render gpu::` offscreen (skips without adapter); app still draws one pane | `nxg-render` files + `Detached` and call sites in `app.rs` |
| S3 | `Tab`/`Pane` refactor, `PaneId` events, one leaf per tab | PR 3 (base PR 2) | `cargo test -p nxgterm` | run `nxgterm`: shell, tabs, resize, exit behave as before | `app.rs`, title/tab/palette sizing |
| S4 | 13 actions, defaults, split/focus/resize/close, dividers | PR 4 (base PR 3) | `cargo test -p nxg-config -p nxgterm` | run `nxgterm`: split, nest, focus, close, last-pane closes tab | `keybindings.rs` + `perform` arms |
| S5 | Mouse capture, divider drag + throttle, `[panes]`, zoom/equalize/dim | PR 5 (base PR 4) | `cargo test --workspace` | run `nxgterm`: click focus, drag divider, edit `[panes]` live | `mouse.rs`, `PanesConfig`, `Changes.panes` |

## S1: Pure split tree (`crates/nxgterm/src/panes.rs`)

- [x] 1.1 RED+GREEN: types (`PaneId`, `Dir`, `Axis`, `CellRect`, MIN consts), `Panes::new`, `get/get_mut/focused/iter/contains`; tests with `Panes<u32>`.
- [x] 1.2 RED+GREEN: `rects` geometry (`a + 1 + b == len`, permille rounding, clamp, tiny area never panics).
- [x] 1.3 RED+GREEN: `split_with` (spec "Split right", "Split refused when too small": `TooSmall`, factory `Err` leaves tree unchanged, new pane focused, unzooms).
- [x] 1.4 RED+GREEN: `close` (sibling promoted, focus to nearest leaf, `Last/Removed/Unknown`, clears zoom; spec "Shell exits in a split").
- [x] 1.5 RED+GREEN: `focus_dir` on asymmetric nests (largest overlap, then tree order), `set_focus`.
- [x] 1.6 RED+GREEN: `resize` (innermost matching-axis divider, minimum clamp), `drag`, `equalize` (leaf-count weights).
- [x] 1.7 RED+GREEN: `toggle_zoom` (single leaf no-op; spec "Zoom toggle" restores sizes), `dividers`, `hit`.
- [x] 1.8 Register `mod panes;` in `main.rs`; clippy clean (allow dead code until S3).

## S2: N-pane renderer (`crates/nxg-render`)

- [x] 2.1 RED: `images::draws`/`grid_clip` at `layout.left > 0` place/clip at the right pixel (spec "Image in the right pane", "Clipped at pane edge"); GREEN: add `left` to dest.x and clip x. Includes "Origin pane unchanged" regression.
- [x] 2.2 RED+GREEN: `paint::dim(color, background, amount)` (0 is identity, 1 reaches background) in `paint.rs`.
- [x] 2.3 RED+GREEN: `PaneView` + `draw_layers(header, panes, overlay, shapes)` in `style.rs`; `Renderer::draw` wraps one origin pane; update `Detached` in `app.rs`.
- [x] 2.4 RED: CPU two-pane test (cells only inside rects, divider `Shape::Rect` visible, single pane equals old frame); GREEN: pane loop in `renderer.rs::render_layers` and `cpu_window.rs`.
- [x] 2.5 RED+GREEN: unfocused hollow cursor (1 px, 2 px at cell height >= 24; none when hidden; glyph keeps fg) and `dim` applied to backgrounds and glyph fg, CPU and `instance::build`.
- [x] 2.6 RED: GPU `Frame` per-pane ranges/`tail_start` unit test; GREEN: refactor `gpu/painter.rs` (`PaneQuads`, per-pane images below/above, scissor via `grid_clip`, tail draw).
- [x] 2.7 RED: image-cache key test, two terminals both with image key 1 must keep distinct textures; GREEN: key `(pane_id, image_key)` and `textures.prune(&[(id, &Terminal)])` in `gpu/image.rs` (D5).
- [x] 2.8 Offscreen GPU vs CPU two-pane + image scene (tolerance 2; skips without adapter) in `gpu/tests.rs`.

## S3: App refactor, one leaf per tab (`crates/nxgterm/src`)

- [x] 3.1 RED: pure helpers for event routing (stale `PaneId` dropped; `Output` redraws only for visible pane of active tab; last `Exited` closes tab, last tab exits); GREEN: `PaneId` counter `next_pane`.
- [x] 3.2 RED+GREEN: introduce `Pane` (terminal, pty, title, `pending_resize`) and `Tab { panes: Panes<Pane> }`; move `Tab::fit` to `Pane::fit`.
- [x] 3.3 GREEN: `UserEvent::{Output(PaneId,_), Exited(PaneId)}`, reader/waiter threads keyed by `PaneId`, `find_pane`.
- [x] 3.4 RED+GREEN: `sync_grid_size` via `rects(area)` + `Pane::fit`; content area from `grid_layout().grid_size(window)` for `title_bar.rs`, `tab_bar.rs`, `command_palette.rs` (sizes unchanged for one pane).
- [x] 3.5 GREEN: route key input, `send`, `paste`, `copy`, `select_all`, `selected_text`, `scroll_viewport` to the focused pane; tab title from focused pane; render via `draw_layers`.
- [x] 3.6 Pin single-pane behavior with tests for sizes, redraw rules and exit flow; manual run of `nxgterm`.

## S4: Actions and split UX

- [ ] 4.1 RED: `keybindings.rs` tests for 13 action names/titles, `Category::Panes`, `ACTIONS` 38, defaults parse without collisions, override/`"none"` removal ("Rebinding"), palette lists `equalize_panes`; GREEN: add actions, chords, macOS placeholders (resolve open question).
- [ ] 4.2 RED+GREEN: `spawn_pane` and `perform(SplitRight|SplitDown)` through `split_with`; spawn error logs `nxgterm: ...`, tree unchanged; `sync_grid_size` after.
- [ ] 4.3 RED+GREEN: `FocusPane(Dir)`, `ResizePane(Dir)` (2 cols / 1 row), `ClosePane` (last pane closes tab) wired in `perform`.
- [ ] 4.4 RED+GREEN: divider `Shape::Rect` emission (default color: fg 25% into bg; width 1) in `app.rs`.
- [ ] 4.5 Document chords and the KDE/GNOME `ctrl+alt+arrows` clash in the default TOML comments.

## S5: Mouse, config, zoom/equalize/dim

- [ ] 5.1 RED+GREEN: `PanesConfig` in `nxg-config/src/lib.rs` (`divider_color`, `divider_width`, `inactive_dim` clamp, nan/inf errors, unknown key names `gap`, `--print-config` parses to defaults); resolve color parser open question.
- [ ] 5.2 RED+GREEN: `reload::Changes.panes` (`old != new`, redraw only); spec "Dim changed live".
- [ ] 5.3 RED+GREEN: `Capture { pane, kind }` lifecycle (spec "Click focuses", "Close during drag"); dropped on release, tab switch, close, zoom, equalize, split, focus loss. `mouse.rs` pane-relative `cell_at`, divider hit and resize cursor.
- [ ] 5.4 RED+GREEN: divider drag via `Panes::drag` ("Divider drag" scenario) with 30 ms `pending_resize` throttle using an injected clock; flush on release.
- [ ] 5.5 GREEN: wire `ZoomPane`, `EqualizePanes`, config-driven dim and divider style into `PaneView`/shapes.
- [ ] 5.6 Run full gate: `cargo fmt --all --check`, clippy `-D warnings`, `cargo test --workspace`, `cargo +1.87 check --workspace --all-targets`; manual check of success criteria.
