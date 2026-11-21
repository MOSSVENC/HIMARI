#Requires -Version 7.0
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string] $Executable,
    [Parameter(Mandatory)] [string] $OutputDirectory
)

$ErrorActionPreference = 'Stop'
$Executable = (Resolve-Path -LiteralPath $Executable).ProviderPath
$null = New-Item -ItemType Directory -Path $OutputDirectory -Force
$OutputDirectory = (Resolve-Path -LiteralPath $OutputDirectory).ProviderPath

Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class RamTrayTest {
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
    public delegate bool EnumProc(IntPtr hwnd, IntPtr data);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback, IntPtr data);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr hwnd, StringBuilder text, int count);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int count);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);
    [DllImport("user32.dll", EntryPoint="GetWindowLongPtrW")] static extern IntPtr GetWindowLongPtr(IntPtr hwnd, int index);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hwnd, uint message, UIntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll")] public static extern bool ShowWindowAsync(IntPtr hwnd, int command);
    public static IntPtr[] Windows(uint pid, bool visibleOnly) {
        var result = new List<IntPtr>();
        EnumProc callback = (hwnd, data) => {
            uint owner; GetWindowThreadProcessId(hwnd, out owner);
            if (owner == pid && (!visibleOnly || HasVisibleArea(hwnd))) result.Add(hwnd);
            return true;
        };
        EnumWindows(callback, IntPtr.Zero);
        return result.ToArray();
    }
    public static string ClassName(IntPtr hwnd) {
        var name = new StringBuilder(256); GetClassName(hwnd, name, name.Capacity); return name.ToString();
    }
    public static string Title(IntPtr hwnd) {
        var name = new StringBuilder(256); GetWindowText(hwnd, name, name.Capacity); return name.ToString();
    }
    public static bool HasVisibleArea(IntPtr hwnd) {
        Rect rect;
        // winit 0.30 event_loop.rs creates a transparent layered message target.
        // Its WS_VISIBLE bit enables WM_PAINT; no layered content is displayed.
        const long eventTargetStyles = 0x080800A0; // NOACTIVATE | LAYERED | TOOLWINDOW | TRANSPARENT
        if (ClassName(hwnd) == "Winit Thread Event Target" && Title(hwnd).Length == 0
            && (GetWindowLongPtr(hwnd, -20).ToInt64() & eventTargetStyles) == eventTargetStyles) return false;
        return IsWindowVisible(hwnd) && GetWindowRect(hwnd, out rect)
            && rect.Right > rect.Left && rect.Bottom > rect.Top;
    }
}
'@

function Wait-Condition([scriptblock]$Check, [string]$Failure, [int]$TimeoutMs = 5000) {
    $deadline = [Diagnostics.Stopwatch]::StartNew()
    while (-not (& $Check)) {
        if ($process.HasExited) { throw "Diagnostic exited early: $($process.ExitCode)" }
        if ($deadline.ElapsedMilliseconds -ge $TimeoutMs) { throw $Failure }
        Start-Sleep -Milliseconds 20
    }
}

function Send-Tray([uint32]$Message, [uint64]$Command = 0) {
    if (-not [RamTrayTest]::PostMessage($script:tray, $Message, [UIntPtr]$Command, [IntPtr]::Zero)) {
        throw 'Could not send tray test message'
    }
}

