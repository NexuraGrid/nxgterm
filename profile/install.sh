#!/bin/sh
# nxgterm tools profile for Linux and macOS.
#
# Installs Yazi, zoxide, ngmux, Bruno CLI (bru) and curl with the system
# package manager (pacman, apt, dnf or Homebrew), falling back to official
# GitHub release binaries in ~/.local/bin where a distribution has no package.
# Optional and idempotent: tools that are already installed are skipped.
#
#   curl -fsSL https://raw.githubusercontent.com/NexuraGrid/nxgterm/main/profile/install.sh | sh -s -- --yes
#
# Run with --help for the options.

set -u

PROFILE_VERSION="0.1.0"
ALL_TOOLS="curl zoxide yazi ngmux bruno"
NGMUX_INSTALLER_URL="https://raw.githubusercontent.com/NexuraGrid/ng_mux/main/install.sh"
BRUNO_NPM_PACKAGE="@usebruno/cli"
MARK_BEGIN="# >>> nxgterm profile >>>"
MARK_END="# <<< nxgterm profile <<<"

DRY_RUN=0
ASSUME_YES=0
SHELL_INIT=1
UNINSTALL_SHELL_INIT=0
NORM_OUT=""
ONLY=""
SKIP=""
BIN_DIR="${NXGTERM_PROFILE_BIN_DIR:-$HOME/.local/bin}"
# Testing knob: install zoxide and Yazi from GitHub releases even when the
# package manager has them.
FORCE_BINARY="${NXGTERM_PROFILE_FORCE_BINARY:-0}"

OS=""
ARCH=""
PM=""
SUDO_DECISION=""
APT_UPDATED=0
RESULTS=""
FAILED=0
WORK_DIR=""

usage() {
	cat <<'EOF'
nxgterm tools profile: installs Yazi, zoxide, ngmux, Bruno CLI (bru) and curl.

Usage: install.sh [OPTIONS]

Options:
  --dry-run               Print what would be done; change nothing
  -y, --yes               Do not ask; answer yes (sudo, Node.js, installers)
  --only <list>           Only these tools (comma separated)
  --skip <list>           Skip these tools (comma separated)
  --no-shell-init         Do not touch shell startup files
  --uninstall-shell-init  Remove the nxgterm block from shell startup files and exit
  -h, --help              Print this help and exit
  --version               Print the profile version and exit

Tools: curl, zoxide, yazi, ngmux, bruno (alias: bru).

Environment:
  NXGTERM_PROFILE_BIN_DIR   Where release binaries go (default: ~/.local/bin)
  GITHUB_TOKEN              Optional token for GitHub API calls (rate limits)
EOF
}

say() { printf '%s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die() {
	printf 'error: %s\n' "$*" >&2
	exit 2
}
have() { command -v "$1" >/dev/null 2>&1; }

cleanup() {
	if [ -n "$WORK_DIR" ] && [ -d "$WORK_DIR" ]; then
		rm -rf "$WORK_DIR"
	fi
}

# --- arguments ---------------------------------------------------------------

# normalize_list <list>: sets NORM_OUT ("a,b c" -> " a b c"), with `bru`
# accepted as an alias of `bruno`. No subshell, so `die` stops the script.
normalize_list() {
	norm_out=""
	for norm_item in $(printf '%s' "$1" | tr ',' ' '); do
		[ "$norm_item" = "bru" ] && norm_item="bruno"
		case " $ALL_TOOLS " in
		*" $norm_item "*) ;;
		*) die "unknown tool '$norm_item' (expected: $ALL_TOOLS)" ;;
		esac
		norm_out="$norm_out $norm_item"
	done
	NORM_OUT=$norm_out
}

