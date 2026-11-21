# Windows rendering

The Windows UI is rendered by Slint 1.17's Skia renderer directly on a native Vulkan surface. Vulkan is required during backend selection; initialization failure stops GUI startup rather than silently selecting another API. The Windows-target dependency graph does not include wgpu. Slint's Vulkan surface owns instance/device/swapchain creation, and Skia records the UI draw commands.

The tracked renderer source is Slint's renderer crate with a narrow patch. It reports the physical adapter and successful queue submissions, supplies the correct Vulkan queue-family metadata to Skia, waits for acquired images before Skia draws, declares the enabled swapchain extension to Skia, transitions the rendered image to presentation layout, and waits for Skia GPU completion before presenting on the same Vulkan queue. The swapchain uses FIFO presentation. Review PATCHES.md alongside the source when upgrading Slint.

Swapchain images are created with color-attachment, transfer-source and
transfer-destination usages. Surface capabilities are checked before creation,
and the exact image usage is supplied to Skia's Vulkan ImageInfo. Ganesh's
check_image_info requires both transfer bits for a wrapped render target.
Leaving ImageInfo.image_usage_flags at zero makes Skia assume these bits; it
does not add them to an image created with color-attachment usage alone.

## Windows hardware validation

Build from WSL/Arch against the Windows SDK and MSVC libraries already mounted into the environment:

    ./scripts/build-windows.sh --features snapshot-tools

The WSL build script reuses a matching archive already cached under target/.
For a fresh checkout, it downloads the pinned rust-skia 0.99.0 Windows MSVC
archive into target/toolchains/skia/ and verifies its SHA-256 before starting
Cargo. SKIA_BINARIES_URL can override this location for a prepared cache.

Then run the bounded test with Windows PowerShell 7:

    pwsh -File scripts/test-graphics.ps1 -Executable '<release directory>\himari.exe' -OutputDirectory '<measurement directory>'

Add -RequireContentUpdates to require a GPU readback check of Overview content
invalidation: changing its temperature text must change the rendered pixels,
and restoring that text must reproduce the original image exactly. These
captures run after cadence and idle measurements.

The script requests a GPU snapshot in default-feature and snapshot-tools builds.
Add -RequireSnapshot to enforce that a readable image was written from the
Vulkan swapchain. The image is read while acquired by Skia, before presentation.
The capture frame runs after cadence and idle measurements because GPU readback
introduces synchronization.
To enforce a submission-gap budget, add -MaximumFrameGapMs 33.34 (or the
appropriate budget for the target display). The script writes results.json
before reporting a budget failure. Without this argument, BudgetStatus is
MeasuredOnly; passing the lifecycle checks is not a performance acceptance.

The isolated process starts no hardware worker, service, or input hook. It verifies the production navigation callbacks over all eleven pages twice, checks resize and minimize/restore, confirms zero queue submissions during a one-second idle interval, and writes a GPU readback of the rendered Overview when requested by the script. It also verifies that vulkan-1.dll was loaded and that the selected Vulkan adapter is integrated or discrete hardware.
The lighting capability is enabled in this isolated UI fixture so that its
sliders and color preview are rendered. The fixture does not exercise hardware
writes or establish correctness of device capabilities and telemetry.

The first submission interval is measured from Slint component creation to the first successful Skia queue submission; it excludes process creation and executable loading. Per-transition intervals measure CPU-observed submission cadence, not GPU timestamps, compositor latency, or display scanout. Windows CPU and memory values apply to this diagnostic process.

A separate snapshot-tools bar test exercises pointer movement and pixel endpoints for the RGB/monitor bars:

    pwsh -File scripts/test-bars.ps1 -Executable '<release executable>' -OutputDirectory '<measurement directory>'

The bar test's Slint take_snapshot() images are CPU-rendered layout checks.
They do not establish Vulkan pixel output. Graphics-smoke uses the renderer's
one-shot request_snapshot() instead and reports source=vulkan-swapchain. Even a
GPU readback verifies the target image rather than compositor timing or scanout.

Each transition also reports frame_stages with the maxima for acquisition,
target preparation, drawing, submission and presentation, plus the stages of
its longest frame. Acquisition includes frame cleanup and swapchain recreation.
Drawing includes layout/text/path work and any work Skia performs inside the
scene callback. Submission includes flushing and synchronous GPU completion.
These CPU wall times distinguish work inside a frame from gaps between render
callbacks; they do not establish which individual drawing operation was slow.

