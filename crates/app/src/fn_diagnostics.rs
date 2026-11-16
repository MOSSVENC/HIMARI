//! Explicit diagnostics for the vendor notification channel.

#[cfg(windows)]
pub(crate) fn spawn_trace(device_path: String) {
    std::thread::spawn(move || {
        use crate::diagnostics::log_action;
        use ram_control_center_protocol::AcpiNotificationListener;

        let mut listener = match AcpiNotificationListener::open(&device_path) {
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
    use ram_control_center_protocol::{AcpiNotificationListener, Dchu};
    use std::time::{Duration, Instant};

    let result = (|| {
        let device = Dchu::open()?;
        let mut listener = AcpiNotificationListener::open(device.path())?;
        println!("Fn notifications: registered at {}", device.path());
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