parse_args() {
	while [ $# -gt 0 ]; do
		case "$1" in
		--dry-run) DRY_RUN=1 ;;
		-y | --yes) ASSUME_YES=1 ;;
		--no-shell-init) SHELL_INIT=0 ;;
		--uninstall-shell-init) UNINSTALL_SHELL_INIT=1 ;;
		--only)
			[ $# -ge 2 ] || die "--only needs a list"
			normalize_list "$2"
			ONLY="$ONLY $NORM_OUT"
			shift
			;;
		--only=*)
			normalize_list "${1#--only=}"
			ONLY="$ONLY $NORM_OUT"
			;;
		--skip)
			[ $# -ge 2 ] || die "--skip needs a list"
			normalize_list "$2"
			SKIP="$SKIP $NORM_OUT"
			shift
			;;
		--skip=*)
			normalize_list "${1#--skip=}"
			SKIP="$SKIP $NORM_OUT"
			;;
		-h | --help)
			usage
			exit 0
			;;
		--version)
			say "nxgterm profile $PROFILE_VERSION"
			exit 0
			;;
		*) die "unknown option '$1' (see --help)" ;;
		esac
		shift
	done
}

selected() {
	if [ -n "$ONLY" ]; then
		case " $ONLY " in *" $1 "*) ;; *) return 1 ;; esac
	fi
	case " $SKIP " in *" $1 "*) return 1 ;; esac
	return 0
}

# --- interaction and execution -------------------------------------------------

# confirm <question>: yes with --yes; in dry-run the question is only shown.
# Reads the answer from /dev/tty so `curl ... | sh` can still ask.
confirm() {
	if [ "$ASSUME_YES" = 1 ]; then
		return 0
	fi
	if [ "$DRY_RUN" = 1 ]; then
		say "[dry-run] would ask: $1 [y/N]"
		return 0
	fi
	if ! (: </dev/tty) 2>/dev/null; then
		warn "cannot ask '$1' without a terminal; rerun with --yes to accept"
		return 1
	fi
	printf '%s [y/N] ' "$1" >/dev/tty
	read -r confirm_answer </dev/tty || confirm_answer=""
	case "$confirm_answer" in
	y | Y | yes | YES | Yes) return 0 ;;
	*) return 1 ;;
	esac
}

# run <command...>: prints the command, then runs it unless --dry-run.
run() {
	if [ "$DRY_RUN" = 1 ]; then
		say "[dry-run] $*"
		return 0
	fi
	say "+ $*"
	"$@"
}

# run_root <command...>: as root, through sudo after asking once.
run_root() {
	if [ "$(id -u)" = 0 ]; then
		run "$@"
		return
	fi
	if ! have sudo; then
		warn "'$*' needs root and sudo is not installed"
		return 1
	fi
	if [ -z "$SUDO_DECISION" ]; then
		if confirm "Use sudo to run package manager commands (first: sudo $*)?"; then
			SUDO_DECISION=yes
		else
			SUDO_DECISION=no
		fi
	fi
	if [ "$SUDO_DECISION" != yes ]; then
		warn "sudo declined; not running: $*"
		return 1
	fi
	run sudo "$@"
}

# done_status: what a successful install is called in this mode.
done_status() {
	if [ "$DRY_RUN" = 1 ]; then printf 'would install'; else printf 'installed'; fi
}

record() {
	# record <tool> <status> <detail>
	RESULTS="$RESULTS$1|$2|$3
"
	[ "$2" = failed ] && FAILED=1
	return 0
}

# --- platform ------------------------------------------------------------------

detect_platform() {
	case "$(uname -s)" in
	Linux) OS=linux ;;
	Darwin) OS=macos ;;
	*) die "unsupported OS '$(uname -s)'; on Windows use profile/install.ps1" ;;
	esac
	case "$(uname -m)" in
	x86_64 | amd64) ARCH=x86_64 ;;
	aarch64 | arm64) ARCH=aarch64 ;;
	*) ARCH="" ;;
	esac
	if [ "$OS" = macos ]; then
		have brew && PM=brew
	elif have pacman; then
		PM=pacman
	elif have apt-get; then
		PM=apt
	elif have dnf; then
		PM=dnf
	fi
}

# pm_has <package>: whether the package manager offers the package.
pm_has() {
	case "$PM" in
	pacman) pacman -Si "$1" >/dev/null 2>&1 ;;
	apt)
		pm_has_candidate=$(apt-cache policy "$1" 2>/dev/null | sed -n 's/^[[:space:]]*Candidate:[[:space:]]*//p')
		[ -n "$pm_has_candidate" ] && [ "$pm_has_candidate" != "(none)" ]
		;;
	dnf) dnf -q info "$1" >/dev/null 2>&1 ;;
	brew) brew info --formula "$1" >/dev/null 2>&1 ;;
	*) return 1 ;;
	esac
}

