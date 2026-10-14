# Feature: jumplist-heap-corruption

## Objective

nxgterm 0.10.0 dies on start on Windows (seen on Windows Server 2016): no window, and no process in Task Manager. Fix it, and make sure this class of crash can never ship again.

## Problem / Why

`jump_list::register()` (crates/nxgterm/src/elevation.rs) built the task title `PROPVARIANT` on top of a Rust `Vec<u16>`.
In `windows` 0.62, `PROPVARIANT` implements `Drop` with `PropVariantClear`, which calls `CoTaskMemFree` on the string. Rust then frees the same buffer a second time.
The result is a double free and heap corruption (0xc0000374) before the window exists. It happens on every start, elevated or not.
Nothing caught it: the code ran only on Windows at startup, and no test or CI step started the app there.

## Scope (authorized: fix + prevention + release 0.10.1)

- **T1** Allocate the title with `SHStrDupW` (COM allocator, exactly what `InitPropVariantFromString` does) and let `PROPVARIANT` own and free it. Windows-only regression tests run in CI on windows-latest: one builds and drops the title values, the other runs the full `register()` path.
- **T2** CI startup smoke test on windows-latest: launch the built `nxgterm.exe` and fail if it exits within a few seconds. This catches any startup crash, not only this one.
- **T3** Release 0.10.1 (bump, tag, manifests).

## Acceptance criteria

- On the test-only commit, the Windows tests fail or crash (RED observed in CI). After the fix they pass.
- The CI smoke step keeps nxgterm alive on windows-latest.
- 0.10.1 starts on Windows Server 2016. This is confirmed by the user manually.

## Checks

fmt, clippy (Linux and x86_64-pc-windows-gnu), `cargo test -p nxgterm`, and the GitHub CI matrix (windows-latest runs the new tests and the smoke test).

## Tasks

- [x] T1 SHStrDupW title + Windows regression tests. Route: inline (one file, cause already understood). Commits 35cfffb (RED: windows-latest died with 0xc0000374 STATUS_HEAP_CORRUPTION) and 0845322 (fix).
- [x] T2 CI Windows startup smoke test (packaging/windows/smoke-start.ps1, used by ci.yml and release.yml). Route: inline. Commit 525041b. Hardened after the native review (approved, lineage review-ff2ffdf831b0012a): keeps the exit code, kills the process tree, and the register test must return Ok.
- [ ] T3 Release 0.10.1. Route: inline (same flow as 0.10.0).

## Progress / Evidence

- Root cause confirmed in `windows-0.62.2/src/extensions/Win32/System/StructuredStorage.rs:32` (`impl Drop for PROPVARIANT`).
- Import tables of 0.9.2 and 0.10.0 compared: the only new imports are ShellExecuteW, OpenProcessToken, GetTokenInformation and PropVariantClear, all present on Server 2016.
