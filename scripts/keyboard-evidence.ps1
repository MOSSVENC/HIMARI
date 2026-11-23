# Read-only snapshot for the boot-time keyboard/filter investigation.
# Run it immediately after a boot, before pressing the internal keyboard.
param(
    [string]$OutputDirectory = 'C:\ProgramData\Himari\input-evidence'
)

$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$path = Join-Path $OutputDirectory "keyboard-evidence-$stamp.txt"
$jsonPath = Join-Path $OutputDirectory "keyboard-evidence-$stamp.json"

function Section($title) { "`n===== $title =====" }

$snapshot = [ordered]@{
    captured_at = (Get-Date).ToString('o')
    last_boot = (Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToString('o')
}

$lines = New-Object System.Collections.Generic.List[string]
$lines.Add("captured_at=$($snapshot.captured_at)")
$lines.Add("last_boot=$($snapshot.last_boot)")

$lines.Add((Section 'services'))
$services = Get-Service HKKbdFltr, HKMouFltr, HimariBoot, CCDCHUService, DCHUService -ErrorAction SilentlyContinue
$lines.Add(($services | Format-Table Name, Status, StartType -AutoSize | Out-String))
$snapshot.services = @($services | Select-Object Name, Status, StartType)

$lines.Add((Section 'class upper filters'))
foreach ($class in @('{4d36e96b-e325-11ce-bfc1-08002be10318}', '{4d36e96f-e325-11ce-bfc1-08002be10318}')) {
    $key = "HKLM:\SYSTEM\CurrentControlSet\Control\Class\$class"
    $filters = (Get-ItemProperty $key -Name UpperFilters -ErrorAction SilentlyContinue).UpperFilters
    $lines.Add("$class -> $($filters -join ', ')")
}
$snapshot.upper_filters = @{
    keyboard = @((Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e96b-e325-11ce-bfc1-08002be10318}' -Name UpperFilters -ErrorAction SilentlyContinue).UpperFilters)
    mouse    = @((Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e96f-e325-11ce-bfc1-08002be10318}' -Name UpperFilters -ErrorAction SilentlyContinue).UpperFilters)
}

$lines.Add((Section 'vendor and HIMARI processes'))
$processes = Get-CimInstance Win32_Process |
    Where-Object { $_.Name -match 'FnKey|HKClipSvc|CC40|ControlCenter|COUWP|himari' } |
    Select-Object Name, ProcessId, CreationDate, CommandLine
$lines.Add(($processes | Format-Table Name, ProcessId, CreationDate -AutoSize | Out-String))
$snapshot.processes = @($processes)

$lines.Add((Section 'keyboard devices'))
$keyboards = Get-PnpDevice -PresentOnly -Class Keyboard -ErrorAction SilentlyContinue
$lines.Add(($keyboards | Format-Table Status, Problem, InstanceId -AutoSize | Out-String))
$snapshot.keyboards = @()
foreach ($keyboard in $keyboards) {
    $props = Get-PnpDeviceProperty -InstanceId $keyboard.InstanceId -ErrorAction SilentlyContinue |
        Where-Object { $_.KeyName -in @(
            'DEVPKEY_Device_BusReportedDeviceDesc',
            'DEVPKEY_Device_LocationInfo',
            'DEVPKEY_Device_Parent',
            'DEVPKEY_Device_Service'
        ) }
    $lines.Add("-- $($keyboard.InstanceId)")
    foreach ($prop in $props) { $lines.Add("   $($prop.KeyName)=$($prop.Data)") }
    $snapshot.keyboards += [ordered]@{
        instance = $keyboard.InstanceId
        status = "$($keyboard.Status)"
        properties = @($props | ForEach-Object { @{ key = $_.KeyName; value = "$($_.Data)" } })
    }
}

$lines.Add((Section 'USB hubs and problem devices'))
$hubs = Get-PnpDevice -PresentOnly -Class USB -ErrorAction SilentlyContinue |
    Where-Object { $_.FriendlyName -match 'Hub|Root' } |
    Select-Object Status, FriendlyName, InstanceId
$lines.Add(($hubs | Format-Table -AutoSize | Out-String))
$snapshot.usb_hubs = @($hubs)
$problems = Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue |
    Where-Object { $_.Status -ne 'OK' } |
    Select-Object Status, Class, FriendlyName, InstanceId
$lines.Add(($problems | Format-Table -AutoSize | Out-String))

$lines.Add((Section 'accessibility, layout, scancode map'))
foreach ($name in 'StickyKeys', 'FilterKeys', 'ToggleKeys') {
    $flags = (Get-ItemProperty "HKCU:\Control Panel\Accessibility\$name" -Name Flags -ErrorAction SilentlyContinue).Flags
    $lines.Add("$name Flags=$flags")
}
$scancode = (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Keyboard Layout' -Name 'Scancode Map' -ErrorAction SilentlyContinue).'Scancode Map'
$lines.Add("Scancode Map=$($scancode -join ',')")
$lines.Add('layouts=' + ((Get-ItemProperty 'HKCU:\Keyboard Layout\Preload' -ErrorAction SilentlyContinue | Select-Object * -ExcludeProperty PS* | Out-String)))
$lines.Add('HiberbootEnabled=' + (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Power' -Name HiberbootEnabled -ErrorAction SilentlyContinue).HiberbootEnabled)

$lines.Add((Section 'input guard log'))
$guardPath = 'C:\ProgramData\Himari\input-guard.log'
if (Test-Path $guardPath) {
    $lines.Add((Get-Item $guardPath | Select-Object CreationTime, LastWriteTime, Length | Format-List | Out-String))
    $lines.Add((Get-Content $guardPath -Tail 40 | Out-String))
    Copy-Item $guardPath (Join-Path $OutputDirectory "input-guard-$stamp.log") -Force
}

$lines.Add((Section 'recent input/boot events'))
$events = Get-WinEvent -FilterHashtable @{
    LogName = 'System'
    StartTime = (Get-Date).AddHours(-2)
} -ErrorAction SilentlyContinue |
    Where-Object { $_.ProviderName -match 'Kernel-Boot|Kernel-Power|Service Control|HID|USB|PnP' } |
    Select-Object -First 40 TimeCreated, ProviderName, Id, LevelDisplayName
$lines.Add(($events | Format-Table -AutoSize | Out-String))

$lines | Set-Content -Path $path -Encoding UTF8
$snapshot | ConvertTo-Json -Depth 6 | Set-Content -Path $jsonPath -Encoding UTF8
Write-Output "evidence: $path"
Write-Output "json:     $jsonPath"
