<#
.SYNOPSIS
Downloads Microsoft's standalone ConPTY for the Windows release (zip and MSI).

.DESCRIPTION
Fetches the pinned NuGet package Microsoft.Windows.Console.ConPTY, checks its
SHA-256 and the SHA-256 of the two x64 files it extracts, and writes the layout
that conpty.dll expects beside nxgterm.exe:

  <Destination>\conpty.dll
  <Destination>\x64\OpenConsole.exe

Any mismatch or missing entry is an error. Works on Windows PowerShell 5.1 and
PowerShell 7. Provenance and license: README.md in this directory.

.PARAMETER Destination
Directory to write into (created if missing), e.g. target\conpty.
#>
# Write-Host is deliberate: progress messages must not end up in the pipeline.
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingWriteHost', '', Justification = 'Build script output')]
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $Destination
)

Set-StrictMode -Version 3
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

# Keep in sync with README.md in this directory.
$Version = '1.25.260930003'
$PackageSha256 = '02b07b349af66d801159bdf9e440d4a1ce78bb951f37fc8609731665afdae7ee'
$PackageUrl = "https://api.nuget.org/v3-flatcontainer/microsoft.windows.console.conpty/$Version/microsoft.windows.console.conpty.$Version.nupkg"
$Files = @(
    @{ Entry = 'runtimes/win-x64/native/conpty.dll'; Target = 'conpty.dll'; Sha256 = 'feeef341d891643c62d30b6b07800bc70f0bc148f44cb8c3bee84aa557ae805a' },
    @{ Entry = 'build/native/runtimes/x64/OpenConsole.exe'; Target = 'x64\OpenConsole.exe'; Sha256 = '3d66b23d0a71bb8eed2b77edc8b9df9bf54ce6c8fb74c863a30e760997f80586' }
)

function Assert-Sha256([string]$Path, [string]$Want) {
    $got = (Get-FileHash -Algorithm SHA256 -Path $Path).Hash.ToLowerInvariant()
    if ($got -ne $Want) { throw "checksum mismatch for $Path (expected $Want, got $got)" }
    Write-Host "  sha256 ok: $got  $(Split-Path -Leaf $Path)"
}

# .NET resolves relative paths against the process directory, not the
# PowerShell location, so make the destination absolute first.
$Destination = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Destination)

# Windows PowerShell 5.1 on older systems defaults to TLS 1.0.
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$work = Join-Path ([IO.Path]::GetTempPath()) ("nxgterm-conpty-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $work | Out-Null
try {
    $package = Join-Path $work "microsoft.windows.console.conpty.$Version.nupkg"
    Write-Host "+ download $PackageUrl"
    Invoke-WebRequest -Uri $PackageUrl -OutFile $package -UseBasicParsing
    Assert-Sha256 $package $PackageSha256

    # A .nupkg is a zip; Expand-Archive in 5.1 only accepts the .zip extension.
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($package)
    try {
        foreach ($file in $Files) {
            $entry = $zip.GetEntry($file.Entry)
            if (-not $entry) { throw "$($file.Entry) not found in the ConPTY $Version package" }
            $target = Join-Path $Destination $file.Target
            New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
            [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $target, $true)
            Assert-Sha256 $target $file.Sha256
        }
    } finally {
        $zip.Dispose()
    }
} finally {
    Remove-Item -Recurse -Force -Path $work -ErrorAction SilentlyContinue
}
Write-Host "ConPTY $Version written to $Destination"
