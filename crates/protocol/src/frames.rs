//! Frame encode/decode for the DCHU packets.
//!
//! Pure translations between the fixed-size byte frames the signed device
//! speaks and the types in [`crate::model`], so every rule is testable
//! without hardware.

use crate::error::{DchuError, Result};
use crate::model::*;

pub fn decode_thermal(buffer: &[u8; 256]) -> ThermalSample {
    ThermalSample {
        cpu: decode_channel(buffer, 2, 16, 18),
        gpu: decode_channel(buffer, 4, 19, 21),
        fan3: decode_channel(buffer, 6, 22, 24),
        fan4: decode_channel(buffer, 36, 38, 40),
    }
}

fn decode_channel(
    buffer: &[u8; 256],
    rpm_offset: usize,
    duty_offset: usize,
    temp_offset: usize,
) -> FanChannel {
    let raw_rpm = u16::from_be_bytes([buffer[rpm_offset], buffer[rpm_offset + 1]]);
    FanChannel {
        rpm: rpm_from_raw(raw_rpm),
        duty_percent: duty_from_raw(buffer[duty_offset]),
        temperature_c: buffer[temp_offset],
    }
}

/// Decode the runtime fan table returned by DCHU command 13 (EEVT).
///
/// Layout confirmed against CC40 and the independent Clevo reference:
/// byte 12 = fan count, byte 14 = initial mode, and three 8-byte records at
/// offset 16 with T1,D1,T2,D2,T3,D3,T4,D4. Duties are native 0..=255.
pub fn decode_rpm_fan_channel(raw: &[u8; 112], present: bool) -> RpmFanChannel {
    let u16be = |index: usize| u16::from_be_bytes([raw[index], raw[index + 1]]);
    let mut channel = RpmFanChannel {
        present,
        id: raw[0],
        open_fan_local_temp: raw[1],
        shutdown_local_temp: raw[2],
        open_fan_temp: raw[3],
        close_fan_temp: raw[4],
        temp_shutdown: raw[5],
        temp_passive: raw[6],
        temp_fan_full: raw[7],
        idle_rpm: u16be(8),
        max_rpm: u16be(10),
        max_turbo_rpm: u16be(12),
        reserved: [0; 8],
        levels: [RpmFanLevel::default(); 10],
    };
    channel.reserved.copy_from_slice(&raw[14..22]);
    for (index, level) in channel.levels.iter_mut().enumerate() {
        *level = RpmFanLevel {
            temp_down_c: raw[22 + index],
            temp_up_c: raw[32 + index],
            rpm_down: u16be(42 + index * 2),
            rpm_up: u16be(62 + index * 2),
            duty_up: raw[82 + index],
            duty_down: raw[92 + index],
            time_down: raw[102 + index] >> 4,
            time_up: raw[102 + index] & 0x0f,
        };
    }
    channel
}

pub fn encode_rpm_fan_channel(channel: &RpmFanChannel) -> [u8; 112] {
    let mut raw = [0_u8; 112];
    raw[0] = channel.id;
    raw[1] = channel.open_fan_local_temp;
    raw[2] = channel.shutdown_local_temp;
    raw[3] = channel.open_fan_temp;
    raw[4] = channel.close_fan_temp;
    raw[5] = channel.temp_shutdown;
    raw[6] = channel.temp_passive;
    raw[7] = channel.temp_fan_full;
    raw[8..10].copy_from_slice(&channel.idle_rpm.to_be_bytes());
    raw[10..12].copy_from_slice(&channel.max_rpm.to_be_bytes());
    raw[12..14].copy_from_slice(&channel.max_turbo_rpm.to_be_bytes());
    raw[14..22].copy_from_slice(&channel.reserved);
    for (index, level) in channel.levels.iter().enumerate() {
        raw[22 + index] = level.temp_down_c;
        raw[32 + index] = level.temp_up_c;
        raw[42 + index * 2..44 + index * 2].copy_from_slice(&level.rpm_down.to_be_bytes());
        raw[62 + index * 2..64 + index * 2].copy_from_slice(&level.rpm_up.to_be_bytes());
        raw[82 + index] = level.duty_up;
        raw[92 + index] = level.duty_down;
        raw[102 + index] = (level.time_down << 4) | (level.time_up & 0x0f);
    }
    raw
}