## Validation record — November 19, 2026

The Windows diagnostic build selected the AMD Radeon 610M integrated adapter
(vendor 1002, device 164e). The process loaded the Windows Vulkan loader. The
first complete run after the synchronization correction produced:

- 22 settled production page transitions;
- successful resize and minimize/restore;
- zero queue submissions during the one-second idle interval;
- 2,131 successful submissions during the measured transition windows;
- submission-interval median 3.24 ms, 95th percentile 4.35 ms, maximum 51.27 ms;
- first submission 1,354.52 ms after component creation;
- peak working set 253,198,336 bytes and peak private bytes 256,024,576 bytes;
- 3,078.125 ms of process CPU time over 15,830.07 ms elapsed.

A second independent window run completed the same lifecycle checks with a
560.96 ms first submission. A third run captured the application's actual
Windows window using PrintWindow and completed with a 703.21 ms first
submission. The screenshot and the off-screen layout image were visually
inspected. The RGB regression also passed ten value cases and thirty pointer
moves across the three channels.

Windows reported a 240 Hz display configuration during validation. Background
load was not frozen, and these are diagnostic samples rather than an idle-system
benchmark. In particular, the results do not establish display scanout cadence,
all interaction latency, complete feature parity, or a fix for low CPU frequency
after boot. Synchronous GPU completion is used for correct ownership transfer;
future asynchronous scheduling work should be evaluated against these checks.

Local evidence is kept in target/validation/native-vulkan/ (excluded from Git).

The production build (default features) also passed the 22-transition and window
lifecycle checks, followed by the Settings backend check. Its executable is
19,028,992 bytes. That sample recorded a 1,373.85 ms first submission, a 6.16 ms
95th-percentile submission interval, and a 133.78 ms maximum (five gaps over
33.34 ms); these outliers remain performance work. The installed canonical
executable was replaced atomically and its SHA-256 matched the Release build.
A production launch reported the same Vulkan adapter and continued sampling
real temperatures and changing CPU frequencies.

Validation gates passed: 73 host unit tests, 76 Windows unit tests, Windows
Clippy with warnings denied (all targets/features), formatting, and Release.
The application/protocol unsafe ceiling remains 102; vendored renderer source
retains its upstream FFI boundaries and is outside that application-only count.

### Stage measurements and shader settings

A stage-instrumented default Release measured a 40.44 ms maximum submission gap.
Its longest Cooling frame spent 36.63 ms in drawing and 1.27 ms in submission,
so removing GPU-completion waiting alone would not resolve that sample's stall.
The renderer now enables Skia's reduced_shader_variations context option.
Two successive Windows runs with that setting completed all 22 transitions,
resize, minimize/restore and idle checks. Both had a 4.32 ms 95th-percentile
submission interval; their maximum gaps were 36.08 ms and 31.91 ms. The first
run therefore still exceeded a 33.34 ms budget. First Cooling draws remained
33.37 ms and 28.64 ms. These sequential measurements have background-load and
driver-cache variation and are not a controlled estimate of the setting's
performance benefit. First-visit drawing latency remains open work.

The original native Vulkan integration's GitHub Actions run 44390756793 passed
both Linux verification and Windows tests, strict Clippy and Release build.

After enabling the full lighting fixture, two final default-feature Release
runs passed the explicit 33.34 ms submission-gap budget: maxima 33.11 ms and
31.93 ms, with 95th percentiles 6.14 ms and 4.45 ms. Both passed the 22-transition,
resize, minimize/restore and idle checks. These values still do not establish
240 Hz pacing or visual animation acceptance. The RGB regression passed ten
value cases and thirty pointer moves with the Vulkan renderer. Both the
instrumented snapshot and the final rendering measurements were inspected;
76 Windows unit tests, host verification and strict Windows Clippy passed.

### Swapchain usage and GPU readback verification

The image-usage correction and GPU readback were tested on a default-feature
Windows Release. The snapshot-source gate rejected the previously installed
build, which used Slint's CPU take_snapshot() path. The corrected build
reported source=vulkan-swapchain, and its 1180 × 820 GPU image was inspected.
The isolated fixture keeps hardware workers disabled, so its unavailable
sensors and driver labels are expected.

