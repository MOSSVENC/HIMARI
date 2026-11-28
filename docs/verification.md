# Verification log

This document records historical runs and the CPU sampling check below.
Recent rendering and deployment measurements are in [Windows rendering](rendering.md);
the baseline repair record is [November 16](repair-status-2026-11-16.md).

Date: 2026-11-07
Target: Firebat/Clevo ZY-T6K, Windows host, signed AcpiBridge device

## Build

```text
target/x86_64-pc-windows-gnu/release/neo-control.exe
target/x86_64-pc-windows-gnu/release/neo-dchu-probe.exe
```

Last successful Release cross-build after the final audit fixes:

```text
neo-control.exe        11,046,912 bytes
sha256                 05e5eb53c6461a70a2de2702ee4124437d0a19f77c2548afe5f6255584f32b9f
neo-dchu-probe.exe     347,648 bytes
```

The main executable imports no `InsydeDCHU.dll`, `HKFltrLib.dll`, `CC40.exe` or
`FnKey.exe`. Its transport imports are limited to Windows system DLLs such as
`kernel32.dll` and `ntdll.dll`.
The original user-mode DLL role is implemented directly in
`neo-control-protocol`; no replacement DLL is loaded at runtime.

Final source gates:

```text
cargo fmt --all -- --check                         passed
cargo test --workspace                             40 passed
cargo clippy --workspace --all-targets \
  --target x86_64-pc-windows-gnu -- \
  -D warnings -D clippy::undocumented_unsafe_blocks passed
cargo build --release --target x86_64-pc-windows-gnu --workspace  passed
clean-target full verification                     passed
```

The same sequence is now the single local gate in `scripts/verify.sh` and is
also configured in `.github/workflows/ci.yml`, with a native Windows test job.

The separate hardware gate is `scripts/hardware-smoke.ps1`. It passed against
the deployed executable, including camera/touchpad/airplane restore, display
care readbacks and DCHU offsets 45-48.

## Direct transport reads

Live probe through:

```text
\\?\ACPI#CLV0001#1#{86994c74-ad43-4812-b7e7-0c420b5c5fd7}
```

Verified successfully:

- command 12 thermal buffer, including CPU/GPU RPM, duty and temperature;
- integer commands 16 and 70;
- settings page 1 offsets 1, 16, 36, 37, 38, 43 and 44;
- settings page 4 offsets 5 and 7;
- command 13 runtime fan curve;
- command 4 GPU mode read: current mode 3, supported mask `0x06`;
- full settings store read, exactly 4096 bytes.

All UI temperatures use an explicit `°C` suffix.

## Direct power and fan writes

A same-value smoke test re-applied the current values:

```text
Performance -> Performance
Automatic -> Automatic
```

Both writes persisted and read back successfully through the direct
AcpiBridge transport.

The full power transition was then re-applied:

```text
WriteAppSettings(1, 16, [2])
WriteAppSettings(1, 1,  [2])
SetDCHU_Data(121, [2, 0, 0, 25])
SetDCHU_Data(79,  [0, 0, 0, 0])
```

Readback after the transition:

```text
settings[1,1,1]  = 02
settings[1,16,1] = 02
DCHU command 79  = success
```

The boot-agent log contains `power reapplied and read back: Performance`.
The same transition sends the vendor Energy Star command. This result proves
command delivery and settings readback; it does not establish that cold-boot
CPU throttling ended. The boot service now records bounded CPU observations
after the policy replay; see [boot investigation](boot-performance-2026-11-19.md).

## CPU telemetry

The CPU clock is sampled from Windows PDH `% Processor Performance` against the
reported nominal/base frequency rather than a hard-coded value. A live sample
during verification was:

```text
clock=4782 MHz
nominal=3601 MHz
```

The current clock therefore changes with load; the nominal value is displayed
separately.

## Boot service

The same executable is registered as the native Windows service
`NeoControlBoot`:

```text
"C:\ProgramData\NeoControl\neo-control.exe" --boot-service --wait-ms 120000
```

Verified service configuration:

- display name: `HIMARI Boot Agent`;
- account: `LocalSystem`;
- start mode: automatic;
- dependency: signed `AcpiBridge` driver;
- no window and no persistent helper process.

The installer also configures SCM failure actions for 1 s, 2 s and 5 s retries
and enables them for non-crash service failure exits. A live reinstall/query
confirmed the configured actions, the service returned to `STOPPED` with exit
code 0, and the boot log recorded another `Performance` replay:

```text
power reapplied and read back: Performance
agent complete in 6 ms (1 attempts)
```

A live Service Control Manager start completed successfully:

```text
state after run: STOPPED
win32 exit code: 0
service exit code: 0
power reapplied and read back: Performance
agent complete in 7 ms (1 attempt)
```

Five SCM start-to-stop measurements completed in 60, 67, 69, 70 and 163 ms.
The first run paid the cold file-cache cost; the worker portion was 6-7 ms.
Install, reinstall and uninstall were exercised through the executable's own
service commands, and the reinstall returned the same service to the stopped
state with both exit codes at zero.

The boot path starts the signed AcpiBridge driver service on demand, retries at
10 ms, writes the full power transition and intentionally skips the expensive
thermal read before restoring the power profile. It does not write Auto fan
mode or create a Slint window.

## FlexiKey XML model

The protocol crate now has unit coverage for:

- parsing all four CC4.0 action types (`keypress`, `launch`, `text`, `disable`);
- round-tripping the vendor XML schema;
- skipping malformed bindings without losing the profile;
- converting keypress bindings to the signed keyboard-filter frame;
- changing the active profile in the model;
- adding, replacing and removing bindings;
- preserving vendor launch event IDs and text start/send keys during edits;
- accepting decimal and hexadecimal trigger codes while rejecting unsupported
  mouse buttons.

A live UI run loaded a temporary four-profile `profile.xml`, rendered the
FlexiKey page in Chinese, selected the second profile, and persisted it through
the atomic XML replacement. The resulting file contained `Active="1"` with
only the Work profile selected, the temporary `.neo.tmp` file was gone, and the
action engine logged a profile reload. The save-and-reload behavior also has an
app unit test.

The FlexiKey page and editor were rendered at the production 1180×820 size.
The editor exposes device and action chips, key capture, key code and
display-name fields, action-specific input, delete, cancel and atomic save
controls. The single-line path field stays on one line; long paths scroll
rather than wrapping into the field border.

A live editor run then exercised the real Windows hooks and profile writer:

```text
trigger capture armed
trigger captured: device=2 code=0x0040 name=Mouse Back
```

Saving that binding was rejected without touching the file because `0x40`
already belonged to another binding. The validation message named the duplicate
trigger. A second run captured the unused left button (`0x01`), saved
successfully, updated `profile.xml`, and reloaded the engine:

```text
profile loaded: 1 keyboard action(s), 3 mouse action(s)
```

The written first binding became `Device="2" KeyCode="1"` with the localized
display name `鼠标左键`. Deleting that binding then reduced the XML to three
`Set_` entries and reloaded:

```text
profile loaded: 1 keyboard action(s), 2 mouse action(s)
```

The temporary test profile was restored from its pre-test copy afterwards; the
runtime `C:\ComboKey\profile.xml` was not modified by this test.

FlexiKey profile management is implemented in the same transactional path:
new profiles are appended and selected, rename trims and validates the name,
and delete refuses to remove the final profile. The model unit tests cover all
three operations. The live UI created a second profile and persisted it as
`Active="1"`; the new profile row, name field, name-save button, two-step delete
button and reload control were rendered at the production size without
clipping.

## Panel low refresh rate

Live reads through the direct settings channel:

```text
settings[1,27,1] = 01   support
settings[1,28,1] = 01   current state
```

A same-value write to `settings[1,28,1]` completed and read back `01`. The
System page gates the control to Power saving and Quiet+ modes, matching the
vendor behavior.

## Display care

The CC4.0 `InsydeDriver.WriteAppSettings(index, length, buffer)` wrapper treats
the first argument as a linear settings index. Its `45..48` calls therefore map
to page 0 offsets, not page numbers. The direct transport was corrected to use
`page 0 / offsets 45..48` before enabling the UI write path.

The `--display-care-smoke` command returned exit code 0. Its same-value pass
covers taskbar auto-hide, taskbar transparency, dark appearance, AC/DC monitor
timeouts, screen saver, and DCHU page 0 offsets 45–48. The UI readback showed
`taskbar_hide=false`, `transparency=true`, `dark=true`, monitor timeouts
`0/0`, and screen saver `disabled / none / 0 min`.

The production UI was rendered at 1180×820 and the Display care page was
inspected in Chinese. The LCD badge, three immediate Windows toggles, AC/DC
monitor timeout steppers, screen-saver mode chips and apply controls fit without
clipping.

Live UI writes:

```text
Taskbar auto-hide: enabled  -> "显示保护设置已应用并读回。"
Taskbar auto-hide: restored -> "显示保护设置已应用并读回。"
Screen saver: same-value    -> "屏幕保护程序已应用并读回。"
```

Both DCHU mirror writes and Windows readbacks succeeded. The taskbar was left
in its original visible state.

## FlexiKey action engine

The app-level action engine is covered by unit tests for CC4.0 scan-code
mapping and profile persistence. A live test enabled the opt-in engine log,
loaded an active profile with an F24 launch binding, injected F24 into the
desktop, and verified:

```text
keyboard hook active
profile loaded: 2 keyboard action(s), 2 mouse action(s)
launch action dispatched: C:\Windows\Temp\neo-flexikey-action.cmd
marker file created: true
```

The XML replacement path is atomic through a `.neo.tmp` file and
`MoveFileExW(MOVEFILE_REPLACE_EXISTING)`. The saved active profile was reloaded
successfully in the live UI test and unit test.
Keypress, text, launch and disable share the same hook table. The hook lives in
the resident tray process, so closing the main window does not stop profile
dispatch.

Mouse dispatch was verified separately with XBUTTON1 (`Mouse Back`, vendor key
code `0x40`):

```text
mouse xbutton event: message=0x020b data=0x00010000
launch action dispatched: C:\ProgramData\NeoControl\mouse-marker.cmd
mouse xbutton event: message=0x020c data=0x00010000
marker file created: true
```

The down edge dispatched the launch action; the up edge was consumed by the
binding hook and did not produce a second action.

### Focused-window integration

A temporary WinForms text box was given focus, with IME composition disabled
so the ASCII result was deterministic. The active profile mapped:

```text
mouse back    -> text "Hello Neo 世界"
mouse forward -> keypress B
keyboard A    -> disable/swallow
```

The harness injected XBUTTON1, XBUTTON2 and A in sequence. The target text box
contained the exact UTF-8 text:

