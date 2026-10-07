# Vendored winpty binaries

`winpty.dll` and `winpty-agent.exe` are the unmodified **x64** builds from
[rprichard/winpty](https://github.com/rprichard/winpty) release **0.4.3**,
archive `winpty-0.4.3-msvc2015.zip`, directory `x64/bin/`:

<https://github.com/rprichard/winpty/releases/download/0.4.3/winpty-0.4.3-msvc2015.zip>

They are embedded in x86_64 Windows builds of nxgterm (`include_bytes!` in
`src/winpty/session.rs`) and unpacked to
`%LOCALAPPDATA%\nxgterm\winpty\0.4.3-<hash>\` the first time the winpty
backend is used, which by default only happens on Windows without ConPTY
(Windows Server 2016). They only import system DLLs (`KERNEL32`,
`ADVAPI32`, `USER32`, `SHELL32`), so no Visual C++ runtime is needed.

winpty is MIT-licensed; see `LICENSE` in this directory (copied from the
upstream repository).

SHA-256:

```
35a48ece2ff4acdcbc8299d4920de53eb86b1fb41e64d2fe5ae7898931bcee89  winpty-0.4.3-msvc2015.zip
936f611c2129600d35ab7aad45546a837f4f3a9ca7f673e5d66b48c313b9cd75  winpty.dll
9add1a61155ec47cf6f347faf776b746eebbde1dc9360d81b8a909da34650642  winpty-agent.exe
```

The same files are vendored by
[ngmux](https://github.com/NexuraGrid/ng_mux) (`internal/ptyx/winpty/bin/`);
the hashes above match both that copy and the upstream archive.

To update: replace both files with the `x64` build of a newer release,
update `VERSION` in `src/winpty/session.rs` and the hashes above, and let
the Windows CI job run `tests/windows.rs`.
