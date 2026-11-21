//! Resident Fn-event ownership, action dispatch and OSD feedback.

use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;

use himari_protocol::{Adjustment, Dchu, FanNotice, FnEvent, FnNotification, PowerMode};
use slint::{ComponentHandle, Weak};

use crate::{MainWindow, localization::tr, osd, osd_glyph::Glyph};

static MAIN_WINDOW: OnceLock<Mutex<Option<Weak<MainWindow>>>> = OnceLock::new();

pub(crate) fn install_ui(window: Weak<MainWindow>) {
    let slot = MAIN_WINDOW.get_or_init(|| Mutex::new(None));
    if let Ok(mut slot) = slot.lock() {
        *slot = Some(window);
    }
}

/// Start the resident listener once. It waits for the original FnKey and
/// forwarding services to release the channel before registering.
pub(crate) fn start() {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    if let Err(error) = std::thread::Builder::new()
        .name("fn-event-owner".to_owned())
        .spawn(owner_loop)
    {
        log_line(&format!("Fn owner thread could not start: {error}"));
    }
}

fn owner_loop() {
    let mut deferred = false;
    loop {
        if crate::process_query::any_running(&["FnKey.exe"]) {
            if !deferred {
                log_line("Fn ownership deferred: FnKey.exe is running");
            }
            deferred = true;
            std::thread::sleep(Duration::from_secs(5));
            continue;
        }
        if deferred {
            log_line("Fn ownership resumed: FnKey.exe exited");
            deferred = false;
        }
        let mut listener = match crate::fn_diagnostics::open_listener() {
            Ok(listener) => listener,
            Err(error) => {
                log_line(&format!("Fn ownership unavailable: {error}"));
                std::thread::sleep(Duration::from_secs(5));
                continue;
            }
        };
        log_line("Fn notification channel acquired");
        loop {
            match listener.wait_next() {
                Ok(notification) => dispatch_notification(notification),
                Err(error) => {
                    log_line(&format!("Fn notification listener stopped: {error}"));
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

fn dispatch_notification(notification: FnNotification) {
    let Some(event) = notification.event() else {
        log_line(&format!(
            "unmapped Fn notification raw={:#018x}",
            notification.raw()
        ));
        return;
    };
    let view = match execute(event) {
        Ok(Some(view)) => view,
        Ok(None) => return,
        Err(error) => {
            log_line(&format!("Fn action failed event={event:?}: {error}"));
            OsdView {
                title: event_title(event).to_owned(),
                detail: format!("{}: {error}", tr("Action failed")),
                accent: ACCENT_ERROR,
                glyph: Glyph::Alert,
                level: -1.0,
                spin: false,
            }
        }
    };
    osd::show_with(
        &view.title,
        &view.detail,
        view.accent,
        view.glyph,
        view.level,
        view.spin,
    );
}

/// Text presented by the OSD for one Fn event.
struct OsdView {
    title: String,
    detail: String,
    accent: slint::Color,
    glyph: Glyph,
    /// Level shown as a five-step bar, negative to hide it.
    level: f32,
    /// Keep the glyph turning, used while anti-dust is running.
    spin: bool,
}

const ACCENT_INFO: slint::Color = slint::Color::from_rgb_u8(66, 211, 255);
const ACCENT_ERROR: slint::Color = slint::Color::from_rgb_u8(255, 91, 104);

impl OsdView {
    fn new(title: String, detail: String, glyph: Glyph) -> Self {
        Self {
            title,
            detail,
            accent: ACCENT_INFO,
            glyph,
            level: -1.0,
            spin: false,
        }
    }

    fn with_level(mut self, level: f32) -> Self {
        self.level = level;
        self
    }

    fn spinning(mut self) -> Self {
        self.spin = true;
        self
    }
}

fn on_off(enabled: bool) -> String {
    if enabled { tr("On") } else { tr("Off") }.to_owned()
}

/// Percentage of the keyboard backlight range. The transport exposes five
/// levels (0 means off) and the vendor OSD shows the same five stops.
fn backlight_text(level: u8) -> String {
    if level == 0 {
        tr("Off").to_owned()
    } else {
        format!("{} {level}/4", tr("Brightness"))
    }
}

/// State-only events: firmware already performed the change, so there is
/// nothing to execute and only the resulting condition is reported.
fn state_view(event: FnEvent) -> Option<OsdView> {
    let view = match event {
        FnEvent::FanNotice(notice) => {
            let detail = match notice {
                FanNotice::AntiDustRunning => tr("Anti-dust running"),
                FanNotice::Turbo => tr("Turbo fan"),
                FanNotice::MaxQ => tr("MaxQ fan profile"),
                FanNotice::Custom => tr("Custom fan profile"),
                FanNotice::Maximum => tr("Maximum fan"),
            }
            .to_owned();
            // Both fans are reported together: the platform runs a CPU and a
            // GPU fan, and one number would hide which one changed.
            let detail = match fan_speeds() {
                Some((cpu, gpu)) => format!("{detail} · CPU {cpu} / GPU {gpu}"),
                None => detail,
            };
            let view = OsdView::new(tr("Cooling").to_owned(), detail, Glyph::Fan);
            if notice == FanNotice::AntiDustRunning {
                view.spinning()
            } else {
                view
            }
        }
        FnEvent::SilentMode(enabled) => {
            OsdView::new(tr("Silent mode").to_owned(), on_off(enabled), Glyph::Moon)
        }
        FnEvent::MobileRadio(enabled) => {
            OsdView::new(tr("Mobile radio").to_owned(), on_off(enabled), Glyph::Waves)
        }
        FnEvent::CapsLock(enabled) => {
            OsdView::new(tr("Caps Lock").to_owned(), on_off(enabled), Glyph::Key)
        }
        FnEvent::NumLock(enabled) => {
            OsdView::new(tr("Num Lock").to_owned(), on_off(enabled), Glyph::Key)
        }
        FnEvent::ScrollLock(enabled) => {
            OsdView::new(tr("Scroll Lock").to_owned(), on_off(enabled), Glyph::Key)
        }
        FnEvent::FlexiAccess(enabled) => OsdView::new(
            tr("FlexiKey access").to_owned(),
            on_off(enabled),
            Glyph::Key,
        ),
        FnEvent::DiscreteGpuSwitch(enabled) => {
            OsdView::new(tr("Discrete GPU").to_owned(), on_off(enabled), Glyph::Bolt)
        }
        FnEvent::CameraMicrophone(enabled) => OsdView::new(
            tr("Camera and microphone").to_owned(),
            on_off(enabled),
            Glyph::Microphone,
        ),
        FnEvent::Docking(connected) => OsdView::new(
            tr("Docking").to_owned(),
            if connected {
                tr("Connected")
            } else {
                tr("Disconnected")
            }
            .to_owned(),
            Glyph::Dock,
        ),
        _ => return None,
    };
    Some(view)
}

/// Title used both for the normal OSD and the failure OSD.
fn event_title(event: FnEvent) -> &'static str {
    match event {
        FnEvent::BacklightStep(_) | FnEvent::BacklightToggle | FnEvent::BacklightCycle => {
            tr("Keyboard backlight")
        }
        FnEvent::FnLockToggle => tr("Fn Lock"),
        FnEvent::FanNotice(_) | FnEvent::NoiselessFanToggle => tr("Cooling"),
        FnEvent::PowerModeCycle => tr("Power mode"),
        FnEvent::ControlCenterToggle => tr("HIMARI"),
        FnEvent::Docking(_) => tr("Docking"),
        FnEvent::NumLock(_) => tr("Num Lock"),
        FnEvent::CapsLock(_) => tr("Caps Lock"),
        FnEvent::ScrollLock(_) => tr("Scroll Lock"),
        FnEvent::SilentMode(_) => tr("Silent mode"),
        FnEvent::MobileRadio(_) => tr("Mobile radio"),
        FnEvent::Camera(_) => tr("Camera"),
        FnEvent::CameraMicrophone(_) => tr("Camera and microphone"),
        FnEvent::TouchpadToggle { .. } => tr("Touchpad"),
        FnEvent::ComboSwap(_) => tr("Win / Fn swap"),
        FnEvent::FlexiAccess(_) => tr("FlexiKey access"),
        FnEvent::DiscreteGpuSwitch(_) => tr("Discrete GPU"),
    }
}

/// Execute one Fn action. `Ok(None)` means the event has no user-visible
/// result, so nothing is presented.
fn execute(event: FnEvent) -> Result<Option<OsdView>, String> {
    if let Some(view) = state_view(event) {
        return Ok(Some(view));
    }
    let title = event_title(event).to_owned();
    match event {
        FnEvent::BacklightStep(direction) => {
            let (detail, level) = adjust_backlight(direction)?;
            Ok(Some(
                OsdView::new(title, detail, Glyph::Keyboard).with_level(level),
            ))
        }
        FnEvent::BacklightToggle => {
            let (detail, level) = toggle_backlight()?;
            Ok(Some(
                OsdView::new(title, detail, Glyph::Keyboard).with_level(level),
            ))
        }
        FnEvent::BacklightCycle => {
            let (detail, level) = cycle_backlight()?;
            Ok(Some(
                OsdView::new(title, detail, Glyph::Keyboard).with_level(level),
            ))
        }
        FnEvent::FnLockToggle => Ok(Some(OsdView::new(title, toggle_fn_lock()?, Glyph::Lock))),
        FnEvent::NoiselessFanToggle => Ok(Some(OsdView::new(
            title,
            toggle_noiseless_fan()?,
            Glyph::Fan,
        ))),
        FnEvent::PowerModeCycle => {
            let (detail, glyph) = cycle_power_mode()?;
            Ok(Some(OsdView::new(title, detail, glyph)))
        }
        FnEvent::ControlCenterToggle => {
            toggle_control_center()?;
            Ok(Some(OsdView::new(
                title,
                tr("Toggle window").to_owned(),
                Glyph::Window,
            )))
        }
        FnEvent::Camera(enabled) => {
            let device = open_dchu()?;
            device
                .set_camera_enabled(enabled)
                .map_err(|error| error.to_string())?;
            Ok(Some(OsdView::new(title, on_off(enabled), Glyph::Camera)))
        }
        FnEvent::TouchpadToggle { .. } => Ok(Some(OsdView::new(
            title,
            toggle_touchpad()?,
            Glyph::Touchpad,
        ))),
        FnEvent::ComboSwap(_) => Ok(Some(OsdView::new(
            title,
            toggle_combo_swap()?,
            Glyph::Keyboard,
        ))),
        _ => Ok(None),
    }
}

fn adjust_backlight(direction: Adjustment) -> Result<(String, f32), String> {
    let device = open_dchu()?;
    let state = device
        .read_rgb_keyboard_state()
        .map_err(|error| error.to_string())?;
    let target = match direction {
        Adjustment::Down => state.brightness.saturating_sub(1),
        Adjustment::Up => state.brightness.saturating_add(1).min(4),
    };
    device
        .set_rgb_keyboard_brightness(target)
        .map_err(|error| error.to_string())?;
    Ok((backlight_text(target), f32::from(target)))
}

fn toggle_backlight() -> Result<(String, f32), String> {
    let device = open_dchu()?;
    let state = device
        .read_rgb_keyboard_state()
        .map_err(|error| error.to_string())?;
    let target = if state.enabled {
        0
    } else {
        state.brightness.max(1)
    };
    device
        .set_rgb_keyboard_brightness(target)
        .map_err(|error| error.to_string())?;
    Ok((backlight_text(target), f32::from(target)))
}

fn cycle_backlight() -> Result<(String, f32), String> {
    let device = open_dchu()?;
    let state = device
        .read_rgb_keyboard_state()
        .map_err(|error| error.to_string())?;
    let target = if !state.enabled || state.brightness >= 4 {
        0
    } else {
        state.brightness + 1
    };
    device
        .set_rgb_keyboard_brightness(target)
        .map_err(|error| error.to_string())?;
    Ok((backlight_text(target), f32::from(target)))
}

fn toggle_fn_lock() -> Result<String, String> {
    let device = open_dchu()?;
    let state = device.read_quick_toggles();
    let current = state
        .fn_lock
        .ok_or_else(|| tr("Fn Lock state is unavailable").to_owned())?;
    device
        .set_quick_toggle(himari_protocol::QuickToggle::FnLock, !current)
        .map_err(|error| error.to_string())?;
    Ok(if current {
        tr("Off").to_owned()
    } else {
        tr("On").to_owned()
    })
}

fn toggle_noiseless_fan() -> Result<String, String> {
    let device = open_dchu()?;
    let state = device.read_platform_state();
    let current = state
        .slow_fan
        .ok_or_else(|| tr("Noiseless fan state is unavailable").to_owned())?;
    device
        .set_slow_fan(!current)
        .map_err(|error| error.to_string())?;
    Ok(if current {
        tr("Off").to_owned()
    } else {
        tr("On").to_owned()
    })
}

/// One stop on the Fn power key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PowerTier {
    /// A platform power profile, applied through the DCHU power packet.
    Profile(PowerMode),
    /// Quiet profile with the discrete GPU powered down. The vendor console
    /// calls this out as a separate tier because it is the only one that
    /// changes what the discrete GPU consumes.
    QuietWithoutDiscreteGpu,
}

impl PowerTier {
    fn label(self) -> &'static str {
        match self {
            Self::Profile(mode) => crate::localization::localized_power_label(mode),
            Self::QuietWithoutDiscreteGpu => tr("Quiet + discrete GPU off"),
        }
    }

    const fn profile(self) -> PowerMode {
        match self {
            Self::Profile(mode) => mode,
            Self::QuietWithoutDiscreteGpu => PowerMode::Quiet,
        }
    }

    /// Glyph that identifies this tier on the overlay.
    const fn glyph(self) -> Glyph {
        match self {
            Self::QuietWithoutDiscreteGpu => Glyph::Moon,
            Self::Profile(mode) => match mode {
                PowerMode::Performance => Glyph::Bolt,
                PowerMode::Entertainment => Glyph::Film,
                PowerMode::PowerSaving => Glyph::Leaf,
                PowerMode::Quiet | PowerMode::QuietPlus => Glyph::Moon,
                _ => Glyph::Gauge,
            },
        }
    }

    const fn powers_down_discrete_gpu(self) -> bool {
        matches!(self, Self::QuietWithoutDiscreteGpu)
    }
}

/// Stops the Fn key cycles through, gated by what the platform reports.
fn power_tiers(capabilities: &himari_protocol::PlatformCapabilities) -> Vec<PowerTier> {
    let mut tiers = Vec::new();
    for mode in [
        PowerMode::Performance,
        PowerMode::Entertainment,
        PowerMode::PowerSaving,
        PowerMode::Quiet,
    ] {
        if capabilities.supports_power_mode(mode) {
            tiers.push(PowerTier::Profile(mode));
        }
    }
    // The GPU tier needs no profile support: it keeps the quiet profile when
    // the platform offers one and otherwise stays on whatever profile is
    // active, but it always powers the discrete GPU down.
    if capabilities.nvidia_power_off && !tiers.iter().any(|tier| tier.powers_down_discrete_gpu()) {
        tiers.push(PowerTier::QuietWithoutDiscreteGpu);
    }
    tiers
}

fn cycle_power_mode() -> Result<(String, Glyph), String> {
    let device = open_dchu()?;
    let capabilities = device
        .read_capabilities()
        .map_err(|error| error.to_string())?;
    let tiers = power_tiers(&capabilities);
    if tiers.is_empty() {
        return Err(tr("No power profiles are supported").to_owned());
    }
    let current_mode = device
        .read_setting_byte(1, 1)
        .and_then(PowerMode::from_u8)
        .ok_or_else(|| tr("Power mode is unavailable").to_owned())?;
    let discrete_gpu_off = crate::gpu_power::is_disabled();
    let current = tiers
        .iter()
        .position(|tier| {
            tier.powers_down_discrete_gpu() == discrete_gpu_off
                && (!tier.powers_down_discrete_gpu() || tier.profile() == current_mode)
                && (tier.powers_down_discrete_gpu() || tier.profile() == current_mode)
        })
        .unwrap_or_else(|| {
            tiers
                .iter()
                .position(|tier| !tier.powers_down_discrete_gpu() && tier.profile() == current_mode)
                .unwrap_or(0)
        });
    let next = tiers[(current + 1) % tiers.len()];

    // Leave the previous tier cleanly: bring the discrete GPU back before the
    // next profile is applied when the cycle moves away from the quiet tier.
    if !next.powers_down_discrete_gpu() && discrete_gpu_off {
        crate::gpu_power::request(true)?;
    }
    device
        .set_power_mode(next.profile())
        .map_err(|error| error.to_string())?;
    crate::runtime_state::persist_power_mode(next.profile());

    let mut detail = next.label().to_owned();
    if next.powers_down_discrete_gpu() {
        crate::gpu_power::request(false)?;
        detail = format!("{detail} · {}", tr("discrete GPU powered off"));
    } else if discrete_gpu_off {
        detail = format!("{detail} · {}", tr("discrete GPU restored"));
    }
    let glyph = next.glyph();
    Ok((detail, glyph))
}

/// Current CPU and GPU fan speeds, from the thermal mailbox.
///
/// The Fn fan notice reports both fans because the platform runs two and a
/// single figure would hide which one moved.
fn fan_speeds() -> Option<(u32, u32)> {
    let sample = open_dchu().ok()?.read_thermal().ok()?;
    if !sample.has_plausible_temperature() {
        return None;
    }
    Some((sample.cpu.rpm, sample.gpu.rpm))
}

/// Flip the Win/Fn key swap and report the state it landed in.
///
/// The vendor handler for the same notification reads the current swap state
/// and shows the opposite, so the action and the feedback agree.
fn toggle_combo_swap() -> Result<String, String> {
    let device = open_dchu()?;
    let current = device
        .read_quick_toggles()
        .swap_win_fn
        .ok_or_else(|| tr("Keys are locked").to_owned())?;
    device
        .set_quick_toggle(himari_protocol::QuickToggle::SwapWinFn, !current)
        .map_err(|error| error.to_string())?;
    Ok(if current {
        tr("Off").to_owned()
    } else {
        tr("On").to_owned()
    })
}

fn toggle_touchpad() -> Result<String, String> {
    let device = open_dchu()?;
    let current = crate::system_controls::touchpad_enabled()?;
    crate::system_controls::set_touchpad_enabled(&device, !current)?;
    Ok(if current {
        tr("Off").to_owned()
    } else {
        tr("On").to_owned()
    })
}

fn toggle_control_center() -> Result<(), String> {
    let weak = MAIN_WINDOW
        .get()
        .and_then(|slot| slot.lock().ok())
        .and_then(|slot| slot.clone())
        .ok_or_else(|| tr("Main window is unavailable").to_owned())?;
    slint::invoke_from_event_loop(move || {
        if let Some(window) = weak.upgrade() {
            if window.window().is_visible() {
                window.hide().ok();
            } else {
                window.show().ok();
            }
        }
    })
    .map_err(|error| error.to_string())
}

fn open_dchu() -> Result<Dchu, String> {
    Dchu::open().map_err(|error| error.to_string())
}

fn log_line(message: &str) {
    use std::io::Write as _;

    crate::diagnostics::log_action(message);
    let root = crate::product::product_data_dir();
    if std::fs::create_dir_all(&root).is_err() {
        return;
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("fn-owner.log"))
    {
        let _ = writeln!(file, "{message}");
    }
}

/// Standalone OSD visual smoke. It opens both windows only in the smoke
/// process and exits after the OSD auto-hide interval.
pub(crate) fn osd_smoke() -> Result<(), Box<dyn std::error::Error>> {
    let ui = MainWindow::new()?;
    crate::osd::install(ui.as_weak());
    crate::osd_layer::start();
    crate::ui_callbacks::apply_persisted_theme(&ui);
    install_ui(ui.as_weak());
    dispatch_notification(FnNotification::from_payload(
        &i64::from(0xcau8).to_le_bytes(),
    )?);
    slint::Timer::single_shot(Duration::from_millis(2_600), || {
        let _ = slint::quit_event_loop();
    });
    slint::run_event_loop()?;
    Ok(())
}

/// Dispatch a synthetic event through the same path as a hardware Fn
/// notification. This validates OSD/action wiring without claiming a physical
/// Fn capture.
pub(crate) fn event_smoke(code: u8) -> Result<(), Box<dyn std::error::Error>> {
    let ui = MainWindow::new()?;
    crate::osd::install(ui.as_weak());
    crate::osd_layer::start();
    crate::ui_callbacks::apply_persisted_theme(&ui);
    install_ui(ui.as_weak());
    dispatch_notification(FnNotification::from_payload(
        &i64::from(code).to_le_bytes(),
    )?);
    slint::Timer::single_shot(Duration::from_millis(2_600), || {
        let _ = slint::quit_event_loop();
    });
    slint::run_event_loop()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every decoded event must have a title, otherwise the failure OSD would
    /// present an empty first line.
    #[test]
    fn every_decoded_event_has_a_title() {
        let events = [
            FnEvent::BacklightStep(Adjustment::Down),
            FnEvent::BacklightStep(Adjustment::Up),
            FnEvent::BacklightToggle,
            FnEvent::BacklightCycle,
            FnEvent::FnLockToggle,
            FnEvent::FanNotice(FanNotice::AntiDustRunning),
            FnEvent::FanNotice(FanNotice::Turbo),
            FnEvent::FanNotice(FanNotice::MaxQ),
            FnEvent::FanNotice(FanNotice::Custom),
            FnEvent::FanNotice(FanNotice::Maximum),
            FnEvent::NoiselessFanToggle,
            FnEvent::PowerModeCycle,
            FnEvent::ControlCenterToggle,
            FnEvent::Docking(true),
            FnEvent::Docking(false),
            FnEvent::NumLock(true),
            FnEvent::NumLock(false),
            FnEvent::CapsLock(true),
            FnEvent::CapsLock(false),
            FnEvent::ScrollLock(true),
            FnEvent::ScrollLock(false),
            FnEvent::SilentMode(true),
            FnEvent::SilentMode(false),
            FnEvent::MobileRadio(true),
            FnEvent::MobileRadio(false),
            FnEvent::Camera(true),
            FnEvent::Camera(false),
            FnEvent::CameraMicrophone(true),
            FnEvent::CameraMicrophone(false),
            FnEvent::TouchpadToggle {
                legacy_enabled: true,
            },
            FnEvent::TouchpadToggle {
                legacy_enabled: false,
            },
        ];
        for event in events {
            assert!(!event_title(event).is_empty(), "missing title: {event:?}");
        }
    }

    /// Firmware-driven notifications must paint without touching hardware, and
    /// they must carry an explicit condition rather than a generic toggle.
    #[test]
    fn state_only_events_report_a_condition() {
        for event in [
            FnEvent::CapsLock(true),
            FnEvent::NumLock(false),
            FnEvent::ScrollLock(true),
            FnEvent::SilentMode(false),
            FnEvent::MobileRadio(true),
            FnEvent::Docking(true),
            FnEvent::Docking(false),
            FnEvent::FanNotice(FanNotice::Turbo),
        ] {
            let view = state_view(event).expect("state event must present");
            assert!(!view.detail.is_empty(), "missing detail: {event:?}");
            assert!(!view.title.is_empty(), "missing title: {event:?}");
        }
    }

    /// Action events are executed instead of being rendered from state.
    #[test]
    fn action_events_are_not_state_only() {
        for event in [
            FnEvent::BacklightToggle,
            FnEvent::BacklightCycle,
            FnEvent::FnLockToggle,
            FnEvent::PowerModeCycle,
            FnEvent::TouchpadToggle {
                legacy_enabled: true,
            },
        ] {
            assert!(
                state_view(event).is_none(),
                "unexpected state view: {event:?}"
            );
        }
    }

    #[test]
    fn backlight_text_covers_off_and_every_level() {
        assert_eq!(backlight_text(0), tr("Off"));
        for level in 1..=4 {
            assert_eq!(
                backlight_text(level),
                format!("{} {level}/4", tr("Brightness"))
            );
        }
    }
}

#[cfg(test)]
mod tier_tests {
    use super::*;
    use himari_protocol::PlatformCapabilities;

    /// The GPU tier must appear whenever the platform can power the discrete
    /// GPU down, because it is the only tier that changes what the discrete GPU
    /// consumes.
    #[test]
    fn gpu_tier_is_offered_when_the_platform_supports_it() {
        let caps = PlatformCapabilities {
            performance: true,
            nvidia_power_off: true,
            ..Default::default()
        };
        let tiers = power_tiers(&caps);
        assert!(tiers.iter().any(|tier| tier.powers_down_discrete_gpu()));
    }

    /// Without the capability the cycle must not pretend the tier exists.
    #[test]
    fn gpu_tier_is_absent_without_the_capability() {
        let caps = PlatformCapabilities {
            performance: true,
            ..Default::default()
        };
        let tiers = power_tiers(&caps);
        assert!(!tiers.iter().any(|tier| tier.powers_down_discrete_gpu()));
        assert!(tiers.contains(&PowerTier::Profile(PowerMode::Performance)));
    }

    /// Quiet is only listed when the platform accepts it, but the GPU tier is
    /// listed independently of that flag.
    #[test]
    fn quiet_profile_and_gpu_tier_are_gated_separately() {
        let caps = PlatformCapabilities {
            power_saving: true,
            nvidia_power_off: true,
            ..Default::default()
        };
        let tiers = power_tiers(&caps);
        assert!(!tiers.contains(&PowerTier::Profile(PowerMode::Quiet)));
        assert!(tiers.iter().any(|tier| tier.powers_down_discrete_gpu()));
        assert_eq!(tiers.len(), 2);
    }
}
