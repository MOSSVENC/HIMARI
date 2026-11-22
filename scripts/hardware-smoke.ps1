param(
    [string]$Executable = 'C:\ProgramData\Himari\himari.exe'
)

# Runs every reversible hardware smoke mode even when one of them fails, then
# reports the full list. One environment-dependent failure (for example a
# display driver that rejects gamma ramps) must not hide the modes that passed.
$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $Executable -PathType Leaf)) {
    throw "HIMARI executable not found: $Executable"
}

$modes = @('--control-sync-smoke', '--input-hooks-smoke', '--settings-smoke', '--system-smoke', '--io-smoke', '--display-care-smoke')
$failures = @()
foreach ($mode in $modes) {
    Write-Host "==> $mode"
    $process = Start-Process -FilePath $Executable -ArgumentList $mode -NoNewWindow -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        $failures += "$mode (exit $($process.ExitCode))"
    }
}

if ($failures.Count -eq 0) {
    Write-Host 'hardware smoke passed'
    exit 0
}
Write-Host ("hardware smoke failures: " + ($failures -join ', '))
exit 1
