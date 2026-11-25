#!/usr/bin/env bash
set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

# This is a ratchet, not a claim that FFI disappears. Every file below is a
# deliberate Win32/ACPI/NVML boundary; a new file or a higher per-file count
# fails the gate. When a later refactor lowers a count, lower its ceiling here.
declare -A ceilings=(
    ["crates/app/src/telemetry.rs"]=26
    ["crates/app/src/driver_manager.rs"]=7
    ["crates/app/src/tray.rs"]=18
    ["crates/app/src/flexikey_engine.rs"]=8
    ["crates/app/src/system_controls/registry.rs"]=1
    ["crates/app/src/system_controls/display_care.rs"]=5
    ["crates/app/src/system_controls/power.rs"]=6
    ["crates/protocol/src/win_device.rs"]=6
    ["crates/protocol/src/bin/neo-acpi-probe.rs"]=3
    # Brightness plus the identity-ramp capability probe: GetDC, the probe's
    # SetDeviceGammaRamp and ReleaseDC join the existing gamma write.
    ["crates/app/src/system_controls/display.rs"]=6
    ["crates/app/src/system_controls/audio.rs"]=2
    ["crates/app/src/input.rs"]=2
    ["crates/app/src/runtime_state.rs"]=1
    ["crates/app/src/flexikey_store.rs"]=1
    ["crates/app/src/input_hooks.rs"]=5
    ["crates/app/src/bin/himari-uninstaller.rs"]=6
    # Native layered OSD: one DIB/DCC section, one window creation, one font
    # rasterisation pass, one presentation call and their teardown. All of the
    # overlay's FFI lives here, grouped into the smallest blocks that keep the
    # safety argument readable.
    ["crates/app/src/osd_layer.rs"]=11
    # MonitorFromPoint + GetMonitorInfoW: the only FFI in the platform profile;
    # everything else there is pure geometry.
    ["crates/app/src/platform_profile.rs"]=2
    # Window and taskbar icons loaded from the executable's own resource:
    # GetModuleHandleW + LoadImageW, GetSystemMetrics, and SendMessageW.
    ["crates/app/src/window_icon.rs"]=4
)

total=0
failed=0
# Includes the five audited hook handle/event boundaries and six uninstaller
# boundaries; the dispatcher and tray ceilings were lowered after refactoring.
max_total=120

while IFS= read -r file; do
    count=$(grep -o -E '\bunsafe\b' "$file" | wc -l)
    total=$((total + count))
    ceiling=${ceilings[$file]:-0}
    if (( count > ceiling )); then
        printf 'unsafe regression: %s has %d (ceiling %d)\n' "$file" "$count" "$ceiling" >&2
        failed=1
    fi
    printf '%3d  %s\n' "$count" "$file"
done < <(grep -R -l -E '\bunsafe\b' crates --include='*.rs' | sort)

if (( total > max_total )); then
    printf 'unsafe total regression: %d (ceiling %d)\n' "$total" "$max_total" >&2
    failed=1
fi

printf 'unsafe total: %d (ceiling %d)\n' "$total" "$max_total"
exit "$failed"
