param(
    [Parameter(Mandatory = $true)]
    [string]$InstallerPath
)

$ErrorActionPreference = 'Stop'
$stage = Join-Path ([System.IO.Path]::GetTempPath()) "alexandria-startup-$([guid]::NewGuid())"
$previousDataDir = $env:ALEXANDRIA_DATA_DIR
$process = $null

try {
    & 7z x $InstallerPath "-o$stage" -y | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Failed to extract the Windows installer' }
    $env:ALEXANDRIA_DATA_DIR = Join-Path $stage 'app-data'
    $binary = Join-Path $stage 'alexandria-node.exe'
    $log = Join-Path $env:ALEXANDRIA_DATA_DIR 'diag.log'
    $process = Start-Process -FilePath $binary -WorkingDirectory $stage -PassThru
    $deadline = (Get-Date).AddSeconds(30)
    while ((Get-Date) -lt $deadline) {
        if ($process.HasExited) {
            throw "Packaged app exited during startup ($($process.ExitCode))"
        }
        Start-Sleep -Milliseconds 500
    }
    if (-not (Test-Path $log)) { throw 'Packaged app did not create its diagnostic log' }
    $diagnostics = Get-Content $log -Raw
    if ($diagnostics -match 'PANIC:' -or $diagnostics -notmatch 'network profile validated:' -or
        $diagnostics -notmatch 'app setup complete') {
        throw 'Packaged app did not complete startup successfully'
    }
    Write-Host 'Packaged Windows app completed setup and remained running for 30 seconds'
} finally {
    if ($log -and (Test-Path $log)) { Get-Content $log | Write-Host }
    if ($process -and -not $process.HasExited) {
        & taskkill /PID $process.Id /T /F | Out-Null
    }
    $env:ALEXANDRIA_DATA_DIR = $previousDataDir
}
