//! Pure data model shared by the transport, workers and UI.
//!
//! Plain data plus the arithmetic that belongs to it: no device handles, no
//! IOCTL frames and no platform conditionals.

use crate::error::{DchuError, Result};

pub const TELEMETRY_COMMAND: i32 = 12;
pub const POWER_COMMAND: i32 = 121;
pub const FAN_COMMAND: i32 = 121;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FanChannel {
    pub rpm: u32,
    pub duty_percent: u8,
    pub temperature_c: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ThermalSample {
    pub cpu: FanChannel,
    pub gpu: FanChannel,
    pub fan3: FanChannel,
    pub fan4: FanChannel,
}

impl ThermalSample {
    /// A usable sample must contain at least one physically plausible CPU or
    /// GPU temperature. This filters the all-zero response some ECs return
    /// while their thermal mailbox is being refreshed.
    pub fn has_plausible_temperature(&self) -> bool {
        is_plausible_temperature(self.cpu.temperature_c)
            || is_plausible_temperature(self.gpu.temperature_c)
    }
}

pub fn is_plausible_temperature(celsius: u8) -> bool {
    (1..=125).contains(&celsius)
}

/// One point in the EC's four-point fan curve. Duty is kept in the vendor's
/// native 0..=255 representation so a read can be round-tripped exactly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FanCurvePoint {
    pub temp_c: u8,
    pub duty_raw: u8,
}

impl FanCurvePoint {
    pub fn duty_percent(self) -> u8 {
        duty_from_raw(self.duty_raw)
    }

    pub const fn is_unused(self) -> bool {
        self.temp_c == 0 && self.duty_raw == 0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FanCurveChannel {
    pub points: [FanCurvePoint; 4],
}

impl FanCurveChannel {
    pub const fn is_unused(self) -> bool {
        self.points[0].is_unused()
            && self.points[1].is_unused()
            && self.points[2].is_unused()
            && self.points[3].is_unused()
    }
}

impl std::ops::Index<usize> for FanCurveChannel {
    type Output = FanCurvePoint;

    fn index(&self, index: usize) -> &Self::Output {
        &self.points[index]
    }
}

/// Runtime fan table read from DCHU command 13 (EEVT). The EC echoes the table
/// it is actually executing, which makes this the authoritative readback after
/// a block-14 custom-curve write.
/// CC40 anti-dust scheduler decoded from page 4 offsets 81..83.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AntiDustSchedule {
    pub enabled: bool,
    pub resume: bool,
    pub day: u8,
    pub hour: u8,
}

impl AntiDustSchedule {
    pub fn from_settings(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 3 {
            return None;
        }
        let day = bytes[1] & 0x7f;
        Some(Self {
            // Page 4/81 is the scheduler initialization byte. The vendor uses
            // day 127 as the "no scheduled day" sentinel.
            enabled: day != 127,
            resume: bytes[1] & 0x80 != 0,
            day,
            hour: bytes[2],
        })
    }

    pub fn to_settings(self) -> [u8; 5] {
        let day = if self.enabled { self.day } else { 127 };
        [1, day | (u8::from(self.resume) << 7), self.hour, 0, 0]
    }

    pub fn command_value(self) -> u32 {
        let day = if self.enabled { self.day } else { 127 };
        u32::from(day) + (u32::from(self.resume) << 7) + (u32::from(self.hour) << 8)
    }

