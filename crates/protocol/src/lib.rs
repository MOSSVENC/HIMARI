//! Direct, in-process transport to the signed AcpiBridge device.
//!
//! HIMARI does not load the vendor Control Center codec DLL, launch
//! `CC40.exe`, replace the signed driver, or write EC/BIOS firmware. On Windows
//! this crate opens the signed AcpiBridge device and sends the recovered DCHU
//! IOCTL frames directly. Non-Windows builds expose the same data model with an
//! `UnsupportedPlatform` error so the UI can still be developed.

mod error;
mod flexikey;
mod fn_events;
mod keyboard_filter;
#[cfg(windows)]
mod win_device;
pub use flexikey::{
    FlexiKeyAction, FlexiKeyBinding, FlexiKeyDevice, FlexiKeyError, FlexiKeyProfile, FlexiKeyStore,
    MacroStep, MacroStepKind,
};
pub use fn_events::{Adjustment, FanNotice, FnEvent, FnNotification};
pub use keyboard_filter::{
    DEVICE_PATH as KEYBOARD_FILTER_DEVICE_PATH, FilterMacro, FilterRecord,
    IOCTL_ADD_FILTER_MACRO as KEYBOARD_FILTER_ADD_MACRO_IOCTL,
    IOCTL_CLEAR_FILTER_MACRO as KEYBOARD_FILTER_CLEAR_MACRO_IOCTL,
    IOCTL_ENABLE_FILTER as KEYBOARD_FILTER_ENABLE_IOCTL,
    IOCTL_ENABLE_USB_DEVICE as KEYBOARD_FILTER_ENABLE_USB_IOCTL,
    IOCTL_REMOVE_FILTER_MACRO as KEYBOARD_FILTER_REMOVE_MACRO_IOCTL, KeyboardFilter,
    encode_filter_macro, encode_filter_macro_removal,
};
#[cfg(windows)]
pub use windows_backend::AcpiNotificationListener;

mod model;
pub use model::*;
mod frames;
pub use error::{DchuError, Result};
pub use frames::*;

