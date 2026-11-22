#Requires -Version 7.0
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Executable,
    [string]$OutputDirectory = (Join-Path $env:TEMP 'himari-perf'),
    [ValidateRange(3, 20)][int]$Runs = 5
)

# Runs --graphics-smoke N times and reports the per-page first-visit worst case
# as a median plus range, along with the global submission maximum. This is a
# report, not a gate: a page is only "inside budget" when the median is <= 16.67
# and the range is printed next to it, not hidden.

$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $Executable -PathType Leaf)) { throw 'Executable is missing' }
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null

$pages = @{}
$globals = @()
$gaps = 0
for ($run = 1; $run -le $Runs; $run++) {
    $out = Join-Path $OutputDirectory "run-$run.out"
    $process = Start-Process -FilePath $Executable -ArgumentList '--graphics-smoke' -Wait -PassThru -RedirectStandardOutput $out
    if ($process.ExitCode -ne 0) { throw "graphics smoke run $run exited $($process.ExitCode)" }
    $text = Get-Content -LiteralPath $out -Raw
    if ($text -match 'submission_interval_max_ms=([0-9.]+).*?gaps_over_33ms=(\d+)') {
        $globals += [double]$Matches[1]
        $gaps += [int]$Matches[2]
    } else { throw "run $run did not report a submission summary" }
    foreach ($line in ($text -split "`r?`n")) {
        if ($line -match 'frame_stages round=1 page=(\d+).*?max_total_ms=([0-9.]+)') {
            $page = [int]$Matches[1]
            $value = [double]$Matches[2]
            if (-not $pages.ContainsKey($page)) { $pages[$page] = @() }
            $pages[$page] += $value
        }
    }
}

function Get-Median([double[]]$values) {
    $sorted = $values | Sort-Object
    $count = $sorted.Count
    if ($count -eq 0) { return [double]::NaN }
    if ($count % 2 -eq 1) { return $sorted[($count - 1) / 2] }
    return ($sorted[$count / 2 - 1] + $sorted[$count / 2]) / 2
}

Write-Output "runs=$Runs executable=$Executable"
Write-Output ("global max_ms: median {0:N2}  min {1:N2}  max {2:N2}  gaps_over_33ms {3}" -f (Get-Median $globals), ($globals | Measure-Object -Minimum).Minimum, ($globals | Measure-Object -Maximum).Maximum, $gaps)
Write-Output ''
Write-Output 'page  median  min    max    <=16.67'
foreach ($page in ($pages.Keys | Sort-Object)) {
    $values = $pages[$page]
    $median = Get-Median $values
    $min = ($values | Measure-Object -Minimum).Minimum
    $max = ($values | Measure-Object -Maximum).Maximum
    $inside = if ($median -le 16.67) { 'yes' } else { 'no' }
    Write-Output ("{0,-5} {1,7:N2} {2,6:N2} {3,6:N2}   {4}" -f $page, $median, $min, $max, $inside)
}
