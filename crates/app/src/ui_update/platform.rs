use super::UiUpdate;
use crate::{
    MainWindow,
    control_sync::{Control, Controls},
    fan_curve_ui::{set_fan_curve_properties, set_toggle_properties},
    formatting::{format_threshold_ranges, toggle_value_text},
    localization::{localized_gpu_label, tr},
};
use himari_protocol::GpuSwitchMode;

pub(super) fn apply(ui: &MainWindow, update: &UiUpdate, accepted: Controls, sync: Controls) {
    if update.extended_reads {
        if accepted.contains(Control::QuickToggles) {
            set_toggle_properties(ui, &update.quick_toggles);
            if let Some(value) = update.quick_toggles.fn_lock {
                ui.set_toggle_fn_text(toggle_value_text(Some(value)).into());
            }
            if let Some(value) = update.quick_toggles.win_lock {
                ui.set_toggle_win_text(toggle_value_text(Some(value)).into());
            }
            if let Some(value) = update.quick_toggles.swap_win_fn {
                ui.set_toggle_swap_text(toggle_value_text(Some(value)).into());
            }
            if let Some(value) = update.quick_toggles.hotkeys_control_center {
                ui.set_toggle_hotkeys_text(toggle_value_text(Some(value)).into());
            }
            if let Some(value) = update.quick_toggles.copilot {
                ui.set_toggle_copilot_text(toggle_value_text(Some(value)).into());
            }
        }
        if accepted.contains(Control::FanCurve) && update.fan_curve.is_some() {
            set_fan_curve_properties(
                ui,
                update.fan_curve.as_ref(),
                sync.contains(Control::FanCurve),
            );
        }
        ui.set_extended_api_text(
            if update.extended_api {
                tr("Available")
            } else {
                tr("Extended transport")
            }
            .into(),
        );
        if accepted.contains(Control::Gpu)
            && let Some(gpu) = update.gpu_switch
        {
            ui.set_gpu_switch_text(
                format!(
                    "{} · {} 0x{:02X}",
                    localized_gpu_label(gpu.current_mode),
                    tr("mask"),
                    gpu.supported_mask
                )
                .into(),
            );
            ui.set_gpu_current_mode(i32::from(gpu.current_mode));
            ui.set_gpu_igpu_supported(gpu.supports(GpuSwitchMode::Integrated));
            ui.set_gpu_dgpu_supported(gpu.supports(GpuSwitchMode::Discrete));
            ui.set_gpu_mshybrid_supported(gpu.supports(GpuSwitchMode::MsHybrid));
            ui.set_gpu_dynamic_supported(gpu.supports(GpuSwitchMode::Dynamic));
        }
        if accepted.contains(Control::Charging) {
            if let Some(charger) = &update.flexi_charger {
                ui.set_flexi_ready(update.flexi_ready);
                ui.set_flexi_mode(i32::from(charger.mode));
                if sync.contains(Control::Charging) {
                    ui.set_requested_flexi_mode(i32::from(charger.mode));
                    ui.set_flexi_start_choice(i32::from(charger.start_threshold));
                    ui.set_flexi_stop_choice(i32::from(charger.stop_threshold));
                }
                ui.set_flexi_mode_text(
                    match charger.mode {
                        0 => tr("Charge to 100%").to_owned(),
                        1 => tr("Preset · 70–80%").to_owned(),
                        2 => tr("Custom thresholds").to_owned(),
                        _ => format!("{} {}", tr("Mode"), charger.mode),
                    }
                    .into(),
                );
                ui.set_flexi_start_text(format!("{}%", charger.start_threshold).into());
                ui.set_flexi_stop_text(format!("{}%", charger.stop_threshold).into());
                ui.set_flexi_ranges_text(
                    format_threshold_ranges(&charger.threshold_choices).into(),
                );
            } else if !update.flexi_ready {
                ui.set_flexi_ready(false);
                ui.set_flexi_mode_text(tr("Restart to refresh the charge channel").into());
                ui.set_flexi_start_text("--".into());
                ui.set_flexi_stop_text("--".into());
                ui.set_flexi_ranges_text("--".into());
            }
        }
    }
}
