# Feature: trust baseline (stability and resource use)

Branch: `feat/trust-baseline`

## Objective

Make nxgterm feel as solid as WezTerm or Ghostty: no flicker in TUIs, no
silent crashes, no lost content on resize, complete keyboard input, and low
RAM/CPU use, especially on Windows.

## Problem

A read-only audit (2026-10-20) found that the event loop is fine
(`ControlFlow::Wait`, no blink timer), but the IO pipeline, the per-frame
rebuild, font duplication, and several missing terminal features cause the
fragile feel and the resource use.

## Why

A terminal is a daily tool; any flicker, freeze or vanishing window erodes
trust. Users compare it directly with WezTerm and Ghostty.

## Audit findings

### Resource use

- R1 Each PTY read (up to 64 KiB) becomes its own `UserEvent::Output(Vec)`
  in an unbounded queue, parsed on the UI thread, no coalescing or
  backpressure (`crates/nxgterm/src/app.rs:2194-2211`).
- R2 wgpu enumerates `Backends::all()`, loading DX12 + Vulkan + GL drivers on
  Windows (`crates/nxg-render/src/gpu/device.rs:28`).
- R3 Fonts duplicated: `Font` cloned with parsed outlines; fallbacks (Segoe UI
  Emoji/Symbol) parsed eagerly; CPU glyph cache never trimmed
  (`crates/nxg-render/src/font.rs`).
- R4 Full instance rebuild every frame, no damage tracking; icon drawn as one
  quad per pixel; palette resolved 3x per frame
  (`crates/nxg-render/src/gpu/instance.rs`, `crates/nxgterm/src/app.rs:1716`).
- R5 Scroll moves the whole region (`copy_within`); scrollback stores full-width
  rows including trailing blanks (Cell = 20 B, ~40 MB per pane at 200 cols)
  (`crates/nxg-core/src/grid.rs`, `terminal/scrollback.rs`).
- R6 CPU renderer repaints the whole window (`crates/nxg-render/src/cpu_window.rs`).
- R7 Image budget 256 MiB per pane, not shared (`crates/nxg-core/src/image/store.rs`).
- R8 1024² atlas cleared when full; each title change adds a mask.

### Robustness

- S1 No synchronized output (mode 2026) — TUIs paint half-updated frames.
- S2 Resize truncates rows instead of pushing them into scrollback; no reflow
  (`crates/nxg-core/src/grid.rs:128`, `terminal/screen.rs:46`).
- S3 Key encoding: no F1–F12, no modifier params on arrows/nav keys, Ctrl only
  on letters, AltGr sends ESC prefix on Windows (`crates/nxgterm/src/keys.rs`).
- S4 No IME support.
- S5 PTY writes (keys, paste, replies) on the UI thread can freeze all tabs
  (`crates/nxgterm/src/app.rs:1037`).
- S6 No panic hook; Windows GUI subsystem + `eprintln!` make crashes and config
  errors invisible.
- S7 Missing: DECRQM, DA2/XTVERSION, DECSCUSR, DEC line drawing, tab stops, IRM,
  OSC 7/8/10/11/12/133, focus events 1004, extra SGR, BEL.
- S8 ConPTY created without `PSEUDOCONSOLE_PASSTHROUGH_MODE` (portable-pty 0.9).
- S9 Config errors and "applies on restart" notices only on stderr.
- S10 Child exit closes the pane immediately; no close confirmation.
- S11 No fuzzing or conformance suite for the parser/grid.

Already solid: GPU lost/outdated recovery, CPU fallback, resize throttle,
bounded scrollback, ~1,040 tests.

## Scope (phases)

1. Trust baseline: S1, S6, R1 (this branch).
2. Windows RAM: R2, R3.
3. Keyboard and input: S3, S4, S5.
4. Resize: S2.
5. Per-frame CPU: R4, R6, R8.
6. Missing sequences and fuzzing: S7, S8, S9, S10, S11, R5, R7.

## Constraints

- MSRV 1.87, edition 2024; no new heavy dependencies.
- Keep behavior identical for programs that do not use the new features.
- Tests with each behavior change (RED then GREEN where a runnable test exists).

## Tasks

- [x] T1 Synchronized output (mode 2026): hold redraws while set, force a frame
  on reset or after ~1 s timeout; DECRQM reply for `CSI ? 2026 $p` + tests