```text
Hello Neo 世界b
```

The engine log recorded both `text action dispatched` and
`keypress action dispatched`; the A trigger logged `trigger disabled` and was
not inserted into the target. With an active IME, scan-code keypresses behave
like the physical key and may enter composition, while text actions use
Unicode and bypass the IME.

The keypress sender now resolves the scan code to a virtual key with
`MapVirtualKeyW(MAPVK_VSC_TO_VK_EX)` and supplies both values to `SendInput`,
falling back to scancode-only injection when Windows has no VK mapping. This
keeps ordinary text fields and app shortcuts working while preserving the
vendor scan-code action model.

## Tray residency and logon startup

Verified through real Windows window handles:

```text
background: process alive, main window at (-30000, -30000), visible false
tray window: present
second launch: new process exits, existing window restored at (362, 110)
tray exit: process exits and tray window is removed
```

The embedded ICO was accepted by `CreateIconFromResourceEx`; the engine log
reported `tray icon: embedded logo` and `tray icon installed`. The current-user
Run entry was installed, removed, and reinstalled successfully:

```text
"C:\ProgramData\NeoControl\neo-control.exe" --background
```

No scheduled task or helper executable was added for tray residency.

## Anti-dust schedule backend

CC40's schedule layout was recovered from the installed `FAN.cs` and added to
the protocol crate:

```text
packed = day + (resume << 7) + (hour << 8)
SetDCHU_Data(121, [packed & 0xff, (packed >> 8) & 0xff, packed >> 16, 40])
WriteAppSettings(4, 81, [1, day | (resume << 7), hour, 0, 0])
ReadAppSettings(4, 81, 3)
```

The read/encode path has unit coverage and the Cooling page exposes a run-now
button plus weekday/hour/resume scheduling. A live same-device cycle restored
the previous schedule after a reversible disable test:

```text
before        = enabled, day 0, hour 0
disabled      = disabled, day 127, hour 0
after restore = enabled, day 0, hour 0
```

The readback and rollback path is now verified against the live device.

## Keyboard filter transport

The signed `HKKbdFltr.sys` interface is reached directly without
`HKFltrLib.dll`:

```text
\\.\{2dc8164f-3a23-4d71-9fa6-91f9ead88613}
```

Verified live:

- `CreateFileW` on the filter interface succeeds;
- `DeviceIoControl(0xBA400, [1])` enables the filter;
- `DeviceIoControl(0xBA400, [0])` disables the filter;
- `DeviceIoControl(0xBA40C)` clears the filter macro set;
- `DeviceIoControl(0xBA41C, [1])` enables the USB filter device;
- filter enable was restored after the test.

The 16-byte collection layout was cross-checked against the vendor collection
builder:

```text
key code:     mode=0, down_delay, up_delay, key, kind=0
macro down:   mode=0, delay, 0, key, kind=1
macro up:     mode=0, 0, delay, key, kind=2
macro frame:  u32 trigger_size, u32 action_size, trigger records, action records
```

The driver macro add/remove path is intentionally not used by the UI. The
resident user-mode action engine covers keypress, launch, text and disable
bindings, while the signed filter device is used for filter enable/disable,
USB filtering and verified clearing. A direct `0xBA404` smoke write returned
success, but the matching `0xBA408` removal returned Windows error `1168`
(`ERROR_NOT_FOUND`); the transactional complete-set path remains the safe
driver-level route if it is needed for compatibility.

## Renderer

- Slint notifier: `GraphicsAPI::NativeOpenGL`.
- GPU 3D engine activity was observed while the window is visible.
- UI animation and hardware polling are separated by a worker channel.
- Navigation updates the rail immediately. One shared capsule travels vertically
  over 320 ms, and the page content dims to 74% over 90 ms, swaps at a fixed
  position, then returns to full opacity over 170 ms.
- Page content uses opacity only during a switch. It does not translate, scale,
  or place a sheet over the workspace. A transparent input guard covers the
  content viewport while the swap is active.
- Native captures showed stable content geometry through the swap. The maximum
  blue-dominant pixel fraction in the content area was 0.093, confined to normal
  accent surfaces rather than a full-area overlay.
- Segmented controls share a single moving highlight pill rather than changing
  each button independently; supported groups use a 240 ms `ease-out-back`
  translation. Pointer presses also render a radial highlight like KernelSU's
  InteractiveHighlight.
- Brightness and effect choices update the requested state before the worker
  command is queued. The selected pill therefore moves immediately, and the
  page does not wait for a hardware response.
- RGB uses three draggable channel tracks. The preview swatch binds directly to
  the requested channel values with no intermediate release event.
- Light/dark switching interpolates the complete palette and gradient set over
  260 ms with an ease-out-cubic curve. Rapid repeated toggles settle on the
  correct final theme and segmented-control selection.
- Option controls share the same motion system: 200–220 ms surface/radius
  morphs, 240 ms `ease-out-back` selection indicators, 240 ms toggle-thumb
  travel, and 260 ms pressed-scale release feedback.
- Debug snapshots at the production 1180×820 size were used to verify the
  overview, performance, cooling, battery, settings, FlexiKey and display-care
  layouts after the empty status-container fix. Windows frame tracing remains
  the final native-runtime confirmation and is kept separate from the visual
  claim.
- `scripts/check-unsafe.sh` fails if a new source file gains an `unsafe` token
  or any allowlisted FFI file exceeds its current ceiling. The current total is
  90 tokens. Windows Clippy fails on any undocumented block.

### Settings backend readiness audit (2026-11-07)

The default GUI path now installs the Slint rendering notifier independently
of `NEO_CONTROL_FRAME_LOG`; that environment variable only controls periodic
frame-log output. The settings page binds its activity dot to an explicit
`renderer-ready` property and displays the reported API string. If no rendering
callback arrives, a 1.5-second watchdog replaces the placeholder instead of
leaving it indefinitely.

The native Settings page was captured at 0, 5 and 15 seconds. The full backend
card produced the same SHA-256 at all three points:

```text
a45d78f02f485c39824e6ac0f10979cfce8544ae5760c46baa41130f3a5cadbf
```

The visible value was `GraphicsAPI::NativeOpenGL`. `--settings-smoke` starts the
real Settings window, waits for both `renderer-ready` and a `GraphicsAPI::*`
report, then exits non-zero on timeout. Three consecutive native Windows runs
returned exit code 0.

Deployed Release artifact:

```text
size    11,052,032 bytes
sha256  753efd672d7d261163ab72148b7098671f5ec905461f5223a10ac4071f36daab
```

### Product frame and display-care response (2026-11-09)

The main window now uses the standard Windows application frame and remains
resizable between 1180×820 and 1920×1200. A native Windows capture confirmed
the minimize, maximize and system-menu style bits are present. The pixel RAM
mark is embedded as SVG for Slint and as a multi-resolution ICO for the tray.

The three display-care toggles now update their bound values on the click path,
then read the resulting Windows state immediately after the serialized command
finishes. They do not wait for the periodic extended snapshot. A full Release
build containing this change was deployed to the Windows host:

```text
size    11,070,976 bytes
sha256  a3126687602860e0ecb2b0657bbf98d63106645bf723fe933a4cdb46e452e37c
```

An interactive display-care click/readback pass could not be completed in this
session because the WSL-to-Windows PowerShell bridge timed out during the UI
automation attempt. The backend/settings readiness check and the window-frame
capture completed separately.

## 2026-11-06 interaction audit

The final interaction audit was performed against the native Windows process.
The Release PE imports `opengl32.dll` and system DLLs only, with no
`InsydeDCHU.dll`, `HKFltrLib.dll`, `CC40.exe` or `FnKey.exe` dependency.

- Native Windows screen captures verified the fixed top header, the single
  hardware-online pill in Overview, and the absence of global status banners.
- Fn lock changed its visible switch state approximately 26 ms after the click;
  the page stayed in place and no status banner appeared.
- The Lighting brightness capsule moved through intermediate positions and
  settled on the chosen option; the first visible motion appeared about 20 ms
  after the click.
- Dragging the R slider from 85 to 253 produced 13 sampled preview colours from
  `(87, 80, 200)` through `(170, 80, 200)` to `(253, 80, 200)`, confirming the
  preview follows pointer movement continuously.
- The Settings `Start with Windows` row was captured on Windows; the switch track
  and label both measured `center_y = 526.5 px`, a 0 px vertical offset.
- Idle frame diagnostics reported zero rendered frames per two-second interval;
  animation intervals produced continuous frame counts. Rendering remains GPU
  backed rather than a CPU software-rendering fallback.

## Thermal stability audit

The worker stores the latest valid thermal sample in `WorkerRuntime`. Full DCHU
readbacks refresh that cache; the intervening one-second snapshots reuse it
instead of sending an empty thermal update to the UI. Samples with no plausible
CPU or GPU temperature are ignored, and the cache is cleared only after three
consecutive invalid or failed full reads.

Temperature formatting maps values outside `1..=125 °C` to `--°C`, so an
all-zero EC response cannot render as `0°C`.

Native Windows screenshots taken at 0, 6 and 15 seconds showed:

```text
CPU   80°C → 92°C → 89°C
GPU   59°C → 59°C → 60°C
```

The temperature area remained populated for the full capture window. The action
log recorded only valid cached samples, with no ignored or failed thermal reads
in that interval.

The Windows deployment was relaunched with the audited binary and captured from
its real Win32 window. Temporary scripts, BMP/PNG frame series, and logs were
removed after verification; `C:\ProgramData\NeoControl` contains only the
canonical executable and boot metadata.

Thermal command 12 still takes roughly 330 ms at the firmware level. It runs on
the hardware worker and does not block the UI thread.

The worker uses a 1 s light system sample and a 5 s full thermal/extended read.
Completed control commands publish their status through a light snapshot first;
the next full read reconciles persisted and hardware values.


## Maintainability refactor runtime check

The refactored Release executable was deployed to
`C:\ProgramData\NeoControl\neo-control.exe` and exercised on the Windows host:

```text
visible window handle    median 63.1 ms   range 47.2–92.3 ms
background handle        median 62.9 ms   range 62.1–64.0 ms
hooks + tray + worker    median 466.9 ms  range 447.9–496.7 ms
```

All eleven extracted pages were captured at the production size: Overview,
Performance, Cooling, Fan curve, Battery, System, Lighting, FlexiKey,
Input & I/O, Display care and Settings. They retained live telemetry, Chinese
labels, the rail wordmark, GPU composition and the accepted motion system. No
captured page was blank or clipped.


## Backend/control module split

The protocol transport and app-side Win32 controls were split without changing
their public APIs:

