use super::UiUpdate;
use crate::{
    MainWindow,
    control_sync::{Control, Controls},
};

pub(super) fn apply(ui: &MainWindow, update: &UiUpdate, accepted: Controls, sync: Controls) {
    if update.extended_reads {
        // Anti-dust exists only in the extended snapshot. Never let the 1 Hz
        // light snapshot clear a previously valid state: doing so made the
        // schedule toggles alternate between "Initialize" and "Enabled".
        if let Some(running) = update.platform.anti_dust_running
            && accepted.contains(Control::AntiDust)
        {
            ui.set_anti_dust_running(running);
        }
        if let Some(schedule) = update.anti_dust_schedule
            && accepted.contains(Control::AntiDust)
        {
            ui.set_anti_dust_supported(true);
            if sync.contains(Control::AntiDust) {
                ui.set_anti_dust_enabled(schedule.enabled);
                ui.set_anti_dust_resume(schedule.resume);
                ui.set_anti_dust_day(if schedule.enabled {
                    i32::from(schedule.day.min(6))
                } else {
                    6
                });
                ui.set_anti_dust_hour(if schedule.enabled {
                    i32::from(schedule.hour.min(23))
                } else {
                    0
                });
            }
        }
    }
    if let Some(table) = update.rpm_fan_table
        && accepted.contains(Control::FanRpm)
    {
        let cpu = table.channels[0]
            .levels
            .iter()
            .map(|level| format!("{}:{}", level.temp_up_c, level.rpm_up))
            .collect::<Vec<_>>()
            .join("  ");
        let gpu = table.channels[1]
            .levels
            .iter()
            .map(|level| format!("{}:{}", level.temp_up_c, level.rpm_up))
            .collect::<Vec<_>>()
            .join("  ");
        ui.set_rpm_curve_text(format!("CPU  {cpu}\nGPU  {gpu}").into());
        ui.set_rpm_editable(table.channels[0].present || table.channels[1].present);
        if sync.contains(Control::FanRpm) {
            for (index, level) in table.channels[0].levels.iter().enumerate() {
                let temp = i32::from(level.temp_up_c);
                let rpm = i32::from(level.rpm_up);
                match index {
                    0 => {
                        ui.set_rpm_cpu_temp_0(temp);
                        ui.set_rpm_cpu_rpm_0(rpm);
                    }
                    1 => {
                        ui.set_rpm_cpu_temp_1(temp);
                        ui.set_rpm_cpu_rpm_1(rpm);
                    }
                    2 => {
                        ui.set_rpm_cpu_temp_2(temp);
                        ui.set_rpm_cpu_rpm_2(rpm);
                    }
                    3 => {
                        ui.set_rpm_cpu_temp_3(temp);
                        ui.set_rpm_cpu_rpm_3(rpm);
                    }
                    4 => {
                        ui.set_rpm_cpu_temp_4(temp);
                        ui.set_rpm_cpu_rpm_4(rpm);
                    }
                    5 => {
                        ui.set_rpm_cpu_temp_5(temp);
                        ui.set_rpm_cpu_rpm_5(rpm);
                    }
                    6 => {
                        ui.set_rpm_cpu_temp_6(temp);
                        ui.set_rpm_cpu_rpm_6(rpm);
                    }
                    7 => {
                        ui.set_rpm_cpu_temp_7(temp);
                        ui.set_rpm_cpu_rpm_7(rpm);
                    }
                    8 => {
                        ui.set_rpm_cpu_temp_8(temp);
                        ui.set_rpm_cpu_rpm_8(rpm);
                    }
                    9 => {
                        ui.set_rpm_cpu_temp_9(temp);
                        ui.set_rpm_cpu_rpm_9(rpm);
                    }
                    _ => {}
                }
            }
            for (index, level) in table.channels[1].levels.iter().enumerate() {
                let temp = i32::from(level.temp_up_c);
                let rpm = i32::from(level.rpm_up);
                match index {
                    0 => {
                        ui.set_rpm_gpu_temp_0(temp);
                        ui.set_rpm_gpu_rpm_0(rpm);
                    }
                    1 => {
                        ui.set_rpm_gpu_temp_1(temp);
                        ui.set_rpm_gpu_rpm_1(rpm);
                    }
                    2 => {
                        ui.set_rpm_gpu_temp_2(temp);
                        ui.set_rpm_gpu_rpm_2(rpm);
                    }
                    3 => {
                        ui.set_rpm_gpu_temp_3(temp);
                        ui.set_rpm_gpu_rpm_3(rpm);
                    }
                    4 => {
                        ui.set_rpm_gpu_temp_4(temp);
                        ui.set_rpm_gpu_rpm_4(rpm);
                    }
                    5 => {
                        ui.set_rpm_gpu_temp_5(temp);
                        ui.set_rpm_gpu_rpm_5(rpm);
                    }
                    6 => {
                        ui.set_rpm_gpu_temp_6(temp);
                        ui.set_rpm_gpu_rpm_6(rpm);
                    }
                    7 => {
                        ui.set_rpm_gpu_temp_7(temp);
                        ui.set_rpm_gpu_rpm_7(rpm);
                    }
                    8 => {
                        ui.set_rpm_gpu_temp_8(temp);
                        ui.set_rpm_gpu_rpm_8(rpm);
                    }
                    9 => {
                        ui.set_rpm_gpu_temp_9(temp);
                        ui.set_rpm_gpu_rpm_9(rpm);
                    }
                    _ => {}
                }
            }
        }
    }
}
