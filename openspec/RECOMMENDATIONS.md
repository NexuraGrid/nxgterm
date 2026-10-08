# Recommendations after v0.1.0

Prioritized follow-ups found while documenting v0.1.0 against the code.
Each item is meant to become its own OpenSpec change
(`openspec/changes/<name>/`), with the affected spec in `openspec/specs/`.

- **P0**: blocks daily use or carries security risk. Do next.
- **P1**: needed for a credible 0.2/1.0. Plan soon.
- **P2**: polish, hygiene or decisions. Schedule when convenient.

## Summary

| # | Pri | Recommendation | Spec affected |
|---|---|---|---|
| 1 | P0 | Manual validation checklist on native Windows | (verification) |
| 2 | P0 | Windows Server 2016 VM and macOS test passes | pty, packaging |
| 3 | P0 | ~~Alternate screen, scroll regions and full-screen app essentials~~ Done in `fullscreen-essentials` (2026-10-07) | terminal-core |
| 4 | P0 | Harden kitty file transmission (`t=f`/`t=t`) | inline-images |
| 5 | P1 | ~~Scrollback with mouse wheel~~ Done in 0.2.0 | terminal-core, rendering |
| 6 | P1 | ~~Selection, copy/paste and bracketed paste~~ Done in 0.3.0 | terminal-core, configuration |
| 7 | P1 | Wide characters and emoji (unicode-width) | terminal-core, rendering |
| 8 | P1 | ~~Font fallback for missing glyphs~~ (0.2.0); italic and underline still open | rendering |
| 9 | P1 | ~~Mouse reporting~~ (0.2.0); full key encoding still open | terminal-core |
| 10 | P1 | OSC support: title, OSC 7, OSC 8, OSC 52 | terminal-core |
| 11 | P1 | Kitty unicode placeholders (images inside ngmux/tmux) | inline-images |
| 12 | P1 | Performance: benchmark, damage tracking, frame pacing | rendering |
| 13 | P1 | Code signing: Authenticode and Apple notarization | packaging |
| 14 | P1 | Publish to AUR, COPR, winget and Homebrew (manifests ready for 0.3.0) | packaging |
| 15 | P1 | Live GUI tests (screenshot tests) | rendering |
| 16 | P1 | Move `std::fs` out of `nxg-core` behind a port | terminal-core, inline-images |
| 17 | P1 | Keep CI actions current | packaging |
| 18 | P2 | Cursor styles (DECSCUSR) and focus events | terminal-core, rendering |
| 19 | P2 | Glyph atlas growth | rendering |
| 20 | P2 | terminfo entry | pty, packaging |
| 21 | P2 | ~~wgpu upgrade path once the MSRV can move~~ Done: wgpu 30, MSRV 1.87 | rendering |
| 22 | P2 | Dedupe `miniz_oxide` and drop the unused workspace dependency | (build) |
| 23 | P2 | ~~Tabs~~ native in 0.2.0; splits vs ngmux still open | (product) |
| 24 | P2 | Configuration documentation and small CLI gaps | configuration |
| 25 | P2 | Character sets, tab stops, DECCOLM, DECALN, DECSCNM and double-size lines | terminal-core |
| 26 | P2 | Numeric keypad mode (DECKPAM/DECKPNM) | terminal-core |
| 27 | P2 | Clipping image placements to scroll regions in renderers | rendering, inline-images |
| 28 | P2 | Live reload when config directory is created at runtime | configuration |

---

## P0

### 1. Manual validation checklist on native Windows

**Why.** Phases 3-5 are covered only by unit and offscreen tests; nothing
has been seen in a real window since phase 2 (WSLg crashes the compositor).

**Do.** Run and record (screenshots plus stderr) on native Windows 10/11:
- [ ] Default start: stderr shows `renderer gpu`, `gpu adapter ...`, `pty native`.
- [ ] `NXGTERM_RENDERER=cpu` and `=gpu`; `[renderer] backend = "cpu"`.
- [ ] GPU fallback: disable the GPU driver or use a VM without 3D (expect `skipped gpu`).
- [ ] Resize, minimize/restore, move between monitors with different scale.
- [ ] Config live reload: change theme, background, padding, font family/size; save an invalid file (previous config kept, error on stderr); change `[shell]` (restart notice).
- [ ] Zoom: Ctrl+=, Ctrl+-, Ctrl+0; limits at 6 and 72 pt.
- [ ] Yazi image preview (kitty protocol), `chafa -f kitty` and `chafa -f sixel`, `timg`, `img2sixel`.
- [ ] Images scroll away with text and disappear on `clear`.
- [ ] `nxgterm --version` and `--print-config` print in PowerShell (console attach).
- [ ] MSI install/uninstall with and without the PATH feature; zip runs from any folder.

