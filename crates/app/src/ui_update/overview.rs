use crate::{
    MainWindow,
    formatting::{
        format_battery_capacity, format_battery_time, format_bytes, format_clock,
        format_power_rate, format_temperature_celsius,
    },
    localization::tr,
    telemetry::SystemSnapshot,
};
use ram_control_center_protocol::{FanChannel, ThermalSample, is_plausible_temperature};

pub(super) fn apply(ui: &MainWindow, system: &SystemSnapshot, thermal: Option<ThermalSample>) {
    ui.set_cpu_load(system.cpu_load_percent.round().clamp(0.0, 100.0) as i32);
    ui.set_memory_load(system.memory_percent() as i32);
    ui.set_disk_load(system.disk_percent() as i32);
    ui.set_cpu_clock(format_clock(system.cpu_clock_mhz).into());
    ui.set_cpu_clock_max(format_clock(system.cpu_max_clock_mhz).into());
    ui.set_memory_text(
        format!(
            "{} / {}",
            format_bytes(system.memory_used_bytes),
            format_bytes(system.memory_total_bytes)
        )
        .into(),
    );
    ui.set_disk_text(
        format!(
            "{} / {}",
            format_bytes(system.disk_used_bytes),
            format_bytes(system.disk_total_bytes)
        )
        .into(),
    );

    if let Some(gpu) = &system.gpu {
        ui.set_gpu_load(gpu.utilization_percent as i32);
        ui.set_gpu_memory_load(if gpu.memory_total_bytes == 0 {
            0
        } else {
            ((gpu.memory_used_bytes as f64 / gpu.memory_total_bytes as f64) * 100.0)
                .round()
                .clamp(0.0, 100.0) as i32
        });
        ui.set_gpu_clock(format_clock(gpu.clock_mhz).into());
        ui.set_gpu_clock_max(format_clock(gpu.max_clock_mhz).into());
        ui.set_gpu_memory_text(
            format!(
                "{} / {}",
                format_bytes(gpu.memory_used_bytes),
                format_bytes(gpu.memory_total_bytes)
            )
            .into(),
        );
    } else {
        ui.set_gpu_load(0);
        ui.set_gpu_memory_load(0);
        ui.set_gpu_clock(tr("NVML unavailable").into());
        ui.set_gpu_clock_max("--".into());
        ui.set_gpu_memory_text("--".into());
    }

    if let Some(battery) = &system.battery {
        ui.set_battery_present(true);
        ui.set_battery_percent(i32::from(battery.percent.unwrap_or(0)));
        ui.set_battery_status_text(
            if battery.charging {
                tr("Charging")
            } else if battery.ac_online {
                tr("Connected · not charging")
            } else {
                tr("Discharging")
            }
            .into(),
        );
        ui.set_battery_ac_text(
            if battery.ac_online {
                tr("AC online")
            } else {
                tr("Battery power")
            }
            .into(),
        );
        ui.set_battery_time_text(
            if battery.ac_online && !battery.charging {
                tr("AC connected").to_owned()
            } else {
                format_battery_time(battery.seconds_remaining)
            }
            .into(),
        );
        ui.set_battery_capacity_text(
            format_battery_capacity(battery.remaining_mwh, battery.max_capacity_mwh).into(),
        );
        ui.set_battery_rate_text(format_power_rate(battery.rate_mw).into());
    } else {
        ui.set_battery_present(false);
        ui.set_battery_percent(0);
        ui.set_battery_status_text(tr("No battery detected").into());
        ui.set_battery_ac_text("--".into());
        ui.set_battery_time_text("--".into());
        ui.set_battery_capacity_text("--".into());
        ui.set_battery_rate_text("--".into());
    }

    let thermal = thermal.unwrap_or_default();
    let cpu = ChannelDisplay::new(thermal.cpu);
    let mut gpu = ChannelDisplay::new(thermal.gpu);
    let fan3 = ChannelDisplay::new(thermal.fan3);
    let fan4 = ChannelDisplay::new(thermal.fan4);
    if !is_plausible_temperature(thermal.gpu.temperature_c)
        && let Some(sensor) = system.gpu.as_ref()
    {
        gpu.temperature = format_temperature_celsius(sensor.temperature_c);
    }
    ui.set_cpu_temp(cpu.temperature.into());
    ui.set_cpu_rpm(cpu.rpm.into());
    ui.set_cpu_duty(cpu.duty.into());
    ui.set_gpu_temp(gpu.temperature.into());
    ui.set_gpu_rpm(gpu.rpm.into());
    ui.set_gpu_duty(gpu.duty.into());
    ui.set_fan3_temp(fan3.temperature.into());
    ui.set_fan3_rpm(fan3.rpm.into());
    ui.set_fan3_duty(fan3.duty.into());
    ui.set_fan4_temp(fan4.temperature.into());
    ui.set_fan4_rpm(fan4.rpm.into());
    ui.set_fan4_duty(fan4.duty.into());
}

struct ChannelDisplay {
    temperature: String,
    rpm: String,
    duty: String,
}
impl ChannelDisplay {
    fn new(channel: FanChannel) -> Self {
        if is_plausible_temperature(channel.temperature_c) {
            Self {
                temperature: format_temperature_celsius(u32::from(channel.temperature_c)),
                rpm: channel.rpm.to_string(),
                duty: channel.duty_percent.to_string(),
            }
        } else {
            Self {
                temperature: "--°C".to_owned(),
                rpm: "--".to_owned(),
                duty: "--".to_owned(),
            }
        }
    }
}