mod dchu;
pub use dchu::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_macro_frame_has_vendor_collection_layout() {
        let frame = encode_filter_macro(
            &[FilterRecord::key_code(0x7A), FilterRecord::key_code(0)],
            &[
                FilterRecord::macro_down(0x7B, 10),
                FilterRecord::macro_up(0x7B, 10),
            ],
        );
        assert_eq!(frame.len(), 72);
        assert_eq!(&frame[0..4], &32_u32.to_le_bytes());
        assert_eq!(&frame[4..8], &32_u32.to_le_bytes());
        assert_eq!(u16::from_le_bytes([frame[8 + 12], frame[8 + 13]]), 0x7A);
        assert_eq!(u16::from_le_bytes([frame[8 + 14], frame[8 + 15]]), 0);
        assert_eq!(u16::from_le_bytes([frame[40 + 14], frame[40 + 15]]), 1);
        assert_eq!(u16::from_le_bytes([frame[56 + 14], frame[56 + 15]]), 2);

        let removal =
            encode_filter_macro_removal(&[FilterRecord::key_code(0x7A), FilterRecord::key_code(0)]);
        assert_eq!(removal.len(), 40);
        assert_eq!(&removal[0..4], &32_u32.to_le_bytes());
        assert_eq!(&removal[4..8], &0_u32.to_le_bytes());
    }

    #[test]
    fn anti_dust_schedule_round_trips_vendor_packing() {
        let schedule = AntiDustSchedule {
            enabled: true,
            resume: true,
            day: 5,
            hour: 3,
        };
        assert_eq!(schedule.command_value(), 5 + 128 + 3 * 256);
        assert_eq!(schedule.to_settings(), [1, 5 | 0x80, 3, 0, 0]);
        assert_eq!(
            AntiDustSchedule::from_settings(&schedule.to_settings()),
            Some(schedule)
        );
        assert!(
            AntiDustSchedule {
                enabled: true,
                resume: false,
                day: 0,
                hour: 24
            }
            .validate()
            .is_err()
        );
        let disabled = AntiDustSchedule {
            enabled: false,
            resume: true,
            day: 3,
            hour: 4,
        };
        assert_eq!(disabled.command_value(), 127 + 128 + 4 * 256);
        assert_eq!(disabled.to_settings()[1], 127 | 0x80);
    }

    #[test]
    fn power_profiles_map_to_the_vendor_energy_star_command() {
        assert_eq!(PowerMode::Performance.energy_star_mode(), Some(0));
        assert_eq!(PowerMode::PowerSaving.energy_star_mode(), Some(1));
        assert_eq!(PowerMode::QuietPlus.energy_star_mode(), Some(1));
        assert_eq!(PowerMode::Quiet.energy_star_mode(), Some(2));
        assert_eq!(PowerMode::Entertainment.energy_star_mode(), Some(2));
        assert_eq!(PowerMode::Turbo.energy_star_mode(), None);
    }

    #[test]
    fn hardware_power_value_preserves_turbo_only_in_performance() {
        assert_eq!(PowerMode::Performance.command_value(false), Some(2));
        assert_eq!(PowerMode::Performance.command_value(true), Some(0x42));
        for turbo in [false, true] {
            assert_eq!(PowerMode::Quiet.command_value(turbo), Some(0));
            assert_eq!(PowerMode::PowerSaving.command_value(turbo), Some(1));
            assert_eq!(PowerMode::Entertainment.command_value(turbo), Some(3));
            assert_eq!(PowerMode::QuietPlus.command_value(turbo), Some(0));
            assert_eq!(PowerMode::Turbo.command_value(turbo), None);
        }
    }

    #[test]
    fn rpm_formula_matches_live_samples() {
        assert_eq!(rpm_from_raw(0), 0);
        assert_eq!(rpm_from_raw(614), 3512);
        assert_eq!(rpm_from_raw(1023), 2108);
    }

    #[test]
    fn duty_rounding_is_bounded() {
        assert_eq!(duty_from_raw(0), 0);
        assert_eq!(duty_from_raw(111), 44);
        assert_eq!(duty_from_raw(255), 100);
    }

    #[test]
    fn fan_curve_layout_and_slopes_match_vendor_table() {
        let mut buffer = [0_u8; 256];
        buffer[12] = 2;
        buffer[14] = 0;
        let baseline = [
            [40_u8, 71_u8, 61_u8, 105_u8, 88_u8, 150_u8, 100_u8, 255_u8],
            [40, 71, 61, 105, 88, 150, 100, 255],
            [0, 0, 0, 0, 0, 0, 0, 0],
        ];
        for (fan, values) in baseline.iter().enumerate() {
            let base = 16 + fan * 8;
            for (index, value) in values.iter().enumerate() {
                buffer[base + index] = *value;
            }
        }
        let curve = decode_fan_curve(&buffer);
        assert_eq!(curve.fan_count, 2);
        assert_eq!(curve.channels[0].points[1].duty_percent(), 41);
        assert!(curve.channels[2].is_unused());
        let frame = encode_fan_curve_frame(&curve);
        assert_eq!(frame[2], 61);
        assert_eq!(frame[3], 105);
        assert_eq!(frame[4], 88);
        assert_eq!(frame[5], 150);
        assert_eq!(&frame[14..20], &[0, 25, 0, 27, 0, 139]);
    }

    #[test]
    fn rpm_fan_channel_round_trips_all_ten_nodes() {
        let mut channel = RpmFanChannel {
            present: true,
            id: 0x5a,
            open_fan_local_temp: 40,
            shutdown_local_temp: 95,
            open_fan_temp: 45,
            close_fan_temp: 40,
            temp_shutdown: 96,
            temp_passive: 80,
            temp_fan_full: 90,
            idle_rpm: 0,
            max_rpm: 5200,
            max_turbo_rpm: 5600,
            reserved: [1, 2, 3, 4, 5, 6, 7, 8],
            levels: [RpmFanLevel::default(); 10],
        };
        for (index, level) in channel.levels.iter_mut().enumerate() {
            *level = RpmFanLevel {
                temp_down_c: 35 + index as u8,
                temp_up_c: 40 + index as u8,
                rpm_down: 1000 + index as u16 * 100,
                rpm_up: 1200 + index as u16 * 100,
                duty_up: 20 + index as u8,
                duty_down: 15 + index as u8,
                time_down: index as u8 % 4,
                time_up: (index as u8 + 1) % 4,
            };
        }
        let raw = encode_rpm_fan_channel(&channel);
        let decoded = decode_rpm_fan_channel(&raw, true);
        assert_eq!(decoded, channel);
        assert!(rpm_fan_channel_matches(&channel, &decoded));
    }

    #[test]
    fn custom_fan_nodes_preserve_fixed_points_and_validate_order() {
        let mut current = FanCurveState::default();
        current.channels[0] = FanCurveChannel {
            points: [
                FanCurvePoint {
                    temp_c: 40,
                    duty_raw: 71,
                },
                FanCurvePoint {
                    temp_c: 61,
                    duty_raw: 105,
                },
                FanCurvePoint {
                    temp_c: 88,
                    duty_raw: 150,
                },
                FanCurvePoint {
                    temp_c: 100,
                    duty_raw: 255,
                },
            ],
        };
        let nodes = CustomFanCurve {
            channels: [
                Some(FanCurveNodes {
                    t2: 55,
                    d2: 40,
                    t3: 85,
                    d3: 70,
                }),
                None,
                None,
            ],
        };
        let target = curve_with_custom_nodes(&current, &nodes).unwrap();
        assert_eq!(target.channels[0].points[0], current.channels[0].points[0]);
        assert_eq!(target.channels[0].points[3], current.channels[0].points[3]);
        assert_eq!(target.channels[0].points[1].temp_c, 55);
        assert_eq!(target.channels[0].points[2].temp_c, 85);
        assert!(nodes.channels[0].unwrap().validate().is_ok());
        assert!(
            FanCurveNodes {
                t2: 70,
                d2: 40,
                t3: 60,
                d3: 70
            }
            .validate()
            .is_err()
        );
        assert!(
            FanCurveNodes {
                t2: 60,
                d2: 80,
                t3: 80,
                d3: 70
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn gpu_switch_modes_and_mask_are_stable() {
        assert_eq!(GpuSwitchMode::Integrated.as_u8(), 1);
        assert_eq!(GpuSwitchMode::Discrete.as_u8(), 2);
        assert_eq!(GpuSwitchMode::MsHybrid.as_u8(), 3);
        assert_eq!(GpuSwitchMode::Dynamic.as_u8(), 4);
        let state = GpuSwitchState {
            current_mode: 3,
            supported_mask: 0b110,
        };
        assert!(state.supports(GpuSwitchMode::Discrete));
        assert!(state.supports(GpuSwitchMode::MsHybrid));
        assert!(!state.supports(GpuSwitchMode::Integrated));
    }

    #[test]
    fn flexi_charger_modes_have_stable_values() {
        assert_eq!(FlexiChargerMode::Disabled.as_u8(), 0);
        assert_eq!(FlexiChargerMode::Preset70To80.as_u8(), 1);
        assert_eq!(FlexiChargerMode::Custom.as_u8(), 2);
        assert_eq!(FlexiChargerMode::Preset70To80.thresholds(), Some((70, 80)));
    }

    #[test]
    fn quick_toggle_values_round_trip() {
        let state = QuickToggleState::from_raw_values([1, 2, 1, 2, 1]);
        assert_eq!(state.fn_lock, Some(true));
        assert_eq!(state.win_lock, Some(true));
        assert_eq!(state.raw_values(), Some([1, 2, 1, 2, 1]));
        assert_eq!(
            QuickToggleState::from_raw_values([0, 1, 2, 3, 4]).raw_values(),
            None
        );
    }

    #[test]
    fn readback_must_match_the_requested_target() {
        assert!(readback_confirmed(3, Some(3)));
        assert!(!readback_confirmed(3, Some(2)));
        assert!(!readback_confirmed(3, None));
    }

    #[test]
    fn decodes_live_feature_page_and_keyboard_capabilities() {
        let mut features = [0_u8; 256];
        let live = [
            0x01, 0x00, 0x25, 0x00, 0x00, 0x04, 0x55, 0x23, 0x01, 0x00, 0x1c, 0x02, 0x00, 0x93,
            0x08, 0x82, 0x10, 0x8d, 0x01, 0x40, 0x11, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x01,
        ];
        features[..live.len()].copy_from_slice(&live);
        let mut fan_info = [0_u8; 256];
        fan_info[12] = 2;
        fan_info[15] = 6;
        fan_info[43] = 3;
        fan_info[50] = 3;
        let caps = decode_platform_capabilities(
            &features,
            0x0893,
            0x0400_0025,
            0x021c,
            4,
            false,
            &fan_info,
        );
        assert_eq!(caps.bios_feature_version, 0x0100);
        assert_eq!(caps.power_mode_ui_id, 8);
        assert!(caps.turbo_fan);
        assert!(!caps.power_saving);
        assert!(caps.performance);
        assert!(caps.entertainment);
        assert!(!caps.quiet);
        assert!(caps.gpu_switch);
        assert!(caps.nvidia_power_off);
        assert!(caps.slow_fan);
        assert!(caps.rpm_fan_control);
        assert!(!caps.simple_gpu_oc);
        assert!(caps.uwp_power_mode);
        assert!(caps.flexikey);
        assert_eq!(caps.keyboard_type, 6);
        assert_eq!(caps.fan_count, 2);
        assert_eq!(caps.fan_exist_mask, 3);
        assert!(caps.override_boot_effect);
    }

    #[test]
    fn rgb_keyboard_encoding_matches_vendor_type_6() {
        let color = RgbColor {
            red: 200,
            green: 100,
            blue: 50,
        };
        assert_eq!(rgb_part_value(6, color), 0x0019_6432);
        assert_eq!(
            rgb_subcommand_packet(240, 0x0019_6432),
            [0x32, 0x64, 0x19, 240]
        );
        let special = RgbColor {
            red: 0,
            green: 254,
            blue: 129,
        };
        assert_eq!(rgb_part_value(6, special), 0x0023_007f);
    }

    #[test]
    fn decodes_confirmed_telemetry_layout() {
        let mut buffer = [0_u8; 256];
        buffer[2..4].copy_from_slice(&614_u16.to_be_bytes());
        buffer[4..6].copy_from_slice(&1023_u16.to_be_bytes());
        buffer[16] = 111;
        buffer[18] = 68;
        buffer[19] = 65;
        buffer[21] = 50;
        let sample = decode_thermal(&buffer);
        assert!(sample.has_plausible_temperature());
        assert_eq!(sample.cpu.rpm, 3512);
        assert_eq!(sample.cpu.temperature_c, 68);
        assert_eq!(sample.cpu.duty_percent, 44);
        assert_eq!(sample.gpu.rpm, 2108);
        assert_eq!(sample.gpu.temperature_c, 50);
        assert_eq!(sample.gpu.duty_percent, 25);

        assert!(!decode_thermal(&[0_u8; 256]).has_plausible_temperature());
        assert!(!is_plausible_temperature(0));
        assert!(!is_plausible_temperature(126));
        assert!(is_plausible_temperature(1));
        assert!(is_plausible_temperature(125));
    }
}
