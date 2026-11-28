//! Hardware backend selection.
//!
//! Windows uses the signed AcpiBridge transport; every other target gets a
//! stub with the same surface so the UI and tests build everywhere.

#[cfg(not(windows))]
use crate::error::{DchuError, Result};
#[cfg(not(windows))]
use crate::fn_events::FnNotification;
#[cfg(not(windows))]
use crate::model::*;

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn readback_confirmed(expected: u8, actual: Option<u8>) -> bool {
    actual == Some(expected)
}

#[cfg(windows)]
pub use crate::windows_backend::Dchu;

#[cfg(not(windows))]
#[derive(Debug)]
pub struct Dchu;

#[cfg(not(windows))]
impl Dchu {
    pub fn open() -> Result<Self> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn path(&self) -> &str {
        ""
    }

    pub fn raw_read_buffer(&self, _command: i32) -> (i32, [u8; 256]) {
        (-1, [0_u8; 256])
    }

    pub fn raw_read_integer(&self, _command: i32) -> (i32, i32) {
        (-1, 0)
    }

    pub fn raw_read_settings(&self, _page: i32, _offset: i32, length: usize) -> (i32, Vec<u8>) {
        (-1, vec![0_u8; length])
    }

    pub fn read_thermal(&self) -> Result<ThermalSample> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn read_platform_state(&self) -> PlatformState {
        PlatformState::default()
    }

    pub fn read_setting_byte(&self, _page: i32, _offset: i32) -> Option<u8> {
        None
    }

    pub fn write_setting_byte(&self, _page: i32, _offset: i32, _value: u8) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_power_mode(&self, _mode: PowerMode) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn release_proc_hot(&self) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_fan_mode(&self, _mode: FanMode) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_slow_fan(&self, _enabled: bool) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_turbo_fan(&self, _enabled: bool) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn run_anti_dust(&self) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_low_refresh_rate(&self, _enabled: bool) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn read_anti_dust_schedule(&self) -> Result<AntiDustSchedule> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_anti_dust_schedule(&self, _schedule: AntiDustSchedule) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_fan_offset(&self, _percent: u8) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn read_fan_curve(&self) -> Result<FanCurveState> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn read_rpm_fan_table(&self) -> Result<RpmFanTable> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn write_rpm_fan_table(&self, _table: &RpmFanTable) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_custom_rpm_fan_table(&self, _table: &RpmFanTable) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_custom_fan_curve(&self, _curve: CustomFanCurve) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn write_fan_curve_table(&self, _curve: &FanCurveState) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn read_quick_toggles(&self) -> QuickToggleState {
        QuickToggleState::default()
    }

    pub fn read_gpu_switch(&self) -> Result<GpuSwitchState> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_gpu_switch(&self, _mode: GpuSwitchMode) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn stage_gpu_switch(&self, _mode: GpuSwitchMode) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn read_flexi_charger(&self) -> Result<FlexiChargerState> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_flexi_charger(
        &self,
        _mode: FlexiChargerMode,
        _start_threshold: u8,
        _stop_threshold: u8,
    ) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn read_capabilities(&self) -> Result<PlatformCapabilities> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn read_rgb_keyboard_state(&self) -> Result<RgbKeyboardState> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_rgb_keyboard_brightness(&self, _level: u8) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_rgb_keyboard_color(&self, _color: RgbColor) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_rgb_keyboard_effect(&self, _effect: u8) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_rgb_keyboard_sleep_timer(&self, _enabled: bool, _seconds: u32) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn set_rgb_keyboard_boot_effect(&self, _enabled: bool) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn notify_airplane_mode(&self, _enabled: bool) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn read_camera_enabled(&self) -> Option<bool> {
        None
    }

    pub fn set_camera_enabled(&self, _enabled: bool) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn read_touchpad_enabled(&self) -> Option<bool> {
        None
    }

    pub fn set_touchpad_enabled(&self, _enabled: bool) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn has_extended_api(&self) -> bool {
        false
    }

    pub fn set_quick_toggle(&self, _toggle: QuickToggle, _enabled: bool) -> Result<()> {
        Err(DchuError::UnsupportedPlatform)
    }
}

#[cfg(not(windows))]
#[derive(Debug)]
pub struct AcpiNotificationListener;

#[cfg(not(windows))]
impl AcpiNotificationListener {
    pub fn open(_device_path: &str) -> Result<Self> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn wait_next(&mut self) -> Result<FnNotification> {
        Err(DchuError::UnsupportedPlatform)
    }

    pub fn wait_next_timeout(
        &mut self,
        _timeout: std::time::Duration,
    ) -> Result<Option<FnNotification>> {
        Err(DchuError::UnsupportedPlatform)
    }
}