pm_install() {
	case "$PM" in
	pacman) run_root pacman -S --needed --noconfirm "$@" ;;
	apt)
		if [ "$APT_UPDATED" = 0 ]; then
			run_root apt-get update || return 1
			APT_UPDATED=1
		fi
		run_root env DEBIAN_FRONTEND=noninteractive apt-get install -y "$@"
		;;
	dnf) run_root dnf install -y "$@" ;;
	brew) run brew install "$@" ;;
	*) return 1 ;;
	esac
}

# --- downloads -------------------------------------------------------------------

work_dir() {
	if [ -z "$WORK_DIR" ]; then
		WORK_DIR=$(mktemp -d 2>/dev/null || mktemp -d -t nxgterm-profile)
	fi
	printf '%s' "$WORK_DIR"
}

fetch() {
	# fetch <url> <dest>
	if have curl; then
		if [ -n "${GITHUB_TOKEN:-}" ] && [ "${1#https://api.github.com/}" != "$1" ]; then
			curl -fsSL --retry 3 -H "Authorization: Bearer $GITHUB_TOKEN" -o "$2" "$1"
		else
			curl -fsSL --retry 3 -o "$2" "$1"
		fi
	elif have wget; then
		wget -qO "$2" "$1"
	else
		warn "need curl or wget to download $1"
		return 1
	fi
}

sha256_of() {
	if have sha256sum; then
		sha256sum "$1" | awk '{print $1}'
	elif have shasum; then
		shasum -a 256 "$1" | awk '{print $1}'
	else
		return 1
	fi
}

# json_tag <release.json>
json_tag() {
	tr ',' '\n' <"$1" | sed -n 's/^[[:space:]{]*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1
}

# json_digest <release.json> <asset>: the SHA-256 GitHub publishes for the
# asset ("digest" precedes "browser_download_url" in every asset object).
json_digest() {
	# shellcheck disable=SC2020 # every one of , { } becomes a newline
	tr ',{}' '\n\n\n' <"$1" | awk -v want="$2" '
		/"digest":/ {
			d = $0; sub(/.*"digest":[[:space:]]*/, "", d); gsub(/["[:space:]]/, "", d)
			sub(/^sha256:/, "", d); digest = d
		}
		/"browser_download_url":/ {
			u = $0; sub(/.*"browser_download_url":[[:space:]]*"/, "", u); sub(/".*/, "", u)
			n = u; sub(/.*\//, "", n)
			if (n == want) { print digest; exit }
			digest = ""
		}'
}

extract_zip() {
	# extract_zip <zip> <dir>
	if have unzip; then
		unzip -q -o "$1" -d "$2"
	elif have bsdtar; then
		bsdtar -xf "$1" -C "$2"
	elif have python3; then
		python3 -I -m zipfile -e "$1" "$2"
	else
		warn "need unzip, bsdtar or python3 to extract $1"
		return 1
	fi
}

# github_binary <repo> <asset pattern with {tag} and {ver}> <binaries...>
# Downloads the latest release asset, verifies its published SHA-256 and
# copies the named binaries into BIN_DIR.
github_binary() {
	gb_repo=$1
	gb_pattern=$2
	shift 2
	if [ "$DRY_RUN" = 1 ]; then
		say "[dry-run] download the latest $gb_repo release asset ($gb_pattern), verify its SHA-256, install $* into $BIN_DIR"
		return 0
	fi
	gb_dir="$(work_dir)/$(printf '%s' "$gb_repo" | tr '/' '_')"
	mkdir -p "$gb_dir/x" || return 1
	fetch "https://api.github.com/repos/$gb_repo/releases/latest" "$gb_dir/release.json" || return 1
	gb_tag=$(json_tag "$gb_dir/release.json")
	[ -n "$gb_tag" ] || {
		warn "could not read the latest $gb_repo release"
		return 1
	}
	gb_asset=$(printf '%s' "$gb_pattern" | sed "s/{tag}/$gb_tag/g; s/{ver}/${gb_tag#v}/g")
	gb_want=$(json_digest "$gb_dir/release.json" "$gb_asset")
	case "$gb_want" in
	[0-9a-f][0-9a-f][0-9a-f][0-9a-f]*) ;;
	*)
		warn "$gb_repo $gb_tag publishes no SHA-256 for $gb_asset; refusing to install it unverified"
		return 1
		;;
	esac
	gb_url="https://github.com/$gb_repo/releases/download/$gb_tag/$gb_asset"
	say "+ download $gb_url"
	fetch "$gb_url" "$gb_dir/$gb_asset" || return 1
	gb_got=$(sha256_of "$gb_dir/$gb_asset") || {
		warn "need sha256sum or shasum to verify $gb_asset"
		return 1
	}
	if [ "$gb_got" != "$gb_want" ]; then
		warn "checksum mismatch for $gb_asset (expected $gb_want, got $gb_got)"
		return 1
	fi
	say "  sha256 ok: $gb_got"
	case "$gb_asset" in
	*.zip) extract_zip "$gb_dir/$gb_asset" "$gb_dir/x" || return 1 ;;
	*.tar.gz) tar -xzf "$gb_dir/$gb_asset" -C "$gb_dir/x" || return 1 ;;
	esac
	mkdir -p "$BIN_DIR" || return 1
	for gb_bin in "$@"; do
		gb_path=$(find "$gb_dir/x" -type f -name "$gb_bin" | head -n 1)
		[ -n "$gb_path" ] || {
			warn "$gb_bin not found in $gb_asset"
			return 1
		}
		cp "$gb_path" "$BIN_DIR/$gb_bin.new" && chmod 0755 "$BIN_DIR/$gb_bin.new" &&
			mv -f "$BIN_DIR/$gb_bin.new" "$BIN_DIR/$gb_bin" || return 1
		say "  installed $BIN_DIR/$gb_bin"
	done
	GB_TAG=$gb_tag
}

