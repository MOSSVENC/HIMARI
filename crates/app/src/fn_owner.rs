//! Resident Fn-event ownership, action dispatch and OSD feedback.

#![forbid(unsafe_code)]
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;

use himari_protocol::FnNotification;
use slint::{ComponentHandle, Weak};

use crate::fn_actions::{execute, log_line};
use crate::fn_intent::{OsdView, event_title};
use crate::localization::tr;
use crate::osd_glyph::Glyph;
use crate::{MainWindow, osd};

static MAIN_WINDOW: OnceLock<Mutex<Option<Weak<MainWindow>>>> = OnceLock::new();

/// Current main window, for actions that only need to show or hide it.
pub(crate) fn main_window() -> Option<Weak<MainWindow>> {
    MAIN_WINDOW
        .get()
        .and_then(|slot| slot.lock().ok())
        .and_then(|slot| slot.clone())
}

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
            match listener.wait_next_timeout(Duration::from_secs(5)) {
                Ok(Some(notification)) => dispatch_notification(notification),
                Ok(None) => {
                    // The vendor listener can return while we own the channel
                    // (a diagnostic, an update or a login-time restart). Give it
                    // the channel back instead of holding it forever: the next
                    // pass through the outer loop defers while FnKey runs.
                    if crate::process_query::any_running(&["FnKey.exe"]) {
                        log_line("Fn ownership released: FnKey.exe returned");
                        break;
                    }
                }
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
                accent: crate::fn_intent::ACCENT_ERROR,
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
