//! Window and taskbar icons, loaded from the executable's own icon resource.
//!
//! Slint's `icon` property accepts a single image, so the backend scales one
//! bitmap to whatever size the shell asks for. That is what makes the titlebar
//! and taskbar mark look soft. The executable already embeds a nine-entry icon,
//! so the window asks Windows for the exact sizes instead.

#[cfg(windows)]
mod platform {
    use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
    use slint::ComponentHandle as _;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_SHARED, LoadImageW, SM_CXICON, SM_CXSMICON,
        SendMessageW, WM_SETICON,
    };

    use crate::MainWindow;

    const ICON_RESOURCE_ID: usize = 1;

    fn load(size_metric: i32) -> isize {
        // SAFETY: the resource id is the integer the build script embeds, the
        // module handle comes from this process, and a failed load returns null
        // which is checked by the caller.
        unsafe {
            let module = GetModuleHandleW(std::ptr::null());
            LoadImageW(
                module,
                ICON_RESOURCE_ID as *const u16,
                IMAGE_ICON,
                size_metric,
                size_metric,
                LR_SHARED,
            ) as isize
        }
    }

    /// Apply the executable's icon at both shell sizes to the window `ui`.
    pub(super) fn apply(ui: &MainWindow) {
        let handle = ui.window().window_handle();
        let Ok(raw) = handle.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(window) = raw.as_raw() else {
            return;
        };
        let hwnd = window.hwnd.get() as *mut core::ffi::c_void;
        // SAFETY: the HWND comes from the live Slint window on the UI thread and
        // the icon handles come from LoadImageW with LR_SHARED, so neither needs
        // to be destroyed by us.
        unsafe {
            let big = load(crate::window_icon::icon_metric(true));
            if big != 0 {
                SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, big);
            }
            let small = load(crate::window_icon::icon_metric(false));
            if small != 0 {
                SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, small);
            }
        }
    }

    pub(super) fn metric(is_big: bool) -> i32 {
        // SAFETY: GetSystemMetrics reads a documented metric and takes no
        // pointers.
        let value = unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(if is_big {
                SM_CXICON
            } else {
                SM_CXSMICON
            })
        };
        if value > 0 {
            value
        } else if is_big {
            32
        } else {
            16
        }
    }
}

#[cfg(windows)]
pub(crate) fn icon_metric(is_big: bool) -> i32 {
    platform::metric(is_big)
}

#[cfg(not(windows))]
pub(crate) fn icon_metric(is_big: bool) -> i32 {
    if is_big { 32 } else { 16 }
}

/// Apply the executable's icon to the window. No-op off Windows.
pub(crate) fn apply(ui: &crate::MainWindow) {
    #[cfg(windows)]
    platform::apply(ui);
    #[cfg(not(windows))]
    let _ = ui;
}
