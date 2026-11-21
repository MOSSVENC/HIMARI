use std::time::Instant;

use himari_protocol::{Dchu, ThermalSample};
use slint::Weak;

use crate::MainWindow;
use crate::control_sync::{Control, ControlSync, Controls};
use crate::diagnostics::log_action;
use crate::driver_manager::DriverHealth;
use crate::formatting::shorten_path;
use crate::localization::tr;
use crate::runtime_state::flexi_recovery_ready;
use crate::system_controls;
use crate::telemetry::SystemSampler;
use crate::ui_update::{UiUpdate, apply_update};

pub(super) struct SnapshotInput<'a> {
    pub(super) status: Option<(String, i32)>,
    pub(super) thermal: Option<ThermalSample>,
    pub(super) driver_health: &'a DriverHealth,
    pub(super) control_sync: &'a ControlSync,
}

pub(super) fn push_snapshot(
    weak: &Weak<MainWindow>,
    sampler: &mut SystemSampler,
    dchu: Option<&Dchu>,
    input: SnapshotInput<'_>,
    read_scope: Controls,
) {
    let SnapshotInput {
        status,
        thermal,
        driver_health,
        control_sync,
    } = input;
    let read_stamp = control_sync.snapshot();
    let extended_reads = read_scope != Controls::NONE;
    log_action(if extended_reads {
        "worker: snapshot begin full"
    } else {
        "worker: snapshot begin light"
    });
    let system = sampler.sample();
    log_action(&format!(
        "worker: system sampled cpu={:.0}% clock={} MHz max={} MHz",
        system.cpu_load_percent, system.cpu_clock_mhz, system.cpu_max_clock_mhz
    ));
    let platform = if read_scope.any(&[
        Control::Power,
        Control::Fan,
        Control::FanOffset,
        Control::AntiDust,
        Control::LowRefresh,
    ]) {
        dchu.map(Dchu::read_platform_state).unwrap_or_default()
    } else {
        Default::default()
    };
    let quick_toggles = if read_scope.contains(Control::QuickToggles) {
        dchu.map(Dchu::read_quick_toggles).unwrap_or_default()
    } else {
        Default::default()
    };
    let fan_curve = sample("fan curve", read_scope.contains(Control::FanCurve), || {
        dchu.and_then(|device| device.read_fan_curve().ok())
    });
    let rpm_fan_table = sample("RPM table", read_scope.contains(Control::FanRpm), || {
        dchu.and_then(|device| device.read_rpm_fan_table().ok())
    });
    let gpu_switch = sample("GPU switch", read_scope.contains(Control::Gpu), || {
        dchu.and_then(|device| device.read_gpu_switch().ok())
    });
    let capabilities = sample("capabilities", read_scope == Controls::ALL, || {
        dchu.and_then(|device| device.read_capabilities().ok())
    });
    let rgb_keyboard = sample(
        "lighting",
        read_scope.any(&[
            Control::Lighting,
            Control::LightingBrightness,
            Control::LightingEffect,
            Control::LightingBoot,
            Control::LightingSleep,
        ]),
        || dchu.and_then(|device| device.read_rgb_keyboard_state().ok()),
    );
    let camera_enabled = sample("camera", read_scope.contains(Control::Camera), || {
        dchu.and_then(Dchu::read_camera_enabled)
    });
    let touchpad_enabled = sample("touchpad", read_scope.contains(Control::Touchpad), || {
        system_controls::touchpad_enabled().ok()
    });
    let airplane_enabled = sample("airplane", read_scope.contains(Control::Airplane), || {
        system_controls::airplane_mode_enabled().ok()
    });
    let brightness = sample(
        "brightness",
        read_scope.contains(Control::Brightness),
        || system_controls::screen_brightness().ok(),
    );
    let blue_light = sample(
        "blue light",
        read_scope.contains(Control::BlueLight),
        || dchu.and_then(|device| device.read_setting_byte(1, 34)),
    );
    let audio = sample(
        "audio",
        read_scope.any(&[Control::Audio, Control::AudioMute]),
        || system_controls::audio_state().ok(),
    );
    let anti_dust_schedule = sample("anti dust", read_scope.contains(Control::AntiDust), || {
        dchu.and_then(|device| device.read_anti_dust_schedule().ok())
    });
    let extended_api = dchu.is_some_and(Dchu::has_extended_api);
    let display_care = sample(
        "display care",
        read_scope.any(&[
            Control::DisplayCare,
            Control::MonitorTimeout,
            Control::ScreenSaver,
        ]),
        || system_controls::display_care_state().ok(),
    );
    let flexi_ready = flexi_recovery_ready();
    let flexi_charger = sample(
        "charging",
        flexi_ready && read_scope.contains(Control::Charging),
        || dchu.and_then(|device| device.read_flexi_charger().ok()),
    );
    let transport_ready = dchu.is_some();
    let transport_label = if transport_ready {
        tr("DCHU connected").to_owned()
    } else {
        tr("DCHU unavailable").to_owned()
    };
    let (transport_detail, transport_device_path) = if let Some(dchu) = dchu {
        (shorten_path(dchu.path()), dchu.path().to_owned())
    } else {
        (
            tr("Signed AcpiBridge device not found").to_owned(),
            tr("Not located").to_owned(),
        )
    };
    push_update(
        weak,
        UiUpdate {
            read_stamp: Some(read_stamp),
            system,
            thermal,
            platform,
            quick_toggles,
            fan_curve,
            rpm_fan_table,
            gpu_switch,
            flexi_charger,
            capabilities,
            rgb_keyboard,
            camera_enabled,
            touchpad_enabled,
            airplane_enabled,
            brightness,
            blue_light,
            audio,
            display_care,
            anti_dust_schedule,
            flexi_ready,
            extended_api,
            transport_ready,
            transport_label,
            transport_detail,
            transport_device_path,
            driver_health: driver_health.clone(),
            busy: false,
            status,
            extended_reads,
        },
    );
}

fn sample<T>(name: &str, enabled: bool, read: impl FnOnce() -> Option<T>) -> Option<T> {
    if !enabled {
        return None;
    }
    let started = Instant::now();
    let value = read();
    log_action(&format!(
        "worker: {name} sampled in {} us",
        started.elapsed().as_micros()
    ));
    value
}

pub(super) fn push_update(weak: &Weak<MainWindow>, update: UiUpdate) {
    let weak = weak.clone();
    log_action("worker: dispatching UI update");
    if let Err(error) = slint::invoke_from_event_loop(move || {
        log_action("ui: applying update");
        if let Some(ui) = weak.upgrade() {
            apply_update(&ui, update);
            log_action("ui: update applied");
        } else {
            log_action("ui: weak handle no longer upgrades");
        }
    }) {
        log_action(&format!("worker: UI dispatch failed: {error}"));
    }
}
