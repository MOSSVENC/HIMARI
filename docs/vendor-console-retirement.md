# Vendor console retirement

## Scope

The current OEM package boundary on this machine is:

```text
CLEVOCO.FnhotkeysandOSD
  FnKey\FnKey.exe
  FnKey\CC40\CC40.exe
  FnKey\CC40\kb.dll and the CC4.0 UI resources
```

`CLEVOCO.ControlCenter3.0` was a legacy package installed locally from
`C:\Users\Administrator\Downloads\Clevo_ControlCenter_7.057.zip`
(Control Center 3.0 Package v7.057), archived 2026-11-06. It was not the
current OEM package and did not provide FnKey or OSD. It was removed on
2026-11-19 with `scripts/retire-vendor-console.ps1 -Execute`. The source archive SHA-256 is
`3f29bfc1a4307ee6014238e37e4bc6844d19b76bed4e5215c555fe4f5818afac`. The
receipt is stored at:

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

The current CC4.0 console is still installed inside
`CLEVOCO.FnhotkeysandOSD`; it was not removed by the CC3.0 cleanup.

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

Remove the locally installed legacy Control Center 3.0 package:

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
