# Packaging Specification

## Purpose

How nxgterm is built into release artifacts and distribution manifests:
the GitHub release workflow, the Linux, Windows and macOS packages, the
manifests for AUR, Fedora/COPR, winget and Homebrew, and the Windows
executable's subsystem behavior.

Sources: `.github/workflows/{ci,release}.yml`, `packaging/**`,
`crates/nxgterm/{Cargo.toml,build.rs,wix/main.wxs}`, `crates/nxgterm/src/main.rs`,
`assets/**`.

## Requirements

### Requirement: Release triggers and version check

The release workflow MUST run on `v*` tags and on manual `workflow_dispatch`.
It MUST read the version from `[workspace.package]` in the root
`Cargo.toml` and, for tags, fail when the tag is not `v<version>`. A tag run
MUST publish a GitHub Release with generated notes; a manual run MUST only
upload the same files as the `release-assets` workflow artifact.

#### Scenario: Mismatched tag
- GIVEN Cargo version `0.1.0`
- WHEN tag `v0.1.1` is pushed
- THEN the `meta` job fails with "tag v0.1.1 does not match the Cargo version 0.1.0"

### Requirement: Release artifacts

Every release run MUST produce, with `<v>` the version:

| Artifact | Built on / by |
|---|---|
| `nxgterm-<v>-{x86_64,aarch64}-linux.tar.gz` | ubuntu-22.04 / ubuntu-22.04-arm (glibc 2.35) |
| `nxgterm_<v>-1_{amd64,arm64}.deb` | cargo-deb |
| `nxgterm-<v>-1.{x86_64,aarch64}.rpm` | cargo-generate-rpm |
| `nxgterm-<v>-x86_64-windows.zip` | windows-2022, MSVC |
| `nxgterm-<v>-x86_64.msi` | cargo-wix, WiX 3.14 |
| `nxgterm-<v>-universal-macos.dmg` and `.tar.gz` | macos-14, lipo of x86_64 + arm64 |
| `nxgterm-<v>-source.tar.gz` | `git archive` with prefix `nxgterm-<v>/` |
| `SHA256SUMS` | `sha256sum` over all of the above |

Builds MUST use `--locked`. Each upload MUST fail when no files are found.

#### Scenario: Manual run
- GIVEN a `workflow_dispatch` run on `main`
- WHEN it completes
- THEN `release-assets` contains every artifact above and no GitHub Release is created

### Requirement: Linux packages

The `.deb` and `.rpm` MUST install `/usr/bin/nxgterm`, the desktop file, the
AppStream metainfo (`io.github.nexuragrid.nxgterm`), the scalable SVG and
hicolor PNG icons (16-512), README and licenses, and the tools profile at
`/usr/share/nxgterm/profile/{install.sh (755),install.ps1}`. Because X11,
Wayland, xkbcommon and GPU libraries are loaded at runtime, the build MUST
need no `-dev` packages and the package MUST declare `libxkbcommon` (and on
Debian a Wayland or X11 client library) explicitly, recommending the rest.
The tarball MUST contain the binary, licenses, README, `share/` desktop
assets and `profile/`.

#### Scenario: Debian install
- GIVEN Debian 12 amd64
- WHEN `sudo apt install ./nxgterm_<v>-1_amd64.deb` runs
- THEN `nxgterm` is on PATH and `/usr/share/nxgterm/profile/install.sh` is executable

### Requirement: Windows packages and executable

The release exe MUST be built for the Windows GUI subsystem (release
builds only) so launching it opens no console window, and MUST call
`AttachConsole(ATTACH_PARENT_PROCESS)` at startup so `--version`, `--help`
and `--print-config` print in the launching console. The exe MUST embed
`assets/nxgterm.ico`; the release job sets `NXGTERM_REQUIRE_ICON=1` so a
missing resource compiler fails the build. The MSI MUST install per machine
to Program Files with a Start Menu shortcut, the profile scripts in
`profile\`, and an optional PATH feature (`ADDLOCAL=Binaries` excludes it).
The zip MUST contain the exe, licenses, README and `profile\`. The workflow
MUST smoke-test `nxgterm.exe --version`.

#### Scenario: Version from PowerShell
- GIVEN the release `nxgterm.exe`
- WHEN `nxgterm.exe --version` runs in PowerShell
- THEN `nxgterm 0.1.0` appears in that console

### Requirement: macOS bundle

The workflow MUST build `aarch64-apple-darwin` and `x86_64-apple-darwin`
with `MACOSX_DEPLOYMENT_TARGET=11.0`, combine them with `lipo` into
`nxgterm.app/Contents/MacOS/nxgterm`, fill `Info.plist` with the version
(validated by `plutil -lint`), add the icns, licenses, README and the profile
in `Contents/Resources/profile/`, and run the binary's `--version`. Without
signing secrets the app MUST be ad-hoc signed; with
`MACOS_CERTIFICATE`/`_PASSWORD`/`MACOS_SIGNING_IDENTITY` it MUST be signed
with hardened runtime, and with the notary secrets the dmg MUST be notarized
and stapled. The signature MUST be verified in either case. The dmg MUST
include an `Applications` symlink.

#### Scenario: No secrets
- GIVEN a repository without macOS secrets
- WHEN the release runs
- THEN the app is ad-hoc signed and users need `xattr -dr com.apple.quarantine`

### Requirement: Distribution manifests

`packaging/` MUST hold, unpublished: AUR `nxgterm-bin` (release binary,
x86_64 and aarch64) and `nxgterm` (cargo build from the source tarball);
`fedora/nxgterm.spec` (Rust >= 1.85, network access for crates); winget
manifests (`NexuraGrid.nxgterm`, WiX installer, machine scope,
`MinimumOSVersion 10.0.14393.0`); and the Homebrew cask
(`nexuragrid/tap/nxgterm`, universal dmg). Committed checksums MAY be
placeholders (`000...0`) until a release exists.
`packaging/update-manifests.sh <version> [SHA256SUMS]` MUST set versions,
URLs and checksums from `SHA256SUMS`, regenerate both `.SRCINFO` files, add a
Fedora changelog entry, and fail when a checksum is missing or the version
is malformed.

#### Scenario: Missing asset checksum
- GIVEN a `SHA256SUMS` without the msi line
- WHEN `update-manifests.sh 0.1.0 SHA256SUMS` runs
- THEN it exits with "SHA256SUMS has no checksum for nxgterm-0.1.0-x86_64.msi"

### Requirement: Continuous integration

Every push to `main` and every pull request MUST run, on Linux, Windows and
macOS, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets`
and `cargo test --workspace` with `RUSTFLAGS=-D warnings`, plus
`cargo check --workspace` on Rust 1.85.

#### Scenario: MSRV regression
- GIVEN a dependency bump that needs Rust 1.88
- WHEN CI runs
- THEN the `MSRV (1.85)` job fails