```text
protocol/src/windows_backend.rs               484 lines
protocol/src/windows_backend/telemetry.rs     109 lines
protocol/src/windows_backend/power_fan.rs     559 lines
protocol/src/windows_backend/rgb.rs           233 lines
protocol/src/windows_backend/gpu.rs            89 lines
protocol/src/windows_backend/charging.rs      148 lines
protocol/src/windows_backend/input.rs          70 lines
protocol/src/windows_backend/quick_toggles.rs  95 lines

app/src/system_controls/mod.rs                 17 lines
app/src/system_controls/audio.rs              111 lines
app/src/system_controls/display.rs             96 lines
app/src/system_controls/power.rs              126 lines
app/src/system_controls/display_care.rs       465 lines
app/src/system_controls/radio.rs               85 lines
app/src/system_controls/registry.rs           108 lines
```

`windows_backend.rs` now contains handle lifecycle, raw IOCTL/settings access
and shared rollback helpers. The domain modules expose the same inherent
`Dchu` methods, so callers did not gain a new layer. Registry access is behind
safe helpers used by display-care and tray-startup code. Service, registry,
power-scheme, device-context, PDH and NVML resources now use RAII wrappers.

## Driver health and driver-only installation

### Required dependency audit

A fresh code-path audit found that the application opens `AcpiBridge` for the
DCHU transport and does not call `KeyboardFilter::open` from the FlexiKey
engine. The current FlexiKey implementation uses `WH_KEYBOARD_LL` and
`WH_MOUSE_LL`; `HKKbdFltr` remains a protocol-level optional path, not a current
runtime requirement.

### Live detection

Run on the Windows host:

```text
neo-control.exe --driver-smoke
acpi_ready=true
acpi_installed=true
keyboard_filter_ready=true
keyboard_filter_installed=true
acpi_payload_ready=true
keyboard_payload_ready=true
payload_path=C:\ProgramData\NeoControl\drivers
```

The driver payload exists only in the local ProgramData deployment. It is
ignored by Git.

### Idempotent installer helper

Run on the Windows host with the driver already available:

```text
neo-control.exe --install-drivers
exit=0
installer: start
installer: AcpiBridge already responding; nothing to install
```

This verified the helper's no-op path and log output. The UAC/`pnputil` path
still needs a clean-machine test with the required device absent.

### Overview UI

The driver-health card is rendered on the Overview page. It shows the required
`AcpiBridge` state, the optional `HKKbdFltr` state, payload presence, and an
explicit install button when the required driver is unavailable. Driver files
are loaded from the executable-adjacent `drivers/` directory or the
ProgramData fallback.

## CPU performance sampling — November 19, 2026

The Windows sampler calculates the displayed clock from the same PDH query's
`Processor Information(_Total)\% Processor Performance` and the nominal MHz
reported by `CallNtPowerInformation`. It requests `PDH_FMT_DOUBLE |
PDH_FMT_NOCAP100` (0x8200). The extra flag explicitly permits boost values.
The fixed `Processor Frequency` is retained as a diagnostic reference; firmware
current-MHz readings are no longer a live-clock fallback. When the performance
query cannot be opened, the application retries every five seconds. Unavailable
samples remain unavailable in the Overview.

Run the read-only diagnostic on Windows:

    himari.exe --cpu-frequency-smoke

It collects twelve samples, 250 ms apart. Each record includes both PDH format
variants read from the same collected sample, the reference frequency, and the
clock used by the application. On this machine the reference stayed at
3601 MHz while the initial measured clocks varied from 4549 to 5017 MHz.
Both format variants returned the same values above 100%; clipping by the
original format was **not reproduced on this machine**. The change removes
the nominal-frequency fallback and makes the sampling contract explicit.

Host tests cover boost conversion and missing/invalid performance values.
The Windows diagnostic completed all twelve valid samples. Its observations
are Windows performance-counter estimates, rather than per-core clock probes.
Neither these observations nor the service's successful profile readback
establish that cold-boot CPU throttling has been resolved. That remains a
separate boot-time hardware acceptance item.

Local evidence: `target/validation/vulkan-followup/native-7.stdout.log` and
`native-results.json` in that directory. The directory is excluded from Git.

## 2026-11-19 Quiet GPU power verification

Windows Release SHA-256:
`aff175b654d5615191fc24dd78b3c34746059fa1e54b134be35044848b522ecb`
(19,098,112 bytes). The installed background process remained a single
instance after deployment.

The recovered CC4.0 `0x0013` operation was exercised on the reference machine:

```text
off_exit=0
ACPI\NVDA0820\NPCF  CM_PROB_DISABLED  STARTED=false

on_exit=0
ACPI\NVDA0820\NPCF  OK  CM_PROB_NONE  STARTED=true
```

The helper log records both `disabling device node and NVPCF` and
`enabling NVPCF and device node` with `transition verified`. This validates the
runtime GPU power path and its immediate restore. It does not validate a reboot
into a different MUX mode. Windows native tests passed 74 application tests,
3 uninstaller tests and 34 protocol tests; strict Clippy passed.

## 2026-11-19 Sleep/resume policy restore verification

Windows Release SHA-256:
`a5503cbcbdaab91bc5c15edd29b8f436d06dd48496cf27af6f86242d4216a173`
(19,102,208 bytes). The implementation was subsequently refactored to use a
wall-clock gap watcher without adding Win32 unsafe boundaries.

The watcher detects a wall-clock gap of at least twelve seconds while polling
every five seconds, reads the persisted `power=2 fan=0` policy and reapplies
the power/fan transition. The earlier forced `WM_POWERBROADCAST` experiment is
not part of the current implementation. A real physical sleep/wake cycle
remains required for final acceptance.

## 2026-11-19 Fn ownership and OSD verification

Windows Release SHA-256:
`01d0fa84ea934cf5903ae3a2969c7a407dcd066e86028e92d8dc6963736208cf`
(19,128,832 bytes). The installed background process remained a single
instance.

The original `FnKey.exe` was running during deployment. The resident owner
therefore deferred instead of registering a second channel:

```text
Fn ownership deferred: FnKey.exe is running
```

Installed-release smoke commands completed with exit code 0:

```text
himari.exe --osd-smoke
himari.exe --fn-event-smoke 0xca
```

The OSD visual capture is
`target/validation/osd/osd-final.png`; it shows the bottom-center Caps Lock OSD with Chinese state text. Windows native tests passed
75 application tests, 3 uninstaller tests and 34 protocol tests; strict Clippy
passed.

These checks prove resident ownership arbitration, the production synthetic
event path and OSD rendering. They do not prove physical Fn capture or a
hardware transition after FnKey is removed; those remain separate acceptance
items.

### Fn channel ownership transition

A controlled ownership test stopped the original `FnKey.exe` and
`CCDCHUService`, started HIMARI, and observed:

```text
takeover_app_processes=1
Fn notification channel acquired
```

The test then stopped the owned listener, restarted `CCDCHUService` and
`FnKey.exe`, and relaunched HIMARI:

```text
restored_app_processes=1
restored_fnk=1
restored_service=Running
```

This proves the resident owner can acquire the direct AcpiBridge notification
channel after the vendor handler releases it and that the original runtime can
be restored. Physical Fn key presses during ownership and action-by-action
hardware validation remain open.

### Fn payload format follow-up

The first physical ownership window exposed a real transport mismatch:
some firmware notifications returned four bytes while the listener required
eight. The parser now accepts both forms, sign-extending the four-byte i32
payload. Protocol tests cover positive and negative narrow/wide values.

Deployed Release SHA-256:
`861777cdd55cc4b6b339deda1c38721df1aba30c12192f4b3f3df5193d7f656b`
(19,129,344 bytes). A second 60-second physical window acquired the channel and
restored FnKey/service/background process, but recorded no new key notifications:

```text
Fn notification channel acquired
restored_app_processes=1
restored_fnk=1 restored_service=Running
```

Therefore the four-byte parser fix is verified by unit tests, but the physical
Fn action/OSD loop is still not accepted.

### Legacy Control Center 3.0 retirement

The locally installed legacy `CLEVOCO.ControlCenter3.0` package was removed with
`scripts/retire-vendor-console.ps1 -Execute`. The receipt confirms the package
was removed while `CLEVOCO.FnhotkeysandOSD`, FnKey, CCDCHUService and the
HIMARI background process remained present. The package containing
the CC4.0 console remains in place pending physical Fn acceptance.

## 2026-11-19 final safe-boundary refactor

Windows Release SHA-256:
`0f75f81fd548704b860b6e6c424c327500109bc6823cfd1d9b89a20cd1bbf51a`
(19,143,680 bytes). The installed background process remained a single
instance.

The new Windows-facing modules were refactored so the project unsafe ratchet
does not increase:

```text
unsafe total: 100 (ceiling 100)
```

Implemented/verified in this build:

- GPU runtime power uses the elevated self-helper plus built-in `pnputil`;
  off produced `CM_PROB_DISABLED`, on restored `OK/CM_PROB_NONE`.
- OSD smoke and synthetic `0xca` event completed with exit code 0; the final
  screenshot is `target/validation/osd/osd-final.png`.
- Fn owner remained deferred while `FnKey.exe` was running.
- `CLEVOCO.ControlCenter3.0` package remained absent; the FnKey package
  remained present.
- Windows native tests: 77 application, 3 uninstaller, 35 protocol.
- Strict Windows Clippy and host `scripts/verify.sh` passed.

Still open: physical Fn action capture after the 4-byte payload fix, physical
sleep/wake validation, and the guarded removal of the FnKey package containing
the CC4.0 console.

## 2026-11-20 OSD theme following and directional page transitions

Windows Release SHA-256:
`298a65f45b6b9f5ce0985993acdec2216f29a2ab598d5b167a64a0a3bd9aa7e6`
(19,147,264 bytes).

The panel theme is now a single persisted state instead of a per-window
default. `theme.ini` stores `dark=0|1`; the parser rejects unknown keys and
unrecognised values so a truncated file cannot leave the UI undefined. The
main window and the OSD overlay each receive the same palette state because
Slint instantiates a `global` once per window.

Verified on the installed build:

- `theme.ini dark=1` -> OSD centre pixel `R28 G32 B44` (dark surface `#1C202C`),
  with the light-on-dark text and cyan accent rendered.
- `theme.ini dark=0` -> OSD centre pixel in the light surface family, with
  dark text, light `#F7F9FD` card and cyan accent border.
- A capture taken 600 ms into the display window shows the card still
  contracted and offset upward; a capture at 1600 ms shows it settled. The
  reveal animation therefore runs on the real path rather than being a static
  layout property.
