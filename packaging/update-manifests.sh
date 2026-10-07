#!/usr/bin/env bash
# Updates the distribution manifests in packaging/ to a released version:
# version numbers, download URLs and SHA-256 checksums (taken from the
# release's SHA256SUMS), and regenerates the AUR .SRCINFO files.
#
#   packaging/update-manifests.sh 0.2.0                 # downloads SHA256SUMS
#   packaging/update-manifests.sh 0.2.0 path/SHA256SUMS # uses a local copy
#
# Review the result with `git diff packaging/` before publishing.

set -euo pipefail

REPO_URL="https://github.com/NexuraGrid/nxgterm"
DIR="${PACKAGING_DIR:-$(cd "$(dirname "$0")" && pwd)}"

die() {
	printf 'error: %s\n' "$*" >&2
	exit 1
}

[[ $# -ge 1 && $# -le 2 ]] || die "usage: $0 <version> [SHA256SUMS file or URL]"
VERSION=${1#v}
[[ $VERSION =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.]+)?$ ]] || die "invalid version '$1'"
SUMS_SOURCE=${2:-$REPO_URL/releases/download/v$VERSION/SHA256SUMS}
RELEASE_DATE=${RELEASE_DATE:-$(date -u +%Y-%m-%d)}

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

if [[ -f $SUMS_SOURCE ]]; then
	cp "$SUMS_SOURCE" "$TMP/SHA256SUMS"
else
	curl -fsSL --retry 3 -o "$TMP/SHA256SUMS" "$SUMS_SOURCE" || die "could not download $SUMS_SOURCE"
fi

# sum <asset>: its SHA-256 from SHA256SUMS ("<hash>  <name>" or "<hash> *<name>").
sum() {
	local hash
	hash=$(awk -v want="$1" '{ name = $2; sub(/^\*/, "", name) } name == want { print $1; exit }' "$TMP/SHA256SUMS")
	[[ $hash =~ ^[0-9a-fA-F]{64}$ ]] || die "SHA256SUMS has no checksum for $1"
	printf '%s' "$hash" | tr 'A-F' 'a-f'
}

LINUX_X86_64=$(sum "nxgterm-$VERSION-x86_64-linux.tar.gz")
LINUX_AARCH64=$(sum "nxgterm-$VERSION-aarch64-linux.tar.gz")
SOURCE=$(sum "nxgterm-$VERSION-source.tar.gz")
MSI=$(sum "nxgterm-$VERSION-x86_64.msi")
DMG=$(sum "nxgterm-$VERSION-universal-macos.dmg")

# edit <file> <sed script>: in place, portable across GNU and BSD sed.
edit() {
	[[ -f $1 ]] || die "missing $1"
	sed -e "$2" "$1" >"$TMP/edit" && cat "$TMP/edit" >"$1"
}

# srcinfo <PKGBUILD>: the .SRCINFO that `makepkg --printsrcinfo` prints, for
# the fields these PKGBUILDs use. The variables come from the PKGBUILD.
# shellcheck disable=SC2154
srcinfo() {
	(
		set +u
		# shellcheck disable=SC1090
		source "$1"
		emit() {
			local key=$1 value
			shift
			for value in "$@"; do
				[[ -n $value ]] && printf '\t%s = %s\n' "$key" "$value"
			done
			return 0
		}
		printf 'pkgbase = %s\n' "$pkgname"
		emit pkgdesc "$pkgdesc"
		emit pkgver "$pkgver"
		emit pkgrel "$pkgrel"
		emit url "$url"
		emit arch "${arch[@]}"
		emit license "${license[@]}"
		emit checkdepends "${checkdepends[@]}"
		emit makedepends "${makedepends[@]}"
		emit depends "${depends[@]}"
		emit optdepends "${optdepends[@]}"
		emit provides "${provides[@]}"
		emit conflicts "${conflicts[@]}"
		emit source "${source[@]}"
		emit sha256sums "${sha256sums[@]}"
		local a ref
		for a in "${arch[@]}"; do
			ref="source_${a}[@]"
			emit "source_$a" "${!ref}"
			ref="sha256sums_${a}[@]"
			emit "sha256sums_$a" "${!ref}"
		done
		printf '\npkgname = %s\n' "$pkgname"
	)
}

# AUR: prebuilt binary package.
BIN_PKGBUILD="$DIR/aur/nxgterm-bin/PKGBUILD"
edit "$BIN_PKGBUILD" "s/^pkgver=.*/pkgver=$VERSION/; s/^pkgrel=.*/pkgrel=1/;
	s/^sha256sums_x86_64=.*/sha256sums_x86_64=('$LINUX_X86_64')/;
	s/^sha256sums_aarch64=.*/sha256sums_aarch64=('$LINUX_AARCH64')/"
srcinfo "$BIN_PKGBUILD" >"$DIR/aur/nxgterm-bin/.SRCINFO"

# AUR: build from source.
SRC_PKGBUILD="$DIR/aur/nxgterm/PKGBUILD"
edit "$SRC_PKGBUILD" "s/^pkgver=.*/pkgver=$VERSION/; s/^pkgrel=.*/pkgrel=1/;
	s/^sha256sums=.*/sha256sums=('$SOURCE')/"
srcinfo "$SRC_PKGBUILD" >"$DIR/aur/nxgterm/.SRCINFO"

# winget (checksums are upper case by convention). `tr`, not ${MSI^^}: macOS
# ships bash 3.2.
MSI_UPPER=$(printf '%s' "$MSI" | tr 'a-f' 'A-F')
for f in "$DIR"/winget/NexuraGrid.nxgterm*.yaml; do
	edit "$f" "s/^PackageVersion: .*/PackageVersion: $VERSION/"
done
edit "$DIR/winget/NexuraGrid.nxgterm.installer.yaml" "
	s|^ReleaseDate: .*|ReleaseDate: $RELEASE_DATE|;
	s|^\(  InstallerUrl: \).*|\1$REPO_URL/releases/download/v$VERSION/nxgterm-$VERSION-x86_64.msi|;
	s|^\(  InstallerSha256: \).*|\1$MSI_UPPER|"
edit "$DIR/winget/NexuraGrid.nxgterm.locale.en-US.yaml" "
	s|^ReleaseNotesUrl: .*|ReleaseNotesUrl: $REPO_URL/releases/tag/v$VERSION|"

# Homebrew cask.
edit "$DIR/homebrew/Casks/nxgterm.rb" "s/^  version \".*\"/  version \"$VERSION\"/;
	s/^  sha256 \".*\"/  sha256 \"$DMG\"/"

# Fedora spec: version, release and a changelog entry for a new version.
SPEC="$DIR/fedora/nxgterm.spec"
if ! grep -q "^Version:[[:space:]]*$VERSION\$" "$SPEC"; then
	entry="* $(LC_ALL=C date -u '+%a %b %d %Y') NexuraGrid <https://github.com/NexuraGrid> - $VERSION-1"
	edit "$SPEC" "s/^Version:\([[:space:]]*\).*/Version:\1$VERSION/;
		s/^Release:\([[:space:]]*\).*/Release:\11%{?dist}/;
		/^%changelog\$/a\\
$entry\\
- Update to $VERSION\\

"
fi

printf 'Updated packaging/ to %s:\n' "$VERSION"
printf '  %-36s %s\n' \
	"nxgterm-$VERSION-x86_64-linux.tar.gz" "$LINUX_X86_64" \
	"nxgterm-$VERSION-aarch64-linux.tar.gz" "$LINUX_AARCH64" \
	"nxgterm-$VERSION-source.tar.gz" "$SOURCE" \
	"nxgterm-$VERSION-x86_64.msi" "$MSI" \
	"nxgterm-$VERSION-universal-macos.dmg" "$DMG"