The final Release completed 22 page transitions, resize, minimize/restore and
the one-second zero-submission idle check on the AMD Radeon 610M. It measured
a 4.35 ms 95th-percentile submission interval and a 30.36 ms maximum. The
first Cooling draw took 26.27 ms. The explicit 4.17 ms budget returned
BudgetStatus=Exceeded. Two preceding runs also exceeded their 16.67 ms and
4.17 ms budgets, with maxima of 28.75 ms and 30.99 ms. Functional lifecycle
checks passed; first-visit latency and high-refresh pacing remain open.

Host verification passed 87 tests. Windows passed 90 native unit tests,
strict all-target/all-feature Clippy, Settings renderer initialization and
control-state synchronization checks. The Windows normal dependency graph
contains Skia, Vulkano and ash. The Release executable is 19,068,928 bytes.
The installed canonical copy was hash-verified against this Release and
launched with --background. It reported Vulkan / Skia / AMD Radeon 610M;
the installation directory contained neither a pending nor a previous image.

Local evidence is in target/validation/vulkan-latency/: release-graphics/,
baseline-gate/, native-results.json, final-verify.log and windows-clippy.log.
The inspected GPU readback is release-graphics/overview.png.

### Page-layer caching and content invalidation

The workspace uses Slint's GPU layer cache while navigation and overlays animate.
Page selection clears its cache-ready state; a 50 ms timer enables caching only
if that navigation request still owns the page. This defers allocation away from
the selection callback, but does not guarantee that a frame has been presented
before allocation. Theme interpolation draws directly until its final color.

Cached Vulkan layers use the parent target's color format. Renderer caches are
cleared on surface replacement, suspension, DPI changes, component destruction
and adapter replacement. Turning off a layer's cache hint releases its layer
entry. Overview GPU readback now verifies that a temperature change modifies
the pixels and that restoring the text reproduces the original image exactly.

Two final default-feature Release runs on the AMD Radeon 610M completed all
22 transitions, resize, minimize/restore, one-second zero-submission idle and
GPU content-update/restoration checks:

| Measurement | Run 1 | Run 2 |
| --- | ---: | ---: |
| Submission interval, median | 3.41 ms | 3.20 ms |
| Submission interval, 95th percentile | 4.28 ms | 4.24 ms |
| Maximum submission interval | 31.83 ms | 33.84 ms |
| First Cooling-page draw | 26.78 ms | 29.56 ms |
| Process CPU time | 2187.50 ms | 2281.25 ms |
| Diagnostic elapsed time | 14443.84 ms | 14441.58 ms |
| Component creation to first submission | 534.82 ms | 545.11 ms |
| 16.67 ms submission-gap budget | Exceeded | Exceeded |

The two installed-baseline runs recorded 2734.38–2875.00 ms CPU time,
4.32–4.38 ms 95th percentiles and 24.09–25.58 ms maximum gaps. The final runs
used less CPU time, but their worst gaps increased. Background load and driver
caches were uncontrolled. These results do not establish an improvement in
first-visit latency, smooth animation, or high-refresh display pacing. The
first complex-page draw and the 16.67 ms budget remain unresolved.

An earlier development run failed to settle page 10. Its cause was not
established; later runs passed the state checks. The failure log is retained
with the other local measurements.

Final validation passed 89 host unit tests, 92 Windows unit tests, strict
all-target/all-feature Windows Clippy, formatting, and the default Release
build. Settings initialization, control-state synchronization, CPU-frequency
sampling and all eleven FlexiKey UI callbacks also passed. The application and
protocol unsafe ceiling remains 102.

Local evidence is in target/validation/vulkan-followup/: release-1/results.json,
release-2/results.json, native-results.json, final-verify.log and
final-windows-clippy.log. The inspected final GPU image is release-2/overview.png.

The 19,074,048-byte Release was deployed to
C:\ProgramData\Himari\himari.exe and launched with
--background. The installed file matched Release SHA-256
4C9722E6FA76AC58EE090BCF253741F0FFA0568FB872F7B75D7F3593C14ABAA3. Its first
submitted frame reported Vulkan / Skia / AMD Radeon 610M. Only the canonical
background instance remained after validation, with no pending replacement
images. Startup registration and drivers were unchanged.

The installed binary returned twelve valid changing CPU-performance samples.
A separate read-only thermal check completed twenty valid samples over 26.97 s
(CPU 66–73°C, GPU 47°C), with no zero or empty sensor results in that interval.
Its first attempt had been stopped at the harness's 25 s deadline after eighteen
valid samples; the completed run used a 40 s deadline because each sample also
incurs synchronous device-read time. This short check does not establish
long-term sensor reliability or resolve cold-boot throttling. Deployment and
sensor evidence are deployment.json, deployed-renderer.txt,
installed-cpu-frequency-smoke.stdout.log and installed-thermal-results.json
in the same local directory.

