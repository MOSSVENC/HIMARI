mod devices;
mod driver;
mod fans;
mod lighting;
mod overview;
mod platform;
mod power;

use crate::{
    MainWindow,
    control_sync::{Control, Controls, ReadStamp},
    driver_manager::DriverHealth,
    localization::tr,
    system_controls::{AudioState, DisplayCareState},
    telemetry::SystemSnapshot,
};
use himari_protocol::{
    AntiDustSchedule, FanCurveState, FanMode, FlexiChargerState, GpuSwitchState,
    PlatformCapabilities, PlatformState, PowerMode, QuickToggleState, RgbKeyboardState,
    RpmFanTable, ThermalSample,
};

#[derive(Clone)]
pub(crate) struct UiUpdate {
    pub(crate) read_stamp: Option<ReadStamp>,
    pub(crate) system: SystemSnapshot,
    pub(crate) thermal: Option<ThermalSample>,
    pub(crate) platform: PlatformState,
    pub(crate) quick_toggles: QuickToggleState,
    pub(crate) fan_curve: Option<FanCurveState>,
    pub(crate) rpm_fan_table: Option<RpmFanTable>,
    pub(crate) gpu_switch: Option<GpuSwitchState>,
    pub(crate) flexi_charger: Option<FlexiChargerState>,
    pub(crate) capabilities: Option<PlatformCapabilities>,
    pub(crate) rgb_keyboard: Option<RgbKeyboardState>,
    pub(crate) camera_enabled: Option<bool>,
    pub(crate) touchpad_enabled: Option<bool>,
    pub(crate) airplane_enabled: Option<bool>,
    pub(crate) brightness: Option<u8>,
    pub(crate) blue_light: Option<u8>,
    pub(crate) audio: Option<AudioState>,
    pub(crate) display_care: Option<DisplayCareState>,
    pub(crate) anti_dust_schedule: Option<AntiDustSchedule>,
    pub(crate) flexi_ready: bool,
    pub(crate) extended_api: bool,
    pub(crate) transport_ready: bool,
    pub(crate) transport_label: String,
    pub(crate) transport_detail: String,
    pub(crate) transport_device_path: String,
    pub(crate) driver_health: DriverHealth,
    pub(crate) busy: bool,
    pub(crate) status: Option<(String, i32)>,
    pub(crate) extended_reads: bool,
}

impl Default for UiUpdate {
    fn default() -> Self {
        Self {
            read_stamp: None,
            system: SystemSnapshot::default(),
            thermal: None,
            platform: PlatformState::default(),
            quick_toggles: QuickToggleState::default(),
            fan_curve: None,
            rpm_fan_table: None,
            gpu_switch: None,
            flexi_charger: None,
            capabilities: None,
            rgb_keyboard: None,
            camera_enabled: None,
            touchpad_enabled: None,
            airplane_enabled: None,
            brightness: None,
            blue_light: None,
            audio: None,
            display_care: None,
            anti_dust_schedule: None,
            flexi_ready: false,
            extended_api: false,
            transport_ready: false,
            transport_label: tr("Connecting to DCHU").to_owned(),
            transport_detail: tr("Opening the signed AcpiBridge device").to_owned(),
            transport_device_path: tr("Not located").to_owned(),
            driver_health: DriverHealth::default(),
            busy: false,
            status: None,
            extended_reads: true,
        }
    }
}

impl UiUpdate {
    fn observed_controls(&self) -> Controls {
        use Control::*;
        let mut observed = Controls::NONE;
        let mut add = |control, available| {
            if available {
                observed = observed.union(Controls::one(control));
            }
        };
        add(
            Power,
            self.platform
                .power_mode
                .and_then(PowerMode::from_u8)
                .is_some(),
        );
        add(
            Fan,
            self.platform.fan_mode.and_then(FanMode::from_u8).is_some(),
        );
        add(FanCurve, self.fan_curve.is_some());
        add(FanRpm, self.rpm_fan_table.is_some());
        add(FanOffset, self.platform.fan_offset.is_some());
        add(AntiDust, self.anti_dust_schedule.is_some());
        for control in [
            Lighting,
            LightingBrightness,
            LightingEffect,
            LightingBoot,
            LightingSleep,
        ] {
            add(control, self.rgb_keyboard.is_some());
        }
        for control in [DisplayCare, MonitorTimeout, ScreenSaver] {
            add(control, self.display_care.is_some());
        }
        add(QuickToggles, self.quick_toggles.fn_lock.is_some());
        add(Camera, self.camera_enabled.is_some());
        add(Touchpad, self.touchpad_enabled.is_some());
        add(Airplane, self.airplane_enabled.is_some());
        add(Audio, self.audio.is_some());
        add(AudioMute, self.audio.is_some());
        add(Brightness, self.brightness.is_some());
        add(BlueLight, self.blue_light.is_some());
        add(Charging, self.flexi_charger.is_some());
        add(Gpu, self.gpu_switch.is_some());
        add(LowRefresh, self.platform.low_refresh_rate_enabled.is_some());
        observed
    }
}
pub(crate) use driver::apply_driver_health;

pub(crate) fn apply_update(ui: &MainWindow, update: UiUpdate) {
    let observed = update.observed_controls();
    let accepted = update
        .read_stamp
        .as_ref()
        .map(ReadStamp::accepted_controls)
        .unwrap_or(Controls::ALL);
    let sync = update
        .read_stamp
        .as_ref()
        .map(ReadStamp::selection_controls)
        .unwrap_or(Controls::NONE);
    ui.set_transport_ready(update.transport_ready);
    ui.set_transport_label(update.transport_label.as_str().into());
    ui.set_transport_detail(update.transport_detail.as_str().into());
    ui.set_transport_device_path(update.transport_device_path.as_str().into());
    apply_driver_health(ui, &update.driver_health);
    ui.set_busy(update.busy);
    if let Some(capabilities) = update.capabilities {
        ui.set_rgb_available(capabilities.keyboard_type != 0);
        ui.set_power_quiet_supported(capabilities.quiet);
        ui.set_power_saving_supported(capabilities.power_saving);
        ui.set_power_performance_supported(capabilities.performance);
        ui.set_power_entertainment_supported(capabilities.entertainment);
        ui.set_rpm_fan_control(capabilities.rpm_fan_control);
        ui.set_display_care_panel_oled(capabilities.oled_panel);
    }
    fans::apply(ui, &update, accepted, sync);
    lighting::apply(ui, &update, accepted, sync);
    devices::apply(ui, &update, accepted, sync);
    platform::apply(ui, &update, accepted, sync);
    overview::apply(ui, &update.system, update.thermal);

    power::apply(ui, &update, accepted, sync);
    if let Some((message, tone)) = update.status {
        ui.set_status_message(message.into());
        ui.set_status_tone(tone);
    }
    if let Some(stamp) = update.read_stamp {
        stamp.acknowledge(observed);
    }
}
