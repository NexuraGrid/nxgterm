#!/bin/sh
# Installs nxgterm from a GitHub Release on Linux and macOS, without sudo.
#
#   curl -fsSL https://raw.githubusercontent.com/NexuraGrid/nxgterm/main/install.sh | sh
#
# Linux: the portable tarball goes to ~/.local/share/nxgterm, with a symlink
# in ~/.local/bin and a desktop entry and icon in ~/.local/share.
# macOS: nxgterm.app goes to /Applications (or ~/Applications when that is
# not writable), with a symlink in ~/.local/bin.
# Every download is checked against the release's SHA256SUMS before use.
# Running it again reinstalls; `nxgterm update` keeps the install current.
#
# Environment:
#   NXGTERM_VERSION      install this version (e.g. 0.10.1) instead of the latest
#   NXGTERM_INSTALL_URL  directory holding SHA256SUMS and the assets, instead
#                        of the GitHub Release (for testing or a mirror)
#   NXGTERM_INSTALL_DIR  Linux install directory (default ~/.local/share/nxgterm)
#   NXGTERM_BIN_DIR      where the `nxgterm` symlink goes (default ~/.local/bin)

set -eu

REPO_URL="https://github.com/NexuraGrid/nxgterm"

say() { printf 'nxgterm: %s\n' "$*"; }
warn() { printf 'nxgterm: warning: %s\n' "$*" >&2; }
die() {
    printf 'nxgterm: error: %s\n' "$*" >&2
    exit 1
}

# Downloads $1 to the file $2 with curl, or wget when there is no curl.
fetch() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL -o "$2" "$1" || die "cannot download $1"
    elif command -v wget >/dev/null 2>&1; then
        wget -q -O "$2" "$1" || die "cannot download $1"
    else
        die "neither curl nor wget is installed"
    fi
}

# Prints the SHA-256 of the file $1.
sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{ print $1 }'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{ print $1 }'
    else
        die "neither sha256sum nor shasum is installed"
    fi
}

# Checks the downloaded asset $1 (named $2) against the SHA256SUMS file $3.
# Lines are `<hash>  <name>`, or `<hash> *<name>` in binary mode.
verify() {
    expected=$(awk -v name="$2" '$2 == name || $2 == "*" name { print $1; exit }' "$3")
    [ -n "$expected" ] || die "$2 is not listed in SHA256SUMS; nothing was installed"
    actual=$(sha256 "$1")
    [ "$actual" = "$expected" ] ||
        die "checksum mismatch for $2 (expected $expected, got $actual); nothing was installed"
}

# Replaces the directory $2 with the directory $1 (on the same filesystem):
# the old copy is moved aside first and removed only once the new one is
# in place, so a failure leaves the previous install untouched.
swap_dir() {
    if [ -e "$2" ]; then
        rm -rf "$2.old"
        mv "$2" "$2.old"
        if ! mv "$1" "$2"; then
            mv "$2.old" "$2"
            die "cannot move the new files into $2"
        fi
        rm -rf "$2.old"
    else
        mv "$1" "$2"
    fi
}

# Points $BIN_DIR/nxgterm at the binary $1, refusing to replace a real file.
link_bin() {
    mkdir -p "$BIN_DIR"
    link="$BIN_DIR/nxgterm"
    if [ -e "$link" ] && [ ! -L "$link" ]; then
        die "$link exists and is not a symlink; move it away and run this again"
    fi
    ln -sf "$1" "$link"
    say "linked $link -> $1"
    case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) warn "$BIN_DIR is not on your PATH; add it to run \`nxgterm\` from a shell" ;;
    esac
}

# --- Platform ---------------------------------------------------------------

case $(uname -s) in
Linux) os=linux ;;
Darwin) os=macos ;;
*) die "unsupported OS $(uname -s); see $REPO_URL/releases for other downloads" ;;
esac

case $(uname -m) in
x86_64 | amd64) arch=x86_64 ;;
aarch64 | arm64) arch=aarch64 ;;
*) die "unsupported architecture $(uname -m); nxgterm ships for x86_64 and aarch64" ;;
esac

: "${HOME:?HOME is not set}"
BIN_DIR=${NXGTERM_BIN_DIR:-$HOME/.local/bin}

# --- Release ----------------------------------------------------------------