target_triple() {
	if [ "$OS" = macos ]; then
		printf '%s-apple-darwin' "$ARCH"
	else
		printf '%s-unknown-linux-musl' "$ARCH"
	fi
}

# --- tools -----------------------------------------------------------------------

installed() {
	have "$1" || [ -x "$BIN_DIR/$1" ]
}

install_curl() {
	if have curl; then
		record curl "already present" "$(command -v curl)"
	elif [ -n "$PM" ] && pm_install curl; then
		record curl "$(done_status)" "$PM"
	else
		record curl failed "install curl with your package manager"
	fi
}

install_zoxide() {
	if installed zoxide; then
		record zoxide "already present" "$(command -v zoxide || printf '%s' "$BIN_DIR/zoxide")"
		return
	fi
	if [ "$FORCE_BINARY" != 1 ] && [ -n "$PM" ] && pm_has zoxide; then
		if pm_install zoxide; then
			record zoxide "$(done_status)" "$PM"
			return
		fi
		warn "zoxide: $PM failed; trying the GitHub release"
	fi
	if [ -z "$ARCH" ]; then
		record zoxide failed "no package and no release binary for $(uname -m)"
	elif github_binary ajeetdsouza/zoxide "zoxide-{ver}-$(target_triple).tar.gz" zoxide; then
		record zoxide "$(done_status)" "GitHub release ${GB_TAG:-latest} -> $BIN_DIR"
	else
		record zoxide failed "see the messages above"
	fi
}

install_yazi() {
	if installed yazi; then
		record yazi "already present" "$(command -v yazi || printf '%s' "$BIN_DIR/yazi")"
		return
	fi
	# Debian, Ubuntu and Fedora have no (current) Yazi package: use the release.
	case "$PM" in
	pacman | brew)
		if [ "$FORCE_BINARY" != 1 ]; then
			if pm_install yazi; then
				record yazi "$(done_status)" "$PM"
				return
			fi
			warn "yazi: $PM failed; trying the GitHub release"
		fi
		;;
	esac
	if [ -z "$ARCH" ]; then
		record yazi failed "no package and no release binary for $(uname -m)"
	elif github_binary sxyazi/yazi "yazi-$(target_triple).zip" yazi ya; then
		record yazi "$(done_status)" "GitHub release ${GB_TAG:-latest} -> $BIN_DIR"
	else
		record yazi failed "see the messages above"
	fi
}

