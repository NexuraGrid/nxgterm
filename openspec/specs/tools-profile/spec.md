# Tools Profile Specification

## Purpose

An optional installer, separate from the terminal binary, that sets up a
terminal workflow: Yazi, zoxide, ngmux, Bruno CLI (`bru`) and curl, plus
shell integration. `profile/install.sh` targets Linux and macOS (POSIX sh);
`profile/install.ps1` targets Windows PowerShell 5.1 and PowerShell 7. Both
ship in every package and can be run from their raw GitHub URL.

Sources: `profile/install.sh`, `profile/install.ps1`, `.github/workflows/ci.yml`
(`profile` job), `README.md`.

## Requirements

### Requirement: Separation from the terminal

nxgterm MUST NOT bundle, download or run any of these tools itself. The
scripts MUST be installed as plain files (`/usr/share/nxgterm/profile/`,
`profile\` next to `nxgterm.exe`, `nxgterm.app/Contents/Resources/profile/`)
and only run when the user invokes them.

#### Scenario: Fresh install
- GIVEN nxgterm installed from any package
- WHEN it is launched
- THEN no tool is installed until the user runs the profile script

### Requirement: Options

Both scripts MUST support, with equivalent semantics:

| install.sh | install.ps1 | Effect |
|---|---|---|
| `--dry-run` | `-DryRun` | print every action; change nothing |
| `-y`, `--yes` | `-Yes` | answer yes to every question |
| `--only <list>` | `-Only <list>` | only these tools (comma separated) |
| `--skip <list>` | `-Skip <list>` | skip these tools |
| `--no-shell-init` | `-NoShellInit` | leave startup files alone |
| `--uninstall-shell-init` | `-UninstallShellInit` | remove the managed block and exit |
| `-h`, `--help` / `--version` | `-Help` / `-Version` | print and exit |

Tool names SHALL be `curl`, `zoxide`, `yazi`, `ngmux`, `bruno`, with `bru`
accepted as an alias of `bruno`. An unknown tool or option MUST abort with a
non-zero exit before any change.

#### Scenario: Unknown tool
- GIVEN `sh install.sh --only nope`
- WHEN it runs
- THEN it exits non-zero without installing anything

### Requirement: Tool sources

Tools MUST be installed in this order: curl, zoxide, yazi, ngmux, bruno.

| Tool | Arch | Debian/Ubuntu | Fedora | macOS | Windows |
|---|---|---|---|---|---|
| curl | pacman | apt | dnf | preinstalled / brew | built-in `curl.exe`; winget `cURL.cURL` / scoop `curl` |
| zoxide | pacman | apt | dnf | brew | winget `ajeetdsouza.zoxide` / scoop `zoxide` |
| Yazi | pacman | GitHub release | GitHub release | brew | winget `sxyazi.yazi` / scoop `yazi` |
| ngmux | official `install.sh` | same | same | same | official `install.ps1` |
| Bruno CLI | `npm install -g @usebruno/cli` everywhere | | | | |
| Node.js (for bru) | `nodejs-lts npm` | `nodejs npm` | `nodejs nodejs-npm` | `node` | winget `OpenJS.NodeJS.LTS` / scoop `nodejs-lts` |

On Windows, winget MUST be preferred and scoop used only without winget.
zoxide and Yazi MUST fall back to their latest GitHub release (musl on
Linux) when no package manager provides them or the package install fails.
Release binaries MUST go to `~/.local/bin` (or `NXGTERM_PROFILE_BIN_DIR`) on
Unix and `%LOCALAPPDATA%\Programs\nxgterm-profile\bin` on Windows, where that
default directory MUST be added to the user `PATH`.

#### Scenario: Windows Server 2016 without winget
- GIVEN no winget and no scoop
- WHEN `install.ps1 -Yes` runs
- THEN zoxide and Yazi are installed from verified GitHub release zips

### Requirement: Checksum verification

Every GitHub release asset MUST be verified against the SHA-256 digest
GitHub publishes for that asset before it is extracted. An asset without a
published digest MUST be refused, and a mismatch MUST fail the tool without
installing anything. Binaries MUST be installed atomically
(`<name>.new` then rename) on Unix.

#### Scenario: Tampered download
- GIVEN a downloaded Yazi zip whose hash differs from the published digest
- WHEN the script verifies it
- THEN it reports "checksum mismatch" and Yazi is marked failed

### Requirement: Consent for privileged or remote actions

Without `--yes`, the scripts MUST ask before using sudo (once per run),
before installing Node.js, and before running the downloaded ngmux
installer (after showing its size and SHA-256). On Unix, questions MUST be
read from `/dev/tty` so `curl ... | sh` can still ask; with no terminal the
answer MUST be "no" with a hint to use `--yes`. In dry-run, questions are
only printed.

#### Scenario: Piped without a TTY
- GIVEN `curl ... | sh` in a non-interactive CI job without `--yes`
- WHEN sudo is needed
- THEN the step is skipped with a warning to rerun with `--yes`

### Requirement: Idempotency

A tool found on `PATH` or in the bin directory MUST be reported as
`already present` and left untouched. Re-running the script MUST NOT
duplicate shell integration and MUST report unchanged files as up to date.

#### Scenario: Second run
- GIVEN every tool installed by a previous run
- WHEN the script runs again
- THEN every tool shows `already present` and each startup file shows `up to date`

### Requirement: Shell integration

Unless disabled, when zoxide or Yazi is selected the scripts MUST write a
block delimited by `# >>> nxgterm profile >>>` and `# <<< nxgterm profile <<<`
containing `zoxide init`, Yazi's `y` wrapper (changes directory when Yazi
exits) and, on Linux and macOS, the bin directory on `PATH`. Targets SHALL be
`~/.bashrc` and `${ZDOTDIR:-~}/.zshrc` (when present or the login shell),
`~/.config/fish/conf.d/nxgterm-profile.fish` (when fish exists), and the
PowerShell profiles `Documents\WindowsPowerShell\profile.ps1` and, when
PowerShell 7 exists, `Documents\PowerShell\profile.ps1`. An existing block
MUST be replaced in place; content outside it MUST be preserved, as must
file permissions and symlinks. Uninstall MUST remove only the block (and the
fish file when it becomes empty).