**Done when** the checklist lives in `packaging/README.md` (or a
`docs/manual-test.md`) and each line has a pass/fail with date and build.

### 2. Windows Server 2016 VM and macOS test passes

**Why.** The winpty fallback and the universal dmg exist only as code and
CI artifacts.

**Do.**
- Server 2016 VM (no ConPTY): install the MSI; expect
  `skipped native: ConPTY is unavailable` then `pty winpty`; check
  resize, input, colors, exit; run `install.ps1 -Yes` without winget (GitHub
  zip fallback for zoxide/Yazi, PATH update).
- macOS (Apple silicon and Intel or Rosetta): open the dmg, apply the
  documented `xattr` workaround, verify Metal adapter, Cmd+=/Cmd+-, config
  under `~/.config/nxgterm/`, Retina scaling, and `install.sh --yes` with brew.

**Done when** both passes are recorded with build hashes, and any bug found
has its own change.

### 3. Alternate screen, scroll regions and full-screen app essentials

**Status: Done** (2026-10-07, change `fullscreen-essentials`)

vim, less and yazi now render and exit cleanly via alternate screen, scroll regions (DECSTBM/DECOM), cell/line editing (ICH/DCH/ECH/IL/DL/REP), save/restore cursor (DECSC/DECRC), full reset (RIS), and DECCKM (application cursor keys). Replay tests for all three apps pass; manual verification on Linux KDE Wayland confirms behavior. See `openspec/specs/terminal-core/spec.md` for complete requirements.

Known limitation: vttest menus 1-2 not run (tool not installed); deviations recorded in `vttest-notes.md`.

### 4. Harden kitty file transmission (`t=f`/`t=t`)

**Why.** Any program whose output reaches the terminal (including `cat` of
an untrusted file or output over SSH) can make nxgterm open any regular
file the user can read, up to 128 MiB per command. The reply reveals
existence, readability and whether the bytes decode, an information oracle,
and large reads are a memory/CPU DoS. On Windows there is no pseudo-file
filter, and an absolute UNC path (`\\host\share\x.png`) passed to
`canonicalize` makes Windows connect to that host over SMB, which can leak
the user's NTLM hash. `t=t` also deletes any matching file under temp dirs.