- `--osd-smoke` returned 0 for both themes.

Evidence:

```text
target/validation/osd-theme/seq-dark-600.png
target/validation/osd-theme/seq-dark-1600.png
target/validation/osd-theme/seq-light-600.png
target/validation/osd-theme/seq-light-1600.png
target/validation/osd-theme/capture-osd.ps1
target/validation/osd-theme/capture-seq.ps1
```

Page navigation no longer fades through a full-surface scrim rectangle. The
page surface animates `x` by 18 px in the navigation direction together with
`opacity`, so the outgoing and incoming pages move apart instead of the whole
workspace flashing to the background colour. The graphics harness now also
asserts `page-offset == 0` at settle, which previously could have stayed
shifted without failing.

### Animation budget: still not accepted

`--graphics-smoke` on the installed build:

```text
renderer=backend=Vulkan renderer=Skia adapter=AMD Radeon(TM) 610M
transitions=22 final_ui_state=passed submitted_frames=2416
submission_interval_p50_ms=3.53 submission_interval_p95_ms=6.19
submission_interval_max_ms=55.52 gaps_over_33ms=2
```

The measurement isolates first-visit page construction rather than per-frame
animation cost. Round one contains every outlier:

```text
round=1 page=1 max_acquire_ms=25.09
round=1 page=2 max_draw_ms=50.72
round=1 page=4 max_present_ms=31.98
```

Every round-two transition stays below 10 ms worst case
(`max_total_ms` between 4.33 and 9.60). The 16.67 ms budget therefore still
fails on a cold navigation, and the remaining work is page warm-up, not
animation timing. Submission intervals are also not scanout timing.

Windows native tests: 73 application, 4 uninstaller, 35 protocol. Host
`scripts/verify.sh` passed and the unsafe ratchet stayed at
`unsafe total: 100 (ceiling 100)` because the theme work added no FFI.

## 2026-11-20 input-guard cadence and dependency hygiene

Windows Release SHA-256:
`b29d38705f3f5bce939699bdba833e6716d54640f99092b048ad076106abe645`
(19,147,776 bytes).

The OEM keyboard/mouse class filter is a machine-wide device shared with the
vendor stack, and it has no read-back path: every correction is a blind write
of `IOCTL_ENABLE_USB_DEVICE(0)` and `IOCTL_ENABLE_FILTER(0)`. The guard used to
repeat that pair every five seconds for the whole session lifetime, which is
twelve shared-device writes per minute forever and an unnecessary opportunity
to desynchronise the vendor's own filter state.

The cadence is now time-bounded:

```text
0..180 s after guard start   5 s    boot protection kept
after 180 s                 60 s    steady state
```

Verified on the installed build: `input-guard.log` recorded
`input filter guard settled to the steady-state cadence` at 11:32:40, exactly
180 seconds after the deployed start, and the process stayed single-instance.

This reduces the write rate, it does not prove the input failure is fixed. The
remaining acceptance is still whether external USB input survives a real
reboot, which requires a user-arranged restart.

Dependency hygiene: `windows-sys` was declared unconditionally in both
crates while every use sits behind `cfg(windows)`, which produced
`warning: unused dependency` on host builds. It moved to
`[target.'cfg(windows)'.dependencies]` in both manifests and the host build is
warning-free. `Cargo.lock` is unchanged.

## 2026-11-20 OSD transparency: attempted, measured, withdrawn

The vendor overlay is a WinForms `OsdForm` window carrying `WS_EX_LAYERED`
(observed `exstyle=0x90008`) and composited through
`UpdateLayeredWindow` with a 32-bit premultiplied ARGB bitmap. Its art is
alpha-bearing PNG line work, so soft edges, translucent panels and shadows are
free. It never presents through a GPU swapchain.

HIMARI presents the overlay through Slint -> Skia -> Vulkan. A
DXGI flip-model swapchain surface carries `DXGI_ALPHA_MODE_IGNORE`, so the
compositor receives an opaque surface. Setting the Slint window `background`
transparent therefore renders nothing, which is why the earlier attempt
produced an invisible window.

A layered-window colour key was implemented and measured instead:

```text
SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED | WS_EX_NOACTIVATE)
SetLayeredWindowAttributes(hwnd, 0x00FF00FF, alpha, LWA_COLORKEY [| LWA_ALPHA])
```

The window accepted the style: enumeration reported
`ex=0x80C0118`, i.e. `WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOPMOST`.

Pixel probes of the captured 320x84 overlay:

```text
key-only   (alpha 0)   outside the card: (251,0,251) (200,0,200) (208,0,208)
key+alpha  (alpha 0xE0) outside the card: (231,13,238) (199,10,204)
card interior:                            (36,43,59)
```

Reading those numbers:

- the colour key was **never applied**: pixels outside the card still carry the
  key colour, at varying blends produced by the card shadow;
- the constant alpha **was** applied: the key colour is blended with the
  desktop instead of removed.

So on this renderer path one can obtain a uniformly translucent rectangle
whose surroundings remain visible, but not a transparent window. A blended
pixel also stops matching the key, which rules out fading the overlay.

The implementation was withdrawn rather than shipped as an ineffective Win32
boundary; `unsafe total` returned to `100 (ceiling 100)` and the overlay is
once again an opaque card, which is the honest current capability.

### What real parity would require

`UpdateLayeredWindow` with a premultiplied ARGB device-independent bitmap,
i.e. rendering the overlay into a CPU-visible bitmap each frame instead of
presenting a swapchain surface. That is the vendor's approach and the only way
to obtain per-pixel alpha here. It is a second renderer for the overlay
(GDI or D2D text plus our own glyphs), not a parameter change, and it replaces
Slint's animation path for that window.

### Correction: the Vulkan surface cannot be transparent here

The layered-window measurement above showed that a colour key is ignored for
this surface. The next step was to ask the swapchain for a premultiplied
composite mode instead, since Slint already creates every window with
`with_transparent(true)` and only the surface alpha was missing.

A diagnostic that recorded each `Swapchain::new` attempt reported:

```text
rejected PreMultiplied: a validation error occurred
accepted Opaque
```

twice, once per window. `VkSurfaceCapabilitiesKHR.supportedCompositeAlpha` on
this driver therefore offers no alpha-bearing mode at all, and the swapchain
cannot carry per-pixel alpha regardless of how the window is styled. Slint's
own layer was never the problem.

The fork now tries `PRE_MULTIPLIED` first and falls back to the advertised
modes, which is a no-op here and a real improvement on drivers that accept it.
The overlay stays opaque, and the documentation no longer implies otherwise.

Remaining options for a translucent overlay, none of which are parameter
changes:

1. A native layered overlay (`UpdateLayeredWindow` with a premultiplied ARGB
   bitmap and GDI text) — the vendor's approach, independent of the GPU
   renderer, but it replaces Slint for that window and adds real FFI.
2. A separate overlay process using a pixel-software renderer, which renders
   into a CPU buffer instead of a swapchain.
3. Changing the whole application renderer to `femtovg`/OpenGL, which accepts
   alpha on Windows but discards the verified Vulkan path.

## 2026-11-21 Native layered OSD with real per-pixel alpha

The Vulkan surface cannot be translucent here, so the overlay was rebuilt as a
Win32 layered window that presents a premultiplied ARGB bitmap, which is the
mechanism the vendor overlay uses.

Implementation in `crates/app/src/osd_layer.rs`:

- a top-down 32bpp DIB section per surface, selected into a memory DC, so the
  pixels can be composed directly in Rust and handed to `UpdateLayeredWindow`;
- the rounded panel, its one-pixel antialiased edge, the accent bar and the
  text are composited per pixel with premultiplied colours; text coverage comes
  from GDI rasterising the glyphs into a second mask bitmap, so font
  antialiasing is the engine's rather than a hand-rolled approximation;
- a single presentation path keeps the window at the stored origin, which is
  what put the first working build in the corner: `render` re-presented at
  `(0,0)` and dragged the window back after `move_to` had positioned it;
- the window is a `STATIC` popup carrying
  `WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`, so it
  never takes focus from the foreground application.

Verified on this machine with `--osd-layer-smoke` and a full-screen capture:

```text
target/validation/osd-theme/layer3-full.png
```

The card renders at the requested origin, the game behind it is visible
through the panel and the rounded corners are fully transparent. Earlier
attempts recorded in this file — colour key, constant alpha, swapchain
composite mode — were all dead ends on the same surface.

FFI is confined to this module: `unsafe total: 110 (ceiling 110)`, ten
boundaries, each with a safety comment. The overlay is not yet wired into the
Fn dispatch; this commit adds the verified primitive and a smoke entry point.

## 2026-11-21 The overlay is wired into the Fn path

The Slint overlay window is retired. Fn events now reach the native layered
overlay through the same `osd::show` entry point, so nothing in the dispatch
had to change:

- `osd_layer::start` spawns one `osd-overlay` thread that owns the layered
  window for the process lifetime;
- `osd::show` assembles the presentation on the UI thread, because the palette
  and the anchor geometry both live there, then queues it to that thread;
- later requests cut the current display short instead of queueing behind it,
  which keeps the overlay honest when keys are pressed in quick succession;
- the anchor is clamped to the desktop, so a hidden or unplaced main window
  cannot push the card off-screen.

Panel, title and detail colours are chosen from the live palette blend, and
the accent comes from the event's own colour, so the card follows the panel
theme the same way the rest of the UI does.

Verified on the installed build with `--osd-smoke`:

```text
target/validation/osd-theme/native-osd-full.png
```

The card shows `大写锁定 / 开启` over the desktop with the wallpaper visible
through the panel.

The Slint overlay component and its `bars.slint` re-export were deleted with
it, and `osd::set_theme` went away because the overlay reads the palette when
it presents. 78 application, 4 uninstaller and 35 protocol host tests pass,
strict Windows Clippy is clean, `unsafe total: 110 (ceiling 110)`.

## 2026-11-21 Missing processor-hot release, and why performance looked capped

The user reported that opening the vendor control center pushed the package to
about 5 GHz, while HIMARI stayed near 4.2 GHz. The vendor decompilation
explains it. Its AC/DC and power-profile handler opens with:

```text
DCHU(121, 42, 1)                      decoded text: Talk EC Release PROC_HOT
num = current power mode
Thread.Sleep(100)
PowerLineStatus = SystemInformation.PowerStatus
switch (num) { ... apply the per-mode fan and profile tables ... }
```

HIMARI never sent that command, so a limit latched by the embedded controller
survived; the vendor released it every time its console re-initialised.

Implemented:

