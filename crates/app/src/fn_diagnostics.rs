//! Explicit diagnostics for the vendor notification channel.

#[cfg(windows)]
pub(crate) fn spawn_trace() {
    std::thread::spawn(move || {
        use crate::diagnostics::log_action;

        let mut listener = match open_listener() {
            Ok(listener) => {
                log_action("ACPI Fn notification subscription registered");
                listener
            }
            Err(error) => {
                log_action(&format!(
                    "ACPI Fn notification listener unavailable: {error}"
                ));
                return;
            }
        };
        loop {
            match listener.wait_next() {
                Ok(value) => log_action(&describe(value)),
                Err(error) => {
                    log_action(&format!("ACPI Fn notification listener stopped: {error}"));
                    return;
                }
            }
        }
    });
}

fn require_direct_mode(flag: Option<u8>) -> ram_control_center_protocol::Result<()> {
    match flag {
        Some(0) => Ok(()),
        Some(1) => Err(ownership_error("DCHUService advertises Arg001 forwarding")),
        value => Err(ownership_error(format!(
            "Arg001 forwarding status is unknown: {value:?}"
        ))),
    }
}

fn ownership_error(detail: impl Into<String>) -> ram_control_center_protocol::DchuError {
    ram_control_center_protocol::DchuError::ControlFailed {
        operation: "coordinate Fn notification ownership",
        detail: detail.into(),
    }
}

#[cfg(windows)]
fn require_services_stopped() -> ram_control_center_protocol::Result<()> {
    use windows_service::service::{ServiceAccess, ServiceState};
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|error| ownership_error(format!("query service manager: {error}")))?;
    for name in ["CCDCHUService", "DCHUService"] {
        let service = match manager.open_service(name, ServiceAccess::QUERY_STATUS) {
            Ok(service) => service,
            Err(windows_service::Error::Winapi(error)) if error.raw_os_error() == Some(1060) => {
                continue;
            }
            Err(error) => return Err(ownership_error(format!("query {name}: {error}"))),
        };
        let state = service
            .query_status()
            .map_err(|error| ownership_error(format!("query {name} status: {error}")))?
            .current_state;
        if state != ServiceState::Stopped {
            return Err(ownership_error(format!("{name} is {state:?}")));
        }
    }
    Ok(())
}

/// The diagnostic caller also coordinates the resident FnKey process. The
/// forwarding flag and service status are checked before driver registration.
pub(crate) fn open_listener()
-> ram_control_center_protocol::Result<ram_control_center_protocol::AcpiNotificationListener> {
    use ram_control_center_protocol::{AcpiNotificationListener, Dchu};

    let device = Dchu::open()?;
    let flag = device.read_setting_byte(1, 12);
    #[cfg(windows)]
    require_services_stopped()?;
    require_direct_mode(flag)?;
    AcpiNotificationListener::open(device.path())
}

fn describe(value: ram_control_center_protocol::FnNotification) -> String {
    format!(
        "ACPI Fn notification: raw={:#018x} message={:?} event={:?}",
        value.raw(),
        value.message(),
        value.event()
    )
}

/// Exercise registration, repeated bounded waits, and handle destruction.
/// Channel ownership must be coordinated before starting this diagnostic.
pub(crate) fn smoke() -> i32 {
    use std::time::{Duration, Instant};

    let result = (|| {
        let mut listener = open_listener()?;
        println!("Fn notifications: subscription registered");
        for iteration in 0..3 {
            let started = Instant::now();
            match listener.wait_next_timeout(Duration::from_millis(150))? {
                Some(value) => println!("{}", describe(value)),
                None => println!(
                    "Fn notifications: timeout iteration={iteration} ms={}",
                    started.elapsed().as_millis()
                ),
            }
        }
        drop(listener);
        println!("Fn notifications: subscription closed");
        Ok::<(), ram_control_center_protocol::DchuError>(())
    })();
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("Fn notifications: {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::require_direct_mode;

    #[test]
    fn service_forwarding_requires_coordinated_release() {
        assert!(require_direct_mode(Some(0)).is_ok());
        assert!(require_direct_mode(Some(1)).is_err());
    }

    #[test]
    fn unreadable_and_unknown_ownership_cannot_register() {
        assert!(require_direct_mode(None).is_err());
        for flag in 2..=u8::MAX {
            assert!(require_direct_mode(Some(flag)).is_err());
        }
    }
}
