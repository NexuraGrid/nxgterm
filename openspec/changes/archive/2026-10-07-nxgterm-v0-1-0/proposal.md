# Proposal: nxgterm v0.1.0

> Retroactive record. Written on 2026-10-07 after all seven roadmap phases
> were implemented (commits `ef7ff76`..`353e771`), to document intent, scope
> and approach as they were actually delivered.

## Intent

Build a fast, configurable, cross-platform terminal emulator that always
starts, on every supported system:

- Linux (Arch, Debian, Ubuntu, Fedora; X11 and Wayland), macOS 11+, and
  Windows 10 / Windows Server 2016 or newer, including machines without
  ConPTY or without a usable GPU.
- Good enough for a daily workflow built around Yazi, zoxide, ngmux and
  Bruno CLI, including inline image previews.
- Distributed through the native channel of each platform.

## Scope (by phase)

| Phase | Delivers | Commit(s) |
|---|---|---|
| 1. Workspace skeleton and CI | Cargo workspace (Rust 2024, MSRV 1.85), `nxg-core` ports (`PtySession`, `Renderer`), `fallback::first_available`, CI on Linux/Windows/macOS + MSRV | `ef7ff76` |
| 2. Window + PTY + text | vte-based terminal (grid, cursor, SGR, erase), native PTY, CPU renderer (softbuffer + fontdue), key encoding; DSR/DA1 replies for ConPTY; PowerShell 7 default on Windows | `0056df1`, `82f20c2`, `6c87523`, `0c860f4` |
| 3. GPU renderer with fallback | wgpu renderer (glyph atlas, instanced quads), software adapters rejected, startup and runtime fallback to CPU, `NXGTERM_RENDERER` | `f763706` |
| 4. Configuration | TOML config, 8 themes and overrides, fonts, padding, live reload, CLI flags, font zoom, display scale | `49ec86f` |
| 5. Images | Kitty graphics protocol subset and sixel, APC pre-filter, bounded image store, pixel size reporting, drawing in both renderers | `e59953e` |
| 6. Windows Server 2016 | Embedded winpty 0.4.3 fallback when ConPTY is missing, `NXGTERM_PTY` | `e5d8369` |
| 7. Packages and tools profile | Release workflow (deb, rpm, tar.gz, msi, zip, universal dmg, source, SHA256SUMS), AUR/COPR/winget/Homebrew manifests, Windows GUI subsystem, tools profile installers | `353e771` |

Resulting domains: `terminal-core`, `pty`, `rendering`, `configuration`,
`inline-images`, `packaging`, `tools-profile` (see `openspec/specs/`).

## Out of scope (v0.1.0)

- Alternate screen buffer, scroll regions (DECSTBM), insert/delete
  line/char, scrollback, mouse reporting, bracketed paste.
- Selection, copy and paste; OSC sequences (titles, OSC 7/8/52).
- Wide characters and emoji (every char is one cell), font fallback for
  missing glyphs, italic and underline rendering, cursor styles.
- Function keys F1-F12 and other extended key encodings.
- Kitty unicode placeholders (images inside multiplexers) and animation.
- Tabs, splits and multiple windows (ngmux covers multiplexing).
- Code signing (Authenticode), Apple notarization without secrets, and
  publishing to AUR, COPR, winget and Homebrew (manifests only).
- A terminfo entry (`TERM=xterm-256color` is used).

## Approach

- Rust workspace, hexagonal: `nxg-core` holds the domain and ports and has
  no window, GPU or OS dependencies; adapters (`nxg-pty`, `nxg-render`) and
  pure config (`nxg-config`) are wired in the `nxgterm` binary.
- Every runtime-selected backend goes through one helper,
  `fallback::first_available`, which tries candidates lazily in order and
  reports skipped ones, so the terminal always starts and says why.
- Strict TDD: pure logic is extracted from OS/GPU code and unit tested on
  every platform; GPU and font tests skip cleanly when unavailable.
- Dependencies pinned to versions that build on Rust 1.85.

## Rollback plan

Each phase is one or a few self-contained commits on `main`; reverting a
phase commit restores the previous working terminal. Runtime fallbacks
(`NXGTERM_RENDERER=cpu`, `NXGTERM_PTY=native|winpty`) let users bypass a
faulty backend without a new release. Packaging manifests are not published
automatically, so nothing reaches users until a maintainer publishes it.