    pub fn validate(self) -> Result<()> {
        if self.day > 127 {
            return Err(DchuError::InvalidArgument(
                "anti-dust day must be within 0..=127".to_owned(),
            ));
        }
        if self.hour > 23 {
            return Err(DchuError::InvalidArgument(
                "anti-dust hour must be within 0..=23".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FanCurveState {
    pub fan_count: u8,
    pub initial_mode: u8,
    pub channels: [FanCurveChannel; 3],
}

/// One of the ten points in CC40's RPM-controlled fan table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RpmFanLevel {
    pub temp_down_c: u8,
    pub temp_up_c: u8,
    pub rpm_down: u16,
    pub rpm_up: u16,
    pub duty_up: u8,
    pub duty_down: u8,
    pub time_down: u8,
    pub time_up: u8,
}

/// A complete fan module from the RPM fan-table protocol. Reserved bytes are
/// retained so read/modify/write operations never discard vendor data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RpmFanChannel {
    pub present: bool,
    pub id: u8,
    pub open_fan_local_temp: u8,
    pub shutdown_local_temp: u8,
    pub open_fan_temp: u8,
    pub close_fan_temp: u8,
    pub temp_shutdown: u8,
    pub temp_passive: u8,
    pub temp_fan_full: u8,
    pub idle_rpm: u16,
    pub max_rpm: u16,
    pub max_turbo_rpm: u16,
    pub reserved: [u8; 8],
    pub levels: [RpmFanLevel; 10],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RpmFanTable {
    pub channels: [RpmFanChannel; 4],
    pub fan12_raw: [u8; 256],
    pub fan34_raw: [u8; 256],
}

impl Default for RpmFanTable {
    fn default() -> Self {
        Self {
            channels: [RpmFanChannel::default(); 4],
            fan12_raw: [0; 256],
            fan34_raw: [0; 256],
        }
    }
}

/// User-editable middle nodes for one fan. Duties are percentages, matching the
/// vendor UI and the block-14 FEVT frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FanCurveNodes {
    pub t2: u8,
    pub d2: u8,
    pub t3: u8,
    pub d3: u8,
}

impl Default for FanCurveNodes {
    fn default() -> Self {
        Self {
            t2: 60,
            d2: 45,
            t3: 80,
            d3: 65,
        }
    }
}

impl FanCurveNodes {
    pub fn validate(self) -> Result<()> {
        if !(20..=98).contains(&self.t2) {
            return Err(DchuError::InvalidArgument(
                "fan curve T2 must be within 20..=98 °C".to_owned(),
            ));
        }
        if self.t3 <= self.t2 || self.t3 > 99 {
            return Err(DchuError::InvalidArgument(
                "fan curve T3 must be greater than T2 and at most 99 °C".to_owned(),
            ));
        }
        if !(20..=100).contains(&self.d2) {
            return Err(DchuError::InvalidArgument(
                "fan curve D2 must be within 20..=100%".to_owned(),
            ));
        }
        if self.d3 < self.d2 || self.d3 > 100 {
            return Err(DchuError::InvalidArgument(
                "fan curve D3 must be monotonic and at most 100%".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Middle nodes for the three hardware fan channels. An unused channel may be
/// represented by all-zero nodes and is omitted from validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CustomFanCurve {
    pub channels: [Option<FanCurveNodes>; 3],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlatformState {
    pub power_mode: Option<u8>,
    pub fan_mode: Option<u8>,
    pub fan_offset: Option<u8>,
    pub turbo_fan: Option<bool>,
    pub slow_fan: Option<bool>,
    pub anti_dust_running: Option<bool>,
    pub power_policy: Option<u8>,
    pub capabilities: Option<i32>,
    pub platform_id: Option<i32>,
    pub low_refresh_rate_supported: Option<bool>,
    pub low_refresh_rate_enabled: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuSwitchState {
    pub current_mode: u8,
    pub supported_mask: u8,
}

impl GpuSwitchState {
    pub const fn current_label(self) -> &'static str {
        match self.current_mode {
            1 => "Integrated GPU",
            2 => "Discrete GPU",
            3 => "MSHybrid",
            4 => "Dynamic",
            _ => "Unknown",
        }
    }

    pub const fn supports(self, mode: GpuSwitchMode) -> bool {
        self.supported_mask & (1 << (mode.as_u8() - 1)) != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GpuSwitchMode {
    Integrated = 1,
    Discrete = 2,
    MsHybrid = 3,
    Dynamic = 4,
}

impl GpuSwitchMode {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Integrated),
            2 => Some(Self::Discrete),
            3 => Some(Self::MsHybrid),
            4 => Some(Self::Dynamic),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Integrated => "Integrated GPU",
            Self::Discrete => "Discrete GPU",
            Self::MsHybrid => "MSHybrid",
            Self::Dynamic => "Dynamic",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FlexiChargerState {
    pub mode: u8,
    pub start_threshold: u8,
    pub stop_threshold: u8,
    pub threshold_choices: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FlexiChargerMode {
    Disabled = 0,
    Preset70To80 = 1,
    Custom = 2,
}

impl FlexiChargerMode {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Disabled),
            1 => Some(Self::Preset70To80),
            2 => Some(Self::Custom),
            _ => None,
        }
    }

    pub const fn thresholds(self) -> Option<(u8, u8)> {
        match self {
            Self::Disabled => None,
            Self::Preset70To80 => Some((70, 80)),
            Self::Custom => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PowerMode {
    Quiet = 0,
    PowerSaving = 1,
    Performance = 2,
    Entertainment = 3,
    QuietPlus = 4,
    Turbo = 66,
}

impl PowerMode {
    pub const UI_MODES: [Self; 4] = [
        Self::PowerSaving,
        Self::Performance,
        Self::Entertainment,
        Self::QuietPlus,
    ];

    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Quiet => "Quiet",
            Self::PowerSaving => "Power saving",
            Self::Performance => "Performance",
            Self::Entertainment => "Entertainment",
            Self::QuietPlus => "Quiet+",
            Self::Turbo => "Turbo",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Quiet => "Lowest platform power and fan activity.",
            Self::PowerSaving => "Prioritizes battery life and lower heat.",
            Self::Performance => "Maximum sustained platform response.",
            Self::Entertainment => "Balances acoustics with sustained response.",
            Self::QuietPlus => "The quietest supported platform profile.",
            Self::Turbo => "Firmware composite turbo profile.",
        }
    }

    /// The vendor Energy Star policy sent through DCHU command 79.
    ///
    /// FnKey maps these UI profiles to DCHU command 79. Keeping the mapping in
    /// one typed place prevents the boot agent and the interactive UI from
    /// applying only half of the vendor power transition.
    pub const fn energy_star_mode(self) -> Option<u8> {
        match self {
            Self::Quiet | Self::Entertainment => Some(2),
            Self::PowerSaving | Self::QuietPlus => Some(1),
            Self::Performance => Some(0),
            Self::Turbo => None,
        }
    }

    /// Command 121/25's hardware value differs from the persisted UI profile.
    /// Performance retains the Turbo fan flag; Quiet+ uses the Quiet value.
    pub const fn command_value(self, turbo_fan: bool) -> Option<u8> {
        match self {
            Self::Performance => Some(2 | if turbo_fan { 0x40 } else { 0 }),
            Self::QuietPlus => Some(0),
            Self::Turbo => None,
            _ => Some(self.as_u8()),
        }
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Quiet),
            1 => Some(Self::PowerSaving),
            2 => Some(Self::Performance),
            3 => Some(Self::Entertainment),
            4 => Some(Self::QuietPlus),
            66 => Some(Self::Turbo),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FanMode {
    Auto = 0,
    Maximum = 1,
    Silent = 3,
    MaxQ = 5,
    Custom = 6,
    AntiDust = 7,
    Noiseless = 8,
    Iqst = 9,
}

impl FanMode {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Auto => "Automatic",
            Self::Maximum => "Maximum",
            Self::Silent => "Silent",
            Self::MaxQ => "MaxQ",
            Self::Custom => "Custom curve",
            Self::AntiDust => "Anti-dust",
            Self::Noiseless => "Noiseless",
            Self::Iqst => "IQST",
        }
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Auto),
            1 => Some(Self::Maximum),
            3 => Some(Self::Silent),
            5 => Some(Self::MaxQ),
            6 => Some(Self::Custom),
            7 => Some(Self::AntiDust),
            8 => Some(Self::Noiseless),
            9 => Some(Self::Iqst),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickToggle {
    FnLock,
    WinLock,
    SwapWinFn,
    HotkeysControlCenter,
    Copilot,
}

impl QuickToggle {
    pub const fn label(self) -> &'static str {
        match self {
            Self::FnLock => "Fn lock",
            Self::WinLock => "Windows key lock",
            Self::SwapWinFn => "Swap Win / Fn",
            Self::HotkeysControlCenter => "Control Center hotkey",
            Self::Copilot => "Copilot key",
        }
    }

    pub const fn settings_offset(self) -> i32 {
        match self {
            Self::FnLock => 36,
            Self::WinLock => 37,
            Self::SwapWinFn => 38,
            Self::HotkeysControlCenter => 43,
            Self::Copilot => 44,
        }
    }

    const fn raw_is_inverted(self) -> bool {
        matches!(self, Self::WinLock)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QuickToggleState {
    pub fn_lock: Option<bool>,
    pub win_lock: Option<bool>,
    pub swap_win_fn: Option<bool>,
    pub hotkeys_control_center: Option<bool>,
    pub copilot: Option<bool>,
}

impl QuickToggleState {
    pub const fn value(self, toggle: QuickToggle) -> Option<bool> {
        match toggle {
            QuickToggle::FnLock => self.fn_lock,
            QuickToggle::WinLock => self.win_lock,
            QuickToggle::SwapWinFn => self.swap_win_fn,
            QuickToggle::HotkeysControlCenter => self.hotkeys_control_center,
            QuickToggle::Copilot => self.copilot,
        }
    }

    pub fn set_value(&mut self, toggle: QuickToggle, enabled: bool) {
        match toggle {
            QuickToggle::FnLock => self.fn_lock = Some(enabled),
            QuickToggle::WinLock => self.win_lock = Some(enabled),
            QuickToggle::SwapWinFn => self.swap_win_fn = Some(enabled),
            QuickToggle::HotkeysControlCenter => self.hotkeys_control_center = Some(enabled),
            QuickToggle::Copilot => self.copilot = Some(enabled),
        }
    }

    /// Vendor ABI encoding: 1 enabled, 2 disabled. The Windows-key lock
    /// field is inverted by CC40's configuration IL.
    pub fn raw_values(self) -> Option<[u8; 5]> {
        Some([
            encode_toggle(QuickToggle::FnLock, self.fn_lock?),
            encode_toggle(QuickToggle::WinLock, self.win_lock?),
            encode_toggle(QuickToggle::SwapWinFn, self.swap_win_fn?),
            encode_toggle(
                QuickToggle::HotkeysControlCenter,
                self.hotkeys_control_center?,
            ),
            encode_toggle(QuickToggle::Copilot, self.copilot?),
        ])
    }

    /// CC40 treats vendor value 0 as an uninitialized preference and builds
    /// its first write from unchecked defaults. Match that behavior explicitly.
    pub fn raw_values_or_defaults(self) -> [u8; 5] {
        [
            encode_toggle(QuickToggle::FnLock, self.fn_lock.unwrap_or(false)),
            encode_toggle(QuickToggle::WinLock, self.win_lock.unwrap_or(false)),
            encode_toggle(QuickToggle::SwapWinFn, self.swap_win_fn.unwrap_or(false)),
            encode_toggle(
                QuickToggle::HotkeysControlCenter,
                self.hotkeys_control_center.unwrap_or(false),
            ),
            encode_toggle(QuickToggle::Copilot, self.copilot.unwrap_or(false)),
        ]
    }

    pub fn from_raw_values(values: [u8; 5]) -> Self {
        Self {
            fn_lock: decode_toggle(QuickToggle::FnLock, values[0]),
            win_lock: decode_toggle(QuickToggle::WinLock, values[1]),
            swap_win_fn: decode_toggle(QuickToggle::SwapWinFn, values[2]),
            hotkeys_control_center: decode_toggle(QuickToggle::HotkeysControlCenter, values[3]),
            copilot: decode_toggle(QuickToggle::Copilot, values[4]),
        }
    }
}

const fn encode_toggle(toggle: QuickToggle, enabled: bool) -> u8 {
    let value = if enabled { 1 } else { 2 };
    if toggle.raw_is_inverted() {
        match value {
            1 => 2,
            2 => 1,
            _ => unreachable!(),
        }
    } else {
        value
    }
}

const fn decode_toggle(toggle: QuickToggle, value: u8) -> Option<bool> {
    if toggle.raw_is_inverted() {
        match value {
            1 => Some(false),
            2 => Some(true),
            _ => None,
        }
    } else {
        match value {
            1 => Some(true),
            2 => Some(false),
            _ => None,
        }
    }
}

/// Capability flags decoded from the vendor feature page and WMI support words.
/// These flags decide which CC4.0 controls are valid on the current platform.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlatformCapabilities {
    pub bios_feature_version: u16,
    pub power_saving: bool,
    pub performance: bool,
    pub flexikey: bool,
    pub uwp_power_mode: bool,
    pub airplane_osd_check: bool,
    pub entertainment: bool,
    pub quiet: bool,
    pub turbo_fan: bool,
    pub gpu_switch: bool,
    pub nvidia_power_off: bool,
    pub slow_fan: bool,
    pub simple_gpu_oc: bool,
    pub rpm_fan_control: bool,
    pub iqst_quiet: bool,
    pub iqst_power_saving: bool,
    pub iqst_performance: bool,
    pub iqst_entertainment: bool,
    pub flexi_charger_auto_enable: bool,
    pub fn_0_9_hotkey: bool,
    pub pd_power_level: bool,
    pub ccd_power_switch: bool,
    pub logo_a_cover_zone: bool,
    pub logo_a_cover: bool,
    pub lightbar_2025: bool,
    pub kb_15color_breath: bool,
    pub kb_15color_dynamic_breath: bool,
    pub pd_ap_controlled: bool,
    pub cpu_oc: bool,
    pub perkey_default_static_color: bool,
    pub dtt: bool,
    pub amd_manual_mode: bool,
    pub power_mode_ui_id: u8,
    pub ui_style_id: u8,
    pub oled_panel: bool,
    pub keyboard_type: u8,
    pub fan_count: u8,
    pub fan_exist_mask: u8,
    pub override_boot_effect: bool,
}

impl PlatformCapabilities {
    pub fn supports_power_mode(self, mode: PowerMode) -> bool {
        match mode {
            PowerMode::Quiet => self.quiet,
            PowerMode::PowerSaving => self.power_saving,
            PowerMode::Performance => self.performance,
            PowerMode::Entertainment => self.entertainment,
            PowerMode::QuietPlus => self.quiet,
            PowerMode::Turbo => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RgbColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RgbKeyboardState {
    pub boot_effect: bool,
    pub effect: u8,
    pub brightness: u8,
    pub enabled: bool,
    pub color: RgbColor,
    pub sleep_enabled: bool,
    pub sleep_seconds: u32,
}

pub fn rpm_from_raw(raw: u16) -> u32 {
    if raw == 0 {
        return 0;
    }
    // Confirmed CC40 formula:
    // round(2 * 60 / (0.00005565217391304348 * raw_rpm))
    (120.0 / (0.00005565217391304348 * f64::from(raw))).round() as u32
}

pub fn duty_from_raw(raw: u8) -> u8 {
    (f32::from(raw) / 255.0 * 100.0).round() as u8
}

pub fn duty_raw_from_percent(percent: u8) -> u8 {
    (f32::from(percent.clamp(0, 100)) / 100.0 * 255.0).round() as u8
}
