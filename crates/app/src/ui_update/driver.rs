use crate::{MainWindow, driver_manager::DriverHealth, localization::tr};

pub(crate) fn apply_driver_health(ui: &MainWindow, driver: &DriverHealth) {
    ui.set_driver_core_ready(driver.core_ready);
    ui.set_driver_core_installed(driver.core_installed);
    ui.set_driver_keyboard_ready(driver.keyboard_ready);
    ui.set_driver_keyboard_installed(driver.keyboard_installed);
    ui.set_driver_payload_ready(driver.payload_ready);
    ui.set_driver_keyboard_payload_ready(driver.keyboard_payload_ready);
    let driver_summary = if !driver.required_missing() {
        tr("Required driver ready")
    } else if !driver.payload_ready {
        tr("Driver package missing")
    } else if driver.core_installed {
        tr("Driver installed but not responding")
    } else {
        tr("Required driver missing")
    };
    let driver_detail = if driver.core_ready {
        if driver.keyboard_ready {
            tr("AcpiBridge is online. The optional keyboard filter is also available.")
        } else {
            tr("AcpiBridge is online. The current release does not require HKKbdFltr.")
        }
    } else if driver.payload_ready {
        tr("The signed OEM driver package is available next to the application.")
    } else {
        tr("Put the signed AcpiBridge package in the drivers folder.")
    };
    ui.set_driver_summary(driver_summary.into());
    ui.set_driver_detail(driver_detail.into());
    ui.set_driver_payload_path(
        if driver.payload_path.as_os_str().is_empty() {
            tr("Not located").to_owned()
        } else {
            driver.payload_path.display().to_string()
        }
        .into(),
    );
}