pub fn validate_rpm_fan_channel(channel: &RpmFanChannel) -> Result<()> {
    if !channel.present {
        return Ok(());
    }
    for index in 1..channel.levels.len() {
        let previous = channel.levels[index - 1];
        let current = channel.levels[index];
        if current.temp_up_c < previous.temp_up_c {
            return Err(DchuError::InvalidArgument(format!(
                "fan {} up-temperature point {} is lower than the previous point",
                channel.id,
                index + 1
            )));
        }
        if current.temp_down_c < previous.temp_down_c {
            return Err(DchuError::InvalidArgument(format!(
                "fan {} down-temperature point {} is lower than the previous point",
                channel.id,
                index + 1
            )));
        }
        if current.rpm_up < previous.rpm_up {
            return Err(DchuError::InvalidArgument(format!(
                "fan {} up-RPM point {} is lower than the previous point",
                channel.id,
                index + 1
            )));
        }
        if current.rpm_down < previous.rpm_down {
            return Err(DchuError::InvalidArgument(format!(
                "fan {} down-RPM point {} is lower than the previous point",
                channel.id,
                index + 1
            )));
        }
    }
    for level in &channel.levels {
        if level.temp_up_c > 100 || level.temp_down_c > 100 {
            return Err(DchuError::InvalidArgument(format!(
                "fan {} contains a temperature above 100 °C",
                channel.id
            )));
        }
        if channel.max_rpm != 0
            && (level.rpm_up > channel.max_rpm.saturating_add(channel.max_turbo_rpm)
                || level.rpm_down > channel.max_rpm.saturating_add(channel.max_turbo_rpm))
        {
            return Err(DchuError::InvalidArgument(format!(
                "fan {} contains an RPM above the supported ceiling",
                channel.id
            )));
        }
    }
    Ok(())
}

pub fn rpm_fan_channel_matches(expected: &RpmFanChannel, actual: &RpmFanChannel) -> bool {
    if expected.present != actual.present {
        return false;
    }
    if !expected.present {
        return true;
    }
    expected.id == actual.id
        && expected.open_fan_local_temp == actual.open_fan_local_temp
        && expected.shutdown_local_temp == actual.shutdown_local_temp
        && expected.open_fan_temp == actual.open_fan_temp
        && expected.close_fan_temp == actual.close_fan_temp
        && expected.temp_shutdown == actual.temp_shutdown
        && expected.temp_passive == actual.temp_passive
        && expected.temp_fan_full == actual.temp_fan_full
        && expected.idle_rpm == actual.idle_rpm
        && expected.max_rpm == actual.max_rpm
        && expected.max_turbo_rpm == actual.max_turbo_rpm
        && expected.reserved == actual.reserved
        && expected.levels == actual.levels
}

pub fn rgb_subcommand_packet(subcommand: u8, value: u32) -> [u8; 4] {
    let bytes = value.to_le_bytes();
    [bytes[0], bytes[1], bytes[2], subcommand]
}

/// CC40 packs RGB bytes differently for the 15-color keyboard (type 6) and
/// other RGB keyboard controllers. Keep this encoding in one tested helper.
pub fn rgb_part_value(keyboard_type: u8, color: RgbColor) -> u32 {
    if keyboard_type == 6 {
        let blue = if (color.red == 0 && color.green == 254 && color.blue == 129)
            || (color.red == 0 && color.green == 255 && color.blue == 127)
        {
            70
        } else {
            color.blue
        };
        (u32::from(blue / 2) << 16) | (u32::from(color.red / 2) << 8) | u32::from(color.green / 2)
    } else {
        let scaled_blue = ((40.0 / 51.0) * f32::from(color.blue)).round() as u8;
        (u32::from(scaled_blue) << 16) | (u32::from(color.red) << 8) | u32::from(color.green)
    }
}

