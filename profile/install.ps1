<#
.SYNOPSIS
nxgterm tools profile for Windows: installs Yazi, zoxide, ngmux, Bruno CLI (bru) and curl.

.DESCRIPTION
Uses winget, or scoop when winget is unavailable. zoxide and Yazi fall back to
their official GitHub release zips (SHA-256 verified) when neither exists, e.g.
on Windows Server 2016. Gives Yazi the Catppuccin Mocha flavor (nxgterm's
default theme) without touching an existing theme.toml. Optional and
idempotent: installed tools are skipped.
Works on Windows PowerShell 5.1 and PowerShell 7.

  irm https://raw.githubusercontent.com/NexuraGrid/nxgterm/main/profile/install.ps1 | iex
  & ([scriptblock]::Create((irm https://raw.githubusercontent.com/NexuraGrid/nxgterm/main/profile/install.ps1))) -Yes

.PARAMETER DryRun
Print what would be done; change nothing.
.PARAMETER Yes
Do not ask; answer yes (Node.js, Git, installers).
.PARAMETER Only
Only these tools (comma separated or an array): curl, zoxide, yazi, yazi-theme, ngmux, bruno (alias bru).
yazi-theme is selected whenever yazi is, unless skipped.
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
    $AllTools = @('curl', 'zoxide', 'yazi', 'yazi-theme', 'ngmux', 'bruno')
    $NgmuxInstallerUrl = 'https://raw.githubusercontent.com/NexuraGrid/ng_mux/main/install.ps1'
    $BrunoNpmPackage = '@usebruno/cli'
    # The official Catppuccin Mocha flavor (MIT), pinned to a commit of
    # yazi-rs/flavors; every file is checked against its SHA-256. A plain
    # download, so neither git nor `ya pkg` is needed.
    $FlavorName = 'catppuccin-mocha'
    $FlavorCommit = '1183892c904f7f0efdf4473e856ed308b7bea98d'
    $FlavorBaseUrl = "https://raw.githubusercontent.com/yazi-rs/flavors/$FlavorCommit/$FlavorName.yazi"
    $FlavorFiles = [ordered]@{
        'flavor.toml'     = 'd4417565d5a15110e66369c88385f7176586b03b3dbb396a383c768cc767a80e'
        'tmtheme.xml'     = '395566f08ceb301b936b91c077690ef94f7aeb651b121553f201c99c2bd4aa77'
        'LICENSE'         = '06a2b04a7ed4f030a87d10b884fc1a2215c5e91b371f69dfe173448e834f3752'
        'LICENSE-tmtheme' = '814096d2c34cc216c624738a49356f32b7237733b4f7edb0685f4e50ef5074ba'
    }
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
        git    = @{ winget = 'Git.Git'; scoop = 'git' }
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
  -Yes                 Do not ask; answer yes (Node.js, Git, installers)
  -Only <list>         Only these tools (comma separated)
  -Skip <list>         Skip these tools (comma separated)
  -NoShellInit         Do not touch the PowerShell profiles
  -UninstallShellInit  Remove the nxgterm block from the PowerShell profiles

Tools: curl, zoxide, yazi, yazi-theme, ngmux, bruno (alias: bru).
yazi-theme installs Yazi's Catppuccin Mocha flavor and, only when no
theme.toml exists, creates one that enables it. It is selected whenever yazi
is (skip it with -Skip yazi-theme). Yazi's config directory is
%AppData%\yazi\config, or YAZI_CONFIG_HOME.
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

    # yazi-theme follows yazi unless it is named in -Only/-Skip.
    function Test-Selected([string]$Tool) {
        if ($Tool -eq 'yazi-theme') {
            if ($State.SkipList -contains $Tool) { return $false }
            if ($State.OnlyList -contains $Tool) { return $true }
            return (Test-Selected 'yazi')
        }
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
        Register-YaziFile
    }

    # file.exe from Git for Windows. Git puts only cmd\ on PATH, not usr\bin\.
    function Find-GitFile {
        $roots = New-Object System.Collections.ArrayList
        $git = Get-CommandPath 'git.exe'
        if ($git) {
            # <root>\cmd\git.exe, <root>\bin\git.exe or <root>\mingw64\bin\git.exe
            $dir = Split-Path -Parent $git
            for ($i = 0; $i -lt 3 -and $dir; $i++) {
                $dir = Split-Path -Parent $dir
                if ($dir) { [void]$roots.Add($dir) }
            }
        }
        $scoop = $env:SCOOP
        if (-not $scoop) { $scoop = Join-Path $env:USERPROFILE 'scoop' }
        foreach ($root in @(
                $(if ($env:ProgramFiles) { Join-Path $env:ProgramFiles 'Git' }),
                $(if (${env:ProgramFiles(x86)}) { Join-Path ${env:ProgramFiles(x86)} 'Git' }),
                (Join-Path $env:LOCALAPPDATA 'Programs\Git'),
                (Join-Path $scoop 'apps\git\current'))) {
            if ($root) { [void]$roots.Add($root) }
        }
        foreach ($root in $roots) {
            $file = Join-Path $root 'usr\bin\file.exe'
            if (Test-Path -LiteralPath $file) { return $file }
        }
        return ''
    }

    # Yazi detects file types with file(1), which Windows lacks: without it
    # previews fail with "Cannot find `file`". Point YAZI_FILE_ONE at the one
    # Git for Windows ships, installing Git when it is missing.
    function Register-YaziFile {
        if ($env:YAZI_FILE_ONE -and (Test-Path -LiteralPath $env:YAZI_FILE_ONE)) {
            Add-Result 'file' 'already present' "YAZI_FILE_ONE=$env:YAZI_FILE_ONE"
            return
        }
        if (Test-Command 'file.exe') {
            Add-Result 'file' 'already present' (Get-CommandPath 'file.exe')
            return
        }
        $file = Find-GitFile
        if (-not $file) {
            $manual = 'install Git for Windows (https://git-scm.com) and rerun'
            if (-not $State.PM) {
                Add-Result 'file' 'skipped' "Yazi previews need file.exe; $manual"
                return
            }
            if (-not (Confirm-Action "Yazi needs file.exe from Git for Windows to preview files. Install Git ($($Packages.git[$State.PM])) with $($State.PM)?")) {
                Add-Result 'file' 'skipped' "Git declined; Yazi previews need file.exe; $manual"
                return
            }
            if (-not (Install-ToolPackage 'git')) {
                Add-Result 'file' 'failed' 'could not install Git for Windows'
                return
            }
            if ($DryRun) {
                Write-Step '[dry-run] would set the user variable YAZI_FILE_ONE to Git''s usr\bin\file.exe'
                Add-Result 'file' 'would install' 'Git for Windows, YAZI_FILE_ONE'
                return
            }
            $file = Find-GitFile
            if (-not $file) {
                Add-Result 'file' 'failed' 'Git installed but usr\bin\file.exe was not found; set YAZI_FILE_ONE to it'
                return
            }
        }
        if ($DryRun) {
            Write-Step "[dry-run] would set the user variable YAZI_FILE_ONE=$file"
            Add-Result 'file' 'would install' "YAZI_FILE_ONE=$file"
            return
        }
        [Environment]::SetEnvironmentVariable('YAZI_FILE_ONE', $file, 'User')
        $env:YAZI_FILE_ONE = $file
        Add-Result 'file' 'installed' "YAZI_FILE_ONE=$file (open a new terminal)"
    }

    # Where Yazi reads its configuration (as Yazi resolves it).
    function Get-YaziConfigDir {
        if ($env:YAZI_CONFIG_HOME -and [IO.Path]::IsPathRooted($env:YAZI_CONFIG_HOME)) { return $env:YAZI_CONFIG_HOME }
        return (Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'yazi\config')
    }

    # Downloads the pinned flavor files, verifies each SHA-256 and only then
    # copies them into $Dest.
    function Install-YaziFlavor([string]$Dest) {
        if ($DryRun) {
            Write-Step "[dry-run] download $FlavorName.yazi (yazi-rs/flavors@$($FlavorCommit.Substring(0, 7))), verify its SHA-256 sums, install it into $Dest"
            return
        }
        Enable-Tls12
        $dir = Join-Path (Get-WorkDir) 'flavor'
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
        Write-Step "+ download $FlavorBaseUrl/"
        foreach ($name in $FlavorFiles.Keys) {
            $file = Join-Path $dir $name
            Invoke-WebRequest -Uri "$FlavorBaseUrl/$name" -OutFile $file -UseBasicParsing
            $got = (Get-FileHash -Algorithm SHA256 -Path $file).Hash.ToLowerInvariant()
            $want = $FlavorFiles[$name]
            if ($got -ne $want) { throw "checksum mismatch for $name (expected $want, got $got)" }
        }
        Write-Step "  sha256 ok: $(@($FlavorFiles.Keys) -join ', ')"
        New-Item -ItemType Directory -Force -Path $Dest | Out-Null
        foreach ($name in $FlavorFiles.Keys) {
            Copy-Item -Force -Path (Join-Path $dir $name) -Destination (Join-Path $Dest $name)
        }
        Write-Step "  installed $Dest"
    }

    # The Catppuccin Mocha flavor, plus a theme.toml that enables it only when
    # the user has none (an existing one is never changed).
    function Install-YaziTheme {
        $conf = Get-YaziConfigDir
        $flavorDir = Join-Path $conf "flavors\$FlavorName.yazi"
        $theme = Join-Path $conf 'theme.toml'
        $didFlavor = $false
        if (Test-Path -LiteralPath (Join-Path $flavorDir 'flavor.toml')) {
            Write-Step "  flavor already present: $flavorDir"
        } else {
            try {
                Install-YaziFlavor $flavorDir
                $didFlavor = $true
            } catch {
                Write-Warn $_.Exception.Message
                Add-Result 'yazi-theme' 'failed' "could not install the $FlavorName flavor"
                return
            }
        }
        if (Test-Path -LiteralPath $theme) {
            if (Select-String -LiteralPath $theme -SimpleMatch -Pattern "`"$FlavorName`"" -Quiet) {
                $status = 'already present'
                if ($didFlavor) { $status = Get-DoneStatus }
                Add-Result 'yazi-theme' $status "$theme uses $FlavorName"
                return
            }
            Write-Step "  $theme exists; left untouched. To use the flavor, add:"
            Write-Step '    [flavor]'
            Write-Step "    dark = `"$FlavorName`""
            Add-Result 'yazi-theme' 'skipped' "$theme exists; add: [flavor] dark = `"$FlavorName`""
            return
        }
        if ($DryRun) {
            Write-Step "[dry-run] would create $theme with [flavor] dark = `"$FlavorName`""
            Add-Result 'yazi-theme' 'would install' "$FlavorName flavor + $theme"
            return
        }
        $text = "# Added by the nxgterm tools profile: Catppuccin Mocha, like nxgterm.`n[flavor]`ndark = `"$FlavorName`"`n"
        $bytes = (New-Object System.Text.UTF8Encoding($false)).GetBytes($text)
        try {
            # CreateNew: never overwrite a theme.toml that appeared meanwhile.
            $stream = [IO.File]::Open($theme, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write)
            try { $stream.Write($bytes, 0, $bytes.Length) } finally { $stream.Dispose() }
            Write-Step "  created $theme"
            Add-Result 'yazi-theme' 'installed' "$FlavorName flavor + $theme"
        } catch {
            Write-Warn $_.Exception.Message
            Add-Result 'yazi-theme' 'failed' "could not create $theme"
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
        Write-Step ('{0,-11} {1,-16} {2}' -f 'TOOL', 'STATUS', 'DETAIL')
        foreach ($row in $State.Results) {
            Write-Step ('{0,-11} {1,-16} {2}' -f $row.Tool, $row.Status, $row.Detail)
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
                    'yazi-theme' { Install-YaziTheme }
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