### Text-stage profiling

Build the optional diagnostic configuration with:

    ./scripts/build-windows.sh --features render-tracing

Run scripts/test-graphics.ps1 against that build as above. Each frame_stages
record is followed by text_stages for the same longest frame: text drawing,
text measurement, glyph drawing and font lookup, in CPU wall milliseconds.
The recorder resets at the scene callback boundary and stores durations in
thread-local memory. Output is emitted after cadence sampling has finished;
render callbacks perform no profiling-log I/O.

These are inclusive spans: glyph drawing is inside text drawing, and font
lookup is inside glyph drawing. Lazy text measurement may also occur during
drawing. The values must not be summed. Timers themselves have overhead, so
use the default-feature Release for performance acceptance. Default builds
compile out the text spans, counters and additional timing fields.

The final diagnostic ran twice on the AMD Radeon 610M. Both runs completed
22 transitions, resize/minimize/restore, the idle check and GPU pixel
update/restoration checks, and emitted 22 matching text_stages records.
Their 16.67 ms submission-gap budgets were exceeded (37.53 and 30.47 ms).
For the longest first-visit Cooling frame:

| CPU wall time | Run 1 | Run 2 |
| --- | ---: | ---: |
| Scene drawing | 32.72 ms | 25.71 ms |
| Text drawing | 17.52 ms | 12.62 ms |
| Glyph drawing, inside text drawing | 16.59 ms | 11.91 ms |
| Font lookup, inside glyph drawing | 1.89 ms | 1.11 ms |
| Text measurement, potentially nested | 6.22 ms | 5.24 ms |

The measurements locate substantial work in the text/glyph drawing calls,
but do not distinguish all work done inside Skia or its driver calls. They
are diagnostic observations, not evidence of a rendering-speed improvement.
Early exploratory traces wrote logs inside the draw callback and are excluded
from these values. Local evidence: target/validation/frame-drawing/tracing-1/
and tracing-2/results.json.

The default-feature Release passed the same lifecycle and GPU-pixel checks
with a 4.28 ms 95th percentile and a 36.38 ms maximum submission gap. Its
16.67 ms budget was exceeded. The optional text recorder was absent from
that build. Host verification passed 89 tests; Windows passed 92 native
unit tests and strict all-target/all-feature Clippy. These builds have not
replaced the installed 4C9722E6 image recorded above.

GitHub Actions run 44423413168 for commit 9bb2c76 completed successfully
on Linux and Windows, including the Windows Release build.

### Tray visibility and Vulkan restoration

Main-window visibility now stays in Slint's event loop. Background launch
starts the tray and waits for an explicit activation. Closing hides the
window through Slint, and tray activation shows it and restores it from
minimized state. The former off-screen repositioning and six-second hide
thread were removed. A hidden window does not trigger the visible-window
renderer-readiness timeout.

The native Windows test in scripts/test-tray.ps1 covers background and
foreground launch. Each launch completes three close/restore and three
minimize/restore cycles, requiring a new Vulkan submission after each
restoration, and exits through the tray command. The background case is
polled for 6.5 seconds before activation: no visible application surface and
no main-window submission are allowed. Winit's transparent layered event
target is identified by its exact class and extended styles; WS_VISIBLE
alone does not mean that this internal message window displays content.
The fixture starts the production resident-window code with hardware workers
and input hooks disabled.

Two default Release graphics runs completed 22 transitions each, window
lifecycle checks, idle zero-submission checks and GPU content-change/restore
checks on the AMD Radeon 610M:

| Measurement | Run 1 | Run 2 |
| --- | ---: | ---: |
| Submission interval, 95th percentile | 4.25 ms | 4.26 ms |
| Maximum submission interval | 35.41 ms | 32.26 ms |
| First Cooling-page draw | 31.70 ms | 27.57 ms |
| 16.67 ms submission-gap budget | Exceeded | Exceeded |

The second run's 1180 × 820 GPU readback was inspected. These results verify
restoration and pixel updates; first-visit drawing latency remains open.
Host verification passed 89 tests, Windows passed 92 native unit tests, and
all-target/all-feature Windows Clippy passed with warnings denied. Removing
the redundant visibility boundaries lowered the application/protocol unsafe
keyword ceiling from 102 to 100.

