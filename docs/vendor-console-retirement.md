# Vendor console retirement

## Current package state on this machine

Verified 2026-11-20 through `Get-AppxPackage`:

```text
CLEVOCO.FnhotkeysandOSD  7.101.0.0
  FnKey\FnKey.exe                      running
  FnKey\CC40\CC40.exe                  the CC4.0 console
  FnKey\CC40\kb.dll                    HKFltrLib client

CLEVOCO.ControlCenter3.0 6.85.0.0
  legacy Control Center 3.0 UI
```

`Control Center 4.0` is not an independent package. It is the `FnKey\CC40`
subdirectory of `CLEVOCO.FnhotkeysandOSD`, so the CC4.0 console cannot be
removed while FnKey and OSD are kept.

## Package structure of the official download

The official Firebat support page for `T6KD -> AMD HX55 -> Win 11` publishes
one combined item, `综合控制中心-CV`:

```text
Clevo_ControlCenter_7.057.zip
size 269.78M, updated 2026-03-16
SHA-256 3f29bfc1a4307ee6014238e37e4bc6844d19b76bed4e5215c555fe4f5818afac
oem.ini: Control Center 3.0 Package v7.057
```

It is an InstallShield wrapper, not a single program. Its `AppInstall`
directory carries several independently provisioned AppX packages:

```text
AppInstall/CC3.0/CC30.appxbundle            CC3.0 6.85.0.0
AppInstall/FnKey/FnKey.appxbundle           FnKey, OSD and FnKey/CC40/CC40.exe
AppInstall/BatteryUtility                  battery utility
AppInstall/CPU_OC                           CPU overclock UI
AppInstall/EnergySave                      energy-saving UI
AppInstall/FanSpeedSetting                 fan-speed UI
AppInstall/Flexikey                        key binding UI
```

So both console generations legitimately appear in one OEM package. The
overlap is the vendor's packaging decision, and `docs/original-package-map.md`
records the extracted layout.

## Correction: provenance of the removed CC3.0 build

An earlier entry in this document stated that `CLEVOCO.ControlCenter3.0
7.68.1.0` had been installed locally from the downloaded archive, and that
removing it was cleanup of a locally introduced package. That claim is not
supported by the evidence now on hand, and the document is corrected here.

Facts:

- The removed build was **7.68.1.0**, removed 2026-11-19 23:01.
- The downloaded archive installs **6.85.0.0**, not 7.68.1.0. A install of
  that archive cannot produce 7.68.1.0.
- The unpacked InstallShield payload on disk predates the download activity:
  `C:\Program Files (x86)\ControlCenter` was created 2026-03-23 20:12, and
  `AppInstall\CC3.0` plus `AppInstall\FnKey` were created 2026-09-27 22:25.
- AppX deployment log records retained on this machine begin 2026-11-06 02:08,
  so the original 7.68.1.0 install is older than the retained log and cannot be
  dated from it.

Consequence: whether 7.68.1.0 arrived with an earlier OEM provisioning or with
a separate manual install is **unproven**, and the removal was performed
against an unproven assumption. The restored package is the older
`6.85.0.0` from the public download, so the current state is not equivalent to
the state before removal.

The app's own behaviour was not a factor: `FnKey.exe`, `CC40.exe`,
`CCDCHUService` and the OEM filter drivers were never touched by the CC3.0
removal. The `vendor-console-retirement.json` receipt is retained unchanged and
must be read together with this correction.

### Recovery options, in ascending risk

1. Leave `6.85.0.0` installed. It is what the official download provisions and
   it is a working legacy console, just older than what was removed.
2. Remove `CLEVOCO.ControlCenter3.0` entirely. `FnKey`, OSD, `CC40.exe` and
   `CCDCHUService` do not depend on it, and a second console UI competing for
   the machine-wide `HKKbdFltr`/`HKMouFltr` filter state is exactly the input
   conflict documented in `docs/input-responsiveness-2026-11-19.md`.
3. Restore `7.68.1.0` exactly, if a factory image or a newer OEM package that
   ships it becomes available. Nothing on this machine currently provides it.

Option 2 is the direction the user asked for, but it must be done deliberately
and recorded, not as cleanup of an assumed mistake.

## CC4.0 target

The CC4.0 console executable lives inside the FnKey package. It cannot be
removed independently without removing the package that also contains FnKey and
OSD. The retirement script therefore keeps that package by default and never
removes it unless all of the following are supplied:

```text
-IncludeFnKey
-AcknowledgeFnAcceptance
C:\ProgramData\Himari\fn-owner-acceptance.json
  { "physicalFnAccepted": true }
```

The marker may only be created after physical Fn combinations, OSD feedback and
the mapped actions have been tested on the target machine. Until then,
`-IncludeFnKey` is deliberately rejected.

## Commands

Read-only inventory:

```powershell
pwsh -File scripts/retire-vendor-console.ps1
```

Remove the legacy Control Center 3.0 package only (keeps FnKey, OSD, CC40 and
CCDCHUService):

```powershell
pwsh -File scripts/retire-vendor-console.ps1 -Execute
```

The script stops only CC40/ControlCenter30/COUWP processes for the default
retirement. It does not stop FnKey or CCDCHUService unless `-IncludeFnKey` is
explicitly requested and accepted.

## Remaining gate

Physical Fn capture is still unverified. The first ownership run showed that
some firmware notifications are 4-byte payloads; the parser now accepts 4-byte
i32 and 8-byte i64 forms. A later 60-second takeover window acquired the
channel but recorded no new physical key notifications, so it is not counted as
physical acceptance.
