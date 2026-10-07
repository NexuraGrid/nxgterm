# PTY Specification

## Purpose

The `nxg-pty` adapters that start the shell in a pseudo-terminal and expose
it through the `PtySession` port: the native backend (Unix pty or Windows
ConPTY through `portable-pty`) and an embedded winpty fallback for Windows
without ConPTY (Windows Server 2016).

Sources: `crates/nxg-pty/src/{lib,backend,shell}.rs`, `src/winpty/*`,
`tests/spawn.rs`, `tests/windows.rs`, `winpty/README.md`.

## Requirements

### Requirement: Backend order and override

Backends MUST be tried with `nxg_core::fallback::first_available`. With
`NXGTERM_PTY` unset, empty, `auto` or any unrecognized value, the order
SHALL be `native` then `winpty` on Windows and `native` alone elsewhere.
`NXGTERM_PTY=native` or `winpty` (case-insensitive, surrounding spaces
ignored) MUST force that single backend on every platform.

#### Scenario: Forced winpty off Windows
- GIVEN Linux and `NXGTERM_PTY=" WinPTY "`
- WHEN the backend order is computed
- THEN it is `[winpty]`
- AND spawning fails with "winpty is only bundled for x86_64 Windows"

#### Scenario: Unknown value means auto
- GIVEN Windows and `NXGTERM_PTY=conpty`
- WHEN the backend order is computed
- THEN it is `[native, winpty]`

### Requirement: ConPTY availability probe

On Windows the native backend MUST first check that `kernel32.dll` exports
`CreatePseudoConsole`. When it does not, the native backend MUST fail with
an error naming ConPTY, `CreatePseudoConsole` and the minimum versions
(Windows 10 1809 / Server 2019), so the next backend is tried.

#### Scenario: Windows Server 2016
- GIVEN kernel32 without `CreatePseudoConsole`
- WHEN the shell is spawned with auto backends
- THEN `native` is skipped with the ConPTY reason
- AND `winpty` starts the shell

### Requirement: Backend failures never abort selection

A panic inside a backend MUST be caught and converted into an error
(`panicked: <message>`), so the next backend still gets its turn. When every
backend fails, the error MUST list each attempt as `name: reason`.

#### Scenario: All backends fail
- GIVEN both backends fail
- WHEN spawning
- THEN the error reads `no pty backend could start the shell; native: ...; winpty: ...`

### Requirement: Default shell

With no `[shell] program` configured, Unix MUST run `$SHELL` (falling back to
`/bin/sh`, via `portable-pty`'s default program). Windows MUST run the first
of `pwsh.exe`, `powershell.exe` found on `PATH` (`pwsh.exe` is also looked up
in `%ProgramFiles%\PowerShell\7\`), else `%ComSpec%`, else `cmd.exe`.
A configured program and its arguments MUST replace the default shell.

#### Scenario: PowerShell 7 preferred
- GIVEN both `pwsh.exe` and `powershell.exe` on `PATH`
- WHEN the Windows shell is resolved
- THEN `pwsh.exe` is used

#### Scenario: Nothing found
- GIVEN no PowerShell and no `ComSpec`
- WHEN the Windows shell is resolved
- THEN `cmd.exe` is used

#### Scenario: Missing configured program
- GIVEN `[shell] program` names a program that does not exist
- WHEN spawning
- THEN spawning returns an error rather than starting the default shell

### Requirement: Child environment

Every backend MUST set `TERM=xterm-256color`, `TERM_PROGRAM=nxgterm` and
`TERM_PROGRAM_VERSION=<crate version>` in the child environment. For winpty,
overrides MUST replace variables case-insensitively and the environment
block MUST be sorted by upper-cased name as `CreateProcess` expects.

#### Scenario: Programs detect the terminal
- GIVEN a spawned child that prints `$TERM_PROGRAM`
- WHEN its output is read
- THEN it contains `nxgterm`

### Requirement: Window size with pixels

Spawning and `PtyControl::resize` MUST pass the cell grid and, when the cell
pixel size is known, the text-area pixel size, so `TIOCGWINSZ` reports
`ws_xpixel`/`ws_ypixel` on Unix. winpty sizes in cells only.

#### Scenario: Pixel size visible to the child
- GIVEN a Unix spawn with 80x24 cells of 9x18 pixels
- WHEN the child calls `ioctl(TIOCGWINSZ)`
- THEN it reports 720x432 pixels

### Requirement: Session lifecycle

The native session MUST drop its copy of the PTY slave after spawning so the
reader sees EOF/EIO when the child exits. Dropping the control MUST kill the
child (failures ignored). `ChildProcess::wait` MUST block until exit, since
ConPTY does not report EOF on the reader.

#### Scenario: Input round trip
- GIVEN a spawned `/bin/sh -c 'read x; echo got:$x'`
- WHEN `ping\r` is written to the control
- THEN the reader yields `got:ping`

### Requirement: Embedded winpty

On x86_64 Windows, winpty 0.4.3 (`winpty.dll`, `winpty-agent.exe`, x64, MIT)
MUST be embedded with `include_bytes!` and unpacked on first use to
`%LOCALAPPDATA%\nxgterm\winpty\0.4.3-<16 hex content hash>\` (falling back to
`%APPDATA%`, then the temp directory). Unpacking MUST write each file to a
unique temporary name and rename it, MUST skip files whose size and content
already match, and MUST treat an identical file left by a concurrent instance
as success. The DLL SHALL be loaded once per process and never unloaded.
Command lines MUST be quoted so `CommandLineToArgvW` restores the original
arguments, and NUL MUST be rejected. Other architectures MUST report that
winpty is not bundled.

#### Scenario: Second launch reuses the binaries
- GIVEN the binaries were unpacked by a previous run
- WHEN winpty is used again
- THEN no file is rewritten

#### Scenario: Provenance
- GIVEN the vendored binaries
- WHEN their SHA-256 is computed
- THEN they match `crates/nxg-pty/winpty/README.md`
  (`winpty.dll` 936f611c..., `winpty-agent.exe` 9add1a61...)

### Requirement: Diagnostics

The application MUST print each skipped backend as
`nxgterm: skipped <name>: <reason>` and the chosen one as `nxgterm: pty <name>`
on stderr.

#### Scenario: Fallback is visible
- GIVEN Windows Server 2016
- WHEN nxgterm starts
- THEN stderr shows `nxgterm: skipped native: ConPTY is unavailable ...` then `nxgterm: pty winpty`