- `Dchu::release_proc_hot` sends `121/[1,0,0,42]`;
- it runs at boot-policy replay, after sleep/resume, on every profile switch,
  and is available as `--release-proc-hot` for measurement.

Measurements on this machine, same package, minutes apart:

```text
99 °C, 4,957 RPM, during sustained compilation   4,200 MHz
81-86 °C, 5,300 RPM, after the load stopped     4,770 MHz
the same, after the release command              4,870 MHz
installed build replaying at boot                4,981 MHz
```

Read honestly: the 4.2 GHz the user first saw was **thermal**, produced by hours
of sustained WSL compilation from this session, not by a regression. The
release command is a real missing step and is worth roughly +120 MHz at equal
temperature; at a cool package the sequence reaches ~4.98 GHz.

Boot log evidence on the installed build:

```text
processor-hot limit released
```

## 2026-11-21 Overlay glyphs and level bars

The overlay was text plus a single accent bar, which carried no state beyond the
words. It now draws a vector glyph per event and a five-step level bar where a
step value exists, so a backlight step and a lock toggle are distinguishable at
a glance rather than by reading.

`crates/app/src/osd_glyph.rs` describes fourteen icons as normalised line and
arc primitives in a 48-unit box and rasterises them with an analytic distance
field, so strokes stay smooth at any display scale. Nothing is taken from vendor
artwork.

Rendering additions in the overlay:

- two-stop vertical gradient inside the panel, so it reads as glass instead of a
  flat block;
- a one-pixel hairline border following the rounded outline;
- the glyph stroked in the event accent colour;
- a five-segment level bar under the detail line, dimmed for empty steps.

Event mapping: backlight to the keyboard glyph with its level, Fn Lock to a
lock, fan notices and noiseless mode to a fan, profile cycling to a gauge,
lock keys to a key cap, touchpad, camera, microphone, dock, radio waves, window
and silent mode to their own glyphs, and failures to an alert ring.

Verified on the machine with `--osd-layer-smoke`
(`target/validation/osd-theme/icon-osd-full.png`): the card shows the keyboard
glyph, `CAPS LOCK`, `ON` and three lit steps.

Still missing for parity: the four undecoded triggers (airplane, combo, gc,
microphone mute) have no glyph because they have no decoded event yet, and
physical Fn feedback still belongs to FnKey while it holds the channel.

81 application, 4 uninstaller and 35 protocol host tests pass, strict Windows
Clippy is clean, `unsafe total: 111 (ceiling 111)`. Deployed Release 51211480…,
boot service exit 0.

## 2026-11-21 The quiet tier that powers down the discrete GPU

The Fn power key cycled only through platform profiles. The UI path already had
`apply_quiet(dchu, disable_dgpu)`, which applies Quiet and then powers the
discrete GPU down, but the Fn path never touched the GPU at all — so cycling to
quiet left the discrete GPU running.

The Fn cycle now builds a tier list instead of a profile list:

```text
Performance, Entertainment, PowerSaving, Quiet     (only where supported)
QuietWithoutDiscreteGpu                            (when nvidia_power_off)
```

Notes on the gating, because it is not the same flag:

- the platform profiles are filtered by `supports_power_mode`, and this machine
  reports `quiet: false`, so the plain quiet profile is not offered here;
- the GPU tier is gated by `nvidia_power_off`, which this machine does report,
  so the tier exists even though the quiet profile does not. It applies the
  quiet profile when the platform accepts it and otherwise keeps the active one,
  but it always powers the discrete GPU down.

Entering the tier calls `gpu_power::request(false)`; leaving it calls
`request(true)` before the next profile is applied. The OSD names the tier and
states which GPU action happened, using the crescent glyph for the GPU tier.

`gpu_power::is_disabled` answers from a marker file written when a power
transition succeeds, rather than paying an elevated device query inside a key
press. The cycle therefore reflects the state it last applied, which is what it
needs to decide the next step.

Tests cover the three gating combinations: the tier appears with the capability,
disappears without it, and stays independent of the quiet profile flag.
84 application, 4 uninstaller and 35 protocol host tests pass, strict Windows
Clippy is clean, `unsafe total: 111 (ceiling 111)`. Deployed Release 97bd1757…,
boot service exit 0.

## 2026-11-21 The in-app wordmark, the filled glyph, and a cached frame

Three defects from the rename and the new renderer, all found by looking at
real captures rather than at the code.

**The wordmark still said RAM.** The header is two separate Slint `Text`
elements — `RAM` on one line, `CONTROL CENTER` on the next — so the rename
sweep, which replaced the joined string, never matched them. The window title
was already `HIMARI`, which made the mismatch easy to miss. Now the header
reads `HIMARI / CONTROL CENTER`, letter-spaced to suit the shorter word, and
the two catalog entries follow.

**Every glyph drew as a filled block.** `distance_to_arc` computed the
out-of-sweep distance by projecting onto a segment that ended at the sample
point, which always gives zero, so every arc pixel counted as covered. The fan
came out as a solid square. The fix returns the distance to the nearer endpoint,
and a test now asserts the fan outline: empty centre, stroked ring, and fewer
than 900 of 2304 samples lit.

**Held frames repainted the whole surface.** Composition is split: panel,
border, text and level bar are baked into `base` once per presentation, and a
held frame copies `base` and redraws only the 34×34 icon box. That made the
anti-dust rotation affordable — the glyph turns while the card is held at
roughly 60 frames per second.

Evidence on the machine, `--fn-event-smoke 0x40` through the real dispatch path
(`target/validation/osd-theme/dust-*.png`):

```text
card: 散热 / 正在运行防尘 · CPU 5298 / GPU 3085
two frames 600 ms apart during the hold: 454 pixels differ in the icon box
```

The fan notice now names both fans, because the platform runs a CPU and a GPU
fan and a single figure would hide which one moved.

## 2026-11-21 Attributing frame gaps to the page that caused them

The graphics smoke reported a single maximum interval, which said nothing about
which page stall it belonged to. Each submission interval is now recorded with
the transition step it fell inside, and the three worst are printed with their
round and page.

First useful result on this machine:

```text
transitions=22  p50=3.38 ms  p95=4.26 ms  max=30.60 ms  gaps_over_33ms=0
transition_gap round=1 page=8  interval_ms=30.60   (FanCurve, first visit)
transition_gap round=1 page=3  interval_ms=14.21
transition_gap round=2 page=8  interval_ms=12.68
```

So the cold cost is the fan-curve page on its first visit, and round 2 is ten
times cheaper: it is construction, not steady-state drawing.

**Experiment that failed, kept on the record.** Converting all eleven
`if selected-page == n : Page {}` blocks into always-instantiated components
with `visible: selected-page == n` moved the cost rather than removing it:

```text
max=38.60 ms  gaps_over_33ms=1   worst: round 2 page 7 (InputIo) 38.60 ms
```

Instantiation was not the expense; showing is. The change was reverted and the
baseline above is the current behaviour. The remaining 13.9 ms over the 16.67 ms
budget on the fan-curve page is an open item, not a solved one.

## 2026-11-22 FanCurve first visit: 30.60 ms → 10.2 ms

`CurveStepper` is a low-count control: nested layout, four Texts, two button
Rectangles and four animation controllers. FanCurve used it 41 times. It now
uses `RpmCell` for the forty curve values — one value line, two faint signs and
a single `TouchArea` that steps down on the left half and up on the right.
`CurveStepper` remains for the OFFSET control.

Four `--graphics-smoke` runs on the reference machine (build with attribution
in both rounds; external AOSP clone had finished, load average 0.4):

```text
page 8 first visit  max_total_ms  9.33  19.49  10.03  10.42   (median 10.2)
page 8 draw stage   max_draw_ms   6.40  16.06   7.29   7.30   (median 7.3)
```

The page that used to be the worst gap no longer appears in the top three:
`10.42 ms` is inside the 16.67 ms budget. One run measured `19.49 ms` under
load; it is kept here rather than dropped.

**Wrong first attempt, kept on the record.** The first `RpmCell` gave the root
rectangle `opacity: enabled ? 1 : 0.62`. Skia then composited forty offscreen
groups every frame:

```text
page 8 draw stage   max_draw_ms  46.71  26.19  26.22  51.26  48.75
page 8 first visit  interval_ms  53-88 ms
```

The disabled state is a text colour now. Drawing cost fell from ~27 ms to
~7 ms, which is the whole difference.

The global maximum has moved to page 5 (Settings), `transition_gap round=1
page=5 interval_ms=58.18`, while its own draw stage is only `7.70 ms`. That gap
sits outside the Vulkan stage timings — acquire/layout/scheduling — and is the
next open item, not a fan-curve problem.

## 2026-11-22 Platform profile: monitor-aware OSD and capability-gated GPU power

The overlay used to place itself with arithmetic against the primary work area
only. `platform_profile` now owns the pixel decision:

- `card_origin(work, scale, border)` is pure: bottom centre, DPI-scaled, clamped
  so a card wider than the work area pins to its left edge instead of leaving
  the screen. Seven unit tests cover 1366×768 at 100 %, 3840×2160 at 150 %,
  a negative-origin secondary monitor, a narrow work area, a partly off-screen
  window and a NaN scale factor.
- `work_area_near(window)` uses `MonitorFromPoint` + `GetMonitorInfoW` so the
  card follows the monitor holding the window centre; the primary work area
  from `SPI_GETWORKAREA` remains the fallback. That is the only new FFI: the
  ratchet moved 111 → 113 deliberately, with both blocks commented.
- `discrete_gpu_power_supported(capabilities)` gates the NVPCF path on the
  platform's `nvidia_power_off` flag. A model without that flag is told the
  control is unsupported instead of receiving a failing `NVPCF` write; a failed
  capability query is logged and leaves the existing signed path unchanged.

Evidence on the reference machine (1920×1080, taskbar visible):

```text
pp-test.exe --gpu-power-status
  NVPCF_INSTANCE=ACPI\NVDA0820\NPCF
  NVPCF_STATUS=OK

pp-test.exe --osd-layer-smoke  (captured 1200 ms in, docs/images/osd-capslock-2026-11-22.png)
  card 320×84 logical, bottom-centre of the work area, above the taskbar

pp-test.exe --graphics-smoke
  run 1: max 42.98 ms  gaps_over_33ms 1
  run 2: max 19.70 ms  gaps_over_33ms 0   page 8 first visit 7.02 ms
```

Host suite after the change: 152 tests (111 app + 4 uninstaller + 37 protocol),
unsafe 113/113, clippy clean.

## 2026-11-22 Cooling page: 24.16 ms → 16.8 ms