- [x] T1b T1 review follow-ups (advisory, non-blocking): hold per pane instead
  of window-wide (R3-shared-hold-across-panes); release the hold when 2026 is
  reset inside the same chunk and set again (R3-hold-not-released-on-intra-chunk-frame-end,
  R4-sync-chunk-boundary-starvation); clear mode 2026 on the 1 s timeout like
  Ghostty; app-level hold test
- [x] T2 Panic hook writing a crash log (Windows `%LOCALAPPDATA%\nxgterm`,
  others the state/cache dir) with a visible message on Windows + tests
- [x] T2b T2 review follow-ups (advisory): elevation relaunch failure shown as a
  crash (main.rs:79) should be a plain error; throttle repeated parser panics in
  one pane (log churn, UI stall); rotation failure must not drop the entry;
  remove unused recovery flag; assert scrollback wiring in tests
- [x] T3 Coalesce PTY output: shared bounded buffer + "wakeup pending" flag,
  one event per drain, reader waits when the buffer is full + tests
- [x] T3b T3 review follow-ups (advisory): drain all pending output before
  handling `Exited` (R1/R3/R4 exit-truncates-tail); make the budget re-post
  actually yield to input (R4-repost-may-not-yield); release retained buffer
  capacity when idle (R3-buffer-retention); avoid front-drain memmove
- [x] T3c Follow-up review leftovers (advisory, minor): stale flood comment
  (app.rs:872), broken TIMEOUT doc link (app.rs:1196), output doc omits the
  stopped state, misleading test name, tests for Exited full drain and
  stopped-pane drain, cap log growth when rotation keeps failing
- [x] T4 Phase 2 — Windows RAM (R2, R3)
- [ ] T4b Fallback coverage index: check each fallback's cmap (ttf-parser)
  before parsing it, so a glyph missing everywhere (e.g. Nerd Font icons) does
  not parse all fallbacks (~100 MiB on Windows); update README GL-fallback
  text; test fallback_families and the backend loop
- [ ] T5 Phase 3 — keyboard and input (S3, S4, S5)
- [ ] T6 Phase 4 — resize without losing content (S2)
- [ ] T7 Phase 5 — per-frame CPU (R4, R6, R8)
- [ ] T8 Phase 6 — sequences, ConPTY passthrough, notices, fuzzing (S7–S11, R5, R7)

## Acceptance criteria

- nvim/Claude Code updates that use 2026 never show half frames.
- A panic leaves a crash log and, on Windows, a visible message.
- `yes` or `cat bigfile` keeps memory bounded and Ctrl+C responsive.