# Strip a leading "v" so both "0.10.1" and "v0.10.1" work.
version=${NXGTERM_VERSION:-}
version=${version#v}

if [ -n "${NXGTERM_INSTALL_URL:-}" ]; then
    base_url=${NXGTERM_INSTALL_URL%/}
elif [ -n "$version" ]; then
    base_url="$REPO_URL/releases/download/v$version"
else
    base_url="$REPO_URL/releases/latest/download"
fi

tmp=$(mktemp -d "${TMPDIR:-/tmp}/nxgterm-install.XXXXXX")
# Partial extractions next to the install directory are removed too.
staging=""
cleanup() { rm -rf "$tmp" ${staging:+"$staging"}; }
trap cleanup EXIT
trap 'exit 130' INT TERM

fetch "$base_url/SHA256SUMS" "$tmp/SHA256SUMS"

# The asset names carry the version: read it from the source tarball's
# entry, so the latest release needs no GitHub API call.
if [ -z "$version" ]; then
    version=$(sed -n 's/^[0-9a-fA-F]*[ *]*nxgterm-\(.*\)-source\.tar\.gz$/\1/p' "$tmp/SHA256SUMS" | head -n 1)
    [ -n "$version" ] || die "cannot read the version from $base_url/SHA256SUMS"
fi

if [ "$os" = linux ]; then
    asset="nxgterm-$version-$arch-linux.tar.gz"
else
    asset="nxgterm-$version-universal-macos.tar.gz"
fi

say "downloading $asset"
fetch "$base_url/$asset" "$tmp/$asset"
verify "$tmp/$asset" "$asset" "$tmp/SHA256SUMS"
say "checksum verified"

# --- Linux ------------------------------------------------------------------

install_linux() {
    install_dir=${NXGTERM_INSTALL_DIR:-$HOME/.local/share/nxgterm}
    install_dir=${install_dir%/}
    # Refuse to replace a directory that is not an earlier nxgterm install.
    if [ -d "$install_dir" ] && [ ! -f "$install_dir/nxgterm" ] &&
        [ -n "$(ls -A "$install_dir")" ]; then
        die "$install_dir exists and is not an nxgterm install; set NXGTERM_INSTALL_DIR"
    fi

    # Extract next to the install directory, so the swap is a rename.
    mkdir -p "$(dirname "$install_dir")"
    staging=$(mktemp -d "$install_dir.new.XXXXXX")
    tar -xzf "$tmp/$asset" -C "$staging"
    # The tarball wraps its files in nxgterm-<v>-<arch>-linux/; install
    # them flat so `nxgterm update` finds them next to the binary.
    root="$staging/nxgterm-$version-$arch-linux"
    [ -x "$root/nxgterm" ] || die "$asset has no nxgterm binary"
    swap_dir "$root" "$install_dir"
    say "installed nxgterm $version in $install_dir"

    exe="$install_dir/nxgterm"
    link_bin "$exe"

    # Desktop entry and icon, with Exec pointing at the binary so the
    # launcher works even when ~/.local/bin is not on the session PATH.
    data_home=${XDG_DATA_HOME:-$HOME/.local/share}
    apps="$data_home/applications"
    icons="$data_home/icons/hicolor/scalable/apps"
    mkdir -p "$apps" "$icons"
    if [ -f "$install_dir/share/nxgterm.desktop" ]; then
        sed "s|^Exec=nxgterm|Exec=$exe|" "$install_dir/share/nxgterm.desktop" >"$apps/nxgterm.desktop"
        say "added the desktop entry $apps/nxgterm.desktop"
    fi
    if [ -f "$install_dir/share/nxgterm.svg" ]; then
        cp "$install_dir/share/nxgterm.svg" "$icons/nxgterm.svg"
    fi
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$apps" >/dev/null 2>&1 || true
    fi
}

# --- macOS ------------------------------------------------------------------

install_macos() {
    if [ -w /Applications ]; then
        apps_dir=/Applications
    else
        apps_dir="$HOME/Applications"
        mkdir -p "$apps_dir"
    fi
    app="$apps_dir/nxgterm.app"

    staging=$(mktemp -d "$apps_dir/.nxgterm.new.XXXXXX")
    tar -xzf "$tmp/$asset" -C "$staging"
    [ -x "$staging/nxgterm.app/Contents/MacOS/nxgterm" ] || die "$asset has no nxgterm.app"
    swap_dir "$staging/nxgterm.app" "$app"
    say "installed nxgterm $version in $app"

    link_bin "$app/Contents/MacOS/nxgterm"
}

if [ "$os" = linux ]; then
    install_linux
else
    install_macos
fi

say "done; run \`nxgterm update\` to install new releases"
