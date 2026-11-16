use super::UiUpdate;
use crate::{
    MainWindow,
    control_sync::{Control, Controls},
    localization::{localized_fan_label, localized_power_label},
};
use ram_control_center_protocol::{FanMode, PowerMode};

pub(super) fn apply(ui: &MainWindow, update: &UiUpdate, accepted: Controls, sync: Controls) {
    if update.extended_reads {
        let power = update.platform.power_mode.and_then(PowerMode::from_u8);
        if accepted.contains(Control::Power)
            && let Some(power) = power
        {
            ui.set_power_mode(power.as_u8() as i32);
            ui.set_power_label(localized_power_label(power).into());
            ui.set_low_refresh_eligible(matches!(
                power,
                PowerMode::Quiet | PowerMode::PowerSaving | PowerMode::QuietPlus
            ));
            if sync.contains(Control::Power) {
                ui.set_requested_power_mode(power.as_u8() as i32);
            }
        }
        if let Some(supported) = update.platform.low_refresh_rate_supported {
            ui.set_low_refresh_supported(supported);
        }
        if accepted.contains(Control::LowRefresh)
            && let Some(enabled) = update.platform.low_refresh_rate_enabled
        {
            ui.set_low_refresh_enabled(enabled);
            ui.set_low_refresh_known(true);
        }
        if accepted.contains(Control::Fan) {
            if let Some(fan) = update.platform.fan_mode.and_then(FanMode::from_u8) {
                ui.set_fan_mode(fan.as_u8() as i32);
                ui.set_fan_label(localized_fan_label(fan).into());
                if sync.contains(Control::Fan) {
                    ui.set_requested_fan_mode(fan.as_u8() as i32);
                }
            }
            if let Some(slow) = update.platform.slow_fan {
                ui.set_slow_fan_active(slow);
            }
            if let Some(turbo) = update.platform.turbo_fan {
                ui.set_turbo_fan_active(turbo);
            }
            if let Some(capabilities) = update.capabilities {
                ui.set_slow_fan_supported(capabilities.slow_fan);
                ui.set_turbo_fan_supported(capabilities.turbo_fan);
            }
            if let Some(power) = power {
                ui.set_turbo_fan_eligible(power == PowerMode::Performance);
            }
        }
        if accepted.contains(Control::FanOffset)
            && let Some(fan_offset) = update.platform.fan_offset
        {
            ui.set_fan_offset_current(i32::from(fan_offset));
            if sync.contains(Control::FanOffset) {
                ui.set_fan_offset_requested(i32::from(fan_offset));
            }
        }
        ui.set_capabilities_text(
            update
                .platform
                .capabilities
                .map(|value| format!("0x{value:04X}"))
                .unwrap_or_else(|| "--".to_owned())
                .into(),
        );
        ui.set_platform_text(
            update
                .platform
                .platform_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "--".to_owned())
                .into(),
        );
    }
}
