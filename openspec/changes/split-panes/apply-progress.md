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
