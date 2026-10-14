# Feature: windows-elevation

## Objective

Make it easy to open nxgterm as Administrator on Windows, the way Windows Terminal does it.

## Problem / Why

Today the only way is the hidden OS gestures: Ctrl+Shift+click, or right-click → More → Run as administrator.
nxgterm has no elevation support of its own, and no visual cue that the window is elevated.

## Scope (authorized: options 1 + 2 + 4)

- **T1** `--elevate` CLI flag. If the process is not elevated, it relaunches itself through `ShellExecuteW` with the `runas` verb (forwarding `--config`) and exits. If it is already elevated, it just runs.
- **T2** Taskbar jump list task "New window (Administrator)", registered at startup through `ICustomDestinationList`. It runs `nxgterm.exe --elevate`.
- **T4** An "(Administrator)" suffix in the OS window title and the integrated title bar when the process is elevated.

Out of scope: a second MSI shortcut (option 3), a config `elevate` key, and drag & drop across UIPI. A non-elevated Explorer cannot drop into an elevated window; the user has accepted that.

## Constraints

- CI runs fmt, clippy (`-D warnings`), and tests on Linux, Windows, and macOS, plus `cargo check` on MSRV 1.87.
- Development happens on Linux, so the Windows-only code must stay thin. Pure logic (argument forwarding, title formatting) lives in cross-platform functions with unit tests.
- No explicit AppUserModelID. Setting one in the process but not in the MSI shortcut would split taskbar grouping. The default exe-derived ID makes the jump list attach to the pinned icon.
- Every `unsafe` block carries a `// SAFETY:` comment. Workspace lint `unsafe_op_in_unsafe_fn = deny` applies.

## Tasks

- [x] T1 `--elevate` flag + runas relaunch (cli.rs, main.rs, new elevation module, Cargo features, configuration spec). Route: delegated (multi-file writer trigger).
- [x] T2 Jump list task (`windows` crate COM, startup call, packaging spec). Route: delegated (same writer).
- [x] T4 "(Administrator)" title suffix (app.rs, spec). Route: delegated (same writer).

## Acceptance criteria

- `nxgterm --elevate` shows the UAC prompt and opens an elevated window. If UAC is cancelled, it prints an error and exits non-zero.
- Right-clicking the pinned or running nxgterm icon shows "New window (Administrator)".
- An elevated window's title reads "<label> (Administrator)". Non-elevated windows are unchanged.
- On non-Windows platforms, `--elevate` is accepted but reports that it is unsupported. Nothing else changes.

## Checks

- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets`, `cargo test --workspace` (Linux, local).
- `cargo check --target x86_64-pc-windows-gnu -p nxgterm` if that target is available locally. If not, Windows compile proof comes from CI.
- Manual Windows smoke test by the user (UAC, jump list, title).

## Delivery

Forecast is about 350 authored lines. Strategy: ask-on-risk, single PR.

## Progress / Evidence

- Branch `feat/windows-elevation` created from `main` (c3edf33).
- T1 done, route delegated (writer), commit `addf820`. RED: `cargo test -p nxgterm -- elevat cli::` showed 6 failed (cli `elevate_combines_with_config`, 5 elevation helper tests with stubs). GREEN: 13 passed.
- T2 done, route delegated (writer), commit `1ae457d`. Uses the `windows` 0.62 crate (MSRV 1.82, already in Cargo.lock). No runnable RED: Windows-only COM code, compile-proved with the Windows clippy check.
- T4 done, route delegated (writer), commit `26161be`. RED: `elevated_titles_get_a_suffix` failed (`"pwsh"` vs `"pwsh (Administrator)"`). GREEN: passed.
- Checks after T4: `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets -- -D warnings` ok; `cargo test -p nxgterm` 258 passed; `cargo clippy -p nxgterm --target x86_64-pc-windows-gnu --all-targets -- -D warnings` ok.
- MSRV: `cargo +1.87 check -p nxgterm` fails on main's pre-existing let-chain in `crates/nxg-render/src/font.rs:411`, which this feature does not touch. The new code uses nothing newer than 1.82. The 1.87 toolchain has no Windows target installed locally.
- Native review (RDD, high risk: process_boundary): consent granted, 4 lenses, approved with 0 blocking findings, acknowledged (lineage review-61825ec9d2bcfab8). Advisory follow-up: call CoInitializeEx before ShellExecuteW (R3 WARNING, elevation.rs:151). The rest are suggestions.
- Pending: manual Windows smoke test (UAC accept/cancel, jump list task, title suffix).

## Next step

Manual Windows smoke test by the user, then the delivery decision (push/PR).
