# Verify Report: nxgterm v0.1.0

- Date: 2026-10-07
- Commit verified: `353e771` (`main`, clean working tree)
- Verdict: **PASS with open manual-validation items** (automated checks all
  green; several behaviors are verified only by tests, not in a real window
  or on the target OS; see "Not yet verified")

## Automated checks (run locally on 2026-10-07, Linux x86_64 / WSL2)

| Check | Command | Result |
|---|---|---|
| Format | `cargo fmt --all --check` | pass |
| Lint | `RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets` | pass, 0 warnings |
| MSRV | `cargo +1.85 check --workspace --all-targets` | pass |
| Tests | `cargo test --workspace` | **310 passed, 0 failed, 0 ignored** |

Test counts by binary:

| Crate / target | Passed |
|---|---|
| `nxg-config` (unit) | 31 |
| `nxg-core` (unit) | 133 |
| `nxg-pty` (unit) | 33 |
| `nxg-pty` `tests/spawn.rs` | 7 |
| `nxg-pty` `tests/windows.rs` | 0 (Windows-only; runs in Windows CI) |
| `nxg-render` (unit, incl. offscreen GPU painter tests) | 66 |
| `nxg-render` `tests/inline_images.rs` | 2 |
| `nxgterm` (unit) | 38 |
| Doc tests | 0 |
| **Total** | **310** |

Notes:
- The offscreen GPU tests ran (an adapter was available; they did not take
  the "skipping GPU test" path). They allow software adapters by design.
- Tests that need a system monospace font or `python3` skip with a message
  when absent; both were present here.

## Continuous integration and release

- CI (`.github/workflows/ci.yml`) is green on `main` for every phase commit,
  latest run `37617191671` for `353e771`: fmt, clippy and tests on
  ubuntu/windows/macos, MSRV 1.85, and the tools-profile job (shellcheck,
  PSScriptAnalyzer, dry runs on sh, pwsh 7 and Windows PowerShell 5.1).
  On Windows CI, `tests/windows.rs` exercises both the native (ConPTY) and
  the winpty backends.
- Release workflow run **`37617197352`** (`workflow_dispatch` on `main`) is
  green: Version and source, Linux x86_64, Linux aarch64, Windows x86_64,
  macOS universal, Checksums. Artifacts: `dist-source`,
  `dist-linux-x86_64`, `dist-linux-aarch64`, `dist-windows-x86_64`,
  `dist-macos-universal`, and the merged `release-assets` containing
  deb and rpm (x86_64 + arm64/aarch64), Linux tarballs, msi, Windows zip,
  universal dmg and macOS tarball, source tarball and `SHA256SUMS`.
- Release run warnings (non-blocking): actions on Node.js 20 are deprecated
  (`checkout@v4`, `upload-artifact@v4`, `download-artifact@v4`);
  `ubuntu-latest` migrates to Ubuntu 26 on 2026-10-19; cargo-deb fell back
  to cargo-binstall on aarch64.

## Verified by the user on native Windows

- Phase 2: a live shell renders and accepts input in the window.
- The ConPTY startup hang is fixed by answering `ESC[6n` (`82f20c2`).
- PowerShell 7 is the default shell when installed (`6c87523`).

## Spec compliance (by domain)

| Domain | Evidence | Status |
|---|---|---|
| terminal-core | unit tests in `terminal.rs`, `grid.rs`, `apc.rs`, `fallback.rs`, `size.rs`, `ports.rs` | covered by tests |
| pty | `backend.rs`, `shell.rs`, `winpty/*` unit tests; `tests/spawn.rs`; `tests/windows.rs` in CI | covered; Server 2016 not run |
| rendering | adapter/format/instance/atlas tests, offscreen GPU vs CPU comparison, `choice.rs` | covered by tests; not exercised in a real window |
| configuration | `nxg-config` tests, `cli.rs`, `reload.rs`, `watch.rs`, `bindings.rs`, `appearance.rs` | covered by tests; live reload not exercised in a real window |
| inline-images | kitty, sixel, decode, store unit tests; end-to-end CPU pixel tests | covered by tests; not checked with Yazi/chafa in a window |
| packaging | release run `37617197352` artifacts; Windows `--version` smoke test; `codesign --verify` | built; installs not exercised on real machines |
| tools-profile | shellcheck, PSScriptAnalyzer, dry runs in CI | dry runs only |

## Not yet verified

- Phases 3-5 in a real window: GPU renderer selection and runtime fallback,
  live config reload, font zoom, display scaling, and inline images with
  Yazi, `kitten icat`, `chafa`, `timg` and `img2sixel`.
- A real Windows Server 2016 machine (winpty fallback end to end, MSI
  install, GUI subsystem behavior).
- The dmg on a real Mac (Gatekeeper workaround, both architectures).
- Installing the deb/rpm/msi on clean systems.
- Non-dry-run executions of the tools profile.
- Publishing to AUR, COPR, winget and Homebrew (manifests still carry
  placeholder checksums until `update-manifests.sh` runs on a tagged release).

GUI checks under WSLg were not used: the compositor crashed there during
development, so interactive validation is reserved for native Windows.
