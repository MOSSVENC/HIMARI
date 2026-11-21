#Requires -Version 7.0
#Requires -RunAsAdministrator
[CmdletBinding()]
param(
    [switch]$Execute,
    [switch]$IncludeFnKey,
    [switch]$AcknowledgeFnAcceptance
)

$ErrorActionPreference = 'Stop'
$app = Join-Path $env:ProgramData 'Himari\himari.exe'
$data = Join-Path $env:ProgramData 'Himari'
$acceptance = Join-Path $data 'fn-owner-acceptance.json'
$receipt = Join-Path $data 'vendor-console-retirement.json'
$cc3 = @(Get-AppxPackage -Name 'CLEVOCO.ControlCenter3.0*')
$fnPackage = @(Get-AppxPackage -Name 'CLEVOCO.FnhotkeysandOSD*')
$originalProcesses = @(Get-CimInstance Win32_Process | Where-Object {
    $_.Name -in @('CC40.exe', 'ControlCenter30.exe', 'COUWP.exe', 'FnKey.exe')
})
$fnAccepted = $false
if (Test-Path -LiteralPath $acceptance) {
    try {
        $fnAccepted = [bool](Get-Content -LiteralPath $acceptance -Raw | ConvertFrom-Json).physicalFnAccepted
    } catch {
        $fnAccepted = $false
    }
}

$plan = [ordered]@{
    Application = $app
    ApplicationExists = Test-Path -LiteralPath $app
    Execute = [bool]$Execute
    ControlCenter30Packages = @($cc3 | Select-Object -ExpandProperty PackageFullName)
    FnKeyPackages = @($fnPackage | Select-Object -ExpandProperty PackageFullName)
    RunningOriginalProcesses = @($originalProcesses | Select-Object Name, ProcessId, ExecutablePath)
    PhysicalFnAcceptanceMarker = $acceptance
    PhysicalFnAccepted = $fnAccepted
    IncludeFnKey = [bool]$IncludeFnKey
}
$plan | ConvertTo-Json -Depth 5

if (-not $Execute) {
    Write-Output 'Dry run only. Use -Execute to retire the old Control Center 3.0 package.'
    exit 0
}
if (-not (Test-Path -LiteralPath $app -PathType Leaf)) {
    throw "Installed HIMARI executable is missing: $app"
}
if ($IncludeFnKey -and (-not $AcknowledgeFnAcceptance -or -not $fnAccepted)) {
    throw 'FnKey removal requires physical Fn acceptance marker and -AcknowledgeFnAcceptance.'
}

$service = Get-Service CCDCHUService -ErrorAction SilentlyContinue
$previousStartType = if ($service) { $service.StartType } else { $null }
$removed = [System.Collections.Generic.List[string]]::new()
try {
    $stopNames = if ($IncludeFnKey) {
        @('CC40.exe', 'ControlCenter30.exe', 'COUWP.exe', 'FnKey.exe')
    } else {
        @('CC40.exe', 'ControlCenter30.exe', 'COUWP.exe')
    }
    $stopProcesses = @(Get-CimInstance Win32_Process | Where-Object { $_.Name -in $stopNames })
    foreach ($process in $stopProcesses) {
        Stop-Process -Id $process.ProcessId -Force -ErrorAction Stop
        Wait-Process -Id $process.ProcessId -Timeout 10 -ErrorAction SilentlyContinue
    }
    $apps = @(Get-Process -Name himari -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $app })
    foreach ($process in $apps) {
        Stop-Process -Id $process.Id -Force -ErrorAction Stop
        Wait-Process -Id $process.Id -Timeout 10 -ErrorAction SilentlyContinue
    }

    foreach ($package in $cc3) {
        Remove-AppxPackage -Package $package.PackageFullName
        $removed.Add($package.PackageFullName)
    }
    if ($IncludeFnKey) {
        if ($service) {
            Stop-Service -Name CCDCHUService -Force
            Set-Service -Name CCDCHUService -StartupType Manual
        }
        foreach ($package in $fnPackage) {
            Remove-AppxPackage -Package $package.PackageFullName
            $removed.Add($package.PackageFullName)
        }
    }

    Start-Process -FilePath $app -ArgumentList '--background'
    Start-Sleep -Seconds 3
    $live = @(Get-Process -Name himari -ErrorAction Stop | Where-Object { $_.Path -eq $app })
    if ($live.Count -ne 1) {
        throw "Expected one HIMARI process after retirement, got $($live.Count)"
    }
    $result = [ordered]@{
        RetiredAt = (Get-Date).ToString('o')
        RemovedPackages = $removed
        FnKeyIncluded = [bool]$IncludeFnKey
        RemainingFnKeyPackages = @(Get-AppxPackage -Name 'CLEVOCO.FnhotkeysandOSD*' | Select-Object -ExpandProperty PackageFullName)
        CCDCHUServiceStartType = (Get-Service CCDCHUService -ErrorAction SilentlyContinue).StartType
        ApplicationPid = $live[0].Id
        ApplicationSha256 = (Get-FileHash -LiteralPath $app -Algorithm SHA256).Hash
    }
    $result | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $receipt -Encoding utf8
    Write-Output "Retirement receipt: $receipt"
} catch {
    if ($IncludeFnKey -and $service -and $previousStartType) {
        try {
            Set-Service -Name CCDCHUService -StartupType $previousStartType
            Start-Service -Name CCDCHUService -ErrorAction SilentlyContinue
        } catch {
            Write-Warning "Could not restore CCDCHUService: $($_.Exception.Message)"
        }
    }
    throw
}
