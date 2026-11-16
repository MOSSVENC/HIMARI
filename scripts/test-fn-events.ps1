#Requires -Version 7.0
#Requires -RunAsAdministrator
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $Executable -PathType Leaf)) { throw 'Release executable is missing' }
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null

# Capture restoration data before touching either part of the vendor listener.
$taskFnKey = @(Get-CimInstance Win32_Process -Filter "Name='FnKey.exe'")
if ($taskFnKey.Count -ne 1) { throw 'Expected one resident FnKey process' }
$taskFnPath = $taskFnKey[0].ExecutablePath
if (-not $taskFnPath -or -not (Test-Path -LiteralPath $taskFnPath)) { throw 'FnKey restoration path is unavailable' }
$taskServices = @(Get-Service -Name CCDCHUService, DCHUService -ErrorAction SilentlyContinue)
foreach ($taskService in $taskServices) {
    if ($taskService.Status -notin @('Running', 'Stopped')) { throw "Service $($taskService.Name) is transitioning" }
}
$taskRunning = @($taskServices | Where-Object Status -eq 'Running' | ForEach-Object Name)

function Invoke-FnDiagnostic([string]$Label, [int]$ExpectedExit) {
    $stdout = Join-Path $OutputDirectory "$Label.stdout.log"
    $stderr = Join-Path $OutputDirectory "$Label.stderr.log"
    $process = Start-Process -FilePath $Executable -ArgumentList '--fn-events-smoke' -PassThru -NoNewWindow -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    try {
        if (-not $process.WaitForExit(15000)) {
            $process.Kill()
            throw "$Label exceeded its 15 second deadline"
        }
        $out = Get-Content -LiteralPath $stdout -Raw
        $err = Get-Content -LiteralPath $stderr -Raw
        Write-Output $out
        if ($err) { Write-Output $err }
        if ($process.ExitCode -ne $ExpectedExit) { throw "$Label returned $($process.ExitCode), expected $ExpectedExit" }
        if ($ExpectedExit -eq 0) {
            if ($out -notmatch 'subscription registered' -or $out -notmatch 'subscription closed') { throw "$Label lifecycle markers are incomplete" }
        } elseif ($err -notmatch 'coordinate Fn notification ownership' -or $out -match 'subscription registered') {
            throw "$Label did not reject the occupied notification channel"
        }
    } finally { $process.Dispose() }
}

if ($taskRunning.Count) { Invoke-FnDiagnostic 'service-occupied' 1 }
try {
    Stop-Process -Id $taskFnKey[0].ProcessId -Force -ErrorAction Stop
    foreach ($taskName in $taskRunning) {
        Stop-Service -Name $taskName -ErrorAction Stop
        (Get-Service -Name $taskName).WaitForStatus('Stopped', [TimeSpan]::FromSeconds(15))
    }
    $taskDeadline = [DateTime]::UtcNow.AddSeconds(5)
    while (Get-Process -Name DCHUService -ErrorAction SilentlyContinue) {
        if ([DateTime]::UtcNow -ge $taskDeadline) { throw 'DCHUService process remains after service stop' }
        Start-Sleep -Milliseconds 100
    }
    if (Get-Process -Name FnKey -ErrorAction SilentlyContinue) { throw 'FnKey restarted before channel testing' }
    foreach ($taskIteration in 1..2) {
        Invoke-FnDiagnostic "released-$taskIteration" 0
        if (Get-Process -Name FnKey -ErrorAction SilentlyContinue) { throw 'FnKey became active during channel testing' }
        foreach ($taskService in $taskServices) {
            $taskService.Refresh()
            if ($taskService.Status -ne 'Stopped') { throw "Service $($taskService.Name) became active during channel testing" }
        }
        Write-Output "Released-channel subscription lifecycle $taskIteration passed"
    }
} finally {
    $taskRestoreErrors = [Collections.Generic.List[string]]::new()
    foreach ($taskName in $taskRunning) {
        try {
            Start-Service -Name $taskName -ErrorAction Stop
            (Get-Service -Name $taskName).WaitForStatus('Running', [TimeSpan]::FromSeconds(15))
        } catch { $taskRestoreErrors.Add("Restore ${taskName}: $_") }
    }
    try {
        if (-not (Get-Process -Name FnKey -ErrorAction SilentlyContinue)) {
            Start-Process -FilePath $taskFnPath -WorkingDirectory (Split-Path -Parent $taskFnPath) | Out-Null
        }
        $taskDeadline = [DateTime]::UtcNow.AddSeconds(5)
        do {
            $taskRestored = @(Get-CimInstance Win32_Process -Filter "Name='FnKey.exe'")
            if ($taskRestored.Count -eq 1 -and $taskRestored[0].ExecutablePath -eq $taskFnPath) { break }
            if ([DateTime]::UtcNow -ge $taskDeadline) { throw 'Original FnKey process restoration could not be confirmed' }
            Start-Sleep -Milliseconds 100
        } while ($true)
    } catch { $taskRestoreErrors.Add("Restore FnKey: $_") }
    if ($taskRestoreErrors.Count) { throw ($taskRestoreErrors -join [Environment]::NewLine) }
    Write-Output 'Original vendor services and FnKey restored'
    Get-CimInstance Win32_Process -Filter "Name='FnKey.exe' OR Name='DCHUService.exe'" | Select-Object Name, ProcessId, ExecutablePath | ConvertTo-Json
}
