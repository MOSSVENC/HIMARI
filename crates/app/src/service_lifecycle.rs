//! Shared, bounded removal of the application's one-shot boot services.

use std::time::{Duration, Instant};

use windows_service::service::{ServiceAccess, ServiceControlAccept, ServiceState};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

pub(crate) fn remove(name: &str, timeout: Duration) -> Result<bool, String> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|error| format!("open service manager: {error}"))?;
    let service = match manager.open_service(
        name,
        ServiceAccess::DELETE | ServiceAccess::STOP | ServiceAccess::QUERY_STATUS,
    ) {
        Ok(service) => service,
        Err(windows_service::Error::Winapi(error)) if error.raw_os_error() == Some(1060) => {
            return Ok(false);
        }
        Err(error) => return Err(format!("open service {name}: {error}")),
    };
    let status = service
        .query_status()
        .map_err(|error| format!("query service {name}: {error}"))?;
    if status.current_state != ServiceState::Stopped {
        // The boot worker exits after applying its policy. Older versions do
        // not accept STOP; give them a bounded opportunity to finish naturally.
        if status
            .controls_accepted
            .contains(ServiceControlAccept::STOP)
        {
            service
                .stop()
                .map_err(|error| format!("stop service {name}: {error}"))?;
        }
        let deadline = Instant::now() + timeout;
        loop {
            let status = service
                .query_status()
                .map_err(|error| format!("query service {name}: {error}"))?;
            if status.current_state == ServiceState::Stopped {
                break;
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "service {name} is still running after {} seconds",
                    timeout.as_secs()
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    service
        .delete()
        .map_err(|error| format!("delete service {name}: {error}"))?;
    Ok(true)
}
