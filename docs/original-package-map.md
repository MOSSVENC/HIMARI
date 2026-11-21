# Original Firebat T6KD package map

## Official source

The official Firebat support page is:

```text
https://www.firebat.com.cn/serve/drive
```

The page's real API is under `/api/fire/`:

```text
/api/fire/getDriveCategory
/api/fire/findDriveCategory
/api/fire/getByCategoryId
```

For `T6KD -> AMD HX55`, the official list contains
`综合控制中心-CV`:

```text
Clevo_ControlCenter_7.057.zip
size: 269.78M
updated: 2026-03-16 14:51:30
SHA-256: 3f29bfc1a4307ee6014238e37e4bc6844d19b76bed4e5215c555fe4f5818afac
```

`oem.ini` inside the archive identifies it as:

```text
Control Center 3.0 Package v7.057
```

## Why both CC3.0 and CC4.0 appear

The archive is not one executable. It is an InstallShield deployment wrapper
whose `AppInstall` directory contains several independently provisioned AppX
packages:

```text
AppInstall/CC3.0/CC30.appxbundle
  legacy Control Center 3.0 UI

AppInstall/FnKey/FnKey.appxbundle
  FnKey, OSD, and the nested CC40 UI (FnKey/CC40/CC40.exe)

AppInstall/BatteryUtility/BatteryUtility.appxbundle
  battery utility

AppInstall/CPU_OC/CPU_OC.appxbundle
  CPU overclocking UI (capability-dependent)

AppInstall/EnergySave/EnergySave.appxbundle
  energy-saving UI

AppInstall/FanSpeedSetting/FanSpeedSetting.appxbundle
  fan-speed UI

AppInstall/Flexikey/Flexikey.appxbundle
  FlexiKey editor/runtime

AppInstall/GPUOverclocking/GPUOverclocking.appxbundle
  GPU overclocking UI (capability-dependent)

AppInstall/Keyboard/Keyboard.appxbundle
  keyboard/RGB UI
```

Each subdirectory has `InstallPackage.bat`, and each script uses:

```text
Dism /Online /Add-ProvisionedAppxPackage
```

The shared hardware payload lives outside `AppInstall`:

```text
Driver/x64/HKKbdFltr.*       keyboard class filter
Driver/x64/HKMouFltr.*       mouse class filter
Driver/x64/HKClipSvc.exe     clipboard service
Driver/x64/HKFltrLib.dll     control interface used by CC40/kb.dll
DCHU/CCDCHUService.exe       AcpiBridge hotkey/notification service
AcpiBridge*.sys/.inf/.cat    signed ACPI bridge driver
```

So the duplication is an OEM deployment strategy: one driver bundle contains
legacy CC3.0 and current FnKey/CC4.0 modules, plus auxiliary AppX tools. The
hardware does not require both consoles; they are alternative UI layers sharing
the same vendor driver boundary.

## Current machine state

```text
CLEVOCO.ControlCenter3.0_6.85.0.0   restored from official CC30.appxbundle
CLEVOCO.FnhotkeysandOSD_7.101.0.0   installed (FnKey + CC40)
C:\Program Files (x86)\ControlCenter official 7.057 installation
```

The machine previously had `CLEVOCO.ControlCenter3.0_7.68.1.0`; that exact AppX
version is newer than the `CC30.appxbundle` inside the official 7.057 archive.
It was removed by a previous cleanup and its exact source package has not yet
been recovered. The restored 6.85.0.0 is a working official fallback, not proof
that the exact 7.68.1.0 package was restored.

## What can be rewritten

HIMARI already talks directly to the signed AcpiBridge device and
implements the user-mode DCHU, RGB, fan, power, GPU, input and OSD behavior.
The vendor control-center UI is therefore replaceable.

The signed kernel driver and firmware/EC paths are a different boundary. They
are required to reach the hardware unless a new signed kernel driver is built.
Replacing those is a separate driver project, not a UI rewrite, and must not be
silently mixed into the application rewrite.

## Safe cleanup rule

Do not remove `C:\Program Files (x86)\ControlCenter`, `AcpiBridge`, or the
keyboard/mouse filter payload while FnKey/CC40 or the current HIMARI
still uses them. Retire the AppX UI modules only after functional and physical
validation; keep the driver boundary until a signed replacement exists.
