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

    pwsh -File scripts/test-graphics.ps1 -Executable '<release directory>\ram-control-center.exe' -OutputDirectory '<measurement directory>'

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
C:\ProgramData\RAM Control Center\ram-control-center.exe and launched with
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
