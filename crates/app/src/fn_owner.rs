//! Resident Fn-event ownership, action dispatch and OSD feedback.

use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;

use ram_control_center_protocol::{
    Adjustment, Dchu, FanNotice, FnEvent, FnNotification, PowerMode,
};
use slint::{ComponentHandle, Weak};

use crate::{MainWindow, localization::tr, osd};

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
            }
        }
    };
    osd::show(&view.title, &view.detail, view.accent);
}

/// Text presented by the OSD for one Fn event.
struct OsdView {
    title: String,
    detail: String,
    accent: slint::Color,
}

const ACCENT_INFO: slint::Color = slint::Color::from_rgb_u8(66, 211, 255);
const ACCENT_ERROR: slint::Color = slint::Color::from_rgb_u8(255, 91, 104);

impl OsdView {
    fn new(title: String, detail: String) -> Self {
        Self {
            title,
            detail,
            accent: ACCENT_INFO,
        }
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
        FnEvent::FanNotice(notice) => OsdView::new(
            tr("Cooling").to_owned(),
            match notice {
                FanNotice::AntiDustRunning => tr("Anti-dust running"),
                FanNotice::Turbo => tr("Turbo fan"),
                FanNotice::MaxQ => tr("MaxQ fan profile"),
                FanNotice::Custom => tr("Custom fan profile"),
                FanNotice::Maximum => tr("Maximum fan"),
            }
            .to_owned(),
        ),
        FnEvent::SilentMode(enabled) => OsdView::new(tr("Silent mode").to_owned(), on_off(enabled)),
        FnEvent::MobileRadio(enabled) => {
            OsdView::new(tr("Mobile radio").to_owned(), on_off(enabled))
        }
        FnEvent::CapsLock(enabled) => OsdView::new(tr("Caps Lock").to_owned(), on_off(enabled)),
        FnEvent::NumLock(enabled) => OsdView::new(tr("Num Lock").to_owned(), on_off(enabled)),
        FnEvent::ScrollLock(enabled) => OsdView::new(tr("Scroll Lock").to_owned(), on_off(enabled)),
        FnEvent::Docking(connected) => OsdView::new(
            tr("Docking").to_owned(),
            if connected {
                tr("Connected")
            } else {
                tr("Disconnected")
            }
            .to_owned(),
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
        FnEvent::ControlCenterToggle => tr("RAM CONTROL CENTER"),
        FnEvent::Docking(_) => tr("Docking"),
        FnEvent::NumLock(_) => tr("Num Lock"),
        FnEvent::CapsLock(_) => tr("Caps Lock"),
        FnEvent::ScrollLock(_) => tr("Scroll Lock"),
        FnEvent::SilentMode(_) => tr("Silent mode"),
        FnEvent::MobileRadio(_) => tr("Mobile radio"),
        FnEvent::Camera(_) => tr("Camera"),
        FnEvent::CameraMicrophone(_) => tr("Camera and microphone"),
        FnEvent::TouchpadToggle { .. } => tr("Touchpad"),
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
            Ok(Some(OsdView::new(title, adjust_backlight(direction)?)))
        }
        FnEvent::BacklightToggle => Ok(Some(OsdView::new(title, toggle_backlight()?))),
        FnEvent::BacklightCycle => Ok(Some(OsdView::new(title, cycle_backlight()?))),
        FnEvent::FnLockToggle => Ok(Some(OsdView::new(title, toggle_fn_lock()?))),
        FnEvent::NoiselessFanToggle => Ok(Some(OsdView::new(title, toggle_noiseless_fan()?))),
        FnEvent::PowerModeCycle => Ok(Some(OsdView::new(title, cycle_power_mode()?))),
        FnEvent::ControlCenterToggle => {
            toggle_control_center()?;
            Ok(None)
        }
        FnEvent::Camera(enabled) => {
            let device = open_dchu()?;
            device
                .set_camera_enabled(enabled)
                .map_err(|error| error.to_string())?;
            Ok(Some(OsdView::new(title, on_off(enabled))))
        }
        FnEvent::TouchpadToggle { .. } => Ok(Some(OsdView::new(title, toggle_touchpad()?))),
        _ => Ok(None),
    }
}

fn adjust_backlight(direction: Adjustment) -> Result<String, String> {
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
    Ok(backlight_text(target))
}

fn toggle_backlight() -> Result<String, String> {
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
    Ok(backlight_text(target))
}

fn cycle_backlight() -> Result<String, String> {
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
    Ok(backlight_text(target))
}

fn toggle_fn_lock() -> Result<String, String> {
    let device = open_dchu()?;
    let state = device.read_quick_toggles();
    let current = state
        .fn_lock
        .ok_or_else(|| tr("Fn Lock state is unavailable").to_owned())?;
    device
        .set_quick_toggle(ram_control_center_protocol::QuickToggle::FnLock, !current)
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

fn cycle_power_mode() -> Result<String, String> {
    let device = open_dchu()?;
    let current = device
        .read_setting_byte(1, 1)
        .and_then(PowerMode::from_u8)
        .ok_or_else(|| tr("Power mode is unavailable").to_owned())?;
    let capabilities = device
        .read_capabilities()
        .map_err(|error| error.to_string())?;
    let modes = [
        PowerMode::Performance,
        PowerMode::Entertainment,
        PowerMode::PowerSaving,
        PowerMode::Quiet,
    ];
    let supported = modes
        .into_iter()
        .filter(|mode| capabilities.supports_power_mode(*mode))
        .collect::<Vec<_>>();
    if supported.is_empty() {
        return Err(tr("No power profiles are supported").to_owned());
    }
    let index = supported
        .iter()
        .position(|mode| *mode == current)
        .map_or(0, |index| (index + 1) % supported.len());
    let target = supported[index];
    device
        .set_power_mode(target)
        .map_err(|error| error.to_string())?;
    crate::runtime_state::persist_power_mode(target);
    Ok(crate::localization::localized_power_label(target).to_owned())
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
