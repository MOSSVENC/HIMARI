#Requires -Version 7.0
#Requires -RunAsAdministrator
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ReleaseDirectory,
    [switch]$StartBackground
)

$ErrorActionPreference = 'Stop'
$taskInstallRoot = Join-Path $env:ProgramData 'RAM Control Center'
$taskLegacyRoot = Join-Path $env:ProgramData 'NeoControl'
$taskExecutable = Join-Path $taskInstallRoot 'ram-control-center.exe'
$taskUninstaller = Join-Path $taskInstallRoot 'ram-control-center-uninstaller.exe'
$taskBinaryNames = @('ram-control-center.exe', 'ram-control-center-uninstaller.exe')

function Assert-PlainDirectory([string]$Path) {
    $candidate = [IO.DirectoryInfo]::new([IO.Path]::GetFullPath($Path))
    while ($null -ne $candidate) {
        if ($candidate.Exists -and ($candidate.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw "Deployment directory redirects: $($candidate.FullName)"
        }
        $candidate = $candidate.Parent
    }
}

function Invoke-ProductCommand([string]$Executable, [string]$Mode, [int]$TimeoutMs = 30000) {
    $process = Start-Process -FilePath $Executable -ArgumentList $Mode -PassThru -NoNewWindow
    try {
        if (-not $process.WaitForExit($TimeoutMs)) {
            $process.Kill()
            throw "$Mode timed out"
        }
        if ($process.ExitCode -ne 0) { throw "$Mode failed with exit code $($process.ExitCode)" }
    } finally { $process.Dispose() }
}

function Install-ProductBinary([string]$Source, [string]$Destination) {
    $staged = "$Destination.pending"
    $previous = "$Destination.previous"
    if ((Test-Path -LiteralPath $staged) -or (Test-Path -LiteralPath $previous)) {
        throw "An interrupted replacement requires inspection: $Destination"
    }
    $expectedHash = (Get-FileHash -LiteralPath $Source).Hash
    $movedPrevious = $false
    $placedReplacement = $false
    $verified = $false
    try {
        Copy-Item -LiteralPath $Source -Destination $staged
        if ((Get-FileHash -LiteralPath $staged).Hash -ne $expectedHash) {
            throw "Staged binary hash mismatch: $Destination"
        }
        if (Test-Path -LiteralPath $Destination) {
            # Retire the old image before moving the verified replacement into
            # place. Windows can reject an overwrite while retaining image sections.
            [IO.File]::Move($Destination, $previous)
            $movedPrevious = $true
        }
        [IO.File]::Move($staged, $Destination)
        $placedReplacement = $true
        if ((Get-FileHash -LiteralPath $Destination).Hash -ne $expectedHash) {
            throw "Installed binary hash mismatch: $Destination"
        }
        $verified = $true
    } catch {
        if ($placedReplacement) { Remove-Item -LiteralPath $Destination -Force }
        if ($movedPrevious) {
            if (-not (Test-Path -LiteralPath $Destination)) {
                [IO.File]::Move($previous, $Destination)
            }
        }
        throw
    } finally {
        if (Test-Path -LiteralPath $staged) { Remove-Item -LiteralPath $staged -Force }
        if ($verified -and (Test-Path -LiteralPath $previous)) {
            Remove-Item -LiteralPath $previous -Force
        }
    }
}

# Validate the complete release before stopping any running application.
foreach ($name in $taskBinaryNames) {
    if (-not (Test-Path -LiteralPath (Join-Path $ReleaseDirectory $name) -PathType Leaf)) {
        throw "Release is missing $name"
    }
}
Assert-PlainDirectory $taskInstallRoot
Assert-PlainDirectory $taskLegacyRoot
Invoke-ProductCommand (Join-Path $ReleaseDirectory 'ram-control-center.exe') '--input-hooks-smoke'

$taskOwnedExecutables = @($taskExecutable, (Join-Path $taskLegacyRoot 'neo-control.exe'))
Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -in $taskOwnedExecutables } | ForEach-Object {
    Stop-Process -Id $_.ProcessId -Force -ErrorAction Stop
}
New-Item -ItemType Directory -Path $taskInstallRoot -Force | Out-Null

# Keep one canonical executable. A failed copy leaves the previous file intact.
foreach ($name in $taskBinaryNames) {
    $source = Join-Path $ReleaseDirectory $name
    $destination = Join-Path $taskInstallRoot $name
    Install-ProductBinary $source $destination
}

