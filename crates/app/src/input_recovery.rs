//! Early reconciliation for the OEM keyboard class filter.
//!
//! The signed filter remains installed and loaded. RAM CONTROL CENTER does
//! not use its macro interception path, so the guard keeps the filter in
//! pass-through mode and excludes USB keyboards from it. The boot service and
//! the interactive process both invoke the guard; it also repairs the state if
//! the vendor stack rewrites the filter flags later in the boot sequence.

#[cfg(windows)]
use std::time::Duration;

#[cfg(windows)]
const RECONCILE_INTERVAL: Duration = Duration::from_secs(5);

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
    let mut last_error: Option<String> = None;
    let mut applied = false;
    loop {
        match reconcile_once() {
            Ok(()) => {
                if !applied || last_error.is_some() {
                    crate::diagnostics::log_action(
                        "input filter guard: keyboard interception disabled; USB keyboard bypass active",
                    );
                }
                applied = true;
                last_error = None;
            }
            Err(error) => {
                if last_error.as_deref() != Some(error.as_str()) {
                    crate::diagnostics::log_action(&format!("input filter guard: {error}"));
                }
                last_error = Some(error);
            }
        }
        std::thread::sleep(RECONCILE_INTERVAL);
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
