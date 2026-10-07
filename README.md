# nxgterm

Fast, configurable, cross-platform terminal emulator written in Rust.

## Platforms

Linux (Arch, Debian, Ubuntu, Fedora), macOS, and Windows 10 / Windows Server 2016 or newer.

## Architecture

Hexagonal: a platform-agnostic core defines ports, and adapters implement them per platform.

| Crate | Role |
|---|---|
| `nxg-core` | Domain types, ports (`Pty`), runtime backend fallback |
| `nxg-pty` | PTY adapters: Unix pty, ConPTY, winpty (Server 2016) |
| `nxg-render` | Renderers: wgpu (GPU) with CPU fallback |
| `nxg-config` | Configuration, themes, fonts, key bindings |
| `nxgterm` | Application binary wiring everything together |

Backends are chosen at runtime with `nxg_core::fallback::first_available`, so the
terminal always starts: ConPTY falls back to winpty, GPU falls back to CPU.

## Roadmap

1. Workspace skeleton and CI ✅
2. Window + PTY + text (CPU renderer)
3. GPU renderer with automatic fallback
4. Configuration, themes, fonts
5. Images: Kitty graphics protocol and Sixel
6. winpty fallback for Windows Server 2016
7. Packages (AUR, deb, rpm, winget, Homebrew) and the tools profile
   (Yazi, zoxide, ngmux, Bruno CLI, curl)

## Development

```sh
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all
```
