# Installs nxgterm from a GitHub Release on Windows.
# Runs on Windows PowerShell 5.1 (Windows Server 2016 and later) and PowerShell 7.
#
#   irm https://raw.githubusercontent.com/NexuraGrid/nxgterm/main/install.ps1 | iex
#
# By default it downloads the MSI and runs msiexec (Program Files, Start Menu
# shortcut; msiexec asks for elevation). With NXGTERM_PORTABLE=1 it installs
# the portable zip in %LOCALAPPDATA%\Programs\nxgterm instead, adds that
# directory to the user PATH and creates a Start Menu shortcut, without
# Administrator rights. Every download is checked against the release's
# SHA256SUMS before use. `nxgterm update` keeps either install current.
#
# Environment:
#   NXGTERM_VERSION      install this version (e.g. 0.10.1) instead of the latest
#   NXGTERM_INSTALL_URL  directory holding SHA256SUMS and the assets, instead
#                        of the GitHub Release (for testing or a mirror); a
#                        local path also works
#   NXGTERM_PORTABLE     set to 1 for the portable install

# Everything runs inside one scriptblock so `iex` leaves no variables or
# functions behind in the caller's session, and errors end the script
# without closing the caller's window.
& {
    Set-StrictMode -Version 2.0
    $ErrorActionPreference = 'Stop'
    # The progress bar makes Invoke-WebRequest many times slower on 5.1.
    $ProgressPreference = 'SilentlyContinue'

    $repoUrl = 'https://github.com/NexuraGrid/nxgterm'

    function Say([string] $message) { Write-Host "nxgterm: $message" }

    # Downloads $url to the file $dest. Accepts local paths and file:// URLs
    # for a test mirror.
    function Get-Download([string] $url, [string] $dest) {
        try {
            if ($url -match '^https?://') {
                Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $dest
            } else {
                Copy-Item -LiteralPath ($url -replace '^file:///?', '') -Destination $dest
            }
        } catch {
            throw "cannot download ${url}: $($_.Exception.Message)"
        }
    }

    # Checks the file $path (named $name) against the SHA256SUMS file $sums.
    # Lines are `<hash>  <name>`, or `<hash> *<name>` in binary mode.
    function Test-Checksum([string] $path, [string] $name, [string] $sums) {
        $expected = $null
        foreach ($line in Get-Content -LiteralPath $sums) {
            $fields = $line.Trim() -split '\s+', 2
            if ($fields.Count -eq 2 -and $fields[1].TrimStart('*') -eq $name) {
                $expected = $fields[0].ToLowerInvariant()
                break
            }
        }
        if (-not $expected) {
            throw "$name is not listed in SHA256SUMS; nothing was installed"
        }
        $actual = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actual -ne $expected) {
            throw "checksum mismatch for $name (expected $expected, got $actual); nothing was installed"
        }
    }

    # Adds $dir to the user PATH once. The value is read and written as
    # REG_EXPAND_SZ so entries such as %USERPROFILE%\bin keep working.
    function Add-UserPath([string] $dir) {
        $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
        if (-not $key) { $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment') }
        try {
            $current = [string] $key.GetValue('Path', '', 'DoNotExpandEnvironmentNames')
            $entries = @($current -split ';' | Where-Object { $_ -ne '' })
            $known = $entries | Where-Object {
                [Environment]::ExpandEnvironmentVariables($_).TrimEnd('\') -ieq $dir.TrimEnd('\')
            }
            if ($known) { return $false }
            $key.SetValue('Path', (($entries + $dir) -join ';'), 'ExpandString')
        } finally {
            $key.Close()
        }
        # Setting a variable through .NET broadcasts WM_SETTINGCHANGE, so
        # Explorer and new shells see the new PATH without a sign-out.
        [Environment]::SetEnvironmentVariable('NXGTERM_INSTALL_REFRESH', '1', 'User')
        [Environment]::SetEnvironmentVariable('NXGTERM_INSTALL_REFRESH', $null, 'User')
        $env:Path = "$env:Path;$dir"
        return $true
    }

    function Install-Msi([string] $version, [string] $baseUrl, [string] $sums, [string] $tmp) {
        $asset = "nxgterm-$version-x86_64.msi"
        $msi = Join-Path $tmp $asset
        Say "downloading $asset"
        Get-Download "$baseUrl/$asset" $msi
        Test-Checksum $msi $asset $sums
        Say 'checksum verified; starting the installer'
        $process = Start-Process -FilePath 'msiexec.exe' -Wait -PassThru `
            -ArgumentList @('/i', "`"$msi`"", '/passive', '/norestart')
        switch ($process.ExitCode) {
            0 { Say "installed nxgterm $version (Start Menu: nxgterm)" }
            3010 { Say "installed nxgterm $version; restart Windows to finish" }
            1602 { throw 'the installer was cancelled' }
            1618 { throw 'another installation is in progress; try again when it finishes' }
            default { throw "msiexec failed with exit code $($process.ExitCode)" }
        }
    }

    function Install-Portable([string] $version, [string] $baseUrl, [string] $sums, [string] $tmp) {
        $asset = "nxgterm-$version-x86_64-windows.zip"
        $zip = Join-Path $tmp $asset
        Say "downloading $asset"
        Get-Download "$baseUrl/$asset" $zip
        Test-Checksum $zip $asset $sums
        Say 'checksum verified'

        $installDir = Join-Path $env:LOCALAPPDATA 'Programs\nxgterm'
        $parent = Split-Path -Parent $installDir
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
        if ((Test-Path -LiteralPath $installDir) -and
            -not (Test-Path -LiteralPath (Join-Path $installDir 'nxgterm.exe')) -and
            (Get-ChildItem -LiteralPath $installDir -Force | Select-Object -First 1)) {
            throw "$installDir exists and is not an nxgterm install"
        }

        # Extract next to the install directory, so the swap is a rename.
        $staging = Join-Path $parent ("nxgterm.new." + [Guid]::NewGuid().ToString('N'))
        try {
            Expand-Archive -LiteralPath $zip -DestinationPath $staging
            # The zip wraps its files in nxgterm-<v>-x86_64-windows\; install
            # them flat so `nxgterm update` finds them next to the exe.
            $root = Join-Path $staging "nxgterm-$version-x86_64-windows"
            if (-not (Test-Path -LiteralPath (Join-Path $root 'nxgterm.exe'))) {
                throw "$asset has no nxgterm.exe"
            }
            if (Test-Path -LiteralPath $installDir) {
                $old = "$installDir.old"
                if (Test-Path -LiteralPath $old) { Remove-Item -LiteralPath $old -Recurse -Force }
                try {
                    Rename-Item -LiteralPath $installDir -NewName (Split-Path -Leaf $old)
                } catch {
                    throw "cannot replace $installDir; close every nxgterm window and run this again"
                }
                try {
                    Move-Item -LiteralPath $root -Destination $installDir
                } catch {
                    Rename-Item -LiteralPath $old -NewName (Split-Path -Leaf $installDir)
                    throw
                }
                Remove-Item -LiteralPath $old -Recurse -Force -ErrorAction SilentlyContinue
            } else {
                Move-Item -LiteralPath $root -Destination $installDir
            }
        } finally {
            if (Test-Path -LiteralPath $staging) {
                Remove-Item -LiteralPath $staging -Recurse -Force -ErrorAction SilentlyContinue
            }
        }
        Say "installed nxgterm $version in $installDir"

        $exe = Join-Path $installDir 'nxgterm.exe'
        if (Add-UserPath $installDir) {
            Say "added $installDir to your user PATH (open a new shell to use it)"
        }

        # Start Menu shortcut that opens shells in the user's home.
        $programs = [Environment]::GetFolderPath('Programs')
        $link = Join-Path $programs 'nxgterm.lnk'
        $shell = New-Object -ComObject WScript.Shell
        $shortcut = $shell.CreateShortcut($link)
        $shortcut.TargetPath = $exe
        $shortcut.WorkingDirectory = $env:USERPROFILE
        $shortcut.IconLocation = "$exe,0"
        $shortcut.Description = 'nxgterm terminal'
        $shortcut.Save()
        [void] [Runtime.InteropServices.Marshal]::ReleaseComObject($shell)
        Say "added the Start Menu shortcut $link"
    }

    $tmp = $null
    try {
        # Windows PowerShell 5.1 may default to TLS 1.0, which GitHub refuses.
        [Net.ServicePointManager]::SecurityProtocol =
            [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

        $arch = $env:PROCESSOR_ARCHITEW6432
        if (-not $arch) { $arch = $env:PROCESSOR_ARCHITECTURE }
        if ($arch -eq 'ARM64') {
            Say 'note: installing the x86_64 build, which Windows on ARM runs emulated'
        } elseif ($arch -ne 'AMD64') {
            throw "unsupported architecture $arch; nxgterm ships for 64-bit Windows (x86_64)"
        }

        # Strip a leading "v" so both "0.10.1" and "v0.10.1" work.
        $version = ''
        if ($env:NXGTERM_VERSION) { $version = $env:NXGTERM_VERSION.Trim() -replace '^v', '' }
        if ($env:NXGTERM_INSTALL_URL) {
            $baseUrl = $env:NXGTERM_INSTALL_URL.TrimEnd('/', '\')
        } elseif ($version) {
            $baseUrl = "$repoUrl/releases/download/v$version"
        } else {
            $baseUrl = "$repoUrl/releases/latest/download"
        }

        $tmp = Join-Path ([IO.Path]::GetTempPath()) ("nxgterm-install." + [Guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $tmp | Out-Null
        $sums = Join-Path $tmp 'SHA256SUMS'
        Get-Download "$baseUrl/SHA256SUMS" $sums

        # The asset names carry the version: read it from the source
        # tarball's entry, so the latest release needs no GitHub API call.
        if (-not $version) {
            $match = Select-String -LiteralPath $sums -Pattern 'nxgterm-(\S+)-source\.tar\.gz\s*$' |
                Select-Object -First 1
            if (-not $match) { throw "cannot read the version from $baseUrl/SHA256SUMS" }
            $version = $match.Matches[0].Groups[1].Value
        }

        if ($env:NXGTERM_PORTABLE -eq '1') {
            Install-Portable $version $baseUrl $sums $tmp
        } else {
            Install-Msi $version $baseUrl $sums $tmp
        }
        Say 'done; run `nxgterm update` to install new releases'
    } catch {
        Write-Host "nxgterm: error: $($_.Exception.Message)" -ForegroundColor Red
    } finally {
        if ($tmp -and (Test-Path -LiteralPath $tmp)) {
            Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}
