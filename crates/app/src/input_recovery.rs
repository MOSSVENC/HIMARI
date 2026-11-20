//! Early reconciliation for the OEM keyboard class filter.
//!
//! The signed filter remains installed and loaded. RAM CONTROL CENTER does
//! not use its macro interception path, so the guard keeps the filter in
//! pass-through mode and excludes USB keyboards from it. The boot service and
//! the interactive process both invoke the guard; it also repairs the state if
//! the vendor stack rewrites the filter flags later in the boot sequence.
//!
//! The filter device is machine-wide and shared with the vendor stack, and it
//! exposes no read-back: every correction is a blind write. A correction is
//! therefore also a possible source of drift for the vendor's own state
//! machine, so the guard writes eagerly only while the boot sequence can still
//! rewrite the flags and backs off afterwards.

use std::time::Duration;
#[cfg(windows)]
use std::time::Instant;

/// Cadence while the boot sequence can still rewrite the filter flags.
const BOOT_RECONCILE_INTERVAL: Duration = Duration::from_secs(5);

/// Steady-state cadence. Twelve times fewer shared-device writes per minute.
const STEADY_RECONCILE_INTERVAL: Duration = Duration::from_secs(60);

/// How long after guard start the boot cadence stays in effect.
const BOOT_WATCH_WINDOW: Duration = Duration::from_secs(180);

/// Reconcile cadence for a guard that has been running for `elapsed`.
#[cfg_attr(not(windows), allow(dead_code))]
fn reconcile_interval(elapsed: Duration) -> Duration {
    if elapsed < BOOT_WATCH_WINDOW {
        BOOT_RECONCILE_INTERVAL
    } else {
        STEADY_RECONCILE_INTERVAL
    }
}

/// Start the process-lifetime filter guard once.
pub(crate) fn start_filter_guard() {
    #[cfg(windows)]
    {
        use std::sync::OnceLock;

        static STARTED: OnceLock<()> = OnceLock::new();
        if STARTED.set(()).is_err() {
            return;
        }
        let result = std::thread::Builder::new()
            .name("input-filter-guard".to_owned())
            .spawn(guard_loop);
        if let Err(error) = result {
            crate::diagnostics::log_action(&format!("input filter guard could not start: {error}"));
        }
    }
}

#[cfg(windows)]
fn guard_loop() {
    let started = Instant::now();
    let mut last_error: Option<String> = None;
    let mut applied = false;
    let mut yielded = false;
    let mut boot_window_open = true;
    loop {
        let elapsed = started.elapsed();
        let interval = reconcile_interval(elapsed);
        if elapsed >= BOOT_WATCH_WINDOW && boot_window_open {
            boot_window_open = false;
            log_guard("input filter guard settled to the steady-state cadence");
        }
        if original_control_center_running() {
            if !yielded {
                log_guard("original Control Center is running; input filter ownership yielded");
            }
            yielded = true;
            applied = false;
            last_error = None;
            std::thread::sleep(interval);
            continue;
        }
        if yielded {
            log_guard("original Control Center exited; input filter guard resumed");
            yielded = false;
        }
        match reconcile_once() {
            Ok(()) => {
                if !applied || last_error.is_some() {
                    log_guard("keyboard interception disabled; USB keyboard bypass active");
                }
                applied = true;
                last_error = None;
            }
            Err(error) => {
                if last_error.as_deref() != Some(error.as_str()) {
                    log_guard(&error);
                }
                last_error = Some(error);
            }
        }
        std::thread::sleep(interval);
    }
}

#[cfg(windows)]
fn original_control_center_running() -> bool {
    crate::process_query::any_running(&[
        "CC40.exe",
        "ControlCenter30.exe",
        "COUWP.exe",
        "ControlCenter.exe",
    ])
}

#[cfg(windows)]
fn log_guard(message: &str) {
    use std::io::Write as _;

    crate::diagnostics::log_action(&format!("input filter guard: {message}"));
    let root = crate::product::product_data_dir();
    if std::fs::create_dir_all(&root).is_err() {
        return;
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("input-guard.log"))
    {
        let _ = writeln!(file, "{message}");
    }
}

#[cfg(windows)]
fn reconcile_once() -> Result<(), String> {
    use ram_control_center_protocol::KeyboardFilter;

    let filter = KeyboardFilter::open()
        .map_err(|error| format!("could not open the keyboard filter device: {error}"))?;
    let usb = filter.set_usb_enabled(false);
    let interception = filter.set_enabled(false);
    match (usb, interception) {
        (_, Err(error)) => Err(format!("could not disable keyboard interception: {error}")),
        (Err(error), Ok(())) => Err(format!(
            "keyboard interception is disabled, but USB bypass is unavailable: {error}"
        )),
        (Ok(()), Ok(())) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconcile_cadence_is_eager_during_boot_then_backs_off() {
        assert_eq!(reconcile_interval(Duration::ZERO), BOOT_RECONCILE_INTERVAL);
        assert_eq!(
            reconcile_interval(BOOT_WATCH_WINDOW - Duration::from_millis(1)),
            BOOT_RECONCILE_INTERVAL
        );
        assert_eq!(
            reconcile_interval(BOOT_WATCH_WINDOW),
            STEADY_RECONCILE_INTERVAL
        );
        assert_eq!(
            reconcile_interval(BOOT_WATCH_WINDOW + Duration::from_secs(3_600)),
            STEADY_RECONCILE_INTERVAL
        );
        assert!(STEADY_RECONCILE_INTERVAL.as_secs() >= 12 * BOOT_RECONCILE_INTERVAL.as_secs());
    }
}