install_ngmux() {
	if installed ngmux; then
		record ngmux "already present" "$(command -v ngmux || printf '%s' "$BIN_DIR/ngmux")"
		return
	fi
	say "ngmux is installed with its official installer:"
	say "  $NGMUX_INSTALLER_URL"
	if [ "$DRY_RUN" = 1 ]; then
		say "[dry-run] download the installer, show its size, then run: NGMUX_INSTALL_DIR=$BIN_DIR sh <installer>"
		record ngmux "would install" "official installer"
		return
	fi
	ng_script="$(work_dir)/ngmux-install.sh"
	if ! fetch "$NGMUX_INSTALLER_URL" "$ng_script"; then
		record ngmux failed "could not download the installer"
		return
	fi
	say "  downloaded $(wc -l <"$ng_script" | tr -d ' ') lines (sha256 $(sha256_of "$ng_script" || printf unknown))"
	if ! confirm "Run the ngmux installer now?"; then
		record ngmux skipped "installer declined"
		return
	fi
	if run env NGMUX_INSTALL_DIR="$BIN_DIR" sh "$ng_script"; then
		record ngmux "$(done_status)" "official installer -> $BIN_DIR"
	else
		record ngmux failed "the ngmux installer failed"
	fi
}

node_packages() {
	case "$PM" in
	pacman) printf 'nodejs-lts npm' ;; # `nodejs-lts` resolves to the current LTS package
	apt) printf 'nodejs npm' ;;
	dnf) printf 'nodejs nodejs-npm' ;;
	brew) printf 'node' ;;
	esac
}

install_bruno() {
	if installed bru; then
		record bruno "already present" "$(command -v bru || printf '%s' "$BIN_DIR/bru")"
		return
	fi
	if ! have node || ! have npm; then
		if [ -z "$PM" ]; then
			record bruno skipped "Node.js is missing; install it (https://nodejs.org) and rerun"
			return
		fi
		# shellcheck disable=SC2046 # word splitting of the package list is intended
		if confirm "Bruno CLI needs Node.js. Install Node.js LTS ($(node_packages)) with $PM?"; then
			if ! pm_install $(node_packages); then
				record bruno failed "could not install Node.js"
				return
			fi
		else
			say "Skipping Bruno CLI: Node.js was not installed."
			record bruno skipped "Node.js declined"
			return
		fi
	fi
	if have node; then
		bru_node_major=$(node -p 'process.versions.node.split(".")[0]' 2>/dev/null || printf 0)
		if [ "${bru_node_major:-0}" -lt 18 ] 2>/dev/null; then
			warn "Node.js $(node --version) is old; Bruno CLI may need a newer LTS"
		fi
	fi
	# Install globally when npm's prefix is writable (Homebrew, nvm, fnm);
	# otherwise into ~/.local so no sudo is needed.
	bru_prefix=""
	if have npm; then
		bru_global=$(npm prefix -g 2>/dev/null || printf '')
		if [ -z "$bru_global" ] || [ ! -w "$bru_global" ]; then
			bru_prefix=$(dirname "$BIN_DIR")
		fi
	elif [ "$OS" = linux ]; then
		bru_prefix=$(dirname "$BIN_DIR")
	fi
	if [ -n "$bru_prefix" ]; then
		bru_ok=0
		run npm install -g --prefix "$bru_prefix" "$BRUNO_NPM_PACKAGE" && bru_ok=1
		bru_where="npm ($bru_prefix)"
	else
		bru_ok=0
		run npm install -g "$BRUNO_NPM_PACKAGE" && bru_ok=1
		bru_where="npm (global)"
	fi
	if [ "$bru_ok" = 1 ]; then
		record bruno "$(done_status)" "$bru_where"
	else
		record bruno failed "npm install $BRUNO_NPM_PACKAGE failed"
	fi
}

# --- shell integration -------------------------------------------------------------