Cooling carried ten `CurveStepper` instances for the anti-dust schedule and the
CPU/GPU curve points. They are `RpmCell`s now. `RpmCell` gained a `signs`
property: the +/- hints are conditional elements, and the dense Cooling rows turn
them off, so those twenty Texts are not constructed at all (the halves still step
on click).

Three `--graphics-smoke` runs after a warm start:

```text
run 1  page 2 total 36.73 ms  draw 35.21 ms   (first run after the build)
run 2  page 2 total 18.39 ms  draw 16.21 ms
run 3  page 2 total 16.81 ms  draw 15.45 ms
```

The page moved from the worst gap (`24.16 ms`, recorded above) to 16.8–18.4 ms:
inside the 16.67 ms budget on the warm run and 0.1–1.7 ms over on the other.
It stays an open item until repeated runs are consistently under the budget.

Eight further warm runs of the same build (`co2.exe`) put the distribution on
record instead of one number:

```text
global max ms   16.34 16.56 17.48 18.97 19.25 20.12 21.34 21.62   gaps_over_33ms 0 in all
page 2 max ms   16.29 16.51 16.52 16.66 16.75 17.80 18.77 19.01
```

Page 2 is at or just under the 16.67 ms budget in four of eight runs (median
16.7 ms); the remaining 1–2 ms is the open gap. Nothing exceeded 33 ms, so the
user-visible stall is gone, but "consistently under budget" is not yet true and
is not claimed.

## 2026-11-22 Fn channel arbitration on the reference machine

`scripts/test-fn-events.ps1` ran against the Release build (`fn-test.exe`) with
administrator rights. It checks three states: vendor services running, the
channel released, and the vendor listener restored.

```text
service-occupied (expected rejection)
  Fn notifications: coordinate Fn notification ownership failed: CCDCHUService is Running
  -> exit 1, no subscription registered

released-1 (FnKey stopped, CCDCHUService/DCHUService stopped)
  Fn notifications: subscription registered
  Fn notifications: timeout iteration=0 ms=164
  Fn notifications: timeout iteration=1 ms=154
  Fn notifications: timeout iteration=2 ms=155
  Fn notifications: subscription closed
  -> exit 0

released-2  same lifecycle, 150/157/153 ms timeouts, exit 0
Original vendor services and FnKey restored
  DCHUService.exe  PID 2004   ...\acpibridge1.inf_amd64_0a24f05be94f40ec\DCHUService.exe
  FnKey.exe        PID 25776  ...\CLEVOCO.FnhotkeysandOSD_7.101.0.0_x64__6h6z29zh29qx0\FnKey\FnKey.exe
```

So the app refuses the occupied channel instead of fighting the vendor process,
acquires it when the vendor listener stops, and the diagnostic restores both
services and the original FnKey process. This does not yet prove a physical Fn
keypress reaches our decoder: that needs a capture window while FnKey is stopped,
which is still open.

## 2026-11-22 Discrete GPU backend naming and capability status

`discrete_gpu_power_supported` became `DiscreteGpuBackend`
(`nvidia-platform-controller` / `unsupported`), so the capability decision has a
name that is separate from the vendor command encoding. `--gpu-power-status`
now prints both:

```text
GPU_POWER_BACKEND=nvidia-platform-controller
GPU_POWER_SUPPORTED=true
NVPCF_INSTANCE=ACPI\NVDA0820\NPCF
NVPCF_STATUS=OK
```

A model without `nvidia_power_off` reports `unsupported` and the NVPCF path
returns "does not report discrete GPU power control" before touching the node.
The command encoding itself stays inside `gpu_power`; moving it behind a second
backend needs a non-NVIDIA machine to test against, which this workspace does
not have.

## 2026-11-22 Bounded Fn capture tool

`himari --fn-capture <seconds>` registers the notification channel, prints every
notification as `raw=<value> message=<..> event=<..>`, ends at the deadline and
closes the subscription. `scripts/fn-capture.ps1` stops FnKey and the DCHU
services, runs the capture window, then restores both.

A 10-second dry run with no key presses on the reference machine:

```text
Fn capture window: 10s. Press the Fn combinations now.
Fn capture: subscription registered
Fn capture: window ended after 10s, events=0
Fn capture: subscription closed
Original vendor services and FnKey restored
```

So the capture path itself is proven: registration, a full window, a clean close
and vendor restoration. The remaining evidence is a run where physical Fn
combinations are pressed inside that window; the tool now exists for it.

## 2026-11-22 Anti-dust card with both fans, after the platform-profile change

`cap.exe --fn-event-smoke 0x40` presented the AntiDust notification and the card
was captured 1.3 s in ([image](images/osd-antidust-2026-11-22.png)):

```text
散热
正在运行防尘 · CPU 4216 / GPU 2001
```

The card is 320×84 logical at the bottom centre of the work area, carries the
spinning fan glyph and reports both fan speeds in one line. This is the same
path the platform-profile placement change runs through, so the card position
and the per-monitor work-area query are exercised by the real overlay, not only
by unit tests.

## 2026-11-22 Fn capture and the resident instance: a real side effect, fixed twice

Running the first capture test while the installed HIMARI was resident exposed a
defect in the test procedure, not in the listener: when FnKey stopped, the
resident app's owner thread acquired the channel; when the script restored
FnKey, the resident app kept the subscription and the vendor listener was left
without it. The log showed `Fn notification channel acquired` as the last line
while `FnKey.exe` was running again.

Two fixes:

1. `scripts/fn-capture.ps1` now stops resident `himari.exe` instances for the
   capture window (never the executable under test) and starts them again with
   `--background` afterwards. An 8-second run then logged the capture lifecycle,
   restored the vendor services, and brought the resident app back as PID 20084
   with `Fn ownership deferred: FnKey.exe is running`.
2. `fn_owner` no longer waits forever. It waits in five-second slices and, when
   `FnKey.exe` reappears, logs `Fn ownership released: FnKey.exe returned` and
   drops the subscription so the vendor listener can register again.

The script fix is verified on the reference machine. The cooperative release in
the owner loop is code-complete and host-tested, but the installed binary predates
it: deploying needs a startup-task change, which the project constraints reserve
for the user, so its hardware run is still pending.

## 2026-11-22 Cooling first visit: the optimisations that did not help

Four experiments were built, measured on the reference machine and reverted.
They are listed so the next attempt does not repeat them.

```text
removed all four FanReadout rows (kept their 40px slots)
    page 2 max_total 18.58 ms   (baseline 16.8-19.0)  -> text rows are not the cost
removed opacity from StageButton and ToggleStatusRow (no extra layers)
    page 2 max_total 17.48-20.07 ms                   -> button layers are not the cost
removed the page opacity fade (no cross-fade at all)
    page 2 max_total 21.24-21.89 ms                   -> the fade is not the cost
cache-rendering-hint always on for the page Flickable
    page 2 max_total 19.95-27.35 ms, global max 57-73 ms -> actively worse
```

What remains is not one element: the page's 24 nested layouts and roughly forty
Text runs make the first frame's raster and text shaping the floor. `gaps_over_33ms`
stays 0, so there is no visible stall; the open gap is the last 1-2 ms above
16.67 ms on some runs.

## 2026-11-22 Where the Cooling first frame actually goes: glyph raster

The `render-tracing` feature was built for the Windows target and the smoke run
printed per-page text spans:

```text
page  1  text draw 5.05 ms   glyph 4.78 ms   font 0.00 ms
page  2  text draw 10.72 ms  glyph 10.01 ms  font 0.01 ms
page  3  text draw 6.37 ms   glyph 5.94 ms   font 0.01 ms
page  8  text draw 5.83 ms   glyph 4.91 ms   font 1.07 ms
page  0  text draw 0.63 ms   glyph 0.18 ms   font 0.01 ms
```

Text measurement is zero on every page. The Cooling frame spends 10 of its
roughly 16.8 ms inside `draw_text` glyph rasterisation; page 8 spends 4.9 ms for
its forty cells. So the remaining 1-2 ms above budget is a glyph-cache cost for
Cooling's mix of CJK labels, six text sizes and its own Latin strings, not a
layout, layer or animation cost (those were excluded in the previous section).

Ways forward, in order of risk:

1. pre-raster the app's text at startup while the window is not yet visible (the
   first submission already costs ~2.9 s, so 10-20 ms there is invisible);
2. reduce Cooling's distinct face/size pairs (a visual decision, not a code
   tweak);
3. accept 16.8-19 ms with zero gaps over 33 ms, which is the current state.

None of these is a one-line change, and the third is already true, so the budget
claim stays open rather than dressed up.

## 2026-11-22 Both OSD palettes through the product path

`--fn-event-smoke 0x40` was captured twice with `theme.ini` set to `dark=0` and
`dark=1`. The card pixel at (958, 965) on the 1920×1080 desktop:

```text
light  R=174 G=183 B=199   ([image](images/osd-antidust-light-2026-11-22.png))
dark   R= 22 G= 26 B= 37   ([image](images/osd-antidust-dark-2026-11-22.png))
```

So the persisted theme reaches the layered overlay through
`ui_callbacks::apply_persisted_theme` and the palette split in `osd::show_with`,
not only the main window. The theme file was restored to `dark=0` after the run.

`--osd-layer-smoke` is a different path: it builds the overlay directly with a
fixed dark style, bypassing the persisted palette. Its CAPS LOCK capture proves
placement and alpha, not theming; do not use it as palette evidence.

`--osd-layer-smoke` is no longer palette-blind: it now reads
`runtime_state::theme_dark()` and shares `osd::card_palette`, so the raw overlay
smoke and the product path render the same colours. Recaptured with `dark=0`,
the card pixel is again `R=174 G=183 B=199`, matching the AntiDust capture above.

## 2026-11-22 Cold-boot replay: what the logs actually prove

`(Get-CimInstance Win32_OperatingSystem).LastBootUpTime` is **2026-11-19
20:41:17**. The `uptime` reset seen at 23:54 on 11-21 was WSL restarting, not a
Windows reboot; it must not be cited as cold-boot evidence.

`boot-agent.log` has an entry at that boot (`uptime_ms=25468`, i.e. +28.0 s):

```text
1795092105074 agent start: target power=2 fan=0 wait_ms=120000 observe_ms=60000
1795092105146 power reapplied and read back: Performance
1795092105146 policy replay complete in 82 ms (1 attempts)
1795092105146 CPU observation start: duration_ms=60000 uptime_ms=25468
... 60 samples, clock 4613-4854 MHz, nominal 3601 MHz, profile=2, policy=2
```

So the boot service did start at a real Windows boot, replayed the saved
`power=2 / fan=0` policy and read it back in **82 ms**, then observed the CPU for
60 s. `Get-CimInstance Win32_Service -Filter "Name='HimariBoot'"` reports
`StartMode=Auto`, `State=Stopped`, `ExitCode=0` — a one-shot service that ran and
exited.

