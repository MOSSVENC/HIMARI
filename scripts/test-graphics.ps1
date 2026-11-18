[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string] $Executable,
    [Parameter(Mandatory)] [string] $OutputDirectory
)

$ErrorActionPreference = 'Stop'
$Executable = (Resolve-Path -LiteralPath $Executable).ProviderPath
$null = New-Item -ItemType Directory -Path $OutputDirectory -Force
$OutputDirectory = (Resolve-Path -LiteralPath $OutputDirectory).ProviderPath
$stdout = Join-Path $OutputDirectory 'vulkan.stdout.log'
$stderr = Join-Path $OutputDirectory 'vulkan.stderr.log'
$snapshot = Join-Path $OutputDirectory 'overview.ppm'
$clock = [Diagnostics.Stopwatch]::StartNew()
if (Test-Path -LiteralPath $snapshot) { Remove-Item -LiteralPath $snapshot -Force }
$oldSnapshotSetting = $env:RAM_GRAPHICS_SMOKE_SNAPSHOT
$env:RAM_GRAPHICS_SMOKE_SNAPSHOT = $snapshot
$process = Start-Process -FilePath $Executable -PassThru -NoNewWindow `
    -ArgumentList @('--graphics-smoke', '--renderer=vulkan') `
    -RedirectStandardOutput $stdout -RedirectStandardError $stderr
$peakWorkingSet = 0L
$peakPrivateBytes = 0L
$cpuTimeMs = 0.0
$vulkanModuleObservedMs = $null
$graphicsModules = @()
try {
    while (-not $process.WaitForExit(100)) {
        $process.Refresh()
        if ($process.HasExited) { break }
        $peakWorkingSet = [Math]::Max($peakWorkingSet, $process.WorkingSet64)
        $peakPrivateBytes = [Math]::Max($peakPrivateBytes, $process.PrivateMemorySize64)
        $cpuTimeMs = $process.TotalProcessorTime.TotalMilliseconds
        try {
            $graphicsModules = @($process.Modules | Where-Object {
                $_.ModuleName -match 'vulkan|amdvlk|nvoglv|atiogl'
            } | ForEach-Object { $_.ModuleName })
        } catch {
            # Module enumeration can briefly fail while Windows is loading the process.
        }
        if ($null -eq $vulkanModuleObservedMs -and $graphicsModules -contains 'vulkan-1.dll') {
            $vulkanModuleObservedMs = $clock.Elapsed.TotalMilliseconds
        }
        if ($clock.Elapsed.TotalSeconds -gt 45) {
            $process.Kill()
            $process.WaitForExit()
            throw 'Vulkan UI check timed out'
        }
    }
    $process.WaitForExit()
    $clock.Stop()
    $process.Refresh()
    $cpuTimeMs = $process.TotalProcessorTime.TotalMilliseconds
    $output = if (Test-Path -LiteralPath $stdout) { Get-Content -LiteralPath $stdout -Raw } else { '' }
    $errors = if (Test-Path -LiteralPath $stderr) { Get-Content -LiteralPath $stderr -Raw } else { '' }
    Write-Output $output
    if ($errors) { Write-Output $errors }
    $result = [pscustomobject]@{
        Renderer = 'native-vulkan-skia'
        ExitCode = $process.ExitCode
        VulkanLoaderObservedMs = $vulkanModuleObservedMs
        GraphicsModules = $graphicsModules
        PeakWorkingSetBytes = $peakWorkingSet
        PeakPrivateBytes = $peakPrivateBytes
        CpuTimeMs = $cpuTimeMs
        DurationMs = $clock.Elapsed.TotalMilliseconds
        Output = $output
        Errors = $errors
    }
    $result | ConvertTo-Json -Depth 4 | Set-Content `
        -LiteralPath (Join-Path $OutputDirectory 'results.json') -Encoding utf8
    if ($process.ExitCode -ne 0) { throw "Vulkan UI check exited with $($process.ExitCode)" }
    if ($null -eq $vulkanModuleObservedMs -and $graphicsModules -notcontains 'vulkan-1.dll') {
        throw 'The process did not load the Vulkan runtime module'
    }
    if ($output -notmatch 'transitions=22 final_ui_state=passed') {
        throw 'The production page-transition state check did not pass'
    }
    if ($output -notmatch 'resize_and_restore=passed idle_interval_ms=1000 idle_submissions=0') {
        throw 'Vulkan resize, minimize/restore or idle rendering did not pass'
    }
    if ($output -notmatch 'type=IntegratedGpu|type=DiscreteGpu') {
        throw 'Vulkan did not select a hardware GPU'
    }
    if (-not (Test-Path -LiteralPath $snapshot) -or (Get-Item -LiteralPath $snapshot).Length -lt 1024) {
        throw 'The Vulkan window snapshot is missing or too small'
    }
} finally {
    if (-not $process.HasExited) {
        $process.Kill()
        $process.WaitForExit()
    }
    $process.Dispose()
    if ($null -eq $oldSnapshotSetting) {
        Remove-Item Env:RAM_GRAPHICS_SMOKE_SNAPSHOT -ErrorAction SilentlyContinue
    } else {
        $env:RAM_GRAPHICS_SMOKE_SNAPSHOT = $oldSnapshotSetting
    }
}

Write-Output "Rendering measurements: $(Join-Path $OutputDirectory 'results.json')"
