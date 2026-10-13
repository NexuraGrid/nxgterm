# Feature: drag and drop on Wayland

Branch: `fix/wayland-drop`

## Objective

Dropping files on nxgterm pastes their paths on every platform, including
Linux Wayland (KDE Plasma on CachyOS), as WezTerm does.

## Problem

winit 0.30.13 does not implement drag and drop on Wayland: `DroppedFile` is
documented "Wayland: Unsupported" and only the X11 backend emits it. With no
`wl_data_device` accepting the offer, the compositor shows a "blocked"
cursor. Windows, macOS and X11 already deliver `DroppedFile` (T1 of
`terminal-ux`).

## Scope

- Linux-only module that attaches to winit's Wayland connection (raw
  display/surface handles), creates a `wl_data_device` for the seat on its
  own event queue, accepts `text/uri-list` offers over the nxgterm surface,
  reads the dropped URI list and sends the paths to the app through
  `EventLoopProxy<UserEvent>`, which pastes them with
  `nxg_core::paste::dropped_path` like `WindowEvent::DroppedFile`.
- Pure, tested `text/uri-list` parsing (comments, CRLF, `file://` with
  `localhost` host, percent-decoding, non-file URIs ignored).
- No change for X11, Windows and macOS (winit already handles them).

## Tasks

- [x] W1 Wayland drag and drop + uri-list parsing + tests + docs
- [ ] W2 Release 0.9.1 (version, README, Fedora changelog, tag, manifests) and install locally

## Route

W1: delegated direct (new module + Cargo + app wiring, 2+ non-trivial files).
W2: inline (release flow, mechanical).

## Checks

`cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D
warnings`, `cargo fmt --all --check`; CI on Linux/macOS/Windows; manual
drop test by the user on KDE Wayland.

## Forecast

~350 authored lines. Delivery: single PR, user authorized commit, push,
merge and release 0.9.1 (2026-10-13).

## Progress

- Root cause confirmed in winit 0.30.13 source (`event.rs` docs; no
  `DroppedFile` in `platform_impl/linux/wayland`). wayland-client 0.31,
  wayland-backend 0.3, smithay-client-toolkit 0.19 already in Cargo.lock.

- W1 done (delegated writer + parent fix): `crates/nxgterm/src/wayland_drop.rs`
  (foreign display via raw-window-handle, own queue on a `wayland-drop`
  thread, data device per seat, uri-list accepted with Copy, pipe read on a
  short-lived thread, `UserEvent::DroppedPaths`), shared
  `Session::drop_paths`, `parse_uri_list` tested, direct dep
  `wayland-client` (no new crates). Parent fix: `finish` only after the
  compositor chose Copy/Move (a protocol error would close winit's shared
  connection). RED: 5 parse tests failing on stub; GREEN after. Checks:
  961/0, clippy linux + windows-gnu clean, fmt clean, release build ok,
  starts on Wayland without DnD errors. Manual drop on KDE pending (user).

## Next step

W2.