**Do.**
- Reject UNC (`\\?\UNC\`, `\\server\`), device (`\\.\`) and non-local paths on Windows before any filesystem call.
- Add `[images] allow_file_transmission = true|false` (default to be decided; kitty itself allows it) and consider restricting `t=f` to files under `$HOME`, temp dirs and the CWD.
- Cap total file-transmission bytes per second / per screen to bound DoS.
- Make error replies uniform (`EBADF` without OS detail) to reduce the oracle.
- Write threat-model notes in `inline-images` spec; add tests for each rule.
- Pair with #16 so the policy lives in an adapter, not in the core.

**Done when** a security review signs off and the spec documents the rules.

## P1

### 5. Scrollback with mouse wheel

**Status: Done** (0.2.0, PR #8: scrollback, mouse wheel, `[scrollback] lines`)

**Why.** Lines that scroll off the top are lost.
**Do.** Ring-buffer scrollback in `nxg-core` (configurable `[scrollback] lines`,
default ~10,000), a viewport offset, mouse wheel and Shift+PageUp/PageDown
in the app, snap to bottom on input. Images keep their anchor in history or
are dropped explicitly (document which). Not on the alternate screen.

### 6. Selection, copy/paste and bracketed paste

**Status: Done** (0.3.0, PR #14: cell/word/line selection, clipboard port, bracketed paste with sanitizing)

**Why.** Basic usability; paste is impossible today.
**Do.** Mouse selection (cell, word, line), selection highlight in both
renderers, copy on Ctrl+Shift+C / Cmd+C, paste on Ctrl+Shift+V / Cmd+V via a
clipboard port (adapter on `arboard` or winit), bracketed paste (`?2004`)
with sanitizing of embedded `ESC[201~`. Add key bindings to the config.

### 7. Wide characters and emoji

**Why.** `print` treats every char as width 1 (a TODO in `terminal.rs`); CJK
file names in Yazi and emoji misalign columns.
**Do.** Use `unicode-width` (check its MSRV) for width 0/1/2, add a
wide-spacer cell flag, handle wrap at the last column, erase both halves,
and render double-width glyphs across two cells. Combining marks attach to
the previous cell.

### 8. Font fallback for missing glyphs; italic and underline

**Status: Partial.** Per-glyph font fallback shipped in 0.2.0 (PR #9); italic and underline are still not drawn.

**Why.** Characters missing from the chosen font (Nerd Font icons used by
Yazi, CJK, emoji) render as `.notdef`; italic and underline are parsed
(`Flags::ITALIC`, `Flags::UNDERLINE`) but never drawn.
**Do.** Per-glyph fallback chain through fontdb (configured family, then
system fallbacks with coverage lookup, cached per char); italic face
selection; underline (and later undercurl, strikethrough) drawn by both
renderers with matching tests.

### 9. Mouse reporting and full key encoding

**Status: Partial.** Mouse reporting shipped in 0.2.0 (PR #8); F-keys and xterm modifier encoding for named keys are still missing.

**Why.** No mouse modes (`?1000/1002/1003/1006`); `keys.rs` lacks F1-F12,
modified arrows/Home/End (`CSI 1;5A`), keypad, and Ctrl on non-letters.
**Do.** SGR mouse encoding with modes, Shift to bypass for selection; xterm
modifier encoding for named keys; F-keys; optionally the kitty keyboard
protocol later.

### 10. OSC support: title, OSC 7, OSC 8, OSC 52

**Why.** `osc_dispatch` is not implemented, so all OSC sequences are dropped.
**Do.** OSC 0/2 window title; OSC 7 current directory (for future
new-window-in-cwd); OSC 8 hyperlinks (underline on hover, open on
Ctrl+click); OSC 52 clipboard write (opt-in config; never read by default);
OSC 4/10/11/12 color queries (some TUIs query the background color to pick a theme).

### 11. Kitty unicode placeholders

**Why.** `U=1` placements are stored but not displayed, so images do not work
inside ngmux or tmux, which is the main workflow.
**Do.** Implement virtual placements: render cells containing `U+10EEEE`
with row/column diacritics and the image id in the foreground color.
Validate with Yazi inside ngmux and `kitten icat --unicode-placeholder`.

### 12. Performance: benchmark, damage tracking, frame pacing

**Why.** Every output chunk requests a redraw and both renderers rebuild the
whole frame; the CPU renderer repaints every pixel. Fine for now, unknown
under `cat large.log` or 4K windows.
**Do.**
1. Measure first: vtebench and a throughput test (`cat` of 100 MB), frame
   time logging behind an env var; record baselines in the repo.
2. Damage tracking: dirty rows in `Terminal`, partial redraw in the CPU
   renderer, instance buffer reuse in the GPU renderer.
3. Frame pacing: coalesce output events and draw at most once per vsync;
   drain the PTY on a budget so input stays responsive.

### 13. Code signing

**Why.** SmartScreen warns on the unsigned MSI/exe; macOS needs the
`xattr` workaround.
**Do.** Authenticode-sign the exe and MSI in the release job (Azure Trusted
Signing or an OV/EV certificate in secrets, `signtool` with timestamp);
enroll in the Apple Developer Program and set the six `MACOS_*` secrets the
workflow already supports. Update the README and cask caveats afterwards.

### 14. Publish to AUR, COPR, winget and Homebrew

**Status: Partial.** Manifests carry real 0.3.0 checksums (PR #17); publishing to each channel is still pending.

**Why.** Manifests carry placeholder checksums; README install commands do
not work yet.
**Do.** Tag `v0.1.0`, run `packaging/update-manifests.sh 0.1.0`, review the
diff, then follow `packaging/README.md`: push both AUR packages, create the
COPR project, open the winget-pkgs PR (`wingetcreate`), create
`NexuraGrid/homebrew-tap`. Consider automating winget and the tap in the
release workflow later.

### 15. Live GUI tests

**Why.** The window, event loop and real surfaces are untested.
**Do.** Add a headless-capable smoke test: run nxgterm under Xvfb (Linux CI)
with `NXGTERM_RENDERER=cpu` and a fixed config, drive a shell command, take a
screenshot and compare to a golden image with tolerance; a second job with
Mesa llvmpipe for the GPU path (software adapters are rejected at runtime,
so add a test-only override). Keep goldens small and per platform.

### 16. Move `std::fs` out of `nxg-core`

**Why.** `kitty/file.rs` reads, canonicalizes and deletes files inside the
core, breaking the "no OS APIs in the core" rule and making #4 harder to
test.
**Do.** Add a `FileSource` (or `ImageFiles`) port in `nxg-core::ports` with
`read(path, offset, size)` and `take_temp(path)`; move the current code and
the security policy to an adapter crate (or `nxg-pty`/a new `nxg-fs`);
inject it into `Terminal`. Core tests use an in-memory fake.

### 17. Keep CI actions current

**Why.** Release run `37617197352` warns that `actions/checkout@v4`,
`upload-artifact@v4` and `download-artifact@v4` target deprecated Node.js 20,
and `ubuntu-latest` moves to Ubuntu 26 on 2026-10-19.
**Do.** Bump the actions to their Node 24 majors, pin CI runners explicitly
(the release already pins `ubuntu-22.04`), make the MSRV job run
`cargo check --workspace --all-targets` as documented in
`openspec/config.yaml`, and add Dependabot for GitHub Actions.

## P2

### 18. Cursor styles and focus events

DECSCUSR (`CSI n SP q`: block, underline, bar, blinking), a `[cursor]`
config section, hollow cursor when unfocused, and focus reporting (`?1004`)
used by vim and tmux.

### 19. Glyph atlas growth

The GPU atlas is a fixed 1024x1024 texture that is cleared and rebuilt when
full; large font sizes with many distinct glyphs (CJK, after #7/#8) will
thrash it every frame. Grow to 2048/4096 within device limits or use
multiple atlas pages, with an LRU before clearing.

### 20. terminfo entry

`TERM=xterm-256color` hides capabilities (sixel, truecolor `Tc`/`RGB`,
`Smulx`, bracketed paste). Write an `nxgterm` terminfo source, ship it in
packages (`/usr/share/terminfo/n/nxgterm`), and switch `TERM` only when the
entry is installed (fall back otherwise, important for SSH).

### 21. wgpu upgrade path

**Status: Done** (wgpu 30, MSRV 1.87, `fontdue` unpinned; translucent
windows on DX12 through DirectComposition).

wgpu is pinned to 26 because 27+ needs Rust 1.88. Decide an MSRV policy
(e.g. "stable minus 6 releases"), then bump MSRV, wgpu and `fontdue`
(drop the `=0.9.3` pin) together in one change with the offscreen GPU tests
as the regression net.

### 22. Dedupe `miniz_oxide`

`Cargo.lock` has `miniz_oxide` 0.8.9 (via `png`) and 0.9.1 (via `flate2`),
and the workspace declares `miniz_oxide = "0.8"` that no crate uses. Remove
the unused declaration and either decode zlib with the same `miniz_oxide`
that `png` uses (dropping `flate2`) or align versions when `png` updates.
Add `cargo deny check bans` (duplicates as warnings) to CI.

### 23. Tabs and splits: decide

**Status: Partial.** Native tabs shipped in 0.2.0 (PR #11); splits vs relying on ngmux is still undecided.

nxgterm has one window and one shell; ngmux provides multiplexing. Record
the decision (ADR in a change's `design.md`): either "no tabs/splits,
ngmux is the answer" (then prioritize #11 so images work inside it) or a
scoped plan for native tabs. Multiple windows (Ctrl+Shift+N) may be worth
it either way.

### 24. Configuration documentation and small CLI gaps

Generate a reference page from `DEFAULT_CONFIG_TOML` (all keys, ranges,
live vs restart), document per-OS paths and env vars in one place, add
`NXGTERM_PTY` to `--help` (it is missing from `cli::USAGE`), add
`nxgterm --check-config [path]` to validate without starting, and make key
bindings configurable once #6 and #9 add more of them.

### 25. Character sets, tab stops and the rest of vttest menus 1-2

Follow-up of `fullscreen-essentials`. vttest menus 1-2 also need G0/G1
character sets (`ESC ( 0` line drawing, SO/SI), tab stops (HTS, TBC),
DECCOLM (`?3`, 132 columns), DECALN (`ESC # 8`), DECSCNM (`?5`) and
double-size lines. Install vttest, run menus 1-2 and record the results
next to `vttest-notes.md` in the archived change.

### 26. Numeric keypad mode (DECKPAM/DECKPNM)

`ESC =` and `ESC >` are accepted and ignored. Supporting them needs winit's
`KeyLocation::Numpad` passed into `keys::encode` so keypad keys send SS3
sequences in application mode.

### 27. Clipping image placements to scroll regions

With a partial scroll region, a placement shifted inside the region can
overlap rows outside it until it is dropped. Renderers do not know the
region; expose it (or a clip rectangle per placement) and clip in both
renderers.

### 28. Live reload when the config directory is created later

Found in the 2026-10-07 smoke test: if `~/.config/nxgterm/` does not exist
at startup, watching fails and live reload stays off for the session.
Watch the nearest existing ancestor (or create the directory) and switch
to the real directory once it appears.
