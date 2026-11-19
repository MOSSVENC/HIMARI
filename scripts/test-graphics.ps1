[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string] $Executable,
    [Parameter(Mandatory)] [string] $OutputDirectory,
    [ValidateRange(0, 1000)] [double] $MaximumFrameGapMs = 0,
    [switch] $RequireSnapshot,
    [switch] $RequireContentUpdates,
    [switch] $RequireTextInvalidation
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
$oldTextSetting = $env:RAM_GRAPHICS_SMOKE_TEXT_CHANGES
$env:RAM_GRAPHICS_SMOKE_SNAPSHOT = $snapshot
if ($RequireTextInvalidation) {
    $env:RAM_GRAPHICS_SMOKE_TEXT_CHANGES = '1'
} else {
    Remove-Item Env:RAM_GRAPHICS_SMOKE_TEXT_CHANGES -ErrorAction SilentlyContinue
}
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
    $cadence = [regex]::Match($output, 'submission_interval_p50_ms=([0-9.]+) submission_interval_p95_ms=([0-9.]+) submission_interval_max_ms=([0-9.]+) gaps_over_33ms=([0-9]+)')
    $performance = $null
    if ($cadence.Success) {
        $culture = [Globalization.CultureInfo]::InvariantCulture
        $maximumGap = [double]::Parse($cadence.Groups[3].Value, $culture)
        $performance = [pscustomobject]@{
            SubmissionIntervalP50Ms = [double]::Parse($cadence.Groups[1].Value, $culture)
            SubmissionIntervalP95Ms = [double]::Parse($cadence.Groups[2].Value, $culture)
            MaximumSubmissionIntervalMs = $maximumGap
            GapsOver33Ms = [int]::Parse($cadence.Groups[4].Value, $culture)
            MaximumAllowedGapMs = if ($MaximumFrameGapMs -gt 0) { $MaximumFrameGapMs } else { $null }
            BudgetStatus = if ($MaximumFrameGapMs -le 0) { 'MeasuredOnly' } elseif ($maximumGap -le $MaximumFrameGapMs) { 'Passed' } else { 'Exceeded' }
        }
    }
    $result = [pscustomobject]@{
        Renderer = 'native-vulkan-skia'
        ExitCode = $process.ExitCode
        VulkanLoaderObservedMs = $vulkanModuleObservedMs
        GraphicsModules = $graphicsModules
        PeakWorkingSetBytes = $peakWorkingSet
        PeakPrivateBytes = $peakPrivateBytes
        CpuTimeMs = $cpuTimeMs
        DurationMs = $clock.Elapsed.TotalMilliseconds
        Performance = $performance
        SnapshotSource = if ($output -match 'snapshot=written source=vulkan-swapchain') { 'vulkan-swapchain' } else { $null }
        ContentUpdates = if ($output -match 'gpu_content_updates=passed restored_pixels=exact') { 'Passed' } else { 'NotMeasured' }
        TextInvalidation = if ($output -match 'gpu_text_invalidation=passed telemetry=changed language=changed palette=changed restored_pixels=exact') { 'Passed' } else { 'NotMeasured' }
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
    if ($null -eq $performance -or [regex]::Matches($output, '(?m)^frame_stages round=').Count -ne 22) {
        throw 'Vulkan per-transition performance measurements are incomplete'
    }
    if (($RequireSnapshot -or $output -match 'snapshot=written') -and
        (-not (Test-Path -LiteralPath $snapshot) -or (Get-Item -LiteralPath $snapshot).Length -lt 1024)) {
        throw 'The Vulkan window snapshot is missing or too small'
    }
    if (($RequireSnapshot -or $output -match 'snapshot=written') -and
        $output -notmatch 'snapshot=written source=vulkan-swapchain') {
        throw 'The rendering snapshot did not come from the Vulkan swapchain'
    }
    if ($RequireContentUpdates -and $result.ContentUpdates -ne 'Passed') {
        throw 'Vulkan content updates and restoration did not pass'
    }
    if ($RequireTextInvalidation -and $result.TextInvalidation -ne 'Passed') {
        throw 'Vulkan text, language or palette updates did not pass'
    }
    if ($performance.BudgetStatus -eq 'Exceeded') {
        throw "Vulkan frame-gap budget exceeded: $($performance.MaximumSubmissionIntervalMs) ms > $MaximumFrameGapMs ms"
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
    if ($null -eq $oldTextSetting) {
        Remove-Item Env:RAM_GRAPHICS_SMOKE_TEXT_CHANGES -ErrorAction SilentlyContinue
    } else {
        $env:RAM_GRAPHICS_SMOKE_TEXT_CHANGES = $oldTextSetting
    }
}

Write-Output "Rendering measurements: $(Join-Path $OutputDirectory 'results.json')"
