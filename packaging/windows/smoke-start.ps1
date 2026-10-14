# Starts nxgterm and fails if it dies during startup.
#
# `--version` never reaches the startup path (config, jump list, window,
# renderer, PTY). A crash there kills the process before any window exists
# and leaves no trace, which is how 0.10.0 shipped broken on Windows.
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [int]$Seconds = 10
)
$ErrorActionPreference = 'Stop'

$app = Start-Process -FilePath $Exe -PassThru
Start-Sleep -Seconds $Seconds
if ($app.HasExited) {
    Write-Error ("nxgterm exited during startup with code 0x{0:X8}" -f $app.ExitCode)
    exit 1
}
Stop-Process -Id $app.Id -Force
Write-Host "nxgterm was still running after $Seconds seconds"