The default Release is 19,073,536 bytes with SHA-256
29E96FD64A15B97ED527E6A7B1488BF9D599877190526EF07EA220E531E28640.
It replaced the canonical C:\ProgramData\Himari executable.
The installed process stayed hidden for 6.5 seconds with zero main-window
submissions, then a second invocation activated the existing process. Its
first frame reported Vulkan / Skia / AMD Radeon 610M. Closing returned it to
the tray. Runtime diagnostic logging was then removed by a clean background
relaunch. Startup registration, service configuration and drivers were
unchanged. These window tests do not establish cold-boot CPU performance.

Local evidence is in target/validation/vulkan-startup/: final-tray/results.json,
release-1/results.json, release-2/results.json, native-results.json,
deployment.json, host-verify.log and windows-clippy.log. The retained GPU image
is release-2/overview.png.

### Shared base typefaces and GPU text invalidation

The renderer now derives variable-font faces from a cached base typeface. The
previous path called FontMgr::new_from_data again for each uncached variation;
that API builds a memory stream from the complete font data. Base and variation
faces now share the existing 64-entry cache. Font-data identity, collection index
and variation settings continue to determine the selected face.

The graphics check accepts -RequireTextInvalidation. After the measured page
transitions, it independently changes temperature text, switches Chinese to
English, and changes the palette. Each change must modify the acquired Vulkan
image; restoring each setting must reproduce the complete baseline image
exactly. These readbacks happen outside cadence sampling. Functional failures
are reported before the frame-budget gate.

Two alternating default-feature Release comparisons used the installed 8a20079
image as the baseline. Hardware workers were disabled in the graphics fixture.
All four runs completed 22 transitions, window resize/minimize/restore,
one-second idle with zero submissions, and temperature-pixel restoration.

| Measurement | Baseline 1 | Shared base 1 | Baseline 2 | Shared base 2 |
| --- | ---: | ---: | ---: | ---: |
| Process CPU time | 2578.13 ms | 2531.25 ms | 2843.75 ms | 2453.13 ms |
| First Cooling draw | 27.65 ms | 26.42 ms | 22.26 ms | 30.30 ms |
| Submission interval P95 | 4.21 ms | 4.21 ms | 4.12 ms | 4.18 ms |
| Maximum submission interval | 31.99 ms | 29.16 ms | 26.53 ms | 33.71 ms |
| Peak private bytes | 262529024 | 255086592 | 262397952 | 259944448 |
| 16.67 ms submission-gap budget | Exceeded | Exceeded | Exceeded | Exceeded |

CPU time and peak private bytes were lower in these two comparisons; background
load and driver caches were uncontrolled. First-visit latency did not improve
consistently. The 16.67 ms gate remains failed. These measurements establish
neither smooth animation nor cold-boot CPU recovery.

A separate run passed telemetry/language/palette pixel invalidation and exact
restoration. Its maximum submission gap was 30.20 ms, also exceeding the gate.
The candidate Overview images matched the baseline byte-for-byte. The inspected
GPU image is final-text-invalidation/overview.png.

Validation passed 89 host tests, 92 native Windows tests and strict Windows
all-target/all-feature Clippy. Both foreground and background tray checks passed
three close/restore and three minimize/restore cycles per launch. The application
and protocol unsafe ceiling remains 100. The Windows normal dependency graph
contains Skia, Vulkano and ash; the submitted-frame report identifies Skia/Vulkan
on the AMD Radeon 610M.

The 19075072-byte default Release was installed at
C:\ProgramData\Himari\himari.exe, with SHA-256
2AFDFA45505DFD1124F7EFCB313FBE498E7D5B6E324736E309D382E302BCFB6F.
Installed validation passed hidden startup, single-instance activation and
close-to-tray. A final background relaunch restored default diagnostic logging.
One recovery image retains the preceding installed build.

Local evidence is in target/validation/text-blob/: final-comparison.json,
final-text-invalidation/results.json, final-tray/results.json,
native-results.json, windows-clippy.log, host-verify.log and deployment.json.
The local Windows CPU recording was reduced to application-only stack data and
module-level sample counts; those samples cover the complete fixture and do not
identify one specific cold-draw function. Full-system trace files and duplicate
PPMs were removed after extraction; cleanup.json records the removed paths and
sizes.
