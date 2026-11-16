use super::{WinDevice, WinEvent};
use crate::{DchuError, FnNotification, Result};
use std::time::Duration;

const IOCTL_ACPI_WAIT_EVENT: u32 = 0x0032_2404;
const IOCTL_ACPI_GET_NOTIFY_VALUE: u32 = 0x0032_2408;

/// A registered AcpiBridge notification subscription.
///
/// The caller must coordinate channel ownership with the resident Fn handler.
/// Registration happens once; each subsequent wait consumes the next signal.
pub struct AcpiNotificationListener {
    // Close the device subscription before closing the event it references.
    device: WinDevice,
    event: WinEvent,
}

impl std::fmt::Debug for AcpiNotificationListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcpiNotificationListener")
            .finish_non_exhaustive()
    }
}

impl AcpiNotificationListener {
    pub fn open(device_path: &str) -> Result<Self> {
        let device = WinDevice::open(device_path, 0, "CreateFileW(AcpiBridge notifications)")?;
        let event = WinEvent::auto_reset()?;
        // Own the handles in their required destruction order even if the
        // registration fails after the driver has processed the request.
        let listener = Self { device, event };
        listener.device.ioctl(
            IOCTL_ACPI_WAIT_EVENT,
            &listener.event.request_bytes(),
            &mut [],
            "DeviceIoControl(0x322404 register event)",
        )?;
        Ok(listener)
    }

    pub fn wait_next(&mut self) -> Result<FnNotification> {
        self.event.wait(u32::MAX)?;
        self.read_notification()
    }

    /// A timeout retains the existing subscription for the next wait.
    pub fn wait_next_timeout(&mut self, timeout: Duration) -> Result<Option<FnNotification>> {
        let milliseconds = timeout.as_millis().min(u128::from(u32::MAX - 1)) as u32;
        if self.event.wait(milliseconds)? {
            self.read_notification().map(Some)
        } else {
            Ok(None)
        }
    }

    fn read_notification(&self) -> Result<FnNotification> {
        let mut value = [0_u8; 8];
        let returned = self.device.ioctl(
            IOCTL_ACPI_GET_NOTIFY_VALUE,
            &[],
            &mut value,
            "DeviceIoControl(0x322408 notification value)",
        )?;
        if returned as usize != value.len() {
            return Err(DchuError::ControlFailed {
                operation: "read AcpiBridge notification value",
                detail: format!("expected 8 bytes, received {returned}"),
            });
        }
        FnNotification::from_payload(&value)
    }
}
