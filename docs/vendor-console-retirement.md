# Vendor console retirement

## Scope

The installed OEM software has two package boundaries:

```text
CLEVOCO.ControlCenter3.0
  ControlCenter30\ControlCenter30.exe
  separate legacy control-center package

CLEVOCO.FnhotkeysandOSD
  FnKey\FnKey.exe
  FnKey\CC40\CC40.exe
  FnKey\CC40\kb.dll and the CC4.0 UI resources
```

The separate Control Center 3.0 package does not provide the current FnKey or
OSD. It was removed on 2026-11-19 with
`scripts/retire-vendor-console.ps1 -Execute`. The receipt is stored at:

```text
C:\ProgramData\RAM Control Center\vendor-console-retirement.json
```

Result after removal:

```text
CLEVOCO.ControlCenter3.0          absent
CLEVOCO.FnhotkeysandOSD          Ok
FnKey.exe                        running
CCDCHUService                    Running / Automatic
RAM CONTROL CENTER               one background process
```

## CC4.0 target

The CC4.0 console executable is inside the FnKey package. It cannot be removed
independently without removing the package that also contains FnKey and OSD.
The retirement script therefore keeps that package by default and never removes
it unless all of the following are supplied:

```text
-IncludeFnKey
-AcknowledgeFnAcceptance
C:\ProgramData\RAM Control Center\fn-owner-acceptance.json
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

Remove the separate old Control Center 3.0 package:

```powershell
pwsh -File scripts/retire-vendor-console.ps1 -Execute
```

The script stops only CC40/ControlCenter30/COUWP processes for the default
retirement. It does not stop FnKey or CCDCHUService unless `-IncludeFnKey` is
explicitly requested and accepted.

## Remaining gate

Physical Fn capture is still being verified. The first ownership run showed
that some firmware notifications are 4-byte payloads; the parser now accepts
4-byte i32 and 8-byte i64 forms. A later 60-second takeover window acquired the
channel but did not record new physical key notifications, so it is not counted
as physical acceptance.