## Checks

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all -- --check`

## Progress

- 2026-10-20: audit done, document created, branch `feat/trust-baseline`.
- 2026-10-20: T1 done (delegated writer; commit a1e7eb1, follow-up moves `mod sync` to main.rs). Core: mode 2026 flag, cleared by
  RIS and screen switches; DECRQM `CSI ? Ps $ p` answers every supported
  private mode (0 for unknown). App: `sync::Hold` holds redraws of visible
  panes while 2026 is set, draws on reset or after 1 s via
  `ControlFlow::WaitUntil`. RED observed (4 core tests, 3 hold tests) then
  GREEN; fmt, clippy `-D warnings`, `cargo test --workspace` pass.
- 2026-10-20: T2 done (delegated writer). New `crash` module: panic hook
  keeps the default stderr output and appends a report (unix time, version,
  OS/arch, thread, message, location, forced backtrace) to
  `%LOCALAPPDATA%\nxgterm\crash.log`, `~/Library/Logs/nxgterm/crash.log` or
  `$XDG_STATE_HOME`/`~/.local/state` `nxgterm/crash.log`; rotated to
  `crash.log.old` past 1 MiB; IO errors ignored. `crash::guard` wraps `main`
  and alone shows the Windows `MessageBoxW`, so panics caught elsewhere
  (pane parser, renderer, pty setup) never pop a dialog. Fatal `run` and
  elevation errors are logged and shown too. Each pane's `advance` runs under
  `catch_unwind`; a panic swaps in a fresh terminal (same size, cell pixels,
  scrollback limit) with a notice naming the log. RED observed (8 tests) then
  GREEN; fmt, clippy `-D warnings` (also on x86_64-pc-windows-gnu),
  `cargo test --workspace` pass.
- 2026-10-20: T3 done (delegated writer). New `inbox` module: per pane a
  `Mutex` + `Condvar` buffer shared by the reader thread (`Producer`) and the
  pane (`Consumer`). Only the first read after a full drain posts a
  payload-free `UserEvent::Output(id)`; the reader waits past 4 MiB pending
  (child blocks on write) and stops when the pane drops its consumer. Each
  event parses at most 1 MiB in one `advance_guarded` call and re-posts
  itself while bytes remain, so keys and redraws interleave; the two
  buffers are swapped and reused, shrunk past 2x the cap. RED observed
  (7 tests, incl. a 50 MiB flood with bounded pending) then GREEN; fmt,
  clippy `-D warnings` (also on x86_64-pc-windows-gnu), `cargo test
  --workspace` pass.
- 2026-10-20: T1b done (delegated writer). `sync::Holds` keeps one hold
  per pane (keyed by `PaneId`): another pane's output neither ends nor
  delays it, a pane's own reset ends it, `about_to_wait` wakes at the
  earliest deadline; holds are released when a pane exits, goes off screen
  or the tab switches. Core tracks `frame_ended` (set when 2026 is reset by
  `CSI ? 2026 l`, RIS or a screen switch while set), taken after each
  advance, so a reset and set in one batch still draws the ended frame and
  holds the next one from now. On the 1 s timeout the app calls
  `Terminal::end_synchronized` on that pane (like Ghostty). Known limit: a
  redraw caused elsewhere draws a held pane as it is (no frame snapshot).
  RED observed (3 core tests, 7 hold tests: missing API) then GREEN, plus
  an app-level test with real terminals; fmt, clippy `-D warnings` (also
  x86_64-pc-windows-gnu), `cargo test --workspace` (1064) pass.
- 2026-10-20: T2b done (delegated writer). A failed elevated relaunch
  shows `crash::error` ("nxgterm could not start: ..."), nothing logged, no
  "crashed". `crash::Recoveries` per pane: the 3rd parser panic within 10 s
  stops the pane instead of resetting it (fresh terminal with a "kept
  failing and was stopped; close it" notice; output drained unparsed so the
  reader never blocks; input dropped so nothing is typed blind). Stop was
  chosen over close so the failure stays visible and the last pane does not
  silently end the window. Only a pane's first panic logs a backtrace
  (thread-local `with_brief_reports` read by the hook). `append_bounded`
  appends even when the rotation fails. The ignored `advance_guarded` bool
  became a `Guarded` outcome that `Session::output` uses. Tests assert the
  scrollback limit of the fresh terminal. RED observed (compile: missing
  API; runtime: rotation test) then GREEN; fmt, clippy `-D warnings` (also
  x86_64-pc-windows-gnu), `cargo test --workspace` (1071) pass.
- 2026-10-20: T3b done (delegated writer). `Exited` first parses all
  pending inbox bytes (budget `usize::MAX`), so the final output is never
  cut. The budget re-post is gone: winit 0.30's Windows proxy uses
  `PostMessageW`, and Win32 hands posted messages out before input and
  `WM_PAINT`, so a re-post loop could starve keys. Panes left with bytes go
  to a `backlog` parsed one budget per `about_to_wait`, with
  `ControlFlow::Poll` only while it is non-empty, so each loop round
  handles queued input and redraws first. The inbox keeps a read offset for
  partial drains and compacts only past half the buffer; once it runs empty
  buffers above 256 KiB (`IDLE_KEEP`) are freed (`Consumer::trim` after
  parsing). Known limit: bytes still in the kernel pty buffer when the
  waiter's `Exited` arrives before the reader's EOF are not read. RED
  observed (compile: missing `IDLE_KEEP`, `trim`, `start`, `control_flow`,
  `defer`) then GREEN; the unbounded-drain inbox test passed at once (the
  drain already allowed it; the `Exited` wiring needs a window and is
  untested). fmt, clippy `-D warnings` (also x86_64-pc-windows-gnu),
  `cargo test --workspace` (1078) pass.

- 2026-10-20: T3c done (delegated (writer trigger: 2+ non-trivial files)).
  Flood comment now names the backlog; `crate::sync::TIMEOUT` doc link
  fixed; the pane-side half of `Session::output` became `Pane::parse`
  (window-free) returning `Parsed { advanced, more }`, and its docs and
  `Session::output`'s cover the stopped state; `Exited` uses
  `EXIT_BUDGET` (`usize::MAX`). New tests: exit drain parses a 3 MiB
  flood plus its tail in one call, a budget leaves the rest pending, a
  stopped pane drains unparsed; `a_stopped_pane_parses_and_sends_nothing`
  renamed `a_stopped_pane_drops_input`. `append_bounded` drops an entry
  once a failed rotation would let the log pass 2x the limit. RED
  observed (compile: missing `Pane::parse`, `Parsed`, `EXIT_BUDGET`;
  runtime: log grew to 60 bytes with limit 10) then GREEN; fmt, clippy
  `-D warnings` (also x86_64-pc-windows-gnu), `cargo test --workspace`
  (1082) pass.

- 2026-10-20: T4 R2 done (delegated (writer trigger: 2+ non-trivial
  files)). `device::backend_order(os, WGPU_BACKEND)` gives DX12, Vulkan,
  GL on Windows; Metal on macOS; Vulkan, GL elsewhere; a non-empty
  `WGPU_BACKEND` is used as is. `GpuRenderer::new` builds one
  `wgpu::Instance` per step (only that backend in the descriptor and in
  `enumerate_adapters`) and stops at the first that gives an adapter,
  device and surface format; a failed step's instance and surface drop
  before the next loads. All failing still errors, so the CPU fallback
  stays. Integrated-GPU preference and `MemoryHints::MemoryUsage` kept.
  RED observed (compile: missing `backend_order`) then GREEN (6 tests);
  fmt, clippy `-D warnings` (also x86_64-pc-windows-gnu), `cargo test
  --workspace` (1088) pass. Not measured on Windows (no GUI here).

- 2026-10-20: T4 R3 done (delegated (writer trigger: 2+ non-trivial
  files)). `FontFaces` keeps the regular and bold file bytes as
  `Arc<[u8]>` and parses them once per pixel size into an
  `Arc<Primary>` shared, through a `Weak` memo, by every `Font` of that
  size while one lives (fontdue tunes outlines to the parse size, so a
  new size still parses its own and output stays identical). Fallbacks
  became `FallbackFace`s holding the fontdb source (path or shared
  bytes) and a `OnceLock`, parsed on the first glyph that reaches them
  in the existing order; a read or parse failure is remembered and
  skipped. `Font` clones share the faces and start with an empty glyph
  cache, so the renderer and the title bar font share one parse. The
  glyph cache is cleared before it would pass 4 MiB of coverage and
  refills (the CPU renderer redraws from it, the GPU atlas copies once).
  RED observed (compile: missing `FallbackFace`, `FontFaces::from_bytes`,
  `shares_faces`, cache budget) then GREEN (5 new tests); fmt, clippy
  `-D warnings` (also x86_64-pc-windows-gnu), `cargo test --workspace`
  (1093) pass. Measured on Linux (counting allocator, system font at
  28 px, style + clone + title font): 61.8 MiB to 16.3 MiB of heap.
  Inferred on Windows: Segoe UI Symbol and Emoji parse to ~38 and
  ~58 MiB (measured from the files), now deferred until needed.

## Delivery

Strategy: `ask-on-risk`. Forecast for phase 1: ~400–600 authored lines.

## Review log

- T1 (a1e7eb1..efb1f0c): high risk, consent granted, 4 lenses, approved and
  acknowledged (lineage review-9e61d80f714c3330). Reviewed boundary: efb1f0c.
- T2 (ee43647): high risk, consent granted, 4 lenses, approved and
  acknowledged (lineage review-1d380fb9f63e5491). Reviewed boundary: ee43647.
- T3 (564f8ed): high risk, consent granted, 4 lenses, approved and
  acknowledged. Reviewed boundary: 564f8ed.
- T1b/T2b/T3b (130f746..895907f): high risk, consent granted, 4 lenses,
  approved and acknowledged. Reviewed boundary: 895907f.
- T3c/T4 (b5163ad..14dc963): medium risk, consent granted, 1 lens, approved
  and acknowledged. Font heap measured 61.8 → 16.3 MiB (Linux, 28 px).
  Reviewed boundary: 14dc963.

## Next step

Phase 3 (T5).
