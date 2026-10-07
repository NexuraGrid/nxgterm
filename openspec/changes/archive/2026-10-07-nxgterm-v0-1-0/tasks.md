# Tasks: nxgterm v0.1.0

> Retroactive checklist. Every task is done; commit hashes refer to `main`.

## Phase 1: Workspace skeleton and CI (`ef7ff76`)

- [x] 1.1 Create the Cargo workspace (resolver 3, Rust 2024, MSRV 1.85, MIT OR Apache-2.0, shared lints)
- [x] 1.2 Add crates `nxg-core`, `nxg-pty`, `nxg-render`, `nxg-config`, `nxgterm`
- [x] 1.3 Define `TermSize` (>= 1x1, default 80x24) with tests
- [x] 1.4 Define the `PtySession` and `Renderer` ports in `nxg-core`
- [x] 1.5 Implement `fallback::first_available` with tests (order, laziness, skipped attempts, all fail)
- [x] 1.6 CI: fmt, clippy, test on Linux/Windows/macOS with `-D warnings`; MSRV 1.85 check

## Phase 2: Window + PTY + text with the CPU renderer (`0056df1`, `82f20c2`, `6c87523`, `0c860f4`)

- [x] 2.1 Grid and cells (char, fg, bg, bold/italic/underline/inverse) with scroll and resize
- [x] 2.2 `Terminal` on vte: print with deferred wrap, CR/LF/BS/TAB, scrolling
- [x] 2.3 CSI cursor movement (CUU/CUD/CUF/CUB/CUP/HVP/CHA/VPA), ED/EL, DECTCEM
- [x] 2.4 SGR: attributes, 16/256/truecolor in semicolon and colon forms
- [x] 2.5 Native PTY adapter via portable-pty; split reader/control/child; TERM=xterm-256color
- [x] 2.6 PTY integration tests (output, TERM and size, input round trip)
- [x] 2.7 CPU renderer: 0RGB `Frame`, palette (xterm 256), backgrounds, block cursor, fontdue glyphs via fontdb monospace discovery
- [x] 2.8 softbuffer window renderer and winit event loop with reader and waiter threads
- [x] 2.9 Key encoding (editing keys, arrows, Ctrl/Alt letters, Shift+Tab)
- [x] 2.10 Answer DSR 5/6 and DA1 and write replies to the PTY so ConPTY starts the shell (`82f20c2`)
- [x] 2.11 Default Windows shell: pwsh, then powershell, then `%ComSpec%`, then cmd.exe (`6c87523`)
- [x] 2.12 Mark phase 2 done in the README roadmap (`0c860f4`)

## Phase 3: GPU renderer with automatic fallback (`f763706`)

- [x] 3.1 `RenderError::{Transient, Fatal}` in the `Renderer` port
- [x] 3.2 Adapter ranking that rejects software adapters (tested)
- [x] 3.3 Surface format choice preferring non-sRGB 8-bit (tested)
- [x] 3.4 Glyph atlas (1024x1024 R8, shelf packer, clear-and-rebuild when full)
- [x] 3.5 Instance building for backgrounds, cursor and glyphs (tested without a GPU)
- [x] 3.6 Painter and WGSL shader; offscreen test against the CPU renderer (diff <= 2)
- [x] 3.7 Device-loss and uncaptured-error tracking turned into `Fatal`
- [x] 3.8 Startup selection via `first_available`, `NXGTERM_RENDERER`, panic catching, stderr diagnostics
- [x] 3.9 Bounded transient retries (3) and runtime fallback to the CPU renderer

## Phase 4: Configuration, themes and fonts (`49ec86f`)

