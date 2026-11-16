#Requires -Version 7.0
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
$taskOutput = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Path $taskOutput -Force | Out-Null
$taskArguments = '"--bars-smoke=' + $taskOutput + '"'
$taskProcess = Start-Process -FilePath $Executable -ArgumentList $taskArguments -PassThru -NoNewWindow `
    -RedirectStandardOutput (Join-Path $taskOutput 'stdout.log') `
    -RedirectStandardError (Join-Path $taskOutput 'stderr.log')
try {
    if (-not $taskProcess.WaitForExit(30000)) {
        $taskProcess.Kill()
        throw 'Native bar regression timed out'
    }
    Get-Content -LiteralPath (Join-Path $taskOutput 'stdout.log')
    Get-Content -LiteralPath (Join-Path $taskOutput 'stderr.log')
    if ($taskProcess.ExitCode -ne 0) {
        throw "Native bar regression failed: $($taskProcess.ExitCode)"
    }
} finally {
    $taskProcess.Dispose()
}
