# Design: nxgterm v0.1.0

> Retroactive record of the architecture and decisions behind v0.1.0.

## Architecture

```
                    +-------------------------------------------+
                    |                 nxgterm (bin)             |
                    |  main/cli -> App (winit ApplicationHandler)|
                    |  choice, bindings, keys, appearance,       |
                    |  reload (diff), watch (notify)             |
                    +----+-----------+-------------+------------+
                         |           |             |
              +----------v--+  +-----v------+  +---v---------+
              | nxg-config  |  | nxg-pty    |  | nxg-render  |
              | TOML, themes|  | native     |  | GpuRenderer |
              | paths (pure)|  | (portable- |  |  (wgpu 26)  |
              +-------------+  |  pty:      |  | CpuWindow-  |
                               |  pty/ConPTY|  |  Renderer   |
                               | winpty     |  |  (softbuffer|
                               |  (embedded)|  |  + fontdue) |
                               +-----+------+  +------+------+
                                     | implements      | implements
                               +-----v-----------------v------+
                               |           nxg-core           |
                               | ports: PtySession{reader,    |
                               |   PtyControl, ChildProcess}, |
                               |   Renderer, RenderError      |
                               | fallback::first_available    |
                               | Terminal = ApcFilter + vte + |
                               |   Grid + Cursor + responses  |
                               | kitty, sixel, ImageStore     |
                               +------------------------------+
```

- `nxg-core` depends only on `vte`, `png` and `flate2`. It knows no
  window, GPU or PTY API. (Exception: `kitty/file.rs` uses `std::fs` for
  `t=f`/`t=t` transmission; see RECOMMENDATIONS.)
- Adapters depend on `nxg-core`; the binary is the only composition root.
- `nxg-config` is pure: path resolution takes an env lookup closure, so it
  is tested for every platform on every host.

## Startup flow

```
main()
 |- attach_parent_console()                  (Windows: console for CLI output)
 |- cli::parse(args) -> Help | Version | PrintConfig | Run{config}
 '- run(config)
     |- path = --config | NXGTERM_CONFIG | platform default
     |- config = Config::load(path)          (missing -> defaults,
     |                                        invalid -> stderr + defaults)
     |- watcher = watch(parent dir of path)  (failure -> live reload disabled)
     '- EventLoop::run_app(App)
         '- resumed() -> App::start()
             |- create window; faces = FontFaces::system(family)
             |- style = font(size x scale) + palette(theme+overrides) + padding
             |- request_inner_size(columns x rows + padding)
             |- select_renderer: first_available(order from
             |     NXGTERM_RENDERER | [renderer].backend)
             |     gpu: enumerate adapters, reject software, catch panics
             |     cpu: softbuffer surface
             |     -> stderr "skipped gpu: ..." / "renderer cpu"
             |- grid = whole cells that fit (window - padding)
             |- spawn_shell_with(WinSize{cells, cell px}, [shell], backends
             |     from NXGTERM_PTY): first_available(native, winpty)
             |     native: ConPTY probe (Windows) -> portable-pty
             |     winpty: unpack embedded DLL+agent -> winpty API
             |     -> stderr "skipped native: ..." / "pty winpty"
             |- spawn_reader thread: read 64 KiB -> UserEvent::Output
             |- spawn_waiter thread: child.wait() -> UserEvent::Exited
             '- Terminal::new(grid); set_cell_pixels

Event loop
  Output(bytes)  -> terminal.advance -> take_responses -> pty.write_all
                    -> request_redraw
  KeyboardInput  -> bindings::resolve (zoom) | keys::encode -> pty.write_all
  Resized/Scale  -> renderer.resize / restyle -> sync_grid_size
                    (terminal.resize + pty.resize with pixels)
  RedrawRequested-> renderer.draw: Ok | Transient (<=3 retries)
                    | Fatal (non-cpu) -> drop, CpuWindowRenderer, redraw
  ConfigChanged  -> Config::load -> reload::diff -> restyle | "on restart"
  Exited / CloseRequested -> exit
```

## Decisions

### Rust over Zig, Go or C++
Rust gives memory safety for a program that parses untrusted byte streams
(VT, kitty, sixel, PNG, zlib), first-class Windows/macOS/Linux support, and
mature crates for every layer (winit, wgpu, softbuffer, vte, portable-pty).
Zig lacks that ecosystem and stable tooling; Go's GC and cgo make GPU and
Win32 work awkward; C++ would need the same safety discipline by hand.