#### Scenario: Uninstall keeps user content
- GIVEN a `.bashrc` with user lines and the managed block
- WHEN `install.sh --uninstall-shell-init` runs
- THEN only the block is removed

### Requirement: Safe execution under `irm | iex`

`install.ps1` MUST run its body in a child scope so functions, strict mode
and `$ErrorActionPreference` do not leak into the user's session, and MUST
NOT call `exit` unless run as a file (only `$LASTEXITCODE` is set), so the
user's PowerShell window stays open.

#### Scenario: Interactive one-liner
- GIVEN `irm .../install.ps1 | iex` in PowerShell 5.1
- WHEN a tool fails
- THEN the window stays open and `$LASTEXITCODE` is non-zero

### Requirement: Summary and exit status

Each run MUST end with a table of tool, status (`installed`, `would install`,
`already present`, `skipped`, `failed`) and detail, warn when the bin
directory is not on `PATH`, and exit non-zero when any tool failed.

#### Scenario: Partial failure
- GIVEN npm is unreachable
- WHEN the script runs
- THEN the summary marks `bruno` failed, the others succeed, and the exit code is 1

### Requirement: CI coverage

CI MUST run shellcheck on `install.sh` (as POSIX sh) and
`update-manifests.sh`, PSScriptAnalyzer at Warning/Error on `install.ps1`,
and dry runs on Linux, macOS, PowerShell 7 and Windows PowerShell 5.1
(including the scriptblock form and `--only` with the `bru` alias).

#### Scenario: Lint regression
- GIVEN a change that introduces a PSScriptAnalyzer warning
- WHEN CI runs
- THEN the `Tools profile (windows-latest)` job fails
