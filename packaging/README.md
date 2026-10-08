# Packaging

Everything here is published from a GitHub Release, which
[`.github/workflows/release.yml`](../.github/workflows/release.yml) builds when a
`v*` tag is pushed. Nothing in this directory is published automatically.

## Release assets

| Asset | Built on | Contents |
|---|---|---|
| `nxgterm-<v>-x86_64-linux.tar.gz`, `nxgterm-<v>-aarch64-linux.tar.gz` | Ubuntu 22.04 (glibc 2.35) | binary, licenses, README, desktop file, metainfo, icon, `profile/` |
| `nxgterm_<v>-1_amd64.deb`, `nxgterm_<v>-1_arm64.deb` | cargo-deb | `/usr/bin/nxgterm`, desktop integration, `/usr/share/nxgterm/profile/` |
| `nxgterm-<v>-1.x86_64.rpm`, `nxgterm-<v>-1.aarch64.rpm` | cargo-generate-rpm | same layout as the `.deb` |
| `nxgterm-<v>-x86_64-windows.zip` | Windows (MSVC) | `nxgterm.exe`, bundled ConPTY (`conpty.dll`, `x64\OpenConsole.exe`), licenses, README, `profile\` |
| `nxgterm-<v>-x86_64.msi` | cargo-wix (WiX 3) | the zip's files in Program Files, Start Menu shortcut, optional PATH entry |
| `nxgterm-<v>-universal-macos.dmg`, `.tar.gz` | macOS (lipo of x86_64 + arm64) | `nxgterm.app`; the profile is in `Contents/Resources/profile/` |
| `nxgterm-<v>-source.tar.gz` | `git archive` | source for the AUR and Fedora builds |
| `SHA256SUMS` | | checksums of all of the above |

Running the workflow by hand (`workflow_dispatch`) builds the same files as
the `release-assets` artifact without creating a release.

## Release checklist

1. Bump `version` in the root `Cargo.toml` and add a `<release>` to
   `assets/linux/io.github.nexuragrid.nxgterm.metainfo.xml`.
2. Tag and push: `git tag v<v> && git push origin v<v>`.
3. When the release is published, update the manifests here and review them:

   ```sh
   packaging/update-manifests.sh <v>     # downloads the release's SHA256SUMS
   git diff packaging/
   ```

   The script sets the version, URLs and checksums in every manifest,
   regenerates both `.SRCINFO` files and adds a Fedora changelog entry. Pass a
   local `SHA256SUMS` path as the second argument to work offline.
4. Publish to each channel below.

## Channels

### AUR (Arch Linux)

Two packages: `nxgterm-bin` (release binary) and `nxgterm` (built from the
source tarball with cargo).

Needs: an [AUR account](https://aur.archlinux.org/register) with an SSH key.

```sh
git clone ssh://aur@aur.archlinux.org/nxgterm-bin.git
cp packaging/aur/nxgterm-bin/{PKGBUILD,.SRCINFO} nxgterm-bin/
cd nxgterm-bin && makepkg -si   # test build on Arch
git add PKGBUILD .SRCINFO && git commit -m "Update to <v>" && git push
```

Same for `nxgterm`. On Arch, `makepkg --printsrcinfo > .SRCINFO` and
`namcap PKGBUILD *.pkg.tar.zst` are worth running before pushing.

### Debian and Ubuntu

The `.deb` from the release installs on Debian 12+ and Ubuntu 22.04+
(amd64 and arm64): `sudo apt install ./nxgterm_<v>-1_amd64.deb`.
An apt repository (signed `Release` files, hosting) is out of scope for now.

### Fedora (RPM and COPR)

The `.rpm` from the release installs with `sudo dnf install ./nxgterm-<v>-1.x86_64.rpm`.

For a COPR repository, use [`fedora/nxgterm.spec`](fedora/nxgterm.spec):

Needs: a [Fedora account](https://accounts.fedoraproject.org/) and a COPR project.

1. Create the project at <https://copr.fedorainfracloud.org/> with the
   Fedora releases and architectures (x86_64, aarch64) you want.
2. In the project settings, enable **internet access during builds**: the
   crates are fetched from crates.io.
3. Build: `copr-cli build <project> nxgterm.spec` after `spectool -g`, or add
   a package with the "SCM" source pointing at this repository and the spec
   path `packaging/fedora/nxgterm.spec`.

Users then run `sudo dnf copr enable <user>/<project> && sudo dnf install nxgterm`.

### winget (Windows)

Needs: a GitHub account to open a pull request against
[microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs).

The manifests in [`winget/`](winget/) go to
`manifests/n/NexuraGrid/nxgterm/<v>/` in that repository:

```powershell
winget validate --manifest packaging\winget
winget install --manifest packaging\winget   # local test (needs admin for the MSI)
```

Then open the pull request (or use `wingetcreate submit packaging\winget`).
`wingetcreate update NexuraGrid.nxgterm --version <v> --urls <msi url> --submit`
does the same for later versions and also fills in the MSI ProductCode.

### Homebrew (macOS)

Needs: a public tap repository `NexuraGrid/homebrew-tap`.

Copy [`homebrew/Casks/nxgterm.rb`](homebrew/Casks/nxgterm.rb) to `Casks/nxgterm.rb`
in the tap and push. Users install with:

```sh
brew install --cask nexuragrid/tap/nxgterm
```

`brew style --cask Casks/nxgterm.rb` and `brew audit --cask --new nexuragrid/tap/nxgterm`
check it. The app is not notarized yet (see below), so the cask prints the
Gatekeeper workaround in its caveats.

## macOS signing and notarization (optional)

Without secrets the release workflow ad-hoc signs `nxgterm.app`; Gatekeeper
then blocks it on first launch and users run:

```sh
xattr -dr com.apple.quarantine /Applications/nxgterm.app
```

To sign with a Developer ID and notarize the `.dmg`, add these repository
secrets (an Apple Developer Program membership is required):

| Secret | Value |
|---|---|
| `MACOS_CERTIFICATE` | base64 of the Developer ID Application `.p12` |
| `MACOS_CERTIFICATE_PASSWORD` | password of that `.p12` |
| `MACOS_SIGNING_IDENTITY` | e.g. `Developer ID Application: Name (TEAMID)` |
| `MACOS_NOTARY_APPLE_ID` | Apple ID used for notarization |
| `MACOS_NOTARY_TEAM_ID` | team ID |
| `MACOS_NOTARY_PASSWORD` | app-specific password for that Apple ID |

The signing and notarization steps run only when these are set.

## Windows notes

- The MSI installs per machine and therefore asks for elevation. The PATH
  feature can be turned off in the feature tree, or from the command line:
  `msiexec /i nxgterm-<v>-x86_64.msi ADDLOCAL=Binaries`.
- The executable embeds `assets/nxgterm.ico` through `crates/nxgterm/build.rs`
  when built on Windows; the release job sets `NXGTERM_REQUIRE_ICON=1` so a
  missing resource compiler fails the build.
- The MSI and the exe are not Authenticode-signed, so SmartScreen may warn on
  first run.
- Both the zip and the MSI ship Microsoft's standalone ConPTY, which inline
  images need. The release job fetches it with
  [`windows/conpty/fetch-conpty.ps1`](windows/conpty/README.md) (pinned
  version, SHA-256 verified) into `target\conpty`, where `main.wxs` reads it;
  run the same script before a local `cargo wix`.