# rc_bin_dir: BIN_DIR as written in startup files ($HOME kept literal).
rc_bin_dir() {
	case "$BIN_DIR" in
	"$HOME"/*) printf '%s' "\$HOME/${BIN_DIR#"$HOME"/}" ;;
	*) printf '%s' "$BIN_DIR" ;;
	esac
}

# The blocks are shell code for later: `$PATH` and `$(...)` stay literal.
# shellcheck disable=SC2016
posix_block() {
	# posix_block <bash|zsh>
	printf '%s\n' "$MARK_BEGIN"
	printf '%s\n' "# Added by the nxgterm tools profile. Remove with: install.sh --uninstall-shell-init"
	printf 'case ":$PATH:" in *":%s:"*) ;; *) export PATH="%s:$PATH" ;; esac\n' "$(rc_bin_dir)" "$(rc_bin_dir)"
	if selected zoxide; then
		printf 'if command -v zoxide >/dev/null 2>&1; then eval "$(zoxide init %s)"; fi\n' "$1"
	fi
	if selected yazi; then
		cat <<'EOF'
# Yazi: `y` changes to the directory Yazi was in when it exits.
function y() {
	local tmp cwd; tmp="$(mktemp -t "yazi-cwd.XXXXXX")"
	command yazi "$@" --cwd-file="$tmp"
	IFS= read -r -d '' cwd < "$tmp"
	[ "$cwd" != "$PWD" ] && [ -d "$cwd" ] && builtin cd -- "$cwd" || builtin true
	command rm -f -- "$tmp"
}
EOF
	fi
	printf '%s\n' "$MARK_END"
}

# shellcheck disable=SC2016
fish_block() {
	printf '%s\n' "$MARK_BEGIN"
	printf '%s\n' "# Added by the nxgterm tools profile. Remove with: install.sh --uninstall-shell-init"
	printf 'if not contains -- "%s" $PATH\n\tset -gx PATH "%s" $PATH\nend\n' "$(rc_bin_dir)" "$(rc_bin_dir)"
	if selected zoxide; then
		printf '%s\n' 'if type -q zoxide' '	zoxide init fish | source' 'end'
	fi
	if selected yazi; then
		cat <<'EOF'
# Yazi: `y` changes to the directory Yazi was in when it exits.
function y
	set tmp (mktemp -t "yazi-cwd.XXXXXX")
	command yazi $argv --cwd-file="$tmp"
	if read -z cwd < "$tmp"; and [ "$cwd" != "$PWD" ]; and test -d "$cwd"
		builtin cd -- "$cwd"
	end
	command rm -f -- "$tmp"
end
EOF
	fi
	printf '%s\n' "$MARK_END"
}

# strip_block <file>: the file without the managed block, on stdout.
strip_block() {
	awk -v b="$MARK_BEGIN" -v e="$MARK_END" '
		$0 == b { skip = 1; next }
		$0 == e { skip = 0; next }
		!skip' "$1"
}

# write_block <file> <block text>: replaces (or appends) the managed block.
write_block() {
	wb_file=$1
	if [ "$DRY_RUN" = 1 ]; then
		say "[dry-run] would write the nxgterm block (zoxide init, y wrapper, PATH) to $wb_file"
		return 0
	fi
	mkdir -p "$(dirname "$wb_file")" || return 1
	wb_tmp="$(work_dir)/rc.new"
	if [ -f "$wb_file" ]; then
		strip_block "$wb_file" >"$wb_tmp" || return 1
	else
		: >"$wb_tmp"
	fi
	if [ -s "$wb_tmp" ] && [ -n "$(tail -n 1 "$wb_tmp")" ]; then
		printf '\n' >>"$wb_tmp"
	fi
	printf '%s\n' "$2" >>"$wb_tmp"
	if [ -f "$wb_file" ] && cmp -s "$wb_tmp" "$wb_file"; then
		say "  $wb_file: up to date"
		return 0
	fi
	# `cat >` keeps the file's permissions and any symlink in place.
	cat "$wb_tmp" >"$wb_file" || return 1
	say "  $wb_file: updated"
}

remove_block() {
	rb_file=$1
	[ -f "$rb_file" ] || return 0
	grep -qxF "$MARK_BEGIN" "$rb_file" || return 0
	if [ "$DRY_RUN" = 1 ]; then
		say "[dry-run] would remove the nxgterm block from $rb_file"
		return 0
	fi
	rb_tmp="$(work_dir)/rc.strip"
	# Also drop the blank separator line(s) left at the end of the file.
	strip_block "$rb_file" | awk '
		/^[[:space:]]*$/ { blank = blank $0 "\n"; next }
		{ printf "%s%s\n", blank, $0; blank = "" }' >"$rb_tmp" || return 1
	if [ "${rb_file##*/}" = "nxgterm-profile.fish" ] && ! grep -q '[^[:space:]]' "$rb_tmp"; then
		rm -f "$rb_file"
		say "  removed $rb_file"
	else
		cat "$rb_tmp" >"$rb_file" || return 1
		say "  cleaned $rb_file"
	fi
}

