use std::os::windows::ffi::OsStrExt;

use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Threading::{CreateEventW, INFINITE, WaitForSingleObject};

use crate::{DchuError, Result};

#[derive(Debug)]
pub(crate) struct WinDevice {
    handle: HANDLE,
}

impl WinDevice {
    pub(crate) fn open(
        path: &str,
        flags_and_attributes: u32,
        operation: &'static str,
    ) -> Result<Self> {
        let wide = std::ffi::OsStr::new(path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        // SAFETY: `wide` is NUL-terminated and remains alive for the call. The
        // remaining pointers are explicitly null, as required by CreateFileW.
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                flags_and_attributes,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            return Err(DchuError::Status {
                operation,
                code: std::io::Error::last_os_error().raw_os_error().unwrap_or(-1),
            });
        }
        Ok(Self { handle })
    }

    pub(crate) fn ioctl(
        &self,
        code: u32,
        input: &[u8],
        output: &mut [u8],
        operation: &'static str,
    ) -> Result<u32> {
        let mut returned = 0_u32;
        // SAFETY: `self.handle` is an open device handle owned by this value.
        // The input/output pointers are null exactly when their lengths are
        // zero, and otherwise refer to live slices for the duration of the call.
        let ok = unsafe {
            DeviceIoControl(
                self.handle,
                code,
                if input.is_empty() {
                    std::ptr::null()
                } else {
                    input.as_ptr().cast()
                },
                input.len() as u32,
                if output.is_empty() {
                    std::ptr::null_mut()
                } else {
                    output.as_mut_ptr().cast()
                },
                output.len() as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(DchuError::Status {
                operation,
                code: std::io::Error::last_os_error().raw_os_error().unwrap_or(-1),
            });
        }
        Ok(returned)
    }
}

impl Drop for WinDevice {
    fn drop(&mut self) {
        // SAFETY: this wrapper uniquely owns the valid handle returned by
        // CreateFileW, and Drop runs exactly once for the value.
        unsafe { CloseHandle(self.handle) };
    }
}

pub(crate) struct WinEvent {
    handle: HANDLE,
}

impl WinEvent {
    pub(crate) fn auto_reset() -> Result<Self> {
        // SAFETY: no security attributes or name are supplied; the returned
        // handle is checked and uniquely owned by this value.
        let handle = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
        if handle.is_null() {
            return Err(DchuError::Status {
                operation: "CreateEventW(AcpiBridge notifications)",
                code: std::io::Error::last_os_error().raw_os_error().unwrap_or(-1),
            });
        }
        Ok(Self { handle })
    }

    pub(crate) fn request_bytes(&self) -> [u8; std::mem::size_of::<usize>()] {
        (self.handle as usize).to_ne_bytes()
    }

    pub(crate) fn wait(&self) -> Result<()> {
        // SAFETY: `self.handle` remains valid until this owner's Drop runs.
        let result = unsafe { WaitForSingleObject(self.handle, INFINITE) };
        if result != WAIT_OBJECT_0 {
            return Err(DchuError::Status {
                operation: "WaitForSingleObject(AcpiBridge notification)",
                code: std::io::Error::last_os_error().raw_os_error().unwrap_or(-1),
            });
        }
        Ok(())
    }
}

impl Drop for WinEvent {
    fn drop(&mut self) {
        // SAFETY: this wrapper uniquely owns the event returned by CreateEventW.
        unsafe { CloseHandle(self.handle) };
    }
}