# Migrate application-owned data and optional local driver payload copies.
$taskDataFiles = @(
    'boot-profile.ini', 'flexi-recovery.marker',
    'repair-backup/keyboard-class-before.reg', 'repair-backup/mouse-class-before.reg', 'repair-backup/user-run-before.reg',
    'drivers/acpibridge/AcpiBridge.inf', 'drivers/acpibridge/AcpiBridge.sys', 'drivers/acpibridge/acpibridge.cat',
    'drivers/hkkbdfltr/HKKbdFltr.inf', 'drivers/hkkbdfltr/HKKbdFltr.sys',
    'drivers/hkkbdfltr/hkkbdfltr.cat', 'drivers/hkkbdfltr/HKFltrInstaller.exe'
)
$taskMigrations = @($taskDataFiles | ForEach-Object { @{ From = $_; To = $_ } }) + @(
    @{ From = 'boot-agent.log'; To = 'migration/boot-agent.log' },
    @{ From = 'driver-install.log'; To = 'migration/driver-install.log' }
)
foreach ($migration in $taskMigrations) {
    $source = Join-Path $taskLegacyRoot $migration.From
    $destination = Join-Path $taskInstallRoot $migration.To
    if ((Test-Path -LiteralPath $source -PathType Leaf) -and -not (Test-Path -LiteralPath $destination)) {
        Assert-PlainDirectory (Split-Path -Parent $source)
        if ((Get-Item -LiteralPath $source).Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw "Application data redirects: $source"
        }
        New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
        Copy-Item -LiteralPath $source -Destination $destination
        if ((Get-FileHash -LiteralPath $source).Hash -ne (Get-FileHash -LiteralPath $destination).Hash) {
            throw "Migrated data hash mismatch: $($migration.From)"
        }
    }
}
Invoke-ProductCommand $taskExecutable '--install-boot-service'
# The service permits 120 seconds for transport readiness, then observes the
# CPU for 60 seconds. Leave 15 seconds for SCM status propagation.
$taskDeadline = [DateTime]::UtcNow.AddSeconds(195)
do {
    $service = Get-CimInstance Win32_Service -Filter "Name='RAMControlCenterBoot'"
    if ($service.State -eq 'Stopped') { break }
    if ([DateTime]::UtcNow -ge $taskDeadline) { throw 'Boot policy replay and CPU observation did not finish within 195 seconds' }
    Start-Sleep -Milliseconds 100
} while ($true)
if ($service.ExitCode -ne 0 -or $service.ServiceSpecificExitCode -ne 0) {
    throw "Boot policy replay failed: $($service.ExitCode)/$($service.ServiceSpecificExitCode)"
}
Invoke-ProductCommand $taskExecutable '--install-startup'
$taskRunValue = Get-ItemPropertyValue -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name RAMControlCenter
if ($taskRunValue -ne "`"$taskExecutable`" --background") { throw 'Startup registration readback mismatch' }
if (Get-CimInstance Win32_Service -Filter "Name='NeoControlBoot'") { throw 'Legacy boot service remains registered' }

$taskRegistration = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\RAMControlCenter'
New-Item -Path $taskRegistration -Force | Out-Null
$taskRegistryValues = @{
    DisplayName = 'RAM CONTROL CENTER'
    Publisher = 'RAM CONTROL CENTER contributors'
    InstallLocation = $taskInstallRoot
    DisplayIcon = $taskExecutable
    UninstallString = "`"$taskUninstaller`" --remove"
}
$taskRegistryValues.GetEnumerator() | ForEach-Object { New-ItemProperty -Path $taskRegistration -Name $_.Key -Value $_.Value -PropertyType String -Force | Out-Null }
foreach ($name in @('NoModify', 'NoRepair')) {
    New-ItemProperty -Path $taskRegistration -Name $name -Value 1 -PropertyType DWord -Force | Out-Null
}
$taskAppPath = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\App Paths\ram-control-center.exe'
New-Item -Path $taskAppPath -Force | Out-Null
Set-Item -Path $taskAppPath -Value $taskExecutable

# Only remove legacy copies after the new service and startup path are verified.
foreach ($migration in $taskMigrations) {
    $source = Join-Path $taskLegacyRoot $migration.From
    $destination = Join-Path $taskInstallRoot $migration.To
    if ((Test-Path -LiteralPath $source -PathType Leaf) -and (Test-Path -LiteralPath $destination -PathType Leaf)) {
        if ((Get-FileHash -LiteralPath $source).Hash -eq (Get-FileHash -LiteralPath $destination).Hash) {
            Remove-Item -LiteralPath $source -Force
        }
    }
}
$taskLegacyExe = Join-Path $taskLegacyRoot 'neo-control.exe'
if (Test-Path -LiteralPath $taskLegacyExe -PathType Leaf) { Remove-Item -LiteralPath $taskLegacyExe -Force }
foreach ($relative in @('drivers/acpibridge', 'drivers/hkkbdfltr', 'drivers', 'repair-backup', '')) {
    $directory = Join-Path $taskLegacyRoot $relative
    if ((Test-Path -LiteralPath $directory -PathType Container) -and @(Get-ChildItem -LiteralPath $directory -Force).Count -eq 0) {
        [IO.Directory]::Delete($directory)
    }
}
foreach ($root in @('HKCU:', 'HKLM:')) {
    foreach ($path in @('Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run', 'Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run32')) {
        $key = Join-Path $root $path
        if (Get-ItemProperty -LiteralPath $key -Name NeoControl -ErrorAction SilentlyContinue) {
            Remove-ItemProperty -LiteralPath $key -Name NeoControl
        }
    }
    foreach ($path in @('Software\Microsoft\Windows\CurrentVersion\Uninstall\NeoControl', 'Software\Microsoft\Windows\CurrentVersion\App Paths\neo-control.exe')) {
        $key = Join-Path $root $path
        if (Test-Path -LiteralPath $key) { Remove-Item -LiteralPath $key -Recurse }
    }
}

$taskReceipt = [pscustomobject]@{
    Installed = $taskExecutable
    Sha256 = (Get-FileHash -LiteralPath $taskExecutable).Hash
    Bytes = (Get-Item -LiteralPath $taskExecutable).Length
    BootService = $service.Name
    BootExitCode = $service.ExitCode
    LegacyFilesRetained = @(Get-ChildItem -LiteralPath $taskLegacyRoot -Force -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Name)
}
$taskReceipt | ConvertTo-Json -Depth 3
if ($StartBackground) { Start-Process -FilePath $taskExecutable -ArgumentList '--background' | Out-Null }
