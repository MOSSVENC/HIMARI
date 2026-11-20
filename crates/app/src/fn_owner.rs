//! Resident Fn-event ownership, action dispatch and OSD feedback.

use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;

use ram_control_center_protocol::{Adjustment, Dchu, FnEvent, FnNotification, PowerMode};
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
    let (title, detail, accent) = presentation(event);
    osd::show(&title, &detail, accent);
    match execute(event) {
        Ok(Some(result)) => osd::show(&title, &result, accent),
        Ok(None) => {}
        Err(error) => {
            let detail = format!("{}: {error}", tr("Action failed"));
            log_line(&format!("Fn action failed event={event:?}: {error}"));
            osd::show(&title, &detail, slint::Color::from_rgb_u8(255, 91, 104));
        }
    }
}

fn presentation(event: FnEvent) -> (String, String, slint::Color) {
    let accent = slint::Color::from_rgb_u8(66, 211, 255);
    let (title, detail) = match event {
        FnEvent::BacklightStep(Adjustment::Down) => {
            (tr("Keyboard backlight"), tr("Brightness down"))
        }
        FnEvent::BacklightStep(Adjustment::Up) => (tr("Keyboard backlight"), tr("Brightness up")),
        FnEvent::BacklightToggle => (tr("Keyboard backlight"), tr("Toggle")),
        FnEvent::BacklightCycle => (tr("Keyboard backlight"), tr("Cycle brightness")),
        FnEvent::FnLockToggle => (tr("Fn Lock"), tr("Toggle")),
        FnEvent::FanNotice(notice) => (
            tr("Cooling"),
            match notice {
                ram_control_center_protocol::FanNotice::AntiDustRunning => tr("Anti-dust running"),
                ram_control_center_protocol::FanNotice::Turbo => tr("Turbo fan"),
                ram_control_center_protocol::FanNotice::MaxQ => tr("MaxQ fan profile"),
                ram_control_center_protocol::FanNotice::Custom => tr("Custom fan profile"),
                ram_control_center_protocol::FanNotice::Maximum => tr("Maximum fan"),
            },
        ),
        FnEvent::NoiselessFanToggle => (tr("Cooling"), tr("Noiseless fan toggle")),
        FnEvent::PowerModeCycle => (tr("Power mode"), tr("Switch profile")),
        FnEvent::ControlCenterToggle => (tr("RAM CONTROL CENTER"), tr("Toggle window")),
        FnEvent::Docking(connected) => (
            tr("Docking"),
            if connected {
                tr("Connected")
            } else {
                tr("Disconnected")
            },
        ),
        FnEvent::NumLock(enabled) => (tr("Num Lock"), if enabled { tr("On") } else { tr("Off") }),
        FnEvent::CapsLock(enabled) => (tr("Caps Lock"), if enabled { tr("On") } else { tr("Off") }),
        FnEvent::ScrollLock(enabled) => (
            tr("Scroll Lock"),
            if enabled { tr("On") } else { tr("Off") },
        ),
        FnEvent::SilentMode(enabled) => (
            tr("Silent mode"),
            if enabled { tr("On") } else { tr("Off") },
        ),
        FnEvent::MobileRadio(enabled) => (
            tr("Mobile radio"),
            if enabled { tr("On") } else { tr("Off") },
        ),
        FnEvent::Camera(enabled) => (tr("Camera"), if enabled { tr("On") } else { tr("Off") }),
        FnEvent::CameraMicrophone(enabled) => (
            tr("Camera and microphone"),
            if enabled { tr("On") } else { tr("Off") },
        ),
        FnEvent::TouchpadToggle { .. } => (tr("Touchpad"), tr("Toggle")),
    };
    (title.to_owned(), detail.to_owned(), accent)
}

fn execute(event: FnEvent) -> Result<Option<String>, String> {
    match event {
        FnEvent::BacklightStep(direction) => adjust_backlight(direction).map(Some),
        FnEvent::BacklightToggle => toggle_backlight().map(Some),
        FnEvent::BacklightCycle => cycle_backlight().map(Some),
        FnEvent::FnLockToggle => toggle_fn_lock().map(Some),
        FnEvent::NoiselessFanToggle => toggle_noiseless_fan().map(Some),
        FnEvent::PowerModeCycle => cycle_power_mode().map(Some),
        FnEvent::ControlCenterToggle => toggle_control_center().map(|()| None),
        FnEvent::Camera(enabled) => {
            let device = open_dchu()?;
            device
                .set_camera_enabled(enabled)
                .map_err(|error| error.to_string())?;
            Ok(Some(if enabled {
                tr("Camera on").to_owned()
            } else {
                tr("Camera off").to_owned()
            }))
        }
        FnEvent::TouchpadToggle { .. } => toggle_touchpad().map(Some),
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
    Ok(format!("{} {target}/4", tr("Brightness")))
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
    Ok(if target == 0 {
        tr("Off").to_owned()
    } else {
        format!("{} {target}/4", tr("Brightness"))
    })
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
    Ok(if target == 0 {
        tr("Off").to_owned()
    } else {
        format!("{} {target}/4", tr("Brightness"))
    })
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
    let osd = crate::OsdWindow::new()?;
    osd::install(osd.as_weak(), ui.as_weak());
    crate::ui_callbacks::apply_persisted_theme(&ui);
    install_ui(ui.as_weak());
    dispatch_notification(FnNotification::from_payload(
        &i64::from(0xcau8).to_le_bytes(),
    )?);
    slint::Timer::single_shot(Duration::from_millis(2_600), || {
        let _ = slint::quit_event_loop();
    });
    slint::run_event_loop()?;
    drop(osd);
    Ok(())
}

/// Dispatch a synthetic event through the same path as a hardware Fn
/// notification. This validates OSD/action wiring without claiming a physical
/// Fn capture.
pub(crate) fn event_smoke(code: u8) -> Result<(), Box<dyn std::error::Error>> {
    let ui = MainWindow::new()?;
    let osd = crate::OsdWindow::new()?;
    osd::install(osd.as_weak(), ui.as_weak());
    crate::ui_callbacks::apply_persisted_theme(&ui);
    install_ui(ui.as_weak());
    dispatch_notification(FnNotification::from_payload(
        &i64::from(code).to_le_bytes(),
    )?);
    slint::Timer::single_shot(Duration::from_millis(2_600), || {
        let _ = slint::quit_event_loop();
    });
    slint::run_event_loop()?;
    drop(osd);
    Ok(())
}
