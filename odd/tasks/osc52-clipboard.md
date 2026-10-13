# Feature: OSC 52 clipboard and flicker-free multiplexer redraws

Branch: `fix/osc52-clipboard`

## Objective

Copying with the mouse inside ngmux (and tmux, nvim, ssh sessions) puts the
text on the system clipboard in nxgterm, as it does in WezTerm, and dragging a
selection in ngmux does not flicker.

## Problem

ngmux v0.8.0 selects on a plain left drag inside a pane and, on release,
copies by sending OSC 52 (`ESC ] 52 ; c ; <base64> BEL`) to the terminal.
nxgterm's `osc_dispatch` (crates/nxg-core/src/terminal/mod.rs) only handles
OSC 0/2, so the request is dropped and nothing is copied.

While dragging, ngmux repaints the pane on every motion event. nxgterm reads
the PTY in chunks and requests a redraw per chunk; a frame split across two
reads may be presented half drawn (hypothesis, to be confirmed in T2).

## Scope

- OSC 52 write: decode base64, set the clipboard (`c` / empty -> clipboard,
  `p` / `s` -> primary selection where supported) through the existing
  `Clipboard` port. Queries (`?`) are refused: no clipboard reads by
  applications. Payload size is capped.
- Investigate the flicker; if confirmed, coalesce PTY output before redraw.

## Constraints

- No clipboard read via OSC 52 (security).
- Linux (X11/Wayland) and Windows.

## Tasks

- [x] T1 OSC 52 clipboard write in nxg-core + app wiring + tests + docs
- [ ] T2 Flicker during ngmux drag: moved to ngmux (not an nxgterm defect; see Progress)

## Route

T1: delegated direct (nxg-core terminal + nxgterm app, 2+ non-trivial files).
T2: delegated direct (investigation + fix).

## Delivery strategy

ask-on-risk. Forecast: ~250 authored changed lines.

## Checks

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`

## Progress

T1 done (route: delegated direct, one bounded writer).
- nxg-core: new `terminal/osc52.rs` parses OSC 52 writes (reuses
  `image::decode::base64`), queues at most one request per kind, drained by
  `Terminal::take_clipboard_requests`. Decisions: empty or `c` -> clipboard,
  `p`/`s` -> PRIMARY, both named -> both; other targets ignored; `?` refused
  with no reply; empty data ignored (no clearing); cap 1 MiB decoded, encoded
  length checked before decoding; invalid base64/UTF-8 ignored.
- nxgterm: `Session::output` drains requests for every pane (on screen or
  not) and writes them via `set_clipboard`, skipping PRIMARY without
  `HAS_PRIMARY`; errors warn like copy.
- RED: 8 of 12 new nxg-core tests failed before dispatch; GREEN after.
- Checks: `cargo test --workspace` ok (nxg-core 394, nxgterm 251 passed);
  clippy `-D warnings` ok (linux and x86_64-pc-windows-gnu);
  `cargo fmt --all --check` ok.

T2 moved to ngmux (route: delegated direct, read-only investigation; no
nxgterm change kept).
- The user reproduced the flicker in other terminals on other machines, so
  it is not nxgterm-specific. A speculative output pacer (coalescing PTY
  reads before redraw) was written test-first (RED 5/6, GREEN 6/6) and then
  reverted: no nxgterm-specific defect was confirmed.
- Cause (confirmed by reading ngmux at 443c8f5): every mouse event that
  returns `repaint` calls `cl.reset()` (internal/server/server.go:368-371;
  same for keys at :348-351), which sets `hasShown = false`
  (internal/server/client.go:318-321). The next `offerFrame` then paints
  with `prev = nil` (client.go:160-163), and `render.Paint` does a full
  repaint starting with `ESC [2J` (internal/render/render.go:562-568). A
  drag selection returns `repaint = true` on every motion
  (internal/server/mouse.go:253,257), so each motion clears the whole
  screen and redraws every cell. The frame exceeds 4 KiB and the PTY splits
  it (measured: one 6000-byte write read as 4095 + 1902 bytes), so
  terminals present the cleared/partial screen: the flash.
- Proposed ngmux fix (not applied): drop `cl.reset()` after `markDirty` for
  mouse (and key) repaints; the diff painter against `c.shown` already
  draws the change. Keep resets for real divergence (session switch,
  resize, `TypeRefresh` that the client sends when its which-key/help
  overlay closes). Optionally wrap `Paint` output in DEC 2026
  (`ESC[?2026h` ... `ESC[?2026l`).
- Unverified: visual check after the ngmux fix.

Next: report T2 to ngmux.