function Get-SubmittedFrames {
    if (-not (Test-Path -LiteralPath $stdout)) { return 0 }
    $stream = [IO.File]::Open($stdout, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
    try {
        $reader = [IO.StreamReader]::new($stream)
        try { return [regex]::Matches($reader.ReadToEnd(), 'tray_frame_submitted=').Count }
        finally { $reader.Dispose() }
    } finally { $stream.Dispose() }
}

$oldFrame = $env:NEO_CONTROL_FRAME_LOG
$oldAction = $env:NEO_CONTROL_ACTION_LOG
$results = @()
try {
    foreach ($background in @($true, $false)) {
        $name = if ($background) { 'background' } else { 'foreground' }
        $frame = Join-Path $OutputDirectory "$name-renderer.txt"
        $action = Join-Path $OutputDirectory "$name-actions.txt"
        foreach ($path in @($frame, $action)) {
            if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Force }
        }
        $env:NEO_CONTROL_FRAME_LOG = $frame
        $env:NEO_CONTROL_ACTION_LOG = $action
        $arguments = @('--tray-smoke')
        if ($background) { $arguments += '--background' }
        $stdout = Join-Path $OutputDirectory "$name.stdout.log"
        $parameters = @{
            FilePath=$Executable; ArgumentList=$arguments; PassThru=$true; NoNewWindow=$true
            RedirectStandardOutput=$stdout
            RedirectStandardError=(Join-Path $OutputDirectory "$name.stderr.log")
        }
        $process = Start-Process @parameters
        try {
            # Observe from process creation, beyond the previous visibility race.
            # Every window lookup and message is scoped to this diagnostic PID.
            $watch = [Diagnostics.Stopwatch]::StartNew()
            $script:tray = [IntPtr]::Zero
            $observations = 0
            do {
                if ($process.HasExited) { throw "Diagnostic exited during startup: $($process.ExitCode)" }
                foreach ($window in @([RamTrayTest]::Windows($process.Id, $false))) {
                    if ([RamTrayTest]::ClassName($window) -eq 'HimariTrayWindow') { $script:tray = $window }
                    if ($background -and [RamTrayTest]::HasVisibleArea($window)) {
                        throw "Background startup exposed a visible window: class=$([RamTrayTest]::ClassName($window)) title=$([RamTrayTest]::Title($window)) elapsed_ms=$($watch.ElapsedMilliseconds)"
                    }
                }
                $observations++
                Start-Sleep -Milliseconds 10
            } while ($watch.ElapsedMilliseconds -lt 6500)
            $startupObservedMs = $watch.ElapsedMilliseconds
            if ($script:tray -eq [IntPtr]::Zero) { throw 'Tray window was not created' }
            if ((Get-Content -LiteralPath $action -Raw) -notmatch 'tray icon installed') { throw 'Tray registration did not succeed' }
            if ($background -and (Test-Path -LiteralPath $frame)) { throw 'Background startup submitted a main-window frame' }
            if (-not $background -and (@([RamTrayTest]::Windows($process.Id, $true)).Count -ne 1 -or -not (Test-Path -LiteralPath $frame))) {
                throw 'Foreground startup did not show and render the main window'
            }
            Send-Tray 0x8002
            Wait-Condition { @([RamTrayTest]::Windows($process.Id, $true)).Count -eq 1 } 'Main window did not open'
            $main = @([RamTrayTest]::Windows($process.Id, $true))[0]
            Wait-Condition { Test-Path -LiteralPath $frame } 'Main window did not submit a Vulkan frame'
            $renderer = [IO.File]::ReadAllText($frame)
            if ($renderer -notmatch 'backend=Vulkan renderer=Skia' -or $renderer -notmatch 'type=IntegratedGpu|type=DiscreteGpu') {
                throw "Unexpected renderer: $renderer"
            }
            for ($round = 0; $round -lt 3; $round++) {
                if (-not [RamTrayTest]::PostMessage($main, 0x10, [UIntPtr]::Zero, [IntPtr]::Zero)) { throw 'Close request failed' }
                Wait-Condition { -not [RamTrayTest]::IsWindowVisible($main) } 'Close did not hide the main window'
                Start-Sleep -Milliseconds 150
                $beforeShow = Get-SubmittedFrames
                Send-Tray 0x8002
                Wait-Condition { [RamTrayTest]::IsWindowVisible($main) } 'Tray did not restore the main window'
                Wait-Condition { (Get-SubmittedFrames) -gt $beforeShow } 'Restored window did not submit another Vulkan frame'
                $null = [RamTrayTest]::ShowWindowAsync($main, 6)
                Wait-Condition { [RamTrayTest]::IsIconic($main) } 'Minimize did not complete'
                $beforeShow = Get-SubmittedFrames
                Send-Tray 0x8002
                Wait-Condition { [RamTrayTest]::IsWindowVisible($main) -and -not [RamTrayTest]::IsIconic($main) } 'Tray did not restore a minimized window'
                Wait-Condition { (Get-SubmittedFrames) -gt $beforeShow } 'Minimized window did not resume Vulkan submission'
            }
            Send-Tray 0x111 2
            if (-not $process.WaitForExit(5000)) { throw 'Tray Exit did not stop the diagnostic' }
            $process.WaitForExit()
            if ($process.ExitCode -ne 0) { throw "Tray lifecycle failed: $($process.ExitCode)" }
            $results += [pscustomobject]@{
                Launch=$name; StartupObservedMs=$startupObservedMs; StartupPolls=$observations
                HiddenStartup=if($background){'Passed'}else{'NotApplicable'}
                WindowRestoreCycles=3; MinimizeRestoreCycles=3; Renderer=$renderer; ExitCode=$process.ExitCode
            }
        } finally {
            if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
            $process.Dispose()
        }
    }
    $results | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'results.json') -Encoding utf8
    $results | ConvertTo-Json -Depth 4
} finally {
    if ($null -eq $oldFrame) { Remove-Item Env:NEO_CONTROL_FRAME_LOG -ErrorAction SilentlyContinue } else { $env:NEO_CONTROL_FRAME_LOG = $oldFrame }
    if ($null -eq $oldAction) { Remove-Item Env:NEO_CONTROL_ACTION_LOG -ErrorAction SilentlyContinue } else { $env:NEO_CONTROL_ACTION_LOG = $oldAction }
}
