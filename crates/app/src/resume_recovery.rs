//! Sleep/resume policy restoration on a dedicated Windows message thread.

#[cfg(windows)]
mod platform {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicU64, Ordering};

    use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::SystemInformation::GetTickCount64;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW,
        MSG, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMECRITICAL, PBT_APMRESUMESUSPEND, RegisterClassW,
        TranslateMessage, WM_POWERBROADCAST, WNDCLASSW, WS_OVERLAPPED,
    };

    use crate::product::product_data_dir;

    const WINDOW_CLASS: &str = "RamControlCenterPowerWindow";
    const WINDOW_TITLE: &str = "RAM Control Center Power Events";
    const LOG_NAME: &str = "resume-recovery.log";
    const DUPLICATE_WINDOW_MS: u64 = 5_000;

    static LAST_RESUME_MS: AtomicU64 = AtomicU64::new(0);

    pub(super) fn start() {
        static STARTED: OnceLock<()> = OnceLock::new();
        if STARTED.set(()).is_err() {
            return;
        }
        if let Err(error) = std::thread::Builder::new()
            .name("power-resume-recovery".to_owned())
            .spawn(run_window)
        {
            log_line(&format!("resume recovery thread could not start: {error}"));
        }
    }

    fn run_window() {
        let class = wide(OsStr::new(WINDOW_CLASS));
        let title = wide(OsStr::new(WINDOW_TITLE));
        // SAFETY: null requests the current process module; it has no pointer
        // prerequisites and the returned handle is process-lifetime owned.
        let instance: HINSTANCE = unsafe { GetModuleHandleW(ptr::null()) };
        let window_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..WNDCLASSW::default()
        };
        // SAFETY: the WNDCLASSW and class-name buffer are live for the call.
        if unsafe { RegisterClassW(&window_class) } == 0 {
            log_line(&format!(
                "resume recovery window registration failed: {}",
                std::io::Error::last_os_error()
            ));
            return;
        }
        // SAFETY: the class name, title and instance are valid; the window is
        // deliberately top-level but never shown so it receives power broadcasts.
        let window: HWND = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                ptr::null(),
            )
        };
        if window.is_null() {
            log_line(&format!(
                "resume recovery window creation failed: {}",
                std::io::Error::last_os_error()
            ));
            return;
        }
        log_line("resume recovery listener ready");
        let mut message = MSG::default();
        loop {
            // SAFETY: null HWND requests this thread's message queue; the MSG
            // storage remains valid for the whole loop.
            let result = unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) };
            if result <= 0 {
                break;
            }
            // SAFETY: the message was produced by GetMessageW for this thread.
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    unsafe extern "system" fn window_proc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_POWERBROADCAST {
            let event = wparam as u32;
            if matches!(
                event,
                PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND | PBT_APMRESUMECRITICAL
            ) {
                // SAFETY: GetTickCount64 has no pointer preconditions.
                let now = unsafe { GetTickCount64() };
                let previous = LAST_RESUME_MS.load(Ordering::Acquire);
                if now.saturating_sub(previous) >= DUPLICATE_WINDOW_MS {
                    LAST_RESUME_MS.store(now, Ordering::Release);
                    match crate::boot_agent::reapply_saved_policy() {
                        Ok(()) => log_line("resume policy restore completed"),
                        Err(error) => log_line(&format!("resume policy restore failed: {error}")),
                    }
                }
                return 1;
            }
        }
        // SAFETY: forwarding the unmodified callback arguments is the
        // documented default-window-procedure contract.
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }

    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    fn log_line(message: &str) {
        use std::io::Write as _;

        let root = product_data_dir();
        if std::fs::create_dir_all(&root).is_err() {
            return;
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join(LOG_NAME))
        {
            let _ = writeln!(file, "{message}");
        }
    }
}

/// Start the process-lifetime resume listener once.
pub(crate) fn start() {
    #[cfg(windows)]
    platform::start();
}