### Own vte-based core instead of `alacritty_terminal`
`alacritty_terminal` brings a full, opinionated terminal model with no
hooks for image protocols (no APC, no DCS sixel pass-through into its grid).
Owning a small core on `vte` lets images be first-class grid citizens
(placements scroll and clear with text) and keeps the domain testable.
Trade-off: VT fidelity must be built incrementally (see RECOMMENDATIONS).

### APC pre-filter in front of vte
vte 0.15 swallows APC strings without a callback, but the kitty graphics
protocol lives in them. `ApcFilter` splits the byte stream into text runs
and complete APC payloads in order, across reads, bounded at 4 MiB. Before
handling an APC the terminal feeds `ESC \` to vte so its state matches what
vte would have done.

### Split PTY port: reader / control / child
The reader blocks and must live on a background thread; writes and resizes
belong to the UI thread; ConPTY never reports EOF on the reader, so child
exit needs its own blocking `wait()` thread. One trait object could not be
split across threads cleanly, so `PtySession` holds three.

### `fallback::first_available` for every runtime choice
One generic helper (lazy constructors, ordered, records skipped attempts)
serves ConPTY -> winpty and GPU -> CPU. Constructors after the winner never
run, and the skipped list feeds uniform stderr diagnostics. Backend
constructors also catch panics so a driver or FFI panic becomes a skip.

### Software GPU adapters rejected
WARP, llvmpipe and SwiftShader emulate a GPU on the CPU at a higher cost
than the simple softbuffer renderer, so the window renderer refuses
`DeviceType::Cpu`. Offscreen tests still allow them to validate shaders.

### wgpu pinned to 26
wgpu 27+ requires Rust 1.88; 26 declares 1.84, which keeps the MSRV at 1.85.
The `webgpu` (wasm) backend is left out.

### fontdue pinned to `=0.9.3`
0.9.4 uses `cast_signed` (stable in Rust 1.87) without declaring a
`rust-version`, which breaks the MSRV silently; an exact pin avoids it.

### TOML over Lua for configuration
Configuration is declarative (font, colors, padding, shell, renderer).
TOML with `deny_unknown_fields` gives precise errors with line numbers,
safe live reload and no embedded interpreter. Scripting can be revisited
if key bindings or hooks ever need it.

### winpty 0.4.3 embedded, binaries from ngmux, verified by SHA-256
Windows Server 2016 has no ConPTY. winpty 0.4.3 x64 (MIT) only imports
system DLLs, so it is embedded with `include_bytes!` and unpacked to a
content-hashed directory under `%LOCALAPPDATA%`. The binaries are the same
ones vendored by ngmux; their SHA-256 matches both that copy and the
upstream `winpty-0.4.3-msvc2015.zip` (recorded in
`crates/nxg-pty/winpty/README.md`).

### Minimum Windows is Server 2016 (10.0.14393)
That is the oldest server release users need; winget's `MinimumOSVersion`
is set accordingly and the winpty fallback covers it.

### ConPTY needs a reply to `ESC[6n`
portable-pty creates the pseudoconsole with `PSEUDOCONSOLE_INHERIT_CURSOR`,
so ConPTY sends DSR 6 at startup and blocks until the terminal answers. The
core therefore answers DSR 5/6 and DA1 from phase 2 (`82f20c2`), and the app
writes queued responses back after every `advance`.

### Tools shipped as a separate profile, not embedded
Embedding Yazi, zoxide, ngmux or Node would bloat the binary, tie releases
to theirs, and bypass package managers. The profile scripts use each
platform's package manager, fall back to checksum-verified upstream releases
and stay optional and idempotent.

### Windows GUI subsystem with console attach
Release builds use `windows_subsystem = "windows"` so launching from
Explorer opens no console; `AttachConsole(ATTACH_PARENT_PROCESS)` restores
output for `--version`, `--help` and `--print-config` from a shell.

### GUI validated on native Windows, not WSLg
Running the window under WSLg crashed the compositor during development, so
interactive GUI validation was done on native Windows; Linux behavior is
covered by unit, integration and offscreen GPU tests.

## Testing strategy

- Pure functions for every decision point (backend order, renderer order,
  shell resolution, adapter ranking, format choice, config diff, key
  encoding, bindings, command-line quoting, env blocks, unpack logic).
- PTY integration tests spawn `/bin/sh` on Unix and run both backends on
  Windows CI (`tests/windows.rs`).
- GPU painter tests render offscreen and compare against the CPU renderer
  (channel difference <= 2); end-to-end image tests feed real escape
  sequences through `Terminal::advance` into the CPU renderer.
