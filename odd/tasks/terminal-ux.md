# Feature: terminal UX batch (drop, copy, block glyphs, title bar)

Branch: `feat/terminal-ux`

## Objective

Close four UX gaps versus WezTerm/Ghostty: dropping files into the terminal,
copying everything the terminal holds, pixel-perfect block/box glyphs (Claude
Code mascot, TUI borders), and a WezTerm-style, configurable title bar.

## Problem

- No `WindowEvent::DroppedFile` handling: agents (Claude Code) cannot receive
  an image path by drag and drop.
- Copy only covers a selection; SelectAll is unbound; no "copy scrollback";
  copy-on-select only reaches PRIMARY.
- Box-drawing / block / sextant / powerline glyphs come from font outlines, so
  they leave gaps or overflow the cell (mascot looks broken).
- The integrated title bar is the tab bar; no centered window title, button
  style or colors cannot be customized.

## Scope

- T1 Drag and drop: dropped file paths are shell-quoted, space-separated and
  pasted through `paste(text)` (keeps bracketed paste). Multiple files in one
  drop are joined.
- T2 Copy: `SelectAll` bound by default (ctrl+shift+a), new `CopyScrollback`
  action (whole history + screen to clipboard), `copy_on_select` option to
  target `clipboard` in addition to `primary`.
- T3 Built-in glyphs: procedural drawing for U+2500–257F box drawing,
  U+2580–259F block elements, U+1FB00–1FB3B sextants and powerline
  U+E0B0–E0B3; config `font.builtin_box_drawing` (default true).
- T4 Title bar (user chose WezTerm style): icon left, window title centered,
  circular buttons right; tabs shown below only with 2+ tabs; config for
  button style (`circles` / `windows`), title bar colors and height.

Out of scope: rounded window corners/shadows, ligatures, HTML copy.

## Tasks

- [x] T1 Drag and drop file paths into the terminal + tests
- [x] T2 Copy everything: SelectAll default binding, CopyScrollback, copy_on_select target + tests
- [x] T3 Built-in procedural box/block/sextant/powerline glyphs (CPU + GPU) + tests
- [x] T4 WezTerm-style configurable title bar + tests
- [ ] T5 Track OSC 0/2 window titles in nxg-core and show them in the
  title bar and tab labels (gap found in T4: the bar shows only the program
  name, the reference shows "~: ngmux - ngmux") + tests

## Route

- T1: inline direct (one event arm + quoting helper, already mapped).
- T2–T4: delegated direct (one writer each; 2+ non-trivial files).
Trigger evidence: explorer mapping (app.rs:792 event loop, paste.rs:15,
selection.rs, font.rs:387, title_bar.rs).

## Checks

`cargo test --workspace`, `cargo clippy --workspace --all-targets`,
`cargo fmt --all --check`. Test-first for quoting, keybinding defaults,
glyph coverage bitmaps and title-bar layout.

## Forecast

~1100 authored changed lines total (T1 ~120, T2 ~180, T3 ~450, T4 ~350).
Delivery strategy: ask-on-risk; one work-unit commit per task, PR slicing
decided by the user before any push.

## Progress

- Exploration done; title bar style chosen: WezTerm style.
- T1 done (inline): `nxg_core::paste::dropped_path` (backslash-escaped on
  Unix, double-quoted on Windows, trailing space) + `WindowEvent::DroppedFile`
  arm in `app.rs` pasting through bracketed paste. RED observed (missing fn),
  GREEN after. Checks: cargo test --workspace 894 passed / 0 failed; clippy
  clean; fmt clean. Commit a50bedb. Native review: granted, one lens
  (reliability), approved and acknowledged (lineage review-bd97f9dfc4dd3ea6).
  Reviewed boundary is now a50bedb.
- T2 done (delegated writer): `Action::CopyScrollback` (`copy_scrollback`,
  unbound, in palette), SelectAll default `ctrl+shift+a` / `cmd+a`,
  `CopyOnSelect { Off, Primary, Clipboard, Both }` accepting bool or string,
  `Terminal::all_text()` (history + screen, selection untouched), README and
  openspec updated. RED observed (missing types/methods), GREEN after.
  Writer checks: cargo test 899/0, clippy -D warnings clean, fmt clean.
  Parent spot check: same results re-run. Note: with clipboard/both,
  select_all also copies to the clipboard (shared copy-on-select path).
  Commit 7e67081; assess medium, under_budget (pending in slice).
- T3 done (delegated writer): new `crates/nxg-render/src/builtin.rs`
  (table-driven box drawing, blocks, sextants, powerline E0B0–E0B3, cell-sized
  coverage, shared rounded edges, 4x4 supersampled arcs/diagonals);
  `Font::with_builtin_glyphs`, built-in glyphs placed at the cell origin
  (xmin 0, ymin = baseline - cell.height); `font.builtin_box_drawing`
  (default true, applies on reload). RED: 22 nxg-render tests failing on
  stubs + config compile error; GREEN after. Writer checks: 923/0, clippy
  clean, fmt clean; parent spot check identical. Gaps: U+1FB3C+ wedges and
  powerline E0B4+ still from fonts; thin arcs at 1px look faint.
  Commit 30114cc; assess medium, slice_budget_reached -> native review of
  a50bedb..30114cc (T2+T3): granted, reliability lens approved and
  acknowledged (lineage review-3331e658a891e63b). Reviewed boundary = 30114cc.
- T4 done (delegated writer): `[window.title_bar]` (button_style
  circles|windows, height, show_title, show_icon, background, foreground,
  minimize, maximize, close), own band (34px default, bg = theme bg 8% toward
  fg), icon left via shape masks, centered/ellipsized title as a text mask,
  14px circle buttons with hover symbols, tab row below only with 2+ tabs,
  renderers take `Header { top, rows }`; applies on reload; macOS and native
  unchanged. RED: compile failures in config/render/nxgterm tests + one
  failing reload assertion; GREEN after. Writer checks: 943/0, clippy clean,
  fmt clean, release build ok; parent spot check identical. Not yet seen
  on screen. Gaps: OSC titles (-> T5), circle hit area is its square.

## Next step

T5.
