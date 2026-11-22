#Requires -Version 7.0
#Requires -RunAsAdministrator
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [ValidateRange(5, 600)][int]$Seconds = 30
)

# Captures physical Fn notifications while the vendor listener is stopped, then
# restores the vendor services and the original FnKey process. The capture
# window is deliberately explicit: press the Fn combinations you want decoded
# while it runs, and the log records the raw value and the decoded event for
# every notification.

$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $Executable -PathType Leaf)) { throw 'Release executable is missing' }
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null

$taskFnKey = @(Get-CimInstance Win32_Process -Filter "Name='FnKey.exe'")
if ($taskFnKey.Count -ne 1) { throw 'Expected one resident FnKey process' }
$taskFnPath = $taskFnKey[0].ExecutablePath
if (-not $taskFnPath -or -not (Test-Path -LiteralPath $taskFnPath)) { throw 'FnKey restoration path is unavailable' }
$taskServices = @(Get-Service -Name CCDCHUService, DCHUService -ErrorAction SilentlyContinue)
foreach ($taskService in $taskServices) {
    if ($taskService.Status -notin @('Running', 'Stopped')) { throw "Service $($taskService.Name) is transitioning" }
}
$taskRunning = @($taskServices | Where-Object Status -eq 'Running' | ForEach-Object Name)

# A resident HIMARI keeps polling for the channel and would grab it the moment
# FnKey stops, leaving the vendor listener without it after restoration. Stop
# the resident instances for the capture window (never the executable under
# test) and start them again afterwards.
$taskResident = @(Get-CimInstance Win32_Process -Filter "Name='himari.exe'" |
    Where-Object { $_.ExecutablePath -and $_.ExecutablePath -ne $Executable })
$taskResidentPaths = @($taskResident | ForEach-Object ExecutablePath)

$stdout = Join-Path $OutputDirectory 'fn-capture.stdout.log'
$stderr = Join-Path $OutputDirectory 'fn-capture.stderr.log'
Remove-Item -LiteralPath $stdout, $stderr -ErrorAction SilentlyContinue

try {
    foreach ($taskProcess in $taskResident) {
        Stop-Process -Id $taskProcess.ProcessId -Force -ErrorAction Stop
    }
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
    if (Get-Process -Name FnKey -ErrorAction SilentlyContinue) { throw 'FnKey restarted before capture' }

    Write-Output "Fn capture window: ${Seconds}s. Press the Fn combinations now."
    $process = Start-Process -FilePath $Executable -ArgumentList "--fn-capture $Seconds" -PassThru -NoNewWindow -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    if (-not $process.WaitForExit(($Seconds + 20) * 1000)) {
        $process.Kill()
        throw "Fn capture exceeded its $($Seconds + 20) second deadline"
    }
    Write-Output "Fn capture exit=$($process.ExitCode)"
    if (Test-Path -LiteralPath $stdout) { Get-Content -LiteralPath $stdout }
    if (Test-Path -LiteralPath $stderr) { Get-Content -LiteralPath $stderr }
    if ($process.ExitCode -ne 0) {
        throw "Fn capture returned $($process.ExitCode); see $stdout and $stderr"
    }
    $captured = Get-Content -LiteralPath $stdout -Raw
    if ($captured -notmatch 'subscription registered' -or $captured -notmatch 'subscription closed') {
        throw 'Fn capture lifecycle markers are incomplete'
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
    foreach ($taskPath in $taskResidentPaths) {
        try {
            Start-Process -FilePath $taskPath -ArgumentList '--background' | Out-Null
        } catch { $taskRestoreErrors.Add("Restore resident ${taskPath}: $_") }
    }
    if ($taskRestoreErrors.Count) { throw ($taskRestoreErrors -join [Environment]::NewLine) }
    Write-Output 'Original vendor services and FnKey restored'
}
