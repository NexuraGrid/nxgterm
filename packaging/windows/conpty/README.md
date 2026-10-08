# Bundled ConPTY (Windows)

The Windows zip and MSI ship Microsoft's standalone ConPTY next to
`nxgterm.exe`:

```
nxgterm.exe
conpty.dll
x64\OpenConsole.exe
LICENSE-conpty
```

The ConPTY built into Windows (kernel32 and the system conhost) strips Kitty
graphics (APC) and Sixel (DCS) output and answers the DA1 query itself, so
programs such as Yazi never see nxgterm's image support. `portable-pty` loads
`conpty.dll` by bare name, so the copy in the application directory wins over
kernel32 without any code change; that `conpty.dll` then starts
`x64\OpenConsole.exe`, which passes graphics through. Without
`x64\OpenConsole.exe` it silently falls back to the system conhost and images
are dropped again. On Windows Server 2016 (no ConPTY at all) nxgterm still
uses winpty.

Source: NuGet package
[Microsoft.Windows.Console.ConPTY](https://www.nuget.org/packages/Microsoft.Windows.Console.ConPTY)
**1.25.260930003** (unmodified, MIT, © Microsoft Corporation, built from
[microsoft/terminal](https://github.com/microsoft/terminal)):

<https://api.nuget.org/v3-flatcontainer/microsoft.windows.console.conpty/1.25.260930003/microsoft.windows.console.conpty.1.25.260930003.nupkg>

The files are not vendored. The release workflow runs
[`fetch-conpty.ps1`](fetch-conpty.ps1), which downloads the package, checks
the hashes below and fails on any mismatch. `LICENSE` here is the MIT license
of microsoft/terminal; it is shipped as `LICENSE-conpty`.

SHA-256:

```
02b07b349af66d801159bdf9e440d4a1ce78bb951f37fc8609731665afdae7ee  microsoft.windows.console.conpty.1.25.260930003.nupkg
feeef341d891643c62d30b6b07800bc70f0bc148f44cb8c3bee84aa557ae805a  conpty.dll           (runtimes/win-x64/native/)
3d66b23d0a71bb8eed2b77edc8b9df9bf54ce6c8fb74c863a30e760997f80586  OpenConsole.exe      (build/native/runtimes/x64/)
```

Local MSI build (from the repository root, after `cargo build --release`):

```powershell
.\packaging\windows\conpty\fetch-conpty.ps1 -Destination target\conpty
cargo wix --package nxgterm --no-build --nocapture
```

To update: change `$Version` and the three hashes in `fetch-conpty.ps1` and
here, then run the release workflow by hand (`workflow_dispatch`).
