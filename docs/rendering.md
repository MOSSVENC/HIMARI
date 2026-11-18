# Windows rendering

The Windows UI is rendered by Slint 1.17's Skia renderer directly on a native Vulkan surface. Vulkan is required during backend selection; initialization failure stops GUI startup rather than silently selecting another API. The Windows-target dependency graph does not include wgpu. Slint's Vulkan surface owns instance/device/swapchain creation, and Skia records the UI draw commands.

The tracked renderer source is Slint's renderer crate with a narrow patch. It reports the physical adapter and successful queue submissions, supplies the correct Vulkan queue-family metadata to Skia, waits for acquired images before Skia draws, declares the enabled swapchain extension to Skia, transitions the rendered image to presentation layout, and waits for Skia GPU completion before presenting on the same Vulkan queue. The swapchain uses FIFO presentation. Review PATCHES.md alongside the source when upgrading Slint.

## Windows hardware validation

Build from WSL/Arch against the Windows SDK and MSVC libraries already mounted into the environment:

    ./scripts/build-windows.sh --features snapshot-tools

The WSL build script reuses a matching archive already cached under target/.
For a fresh checkout, it downloads the pinned rust-skia 0.99.0 Windows MSVC
archive into target/toolchains/skia/ and verifies its SHA-256 before starting
Cargo. SKIA_BINARIES_URL can override this location for a prepared cache.

Then run the bounded test with Windows PowerShell 7:

    pwsh -File scripts/test-graphics.ps1 -Executable '<release directory>\ram-control-center.exe' -OutputDirectory '<measurement directory>'

Add -RequireSnapshot when checking a snapshot-tools build. A default-feature
Release can run the same test without taking an off-screen snapshot.
To enforce a submission-gap budget, add -MaximumFrameGapMs 33.34 (or the
appropriate budget for the target display). The script writes results.json
before reporting a budget failure. Without this argument, BudgetStatus is
MeasuredOnly; passing the lifecycle checks is not a performance acceptance.

The isolated process starts no hardware worker, service, or input hook. It verifies the production navigation callbacks over all eleven pages twice, checks resize and minimize/restore, confirms zero queue submissions during a one-second idle interval, and optionally writes an off-screen snapshot of the rendered Overview. It also verifies that vulkan-1.dll was loaded and that the selected Vulkan adapter is integrated or discrete hardware.
The lighting capability is enabled in this isolated UI fixture so that its
sliders and color preview are rendered. The fixture does not exercise hardware
writes or establish correctness of device capabilities and telemetry.

The first submission interval is measured from Slint component creation to the first successful Skia queue submission; it excludes process creation and executable loading. Per-transition intervals measure CPU-observed submission cadence, not GPU timestamps, compositor latency, or display scanout. Windows CPU and memory values apply to this diagnostic process.

A separate snapshot-tools bar test exercises pointer movement and pixel endpoints for the RGB/monitor bars:

    pwsh -File scripts/test-bars.ps1 -Executable '<release executable>' -OutputDirectory '<measurement directory>'

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
