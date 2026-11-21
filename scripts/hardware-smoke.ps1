param(
    [string]$Executable = 'C:\ProgramData\Himari\himari.exe'
)

$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $Executable -PathType Leaf)) {
    throw "HIMARI executable not found: $Executable"
}

$modes = @('--control-sync-smoke', '--input-hooks-smoke', '--settings-smoke', '--system-smoke', '--io-smoke', '--display-care-smoke')
foreach ($mode in $modes) {
    Write-Host "==> $mode"
    $process = Start-Process -FilePath $Executable -ArgumentList $mode -NoNewWindow -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        throw "$mode failed with exit code $($process.ExitCode)"
    }
}

Write-Host 'hardware smoke passed'
