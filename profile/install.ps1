<#
.SYNOPSIS
nxgterm tools profile for Windows: installs Yazi, zoxide, ngmux, Bruno CLI (bru) and curl.

.DESCRIPTION
Uses winget, or scoop when winget is unavailable. zoxide and Yazi fall back to
their official GitHub release zips (SHA-256 verified) when neither exists, e.g.
on Windows Server 2016. Optional and idempotent: installed tools are skipped.
Works on Windows PowerShell 5.1 and PowerShell 7.

  irm https://raw.githubusercontent.com/NexuraGrid/nxgterm/main/profile/install.ps1 | iex
  & ([scriptblock]::Create((irm https://raw.githubusercontent.com/NexuraGrid/nxgterm/main/profile/install.ps1))) -Yes

.PARAMETER DryRun
Print what would be done; change nothing.
.PARAMETER Yes
Do not ask; answer yes (Node.js, installers).
.PARAMETER Only
Only these tools (comma separated or an array): curl, zoxide, yazi, ngmux, bruno (alias bru).
.PARAMETER Skip
Skip these tools.
.PARAMETER NoShellInit
Do not touch the PowerShell profiles.
.PARAMETER UninstallShellInit
Remove the nxgterm block from the PowerShell profiles and stop.
#>
# Write-Host is deliberate: installer messages must not end up in the pipeline.
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingWriteHost', '', Justification = 'Interactive installer output')]
# The parameters are read inside the child scope below, which the rule misses.
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSReviewUnusedParameter', '', Justification = 'Used in the child scope')]
[CmdletBinding()]
param(
    [switch]$DryRun,
    [switch]$Yes,
    [string[]]$Only = @(),
    [string[]]$Skip = @(),
    [switch]$NoShellInit,
    [switch]$UninstallShellInit,
    [switch]$Help,
    [switch]$Version
)

# Run as a file, `exit` sets the exit code. From `irm | iex` or a scriptblock
# `exit` would close the user's PowerShell window, so only LASTEXITCODE is set.
$nxgRunAsFile = [bool]$MyInvocation.MyCommand.Path

# Everything else runs in a child scope so that, under `irm | iex`, functions,
# strict mode and $ErrorActionPreference do not leak into the user's session.
$nxgExitCode = & {
    Set-StrictMode -Version 2.0
    $ErrorActionPreference = 'Stop'

    $ProfileVersion = '0.1.0'
    $AllTools = @('curl', 'zoxide', 'yazi', 'ngmux', 'bruno')
    $NgmuxInstallerUrl = 'https://raw.githubusercontent.com/NexuraGrid/ng_mux/main/install.ps1'
    $BrunoNpmPackage = '@usebruno/cli'
    $MarkBegin = '# >>> nxgterm profile >>>'
    $MarkEnd = '# <<< nxgterm profile <<<'
    # A custom NXGTERM_PROFILE_BIN_DIR is left off PATH (the user manages it).
    $BinDir = $env:NXGTERM_PROFILE_BIN_DIR
    $ManagePath = -not $BinDir
    if (-not $BinDir) { $BinDir = Join-Path $env:LOCALAPPDATA 'Programs\nxgterm-profile\bin' }
    # Testing knobs: release binaries even when a package manager exists, and
    # another directory in place of Documents for the PowerShell profiles.
    $ForceBinary = ($env:NXGTERM_PROFILE_FORCE_BINARY -eq '1')
    $DocumentsDir = $env:NXGTERM_PROFILE_DOCUMENTS
    if (-not $DocumentsDir) { $DocumentsDir = [Environment]::GetFolderPath('MyDocuments') }

    # Package ids, verified against microsoft/winget-pkgs and ScoopInstaller/Main.
    $Packages = @{
        curl   = @{ winget = 'cURL.cURL'; scoop = 'curl' }
        zoxide = @{ winget = 'ajeetdsouza.zoxide'; scoop = 'zoxide' }
        yazi   = @{ winget = 'sxyazi.yazi'; scoop = 'yazi' }
        node   = @{ winget = 'OpenJS.NodeJS.LTS'; scoop = 'nodejs-lts' }
    }

    $State = @{
        Results  = (New-Object System.Collections.ArrayList)
        Failed   = $false
        WorkDir  = $null
        PM       = ''
        OnlyList = @()
        SkipList = @()
    }

    function Show-Usage {
        @'
nxgterm tools profile: installs Yazi, zoxide, ngmux, Bruno CLI (bru) and curl.

Usage: install.ps1 [-DryRun] [-Yes] [-Only <list>] [-Skip <list>]
                   [-NoShellInit] [-UninstallShellInit] [-Help] [-Version]

  -DryRun              Print what would be done; change nothing
  -Yes                 Do not ask; answer yes (Node.js, installers)
  -Only <list>         Only these tools (comma separated)
  -Skip <list>         Skip these tools (comma separated)
  -NoShellInit         Do not touch the PowerShell profiles
  -UninstallShellInit  Remove the nxgterm block from the PowerShell profiles

Tools: curl, zoxide, yazi, ngmux, bruno (alias: bru).
With irm | iex, pass options through a scriptblock:
  & ([scriptblock]::Create((irm <url>/install.ps1))) -Yes -Skip bruno
'@ | Write-Host
    }

    function Write-Step([string]$Text) { Write-Host $Text }
    function Write-Warn([string]$Text) { Write-Host "warning: $Text" -ForegroundColor Yellow }

    function ConvertTo-ToolList([string[]]$Items) {
        $out = @()
        foreach ($item in $Items) {
            foreach ($name in ($item -split '[,\s]+')) {
                if (-not $name) { continue }
                $name = $name.ToLowerInvariant()
                if ($name -eq 'bru') { $name = 'bruno' }
                if ($AllTools -notcontains $name) {
                    throw "unknown tool '$name' (expected: $($AllTools -join ', '))"
                }
                $out += $name
            }
        }
        return , $out
    }

    function Test-Selected([string]$Tool) {
        if ($State.OnlyList.Count -gt 0 -and $State.OnlyList -notcontains $Tool) { return $false }
        return ($State.SkipList -notcontains $Tool)
    }

    function Get-DoneStatus { if ($DryRun) { 'would install' } else { 'installed' } }

    function Add-Result([string]$Tool, [string]$Status, [string]$Detail) {
        [void]$State.Results.Add([pscustomobject]@{ Tool = $Tool; Status = $Status; Detail = $Detail })
        if ($Status -eq 'failed') { $State.Failed = $true }
    }

    # Yes with -Yes; in a dry run the question is only shown.
    function Confirm-Action([string]$Question) {
        if ($Yes) { return $true }
        if ($DryRun) {
            Write-Step "[dry-run] would ask: $Question [y/N]"
            return $true
        }
        try {
            $answer = Read-Host "$Question [y/N]"
        } catch {
            Write-Warn "cannot ask '$Question' in a non-interactive session; rerun with -Yes to accept"
            return $false
        }
        return ($answer -match '^(y|yes)$')
    }

    # Prints a native command, then runs it unless -DryRun. True on exit code 0.
    function Invoke-Native([string]$Exe, [string[]]$Arguments) {
        $line = "$Exe $($Arguments -join ' ')"
        if ($DryRun) {
            Write-Step "[dry-run] $line"
            return $true
        }
        Write-Step "+ $line"
        & $Exe @Arguments | Out-Host
        return ($LASTEXITCODE -eq 0)
    }

    function Test-Command([string]$Name) {
        return [bool](Get-Command $Name -ErrorAction SilentlyContinue)
    }

    function Get-CommandPath([string]$Name) {
        $cmd = Get-Command $Name -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($cmd) { return $cmd.Source }
        return ''
    }

    # Picks up PATH changes made by installers in this session.
    function Sync-SessionPath {
        $machine = [Environment]::GetEnvironmentVariable('Path', 'Machine')
        $user = [Environment]::GetEnvironmentVariable('Path', 'User')
        $env:Path = (@($machine, $user, $BinDir) | Where-Object { $_ }) -join ';'
    }

    function Get-PackageManager {
        if (Test-Command 'winget.exe') { return 'winget' }
        if (Test-Command 'scoop') { return 'scoop' }
        return ''
    }

    function Install-ToolPackage([string]$Key) {
        $ids = $Packages[$Key]
        switch ($State.PM) {
            'winget' {
                $ok = Invoke-Native 'winget.exe' @('install', '--id', $ids.winget, '--exact', '--source', 'winget',
                    '--accept-package-agreements', '--accept-source-agreements')
            }
            'scoop' {
                $ok = Invoke-Native 'scoop' @('install', $ids.scoop)
            }
            default { return $false }
        }
        if ($ok -and -not $DryRun) { Sync-SessionPath }
        return $ok
    }

    function Get-WorkDir {
        if (-not $State.WorkDir) {
            $State.WorkDir = Join-Path ([IO.Path]::GetTempPath()) ("nxgterm-profile-" + [guid]::NewGuid().ToString('N'))
            New-Item -ItemType Directory -Force -Path $State.WorkDir | Out-Null
        }
        return $State.WorkDir
    }

    function Enable-Tls12 {
        # Windows PowerShell 5.1 on older systems defaults to TLS 1.0.
        try {
            [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
        } catch {
            Write-Verbose "could not enable TLS 1.2: $($_.Exception.Message)"
        }
    }

    function Get-Arch {
        switch ($env:PROCESSOR_ARCHITECTURE) {
            'AMD64' { return 'x86_64' }
            'ARM64' { return 'aarch64' }
            default { return '' }
        }
    }

    # Downloads the latest release asset of $Repo, verifies the SHA-256 that
    # GitHub publishes for it and copies $Binaries into $BinDir (added to the
    # user PATH).
    function Install-GitHubBinary([string]$Repo, [string]$Pattern, [string[]]$Binaries) {
        if ($DryRun) {
            Write-Step "[dry-run] download the latest $Repo release asset ($Pattern), verify its SHA-256, install $($Binaries -join ', ') into $BinDir and add it to the user PATH"
            return 'latest'
        }
        Enable-Tls12
        $headers = @{ 'User-Agent' = 'nxgterm-profile' }
        if ($env:GITHUB_TOKEN) { $headers['Authorization'] = "Bearer $($env:GITHUB_TOKEN)" }
        $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -Headers $headers
        $tag = [string]$release.tag_name
        $assetName = $Pattern.Replace('{tag}', $tag).Replace('{ver}', $tag.TrimStart('v'))
        $asset = $release.assets | Where-Object { $_.name -eq $assetName } | Select-Object -First 1
        if (-not $asset) { throw "$Repo $tag has no asset $assetName" }
        $digest = ''
        if ($asset.PSObject.Properties['digest'] -and $asset.digest) { $digest = [string]$asset.digest }
        if ($digest -notmatch '^sha256:([0-9a-f]{64})$') {
            throw "$Repo $tag publishes no SHA-256 for $assetName; refusing to install it unverified"
        }
        $want = $Matches[1]
        $dir = Join-Path (Get-WorkDir) ($Repo -replace '/', '_')
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
        $zip = Join-Path $dir $assetName
        Write-Step "+ download $($asset.browser_download_url)"
        Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip -UseBasicParsing
        $got = (Get-FileHash -Algorithm SHA256 -Path $zip).Hash.ToLowerInvariant()
        if ($got -ne $want) { throw "checksum mismatch for $assetName (expected $want, got $got)" }
        Write-Step "  sha256 ok: $got"
        $extract = Join-Path $dir 'x'
        Expand-Archive -Path $zip -DestinationPath $extract -Force
        New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
        foreach ($bin in $Binaries) {
            $file = Get-ChildItem -Path $extract -Recurse -File -Filter $bin | Select-Object -First 1
            if (-not $file) { throw "$bin not found in $assetName" }
            Copy-Item -Force -Path $file.FullName -Destination (Join-Path $BinDir $bin)
            Write-Step "  installed $(Join-Path $BinDir $bin)"
        }
        if ($ManagePath) {
            Add-UserPath $BinDir
        } else {
            Write-Step "  note: add $BinDir to PATH yourself (custom NXGTERM_PROFILE_BIN_DIR)"
        }
        return $tag
    }

    function Add-UserPath([string]$Dir) {
        $current = [Environment]::GetEnvironmentVariable('Path', 'User')
        $want = $Dir.TrimEnd('\')
        foreach ($entry in (@($current -split ';') | Where-Object { $_ })) {
            if ($entry.TrimEnd('\') -ieq $want) { return }
        }
        $new = (@($current, $Dir) | Where-Object { $_ }) -join ';'
        [Environment]::SetEnvironmentVariable('Path', $new, 'User')
        Write-Step "  added $Dir to the user PATH"
        Sync-SessionPath
    }

    function Test-Installed([string]$Exe) {
        return (Test-Command $Exe) -or (Test-Path -LiteralPath (Join-Path $BinDir $Exe))
    }

    function Get-InstalledPath([string]$Exe) {
        $path = Get-CommandPath $Exe
        if ($path) { return $path }
        return (Join-Path $BinDir $Exe)
    }

    # --- tools ------------------------------------------------------------------

    function Install-Curl {
        # Windows 10 1803+ and Server 2019+ ship curl.exe; `curl` alone may be the
        # Invoke-WebRequest alias in Windows PowerShell.
        if (Test-Command 'curl.exe') {
            Add-Result 'curl' 'already present' (Get-CommandPath 'curl.exe')
        } elseif ($State.PM -and (Install-ToolPackage 'curl')) {
            Add-Result 'curl' (Get-DoneStatus) $State.PM
        } else {
            Add-Result 'curl' 'failed' 'install winget or scoop, or curl from https://curl.se/windows/'
        }
    }

    function Install-WithFallback([string]$Tool, [string]$Exe, [string]$Repo, [string]$Pattern, [string[]]$Binaries) {
        if (Test-Installed $Exe) {
            Add-Result $Tool 'already present' (Get-InstalledPath $Exe)
            return
        }
        if ($State.PM -and -not $ForceBinary) {
            if (Install-ToolPackage $Tool) {
                Add-Result $Tool (Get-DoneStatus) $State.PM
                return
            }
            Write-Warn "${Tool}: $($State.PM) failed; trying the GitHub release"
        }
        $arch = Get-Arch
        if (-not $arch) {
            Add-Result $Tool 'failed' "no release binary for $($env:PROCESSOR_ARCHITECTURE)"
            return
        }
        try {
            $tag = Install-GitHubBinary $Repo ($Pattern.Replace('{arch}', $arch)) $Binaries
            Add-Result $Tool (Get-DoneStatus) "GitHub release $tag -> $BinDir"
        } catch {
            Write-Warn $_.Exception.Message
            Add-Result $Tool 'failed' 'see the messages above'
        }
    }

    function Install-Zoxide {
        Install-WithFallback 'zoxide' 'zoxide.exe' 'ajeetdsouza/zoxide' 'zoxide-{ver}-{arch}-pc-windows-msvc.zip' @('zoxide.exe')
    }

    function Install-Yazi {
        Install-WithFallback 'yazi' 'yazi.exe' 'sxyazi/yazi' 'yazi-{arch}-pc-windows-msvc.zip' @('yazi.exe', 'ya.exe')
        if (-not (Test-Command 'git.exe')) {
            Write-Step "  note: Yazi uses file.exe from Git for Windows to detect file types (winget install Git.Git)"
        }
    }

    function Test-NgmuxInstalled {
        if (Test-Command 'ngmux.exe') { return $true }
        $candidates = @(
            (Join-Path $env:LOCALAPPDATA 'Programs\ngmux\ngmux.exe'),
            (Join-Path $env:ProgramFiles 'ngmux\ngmux.exe')
        )
        foreach ($candidate in $candidates) {
            if (Test-Path -LiteralPath $candidate) { return $true }
        }
        return $false
    }

    function Install-Ngmux {
        if (Test-NgmuxInstalled) {
            Add-Result 'ngmux' 'already present' (Get-InstalledPath 'ngmux.exe')
            return
        }
        Write-Step 'ngmux is installed with its official installer:'
        Write-Step "  $NgmuxInstallerUrl"
        if ($DryRun) {
            Write-Step '[dry-run] download the installer, show its size and SHA-256, then run it'
            Add-Result 'ngmux' 'would install' 'official installer'
            return
        }
        try {
            Enable-Tls12
            $script = Join-Path (Get-WorkDir) 'ngmux-install.ps1'
            Invoke-WebRequest -Uri $NgmuxInstallerUrl -OutFile $script -UseBasicParsing
            $text = [IO.File]::ReadAllText($script)
            $lines = @($text -split "`n").Count
            $hash = (Get-FileHash -Algorithm SHA256 -Path $script).Hash.ToLowerInvariant()
            Write-Step "  downloaded $lines lines (sha256 $hash)"
            if (-not (Confirm-Action 'Run the ngmux installer now?')) {
                Add-Result 'ngmux' 'skipped' 'installer declined'
                return
            }
            Write-Step "+ & $script"
            & ([scriptblock]::Create($text)) | Out-Host
            Sync-SessionPath
            Add-Result 'ngmux' 'installed' 'official installer'
        } catch {
            Write-Warn $_.Exception.Message
            Add-Result 'ngmux' 'failed' 'the ngmux installer failed'
        }
    }

    function Install-Bruno {
        if (Test-Installed 'bru.cmd') {
            Add-Result 'bruno' 'already present' (Get-InstalledPath 'bru.cmd')
            return
        }
        # npm.cmd, not npm: npm.ps1 is blocked by the default execution policy.
        if (-not (Test-Command 'node.exe') -or -not (Test-Command 'npm.cmd')) {
            if (-not $State.PM) {
                Add-Result 'bruno' 'skipped' 'Node.js is missing; install it (https://nodejs.org) and rerun'
                return
            }
            if (-not (Confirm-Action "Bruno CLI needs Node.js. Install Node.js LTS ($($Packages.node[$State.PM])) with $($State.PM)?")) {
                Write-Step 'Skipping Bruno CLI: Node.js was not installed.'
                Add-Result 'bruno' 'skipped' 'Node.js declined'
                return
            }
            if (-not (Install-ToolPackage 'node')) {
                Add-Result 'bruno' 'failed' 'could not install Node.js'
                return
            }
            if (-not $DryRun -and -not (Test-Command 'npm.cmd')) {
                Add-Result 'bruno' 'failed' 'Node.js installed but npm is not on PATH yet; open a new shell and rerun'
                return
            }
        }
        if (Invoke-Native 'npm.cmd' @('install', '-g', $BrunoNpmPackage)) {
            Add-Result 'bruno' (Get-DoneStatus) 'npm (global)'
        } else {
            Add-Result 'bruno' 'failed' "npm install $BrunoNpmPackage failed"
        }
    }

    # --- shell integration --------------------------------------------------------

    function Get-ProfilePath {
        $docs = $DocumentsDir
        $paths = @(Join-Path $docs 'WindowsPowerShell\profile.ps1')
        $pwshDir = Join-Path $docs 'PowerShell'
        if ((Test-Command 'pwsh.exe') -or (Test-Path -LiteralPath $pwshDir)) {
            $paths += (Join-Path $pwshDir 'profile.ps1')
        }
        return $paths
    }

    function Get-ProfileBlock {
        $lines = @(
            $MarkBegin,
            '# Added by the nxgterm tools profile. Remove with: install.ps1 -UninstallShellInit'
        )
        if (Test-Selected 'zoxide') {
            $lines += @(
                'if (Get-Command zoxide -ErrorAction SilentlyContinue) {',
                '    Invoke-Expression (& { (zoxide init powershell | Out-String) })',
                '}'
            )
        }
        if (Test-Selected 'yazi') {
            $lines += @(
                '# Yazi: `y` changes to the directory Yazi was in when it exits.',
                'function y {',
                '    $tmp = (New-TemporaryFile).FullName',
                '    yazi.exe @args --cwd-file="$tmp"',
                '    $cwd = Get-Content -Path $tmp -Encoding UTF8',
                '    if ($cwd -and $cwd -ne $PWD.Path -and (Test-Path -LiteralPath $cwd -PathType Container)) {',
                '        Set-Location -LiteralPath (Resolve-Path -LiteralPath $cwd).Path',
                '    }',
                '    Remove-Item -Path $tmp',
                '}'
            )
        }
        $lines += $MarkEnd
        return $lines
    }

    # The lines of $Path without the managed block, and whether it has a BOM.
    function Read-ProfileContent([string]$Path) {
        $result = @{ Lines = @(); Bom = $false; Exists = $false }
        if (-not (Test-Path -LiteralPath $Path)) { return $result }
        $result.Exists = $true
        $bytes = [IO.File]::ReadAllBytes($Path)
        $result.Bom = ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF)
        $text = [IO.File]::ReadAllText($Path)
        $skip = $false
        $kept = New-Object System.Collections.ArrayList
        foreach ($line in ($text -split "`r?`n")) {
            if ($line -eq $MarkBegin) { $skip = $true; continue }
            if ($line -eq $MarkEnd) { $skip = $false; continue }
            if (-not $skip) { [void]$kept.Add($line) }
        }
        # Drop trailing blank lines (and the empty string after the final newline).
        while ($kept.Count -gt 0 -and $kept[$kept.Count - 1].Trim() -eq '') { $kept.RemoveAt($kept.Count - 1) }
        $result.Lines = @($kept)
        return $result
    }

    function Write-ProfileText([string]$Path, [string[]]$Lines, [bool]$Bom) {
        $text = ''
        if ($Lines.Count -gt 0) { $text = ($Lines -join "`r`n") + "`r`n" }
        $dir = Split-Path -Parent $Path
        if (-not (Test-Path -LiteralPath $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
        [IO.File]::WriteAllText($Path, $text, (New-Object System.Text.UTF8Encoding($Bom)))
    }

    function Write-ProfileBlock([string]$Path) {
        if ($DryRun) {
            Write-Step "[dry-run] would write the nxgterm block (zoxide init, y wrapper) to $Path"
            return
        }
        $current = Read-ProfileContent $Path
        $lines = @($current.Lines)
        if ($lines.Count -gt 0) { $lines += '' }
        $lines += Get-ProfileBlock
        $newText = ($lines -join "`r`n") + "`r`n"
        if ($current.Exists -and ([IO.File]::ReadAllText($Path) -eq $newText)) {
            Write-Step "  ${Path}: up to date"
            return
        }
        Write-ProfileText $Path $lines $current.Bom
        Write-Step "  ${Path}: updated"
    }

    function Clear-ProfileBlock([string]$Path) {
        if (-not (Test-Path -LiteralPath $Path)) { return }
        if (-not (Select-String -LiteralPath $Path -SimpleMatch -Pattern $MarkBegin -Quiet)) { return }
        if ($DryRun) {
            Write-Step "[dry-run] would remove the nxgterm block from $Path"
            return
        }
        $current = Read-ProfileContent $Path
        Write-ProfileText $Path $current.Lines $current.Bom
        Write-Step "  cleaned $Path"
    }

    function Initialize-Shell {
        if (-not (Test-Selected 'zoxide') -and -not (Test-Selected 'yazi')) { return }
        Write-Step ''
        Write-Step 'Shell integration:'
        foreach ($path in Get-ProfilePath) {
            try { Write-ProfileBlock $path } catch { Write-Warn "could not update ${path}: $($_.Exception.Message)" }
        }
        $policy = Get-ExecutionPolicy
        if ($policy -eq 'Restricted' -or $policy -eq 'AllSigned') {
            Write-Step "  note: the execution policy is $policy, so profiles do not load. To allow them:"
            Write-Step '    Set-ExecutionPolicy -Scope CurrentUser RemoteSigned'
        }
    }

    function Uninstall-Shell {
        Write-Step 'Removing the nxgterm block from the PowerShell profiles:'
        $docs = $DocumentsDir
        foreach ($path in @((Join-Path $docs 'WindowsPowerShell\profile.ps1'), (Join-Path $docs 'PowerShell\profile.ps1'))) {
            try { Clear-ProfileBlock $path } catch { Write-Warn "could not clean ${path}: $($_.Exception.Message)" }
        }
    }

    # --- main ---------------------------------------------------------------------

    function Show-Summary {
        Write-Step ''
        Write-Step 'Summary'
        Write-Step ('{0,-8} {1,-16} {2}' -f 'TOOL', 'STATUS', 'DETAIL')
        foreach ($row in $State.Results) {
            Write-Step ('{0,-8} {1,-16} {2}' -f $row.Tool, $row.Status, $row.Detail)
        }
    }

    function Invoke-Profile {
        if ($Help) { Show-Usage; return 0 }
        if ($Version) { Write-Host "nxgterm profile $ProfileVersion"; return 0 }
        try {
            $State.OnlyList = ConvertTo-ToolList $Only
            $State.SkipList = ConvertTo-ToolList $Skip
        } catch {
            Write-Host "error: $($_.Exception.Message)" -ForegroundColor Red
            return 2
        }
        if ($UninstallShellInit) { Uninstall-Shell; return 0 }

        $State.PM = Get-PackageManager
        $pmText = $State.PM
        if (-not $pmText) { $pmText = 'none (GitHub release binaries only)' }
        Write-Step "nxgterm tools profile $ProfileVersion"
        Write-Step "  system:          Windows $([Environment]::OSVersion.Version) $($env:PROCESSOR_ARCHITECTURE), PowerShell $($PSVersionTable.PSVersion)"
        Write-Step "  package manager: $pmText"
        Write-Step "  binaries:        $BinDir"
        if ($DryRun) { Write-Step '  mode:            dry run (nothing is changed)' }
        Write-Step ''

        foreach ($tool in $AllTools) {
            if (-not (Test-Selected $tool)) {
                Add-Result $tool 'skipped' 'not selected'
                continue
            }
            Write-Step "==> $tool"
            try {
                switch ($tool) {
                    'curl' { Install-Curl }
                    'zoxide' { Install-Zoxide }
                    'yazi' { Install-Yazi }
                    'ngmux' { Install-Ngmux }
                    'bruno' { Install-Bruno }
                }
            } catch {
                Write-Warn $_.Exception.Message
                Add-Result $tool 'failed' 'unexpected error'
            }
        }
        if (-not $NoShellInit) { Initialize-Shell }
        Show-Summary
        if ($State.Failed) { return 1 }
        return 0
    }

    try {
        Invoke-Profile | Select-Object -Last 1
    } finally {
        if ($State.WorkDir -and (Test-Path -LiteralPath $State.WorkDir)) {
            Remove-Item -Recurse -Force -LiteralPath $State.WorkDir -ErrorAction SilentlyContinue
        }
    }
}

if ($nxgRunAsFile) { exit $nxgExitCode }
$global:LASTEXITCODE = $nxgExitCode
