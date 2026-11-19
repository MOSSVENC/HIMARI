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
    let mut yielded = false;
    loop {
        if original_control_center_running() {
            if !yielded {
                log_guard("original Control Center is running; input filter ownership yielded");
            }
            yielded = true;
            applied = false;
            last_error = None;
            std::thread::sleep(RECONCILE_INTERVAL);
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
        std::thread::sleep(RECONCILE_INTERVAL);
    }
}

#[cfg(windows)]
fn is_original_control_center_process(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "cc40.exe" | "controlcenter30.exe" | "couwp.exe" | "controlcenter.exe"
    )
}

#[cfg(windows)]
fn original_control_center_running() -> bool {
    use std::mem::size_of;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    // SAFETY: the snapshot handle is checked before use and closed exactly once;
    // PROCESSENTRY32W is initialized with its required structure size.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..PROCESSENTRY32W::default()
        };
        let mut found = false;
        let mut present = Process32FirstW(snapshot, &mut entry) != 0;
        while present {
            let end = entry
                .szExeFile
                .iter()
                .position(|character| *character == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
            if is_original_control_center_process(&name) {
                found = true;
                break;
            }
            present = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        found
    }
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

#[cfg(all(test, windows))]
mod tests {
    use super::is_original_control_center_process;

    #[test]
    fn original_control_center_names_are_case_insensitive() {
        assert!(is_original_control_center_process("CC40.exe"));
        assert!(is_original_control_center_process("controlcenter30.EXE"));
        assert!(is_original_control_center_process("COUWP.exe"));
        assert!(!is_original_control_center_process(
            "ram-control-center.exe"
        ));
        assert!(!is_original_control_center_process("FnKey.exe"));
    }
}