- [x] 4.1 `nxg-config`: schema with defaults, `deny_unknown_fields`, font size clamp, colors `#rgb`/`#rrggbb`
- [x] 4.2 Eight built-in themes with contrast tests and per-color overrides
- [x] 4.3 Config path resolution per OS with `NXGTERM_CONFIG` (pure, tested)
- [x] 4.4 Documented `DEFAULT_CONFIG_TOML` that parses to the defaults; `--print-config`
- [x] 4.5 CLI parsing (`--config`, `--config=`, `--print-config`, `-V`, `-h`)
- [x] 4.6 `Style` and `WindowRenderer::set_style` for live restyle in both renderers; padding
- [x] 4.7 Font family selection with fallback list and missing-family warning; bold face
- [x] 4.8 Display scale for font and padding; `ScaleFactorChanged` handling
- [x] 4.9 Live reload: parent-dir watcher, 200 ms debounce, `reload::diff`, restart-only sections
- [x] 4.10 Font zoom bindings (Ctrl/Cmd + `=`/`+`/`-`/`0`)
- [x] 4.11 `[shell] program/args` override

## Phase 5: Inline images (`e59953e`)

- [x] 5.1 `ApcFilter` byte pre-filter with split-chunk and overflow tests
- [x] 5.2 Bounded decoders: incremental base64, zlib, PNG, raw RGB/RGBA
- [x] 5.3 `ImageStore` with 256 MiB budget, 4096 images/placements, eviction, scroll and clear
- [x] 5.4 Kitty command parser and handler: t/T/p/d/q, chunking, replies, quiet modes, deletion targets
- [x] 5.5 File and temp-file transmission with regular-file and temp-dir checks
- [x] 5.6 Sixel decoder (HLS/RGB registers, VT340 palette, repeats, transparency) and DECSDM
- [x] 5.7 Pixel size reporting: `WinSize` with cell pixels, `TIOCGWINSZ`, XTWINOPS 14/16/18, XTSMGRAPHICS
- [x] 5.8 `TERM_PROGRAM=nxgterm` and `TERM_PROGRAM_VERSION`; DA1 advertises sixel
- [x] 5.9 CPU image blit and GPU image pipeline with identical nearest-neighbor sampling and z-order
- [x] 5.10 End-to-end tests from escape sequences to CPU pixels

## Phase 6: winpty fallback for Windows Server 2016 (`e5d8369`)

- [x] 6.1 `Backend` enum, `NXGTERM_PTY` order (tested), panic-to-error conversion
- [x] 6.2 ConPTY probe (`CreatePseudoConsole` export) as the native backend precondition
- [x] 6.3 Vendor winpty 0.4.3 x64 binaries with provenance, license and SHA-256
- [x] 6.4 Content-hashed, race-safe unpacking to `%LOCALAPPDATA%\nxgterm\winpty\`
- [x] 6.5 Windows command-line quoting and case-insensitive environment block (tested everywhere)
- [x] 6.6 winpty session via the C API (dynamic loading, pipes, resize, wait)
- [x] 6.7 Windows integration tests for both backends and auto selection

## Phase 7: Packages and the tools profile (`353e771`)

- [x] 7.1 Release workflow: version check, source tarball, Linux x86_64/aarch64, Windows, macOS universal, SHA256SUMS, release on tags
- [x] 7.2 cargo-deb and cargo-generate-rpm metadata with runtime-loaded library dependencies
- [x] 7.3 WiX installer (per machine, Start Menu, optional PATH) and portable zip
- [x] 7.4 Windows icon via `build.rs` (`NXGTERM_REQUIRE_ICON`), GUI subsystem and `AttachConsole`
- [x] 7.5 macOS app bundle, `Info.plist`, ad-hoc or Developer ID signing, optional notarization, dmg
- [x] 7.6 Desktop file, AppStream metainfo and icon set
- [x] 7.7 Manifests: AUR (`nxgterm`, `nxgterm-bin`), Fedora spec, winget, Homebrew cask; `update-manifests.sh`
- [x] 7.8 `profile/install.sh` (POSIX sh) and `profile/install.ps1` (PS 5.1/7) with options, checksum-verified releases, consent prompts and shell integration
- [x] 7.9 CI job for the profile: shellcheck, PSScriptAnalyzer, dry runs on every shell
- [x] 7.10 README and `packaging/README.md` (install table, release checklist, channels)