/// Decode CC40's feature page (page 7), WMI support words and command 13 data.
pub fn decode_platform_capabilities(
    feature_page: &[u8; 256],
    wmi_16: i32,
    wmi_122: i32,
    wmi_96: i32,
    ui_style_id: u8,
    oled_panel: bool,
    fan_info: &[u8; 256],
) -> PlatformCapabilities {
    let flag = |value: i32, bit: u32| (value as u32 & (1_u32 << bit)) != 0;
    let version = u16::from_be_bytes([feature_page[0], feature_page[1]]);
    let mut caps = PlatformCapabilities {
        bios_feature_version: version,
        ui_style_id,
        oled_panel,
        keyboard_type: match fan_info[15] {
            3 | 19 | 35 | 51 | 243 => 3,
            value => value,
        },
        fan_count: fan_info[12],
        fan_exist_mask: fan_info[50],
        override_boot_effect: fan_info[43] & 1 != 0,
        ..PlatformCapabilities::default()
    };

    if flag(wmi_16, 0) {
        caps.uwp_power_mode = true;
        caps.quiet = true;
        caps.power_saving = true;
        caps.performance = true;
        caps.entertainment = true;
    }
    caps.flexikey = flag(wmi_16, 1);
    caps.cpu_oc = flag(wmi_122, 23);
    caps.airplane_osd_check = flag(wmi_122, 6);
    caps.perkey_default_static_color = flag(wmi_96, 8);
    caps.dtt = flag(wmi_96, 12);
    caps.amd_manual_mode = flag(wmi_96, 13);

    if version == 0x0100 {
        caps.turbo_fan = feature_page[16] & 0x10 != 0;
        if feature_page[17] & 0x0f != 0 {
            caps.quiet = feature_page[17] & 1 != 0;
            caps.power_saving = feature_page[17] & 2 != 0;
            caps.performance = feature_page[17] & 4 != 0;
            caps.entertainment = feature_page[17] & 8 != 0;
            if feature_page[17] & 0x80 != 0 {
                caps.quiet = false;
            }
        }
        caps.power_mode_ui_id = feature_page[17] >> 4;
        caps.gpu_switch = feature_page[18] & 1 != 0;
        caps.nvidia_power_off = feature_page[19] & 0x40 != 0;
        caps.slow_fan = feature_page[20] & 1 != 0;
        caps.simple_gpu_oc = feature_page[20] & 0x08 != 0;
        caps.rpm_fan_control = feature_page[20] & 0x10 != 0;
        caps.iqst_quiet = feature_page[20] & 0x40 != 0;
        caps.iqst_power_saving = feature_page[20] & 0x80 != 0;
        caps.iqst_performance = feature_page[21] & 1 != 0;
        caps.iqst_entertainment = feature_page[21] & 2 != 0;
        caps.flexi_charger_auto_enable = feature_page[21] & 4 != 0;
        caps.fn_0_9_hotkey = feature_page[21] & 0x20 != 0;
        caps.pd_power_level = feature_page[23] & 2 != 0;
        caps.ccd_power_switch = feature_page[23] & 8 != 0;
        caps.logo_a_cover_zone = feature_page[23] & 0x20 != 0;
        caps.logo_a_cover = feature_page[23] & 0x40 != 0;
        caps.lightbar_2025 = feature_page[23] & 0x80 != 0;
        caps.kb_15color_breath = feature_page[24] & 0x08 != 0;
        caps.kb_15color_dynamic_breath = feature_page[25] & 2 != 0;
        caps.pd_ap_controlled = feature_page[25] & 4 != 0;
    } else if feature_page[17] & 0x0f != 0 {
        caps.quiet = feature_page[17] & 1 != 0;
        caps.power_saving = feature_page[17] & 2 != 0;
        caps.performance = feature_page[17] & 4 != 0;
        caps.entertainment = feature_page[17] & 8 != 0;
        caps.power_mode_ui_id = feature_page[17] >> 4;
    }
    caps
}

