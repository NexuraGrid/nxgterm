# Feature: wide and zero-width character support

Branch: `fix/cross-platform-symbol-fallbacks`

## Objective

Render emoji and CJK characters across two cells and keep zero-width
characters (combining marks, ZWJ, variation selectors) from taking a cell,
so TUIs (Claude Code, yazi, nvim) stay aligned on every platform.

## Problem

`crates/nxg-core/src/terminal/mod.rs` `print()` treats every char as width 1
(explicit TODO). Applications compute width 2 for wide chars, so text after
them shifts and box borders break. Zero-width chars each occupy a cell.
Fallback glyphs are scaled down to fit one cell, so wide glyphs look tiny.

## Scope

- Core: per-char width via `unicode-width` (already in Cargo.lock
  transitively), wide cell + trailing spacer cell, wrap when a wide char does
  not fit at the last column, overwriting/erasing either half clears both.
- Core: zero-width chars do not advance the cursor and do not take a cell.
- Selection/copy text skips spacer cells.
- Render: a wide cell's glyph is fitted into two cells; spacer draws nothing
  but its background.

Out of scope: color emoji (fontdue limitation), full grapheme-cluster
rendering of combining marks on top of the base glyph.

## Tasks

- [x] T1 Wide characters in core (width, spacer flag, wrap, edits) + selection text + tests
- [x] T2 Zero-width characters do not advance or occupy cells + tests
- [x] T3 Renderer draws wide glyphs across two cells; spacer skipped + tests

## Route

T1–T3: delegated direct (one writer; 2+ non-trivial files across nxg-core
and nxg-render).

## Checks

`cargo test --workspace`, `cargo clippy --workspace --all-targets`,
`cargo fmt --check`. Test-first: RED for width behavior before code.

## Forecast

~300–400 authored changed lines. Delivery strategy: ask-on-risk.

## Progress

- Prior commit on branch: 576f20a fix(render): Windows/macOS symbol fallbacks.
- T1–T3 implemented (uncommitted, delegated writer). Design: `Flags::WIDE`
  and `Flags::WIDE_SPACER` (bits 5/6, `Flags` stays `u8`); `cell::repair_wide`
  blanks orphan halves after ICH/DCH/ECH/EL/ED and grid resize; `print`
  clears the other half on overwrite, wraps a wide char early at the last
  column (autowrap) or writes it over the last two columns (no autowrap);
  1-column grids print wide chars in one cell. Width 0/`None` chars dropped.
  Selection text skips spacers; word selection reads a spacer as its char.
  Renderer: `Font::glyph(ch, bold, wide)` fits fallbacks into two cells,
  atlas keyed `(char, bold, wide)`, cursor covers both cells (CPU + GPU).
- RED observed: 13 core tests (cell, edit, testing invariant, terminal
  print/zero-width/resize) and 5 render tests (font, CPU renderer, paint
  cursor, GPU instance x2) failed before implementation; selection test
  went RED once print produced spacers.
- Verification: `cargo test --workspace` 703 passed / 0 failed;
  `cargo clippy --workspace --all-targets -- -D warnings` clean;
  `cargo fmt --check` clean. Diff: 16 files, +508/-71.
- Limitations: the last-column cell skipped when a wide char wraps early is
  left untouched and copies as part of the joined line (usually a space); combining marks are not rendered.
- Next: commit T1–T3 work unit; RDD assess.