Still unverified by a new boot: the 1be0ba8 processor-hot-limit release and USB
input survival after a cold start. Those need a fresh Windows reboot, which this
workspace cannot trigger without the user.

One more typography experiment: `FanReadout`'s four columns were moved from 12 px
to 11 px (matching the curve cells). Page 2 first visit stayed at 17.2-18.3 ms,
so the cost does not track a single font-size class either. Reverted.

## 2026-11-22 Five-run first-visit report (canonical)

`scripts/perf-report.ps1` runs `--graphics-smoke` N times and prints the per-page
first-visit worst case as median/min/max plus the global maximum. Five runs on
the reference machine:

```text
global max_ms: median 21.10  min 18.65  max 35.58  gaps_over_33ms 1

page  median  min    max    <=16.67
0        3.72   2.97   5.67   yes
1        8.27   6.92   8.81   yes
2       18.60  18.22  35.51   no
3       10.24   9.44  14.55   yes
4        9.34   8.91  10.08   yes
5        9.01   8.64  12.45   yes
6        6.64   6.37   8.29   yes
7        6.87   6.76   9.05   yes
8        9.45   9.19  13.44   yes
9        6.94   6.64   9.11   yes
10      10.19   9.64  11.42   yes
```

Page 2 (Cooling) is the only page above the 16.67 ms budget. Its cost is glyph
rasterisation (10.0 of 16.8 ms in the traced build), and the earlier experiments
show it cannot be moved by removing rows, layers, the fade, the cache hint, a
font-size class or fractional page offsets. The systemic fix is a startup
prewarm of every page while the window is still off-screen or hidden; that is a
startup-path change with a real risk of a taskbar/window flash, so it is designed
and not executed without the user watching it.

## 2026-11-22 Reversible hardware smoke on the current build

`scripts/hardware-smoke.ps1` now runs every mode and reports the full failure
list instead of throwing on the first one. Against `theme-smoke.exe`:

```text
--control-sync-smoke    passed (stale readbacks, rapid edits, failed writes, drafts)
--input-hooks-smoke     passed (100 acknowledged transitions, empty/keyboard/mouse/both/removal/install failure)
--settings-smoke        passed
--system-smoke          brightness 75% reapplied, audio 69% unmuted, airplane false,
                        blue_light: the display driver rejected the gamma ramp
--io-smoke              camera toggled/restored, touchpad mirror 0-32 ms, airplane toggled/restored
--display-care-smoke    taskbar/transparency/dark/monitor timeouts/saver + DCHU offsets 45-48 readback
failures: --system-smoke (exit 1)
```

The blue-light failure is environmental, not a regression: a direct probe with
an **identity** gamma ramp also returns false from `SetDeviceGammaRamp` with
Win32 error 298, so this display state rejects every ramp (HDR or driver policy).
The other five modes and the rest of the system smoke pass on the refactored
build. Blue-light control stays unverified on this display until the ramp is
accepted again.

The blue-light failure above is now a capability, not a red run. `gamma_ramp_supported`
probes with an identity ramp once (cached in a `OnceLock`), releases the screen DC
and returns false when the driver rejects it. `--system-smoke` then prints
`blue_light: skipped, this display driver rejects gamma ramps` instead of failing:

```text
--system-smoke exit=0
brightness: 75% reapplied
audio: 69% unmuted reapplied
airplane: false
blue_light: skipped, this display driver rejects gamma ramps

scripts/hardware-smoke.ps1 -Executable gamma.exe
hardware smoke passed
```

So all six reversible modes pass on the refactored build; blue-light control itself
remains unverified while the display rejects every ramp. The UI still attempts the
write and reports the driver error; gating the control on the same probe is the
next step if this display state persists.

The same probe now gates the control. `ui_callbacks::apply_capabilities` runs
next to `apply_persisted_theme` at startup and sets `blue-light-supported`; the
Input/I/O page disables the BLUE LIGHT stepper and its apply button and prints
"This display driver rejects gamma ramps, so blue-light is unavailable" when the
value is false. On this machine the probe is false (the smoke prints the skip
line), so a user can no longer trigger the driver error from the UI.

Visual capture of the disabled control is still pending; the evidence here is the
probe result, the compiled binding and the full hardware-smoke suite passing.

`--snapshot=7` with the `snapshot-tools` feature now applies the persisted theme
and the capability probe, so a snapshot shows the same state a user gets. The
Input/I/O page on this machine renders with the BLUE LIGHT stepper's apply button
disabled and the localized reason:

```text
此显示器驱动不支持伽马校正，蓝光滤镜不可用
```

[image](images/input-io-blue-light-disabled-2026-11-22.png) — the rest of the
page (brightness, volume, device switches) stays enabled. This closes the
visual-evidence gap from the previous round.

## 2026-11-22 Full-page snapshot audit found two Cooling defects

`--snapshot=0..10` captured all eleven pages. On Cooling:

- the runtime label under the page title was still English ("Reading ten-point
fan table"); the zh-CN catalog lacked that msgid, so the Chinese page mixed
languages;
- `FanReadout` used fixed 38/86/54/52 px columns from the left while the Live
fans header right-aligned 80/50/48 px columns, so values did not sit under their
labels.

Both are fixed: `FanReadout` now uses `width: parent.width`, the same 8 px
spacing and the same 80/50/48 column widths as the header, and the catalog gained
"正在读取十点风扇表". [After snapshot](images/cooling-2026-11-22.png).

The "占空比" label that touched the card edge was a padding mistake: the header
row and `FanReadout` set `width: parent.width`, but the parent `VerticalLayout`
has 18 px padding, so the row was 36 px wider than the content area and overflowed
the card. Removing the width override lets the padded parent size the row; the
header and values now sit inside the card.

## 2026-11-22 Overview: the corner halo was washing out the fan rows

The page snapshot showed a 420 px accent circle at 45 % opacity reaching down to
FAN 3/4 and the DCHU badge, lowering their contrast. The halo is now 360 px at
18 % and sits wholly in the card corner; the fan rows and the badge are readable
again. [snapshot](images/overview-2026-11-22.png).

## 2026-11-22 System page: hard-coded English statuses

The page snapshot showed `Connected` and `Not located` in an otherwise Chinese
page. Both were literals bypassing `@tr`: the transport row used
`root.transport-ready ? "Connected" : "Unavailable"`, and the root
`transport-device-path` default was `"Not located"`. They now use `@tr`, and the
snapshot reads `已连接` and `设备路径 未找到`
([image](images/system-2026-11-22.png)).

`--snapshot` now also selects the translation bundle before creating the
component tree, because `@tr` property defaults are evaluated at instantiation.

Another font hypothesis was tested and rejected: `MicroLabel` moved from
Bahnschrift to Microsoft YaHei UI (to avoid CJK fallback in the Cooling header and
elsewhere). Four runs: page 2 median 18.95 ms (range 15.58-54.81), against the
18.60 ms baseline, so fallback is not the cost either. Reverted.

## 2026-11-22 Off-screen prewarm probe: no visible flash

`--prewarm-probe` (snapshot-tools build) creates the window, moves it to
(-4000, -4000), shows it, walks all eleven pages on 40 ms timers and hides it.

```text
prewarm probe complete pages=11      (process exit code 2170 from Slint's quit path)
```

A screen capture taken 350 ms into the walk shows only the desktop, taskbar and
the already-open PowerShell window: no HIMARI window, no taskbar button, no
flicker ([capture](images/prewarm-probe-2026-11-22.png)). So an off-screen glyph
prewarm is technically possible without a visible artifact on this single-monitor
desktop.

It is not wired into startup yet. Two open questions remain: whether Slint can
run the probe's event loop and then the resident UI's own loop in one process, and
what the extra 11 × 40 ms adds to the tray-resident start path. Both need the
user's acceptance of a startup-path change.

## 2026-11-22 Opt-in prewarm closes the budget, but the window flashes first

With `HIMARI_PREWARM=1`, the graphics smoke walks all pages off-screen before
measuring. Three runs:

```text
global max median 11.57 ms  gaps_over_33ms 0

page  median  min    max
0      4.45   3.62   5.15
1      5.46   4.91   5.64
2      5.31   4.99   6.90     <- was 18.60
3-10   2.65-3.85             all yes
```

Every page is inside 16.67 ms. The tray-resident start path also logged
`prewarm pages=11 elapsed_ms=792` and stayed hidden.

But the position trace at 100 ms intervals shows the window maps at its default
position before the move lands:

```text
100-200 ms   0,0        <- visible flash
300-500 ms   -4000,-4000
600-700 ms   0,0        <- after hide, before the centred restore
800 ms+      370,106    centred
```

So prewarm stays **opt-in** (`HIMARI_PREWARM=1`); it is not on by default. The
remaining problem is windowing, not glyphs: the window must be created off-screen
(or cloaked) before its first map. Until then the budget claim stays open for the
default start path even though the mechanism is proven.

## 2026-11-22 DWM cloak attempt: rejected, prewarm stays opt-in

To make the prewarm default without the 0,0 flash, the window was cloaked with
`DwmSetWindowAttribute(DWMWA_CLOAK)` right after `show()`, moved off-screen, walked,
then the cloak was released after the resident UI's show. Two variants were tried
(release at the end of the walk, and release 60 ms after show). Both left the
window invisible:

```text
250 ms  screen clean, window rect 0,0 (cloak appears to work)
750 ms  no window on screen
1700 ms no window on screen, taskbar icon present
```

A hidden window ignores the cloak release, and the release after show did not
recover it either. The cloak code and the default-on gate were reverted; the
prewarm remains opt-in (`HIMARI_PREWARM=1`). A working solution needs a window
created off-screen or cloaked before its first map, not a post-show attribute flip.

The unsafe ratchet also caught that the gamma capability probe added three FFI
calls to `system_controls/display.rs` without a ceiling update (the build gate
printed `unsafe regression: ... has 6 (ceiling 3)`). The ceiling is now 6 with the
reason in the script and the total 113 → 116.


## 2026-11-22 Physical Fn capture and offline vendor-string decryption

A 10-minute capture window (`--fn-capture 600`, timestamps added) produced real
notifications while the vendor listener was stopped:

```text
75.2s  0x95 -> ControlCenterToggle
76.7s  0x5d -> (was unmapped)
77.5s  0xd7 -> (was unmapped)
78.2s  0xfb -> vendor default branch
79.4s  0xfa -> vendor default branch
80.8s  0xfa -> vendor default branch
85.0s  0xf7 -> Camera(true)
85.7s  0x85 -> (was unmapped)
101.0s 0x82 -> BacklightStep(Up)
101.3s 0x82 -> BacklightStep(Up)
```

Vendor strings are obfuscated through `\u000f\u0010::\u0003(int)`, which reads
a resource from the calling assembly; calling it from a probe assembly returned
`X0X`. A `DynamicMethod` owned by the FnKey module fixes the caller context and
decrypts the real strings:

```text
IL_0cd9 (0x5d): w_arg253^ / \TPStatus.exe
IL_2460 (0xd7): SDCA029 / SDC4143 / clevofnkey^1ff   (F2 display switch, model-gated)
IL_1c1b (0x85): no strings; the branch calls the airplane toggle path (F11)
```

The decoder now maps 0x5d to touchpad toggle, 0xd7 to `DisplaySwitch`, 0x85 to
`AirplaneToggle`, and leaves 0xfa/0xfb unhandled because the vendor dispatch
itself sends them to the default branch. The keycaps supplied by the user
(F1 touchpad, F2 LCD, F10 camera, F11 airplane, F12 sleep) and the event order
line up with this mapping. `fn_events` tests pin all five IDs; the
protocol suite is 38 tests. GPU Turbo reports the real capability: this machine
does not expose `simple_gpu_oc`, so the action says so instead of pretending.


## 2026-11-25 Application icon replaced

The previous icon was a hand-drawn chibi portrait that did not match the
intended tone. It is replaced with artwork supplied by the user
(`assets/art/himari-art-clean.jpg`, 1328x1776). The "元宝 AI生成" watermark at
x 1170-1310 / y 1660-1748 was removed in place: the region is filled by cloning
the same-width texture from immediately above it, blended with a 14 px feather
and averaged against a second sample further up to break vertical streaking. At
normal viewing size the patch is invisible; at 3x zoom a faint horizontal smear
remains. `assets/art/himari-art-crop.jpg` keeps the alternative that simply cuts
the bottom 126 px losslessly.

Icon pipeline:

- `assets/himari_logo.png` is the source of truth: a 720x720 crop at (266, 90)
  scaled to 512, with 112 px rounded corners, a radial vignette and a soft
  top-left sheen. It carries alpha.
- `scripts/export-logo.py` renders 16/20/24/32/40/48/64/128/256 from that PNG
  through librsvg and packs `assets/himari_logo.ico` (9 entries, 200,403 bytes).
  It no longer rewrites the PNG, so repeated runs cannot degrade the artwork;
  the earlier version regenerated the PNG from an SVG that referenced the PNG.
- `assets/himari_logo.svg` is a thin wrapper for SVG consumers and is not used
  by the app; `crates/app/ui/main.slint` now points at the PNG for both the
  window icon and the in-app mark.
- Verified on the host: the built `himari.exe` reports a 32x32 associated icon
  showing the new portrait, and the running window shows it in the sidebar.
- 118 host tests still pass; the Windows release build succeeds.

## 2026-11-25 Icon takes the full artwork, uncropped

The user supplied a second, cleaner illustration (1536x1536, no watermark) and
asked for it to be used whole rather than cropped. `assets/himari_logo.png` is
now that image scaled to 512 with only the 112 px rounded corner mask: no crop,
no vignette, no extra compositing. Source kept as
`assets/art/himari-icon-source.jpg`.

`scripts/export-logo.py` rebuilt `assets/himari_logo.ico` (9 entries, 190,890
bytes) from it. Verified on the host: `ExtractAssociatedIcon` on the built
`himari.exe` returns a 32x32 icon showing the new portrait, and the running
window shows it in the sidebar header next to "HIMARI". The previous black-dress
artwork stays in `assets/art/` for reference.

## 2026-11-25 Icon sharpness: three real causes, all fixed

The deployed binary was replaced through the product's own life cycle this time:
`himari-uninstaller.exe --remove` (refuses while HIMARI runs; it reported
"Exit HIMARI from its tray menu before uninstalling" until the process was
stopped), then `scripts/deploy.ps1` with a staged release directory. The
uninstaller removed `himari.exe`, the `HimariBoot` service and the `Run` value,
retained recovery data, and reported a restart-pending entry for itself. The
deployment re-registered everything and reported
`Sha256 B95D92292BA1DEDEF5F7A19C5F9A3A807F5D1903A716AE9C0D3E92A0BC6D3A0D`,
`BootExitCode 0`.

The blur had three independent causes:

1. `tray.rs` scored every ICO entry against a hard-coded 32 px
   (`width.abs_diff(32)`), so the tray always received a 32 px image. This
   machine reports `SM_CXSMICON=16`, so Windows scaled 32 → 16. The selection now
   targets `GetSystemMetrics(SM_CXSMICON)`.
2. The ICO stored PNG-compressed payloads at every size. `scripts/export-logo.py`
   now emits 32-bit DIBs for 16/20/24/32/40/48/64 and keeps PNG only for
   128/256 (7 DIB + 2 PNG, 205,389 bytes). The decoder is a small pure-Python
   PNG reader, so the script needs only `rsvg-convert`.
3. `main.slint` set `icon:` to one 512 px image, which the backend scaled for the
   titlebar and taskbar. That property is gone; the new `window_icon.rs` loads
   the executable's own icon at `SM_CXICON` and `SM_CXSMICON` and applies both
   with `WM_SETICON` right after the window is shown (visible start and tray
   show). The unsafe ratchet gained the four documented FFI calls (116 → 120).

Verified: `ExtractAssociatedIcon` on the installed binary returns the new
portrait at 32x32, and the 16 px extraction shows the same colours, which
confirms the DIB encoder's channel order and alpha mask.

## 2026-11-26 16 px cannot hold a detailed illustration: flat mark for small sizes

Measured evidence: the tray on this machine is 16 px (`SM_CXSMICON=16`), and a
12x nearest-neighbour zoom of the tray icon showed the artwork reduced to
unreadable colour noise — the face occupies about six pixels. The 32 px taskbar
entry from the same ICO renders the portrait correctly, so this was never a
scaling bug at that size.

`assets/himari_logo_small.svg` is a flat mark that keeps the identity (silver
hair mass, skin, crimson lips, deep-crimson tile) with shapes wide enough to
survive 16 px. `scripts/export-logo.py` now renders 16/20/24 from that mark and
32 and above from the artwork, so:

```text
16 DIB  small mark      32 DIB  artwork
20 DIB  small mark      40 DIB  artwork
24 DIB  small mark      48 DIB  artwork
                        64 DIB  artwork
                        128 PNG artwork
                        256 PNG artwork
```

Reinstalled through the product life cycle again (uninstaller `--remove`, then
`deploy.ps1`), `Sha256 AA7C23780CE633A98FFCD8881AAFABEE6E772BBAA106A96F7C28157FA2F12C8C`,
`BootExitCode 0`. The tray cache was cleared and Explorer restarted before
capture.

Verified from a live screen capture at native pixels:

- tray (16 px): flat mark, single-pixel edges, no blur — previously an
  unreadable smear;
- taskbar button (32 px): the artwork portrait, sharp as a 32 px render can be.

## 2026-11-26 Correction: the flat mark only helps at 16 px

The first attempt applied the flat mark to 16/20/24. A side-by-side render at
16/24/32/48 ([comparison](images/icon-size-comparison-2026-11-26.png), left
artwork, right flat mark) shows that was wrong: from 24 px upward the artwork is
clearly better and the flat mark reads as a plain helmet-shaped head. The split
is therefore 16 px only:

```text
16        flat mark   (the artwork is colour noise at this size)
20 - 256  artwork
```

Reinstalled through the life cycle (uninstaller `--remove`, then `deploy.ps1`),
`Sha256 456129ADE8961A4E323D83FCE175B81D58023FBB0E56B5644B6006F211BB9085`,
`BootExitCode 0`; tray cache cleared and Explorer restarted.

Standing limitation, measured rather than assumed: this display reports
`SM_CXSMICON=16`, and a 1536 px illustration reduced to 16 px cannot stay sharp
in the tray or title bar. Sharpness there would require a purpose-drawn icon, not
a further scaling change.

## 2026-11-28 Page prewarm on by default, with a parked first map

Slint maps the window at its default position before `set_position` lands, so an
off-screen prewarm used to flash a full-size window at 0,0. The window is now
shrunk to 2x2 before its first map, moved off-screen, resized back and walked.

Position trace of a real `--background` start (100 ms sampling):

```text
100-200 ms   13x13  at 0,0        <- parked, effectively invisible
300-500 ms  1193x856 at -4000,-4000  <- walk happens here
600 ms+      13x13  at 0,0        <- hidden until the tray opens it
```

Measured page costs, three runs, no `HIMARI_PREWARM` set:

```text
global max median 25.07 ms, gaps_over_33ms 0
page 0 15.20  page 1 15.45  page 2 20.48 (Cooling)  page 3 16.02
page 4 18.00  page 5 14.37  page 6 14.82  page 7 14.76  page 8 17.83
page 9 14.04  page 10 15.56
```

Cooling is still above 16.67 ms and pages 4 and 8 also exceed it in the same run
while their minimums are 11-16 ms, so this harness is not yet a stable verdict.
The measurement also perturbs itself: `--graphics-smoke` runs the prewarm inside
the measured process, adding a second event loop and a window resize that cost
more than the glyphs saved.

Two concealment modes exist so the two purposes stop fighting: `Parked` for the
real startup path (2x2 first map) and `Solid` for measurement (off-screen only,
no resize). 156 tests pass; the Windows release builds.

Still open: a measurement path that does not show a window at all, and the
Cooling first-visit budget itself.

## 2026-11-28 Why the unsafe count cannot reach 60 without deleting features

Measured before attempting further reduction:

```text
unsafe occurrences in crates/            119
distinct Win32/ACPI/NVML calls             63
repeated calls worth sharing                1  (CloseHandle, 5 sites in 4 files)
unsafe fn / extern declarations            11
```

Every block is one documented FFI boundary; there is almost no duplication to
collapse. Deduplicating `CloseHandle` behind `close_owned_handle` in the protocol
crate removed exactly one block (120 -> 119, ratchet lowered to match). Reaching
60 would mean either merging unrelated calls into shared blocks, which makes the
safety argument weaker and the review harder, or deleting the capability behind
the FFI. Neither is a quality improvement, so the count target is replaced by
properties that can be enforced:

- every unsafe expression carries a SAFETY rationale (gate in
  `scripts/check-unsafe.sh`, verified to fail when one is removed);
- unsafe appears only inside `#[cfg(windows)]` platform sections;
- the per-file ratchet only ever moves down.