pub fn decode_fan_curve(buffer: &[u8; 256]) -> FanCurveState {
    let mut channels = [FanCurveChannel::default(); 3];
    for (fan, channel) in channels.iter_mut().enumerate() {
        let base = 16 + fan * 8;
        for (point, slot) in channel.points.iter_mut().enumerate() {
            slot.temp_c = buffer[base + point * 2];
            slot.duty_raw = buffer[base + point * 2 + 1];
        }
    }
    FanCurveState {
        fan_count: buffer[12],
        initial_mode: buffer[14],
        channels,
    }
}

fn fan_curve_slope(low: FanCurvePoint, high: FanCurvePoint) -> u16 {
    if high.temp_c <= low.temp_c {
        return 0;
    }
    let low_duty = f32::from(low.duty_percent());
    let high_duty = f32::from(high.duty_percent());
    let delta_temp = f32::from(high.temp_c - low.temp_c);
    (((high_duty - low_duty) / delta_temp) * 2.55 * 16.0)
        .round()
        .clamp(0.0, u16::MAX as f32) as u16
}

/// Build the 256-byte block-14 FEVT frame from the EC's runtime table.
///
/// Only the middle nodes are present in the frame; the EC retains T1/D1 and
/// T4/D4. Slopes are stored high byte first, matching FAN.Write_WMI14.
pub fn encode_fan_curve_frame(curve: &FanCurveState) -> [u8; 256] {
    let mut frame = [0_u8; 256];
    for (fan, channel) in curve.channels.iter().enumerate() {
        if channel.is_unused() {
            continue;
        }
        let data = 2 + fan * 4;
        frame[data] = channel.points[1].temp_c;
        frame[data + 1] = channel.points[1].duty_raw;
        frame[data + 2] = channel.points[2].temp_c;
        frame[data + 3] = channel.points[2].duty_raw;

        let slopes = 0x0E + fan * 6;
        for (index, pair) in [(0, 1), (1, 2), (2, 3)].into_iter().enumerate() {
            let slope = fan_curve_slope(channel.points[pair.0], channel.points[pair.1]);
            frame[slopes + index * 2] = (slope >> 8) as u8;
            frame[slopes + index * 2 + 1] = slope as u8;
        }
    }
    frame
}

/// Apply user-edited middle nodes to the EC's current runtime table.
pub fn curve_with_custom_nodes(
    current: &FanCurveState,
    nodes: &CustomFanCurve,
) -> Result<FanCurveState> {
    let mut target = *current;
    for (fan, custom) in nodes.channels.iter().copied().enumerate() {
        let Some(custom) = custom else {
            if !target.channels[fan].is_unused() {
                return Err(DchuError::InvalidArgument(format!(
                    "fan channel {fan} is present but no custom nodes were supplied"
                )));
            }
            continue;
        };
        custom.validate()?;
        if target.channels[fan].is_unused() {
            return Err(DchuError::InvalidArgument(format!(
                "fan channel {fan} is not present on this platform"
            )));
        }
        let middle = [custom.t2, custom.t3];
        target.channels[fan].points[1] = FanCurvePoint {
            temp_c: middle[0],
            duty_raw: duty_raw_from_percent(custom.d2),
        };
        target.channels[fan].points[2] = FanCurvePoint {
            temp_c: middle[1],
            duty_raw: duty_raw_from_percent(custom.d3),
        };
    }
    Ok(target)
}

pub fn fan_curve_matches(expected: &FanCurveState, actual: &FanCurveState) -> bool {
    expected
        .channels
        .iter()
        .zip(actual.channels.iter())
        .all(|(want, got)| {
            if want.is_unused() {
                return got.is_unused();
            }
            (1..=2).all(|index| {
                want.points[index].temp_c == got.points[index].temp_c
                    && want.points[index]
                        .duty_raw
                        .abs_diff(got.points[index].duty_raw)
                        <= 1
            })
        })
}