bash_rc() { printf '%s' "$HOME/.bashrc"; }
zsh_rc() { printf '%s' "${ZDOTDIR:-$HOME}/.zshrc"; }
fish_rc() { printf '%s' "${XDG_CONFIG_HOME:-$HOME/.config}/fish/conf.d/nxgterm-profile.fish"; }

login_shell() { basename "${SHELL:-sh}"; }

setup_shell_init() {
	if ! selected zoxide && ! selected yazi; then
		return 0
	fi
	say ""
	say "Shell integration:"
	si_any=0
	if [ -f "$(bash_rc)" ] || [ "$(login_shell)" = bash ]; then
		write_block "$(bash_rc)" "$(posix_block bash)" || warn "could not update $(bash_rc)"
		si_any=1
	fi
	if [ -f "$(zsh_rc)" ] || [ "$(login_shell)" = zsh ]; then
		write_block "$(zsh_rc)" "$(posix_block zsh)" || warn "could not update $(zsh_rc)"
		si_any=1
	fi
	if have fish || [ -d "${XDG_CONFIG_HOME:-$HOME/.config}/fish" ]; then
		write_block "$(fish_rc)" "$(fish_block)" || warn "could not update $(fish_rc)"
		si_any=1
	fi
	if [ "$si_any" = 0 ]; then
		say "  no bash, zsh or fish configuration found; skipped"
	fi
}

uninstall_shell_init() {
	say "Removing the nxgterm block from shell startup files:"
	for ui_file in "$(bash_rc)" "$(zsh_rc)" "$(fish_rc)"; do
		remove_block "$ui_file" || warn "could not clean $ui_file"
	done
}

# --- main ------------------------------------------------------------------------

summary() {
	say ""
	say "Summary"
	printf '%-8s %-16s %s\n' "TOOL" "STATUS" "DETAIL"
	printf '%s' "$RESULTS" | while IFS='|' read -r s_tool s_status s_detail; do
		[ -n "$s_tool" ] && printf '%-8s %-16s %s\n' "$s_tool" "$s_status" "$s_detail"
	done
	case ":$PATH:" in
	*":$BIN_DIR:"*) ;;
	*) say "" && say "Note: $BIN_DIR is not on PATH in this shell; open a new shell (or add it)." ;;
	esac
}

main() {
	parse_args "$@"
	trap cleanup EXIT
	trap 'exit 130' INT TERM
	detect_platform
	if [ "$UNINSTALL_SHELL_INIT" = 1 ]; then
		uninstall_shell_init
		exit 0
	fi
	say "nxgterm tools profile $PROFILE_VERSION"
	say "  system:          $OS ${ARCH:-$(uname -m)}"
	say "  package manager: ${PM:-none (GitHub release binaries only)}"
	say "  binaries:        $BIN_DIR"
	[ "$DRY_RUN" = 1 ] && say "  mode:            dry run (nothing is changed)"
	say ""
	for m_tool in $ALL_TOOLS; do
		if ! selected "$m_tool"; then
			record "$m_tool" skipped "not selected"
			continue
		fi
		say "==> $m_tool"
		"install_$m_tool"
	done
	if [ "$SHELL_INIT" = 1 ]; then
		setup_shell_init
	fi
	summary
	if [ "$FAILED" = 1 ]; then
		exit 1
	fi
}

main "$@"
