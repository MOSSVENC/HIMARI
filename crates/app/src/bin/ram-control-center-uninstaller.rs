#[allow(dead_code)]
#[path = "../product.rs"]
mod product;

#[cfg(any(windows, test))]
#[path = "../uninstall_files.rs"]
mod uninstall_files;

#[cfg(windows)]
#[path = "../service_lifecycle.rs"]
mod service_lifecycle;

#[cfg(windows)]
mod windows_uninstall {
    use crate::product;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::path::PathBuf;
    use std::time::Duration;

    use windows_registry::{CURRENT_USER, LOCAL_MACHINE};
    use windows_sys::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_DELAY_UNTIL_REBOOT, MoveFileExW};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, INFINITE, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::Shell::{
        IsUserAnAdmin, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
    };

    const ERROR_CANCELLED: i32 = 1223;
    const REGISTRY_NOT_FOUND: [u32; 4] = [0x8007_0002, 0x8007_0003, 2, 3];

    pub fn run() -> Result<String, String> {
        let args = std::env::args().skip(1).collect::<Vec<_>>();
        let mode = match args.as_slice() {
            [] => "--plan",
            [mode] if matches!(mode.as_str(), "--plan" | "--remove") => mode,
            _ => {
                return Err(
                    "usage: ram-control-center-uninstaller.exe [--plan|--remove]".to_owned(),
                );
            }
        };
        let roots = installation_roots()?;
        if mode == "--plan" {
            let mut lines = vec!["Application uninstall plan:".to_owned()];
            for root in &roots {
                for path in crate::uninstall_files::plan(root).map_err(|error| error.to_string())? {
                    lines.push(format!("file: {}", path.display()));
                }
            }
            lines.push(format!(
                "services: {}, {}",
                product::SERVICE_NAME,
                product::LEGACY_SERVICE_NAME
            ));
            lines.push("startup and application registration: current user and machine".to_owned());
            lines.push("Recovery backups and unrecognized files are retained and reported. Run --remove to execute.".to_owned());
            return Ok(lines.join("\n"));
        }
        if !is_elevated() {
            return run_elevated();
        }
        ensure_app_is_closed()?;
        let mut report = Vec::new();
        remove_service(product::SERVICE_NAME, &mut report)?;
        remove_service(product::LEGACY_SERVICE_NAME, &mut report)?;
        remove_startup_entries(&mut report)?;
        remove_application_registry(&mut report)?;
        let mut failures = Vec::new();
        for root in roots {
            let cleanup =
                crate::uninstall_files::remove(&root).map_err(|error| error.to_string())?;
            for file in cleanup.removed {
                report.push(format!("removed: {}", file.display()));
            }
            for file in cleanup.retained {
                report.push(format!("retained: {}", file.display()));
            }
            for (file, error) in cleanup.failures {
                if std::env::current_exe().is_ok_and(|current| current == file) {
                    schedule_self_removal(&file)?;
                    report.push(format!("pending restart: {}", file.display()));
                } else {
                    failures.push(format!("{}: {error}", file.display()));
                }
            }
        }
        let log = std::env::temp_dir().join(format!(
            "ram-control-center-uninstall-{}.log",
            std::process::id()
        ));
        report.extend(failures.iter().cloned());
        std::fs::write(&log, report.join("\n"))
            .map_err(|error| format!("write uninstall report: {error}"))?;
        if !failures.is_empty() {
            return Err(format!("cleanup incomplete; report: {}", log.display()));
        }
        Ok(format!("{}\nReport: {}", report.join("\n"), log.display()))
    }

    fn installation_roots() -> Result<Vec<PathBuf>, String> {
        let base = std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute() && path.is_dir())
            .ok_or_else(|| {
                "ProgramData must resolve to an existing absolute directory".to_owned()
            })?;
        Ok(vec![
            base.join(product::PRODUCT_DATA_DIR),
            base.join(product::LEGACY_DATA_DIR),
        ])
    }

    fn ensure_app_is_closed() -> Result<(), String> {
        use windows_sys::Win32::System::Threading::OpenMutexW;
        for name in [product::MUTEX_NAME, r"Local\NeoControlUiInstance"] {
            let name = wide(OsStr::new(name));
            // SAFETY: the name is NUL-terminated. Any returned handle is owned
            // immediately; OpenMutexW only observes an existing mutex.
            unsafe {
                let handle = OpenMutexW(0x0010_0000, 0, name.as_ptr());
                if !handle.is_null() {
                    let _owner = OwnedHandle::from_raw_handle(handle);
                    return Err(
                        "Exit RAM CONTROL CENTER from its tray menu before uninstalling."
                            .to_owned(),
                    );
                }
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(2) {
                    return Err(format!("check running application: {error}"));
                }
            }
        }
        Ok(())
    }

    fn is_elevated() -> bool {
        // SAFETY: IsUserAnAdmin has no pointer or lifetime requirements.
        unsafe { IsUserAnAdmin() != 0 }
    }

    fn run_elevated() -> Result<String, String> {
        let executable = std::env::current_exe()
            .map_err(|error| format!("resolve uninstaller path: {error}"))?;
        let verb = wide(OsStr::new("runas"));
        let file = wide(executable.as_os_str());
        let parameters = wide(OsStr::new("--remove"));
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            hwnd: std::ptr::null_mut(),
            lpVerb: verb.as_ptr(),
            lpFile: file.as_ptr(),
            lpParameters: parameters.as_ptr(),
            lpDirectory: std::ptr::null(),
            nShow: 1,
            ..Default::default()
        };
        // SAFETY: `info` and all UTF-16 strings remain live for the call.
        if unsafe { ShellExecuteExW(&mut info) } == 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_CANCELLED) {
                return Err("administrator approval was cancelled".to_owned());
            }
            return Err(format!("start elevated uninstaller: {error}"));
        }
        wait_for_process(info.hProcess)
    }

    fn wait_for_process(handle: HANDLE) -> Result<String, String> {
        if handle.is_null() {
            return Err("elevated uninstaller returned no process handle".to_owned());
        }
        // SAFETY: ShellExecuteExW returned a valid, uniquely owned process handle.
        let process = unsafe { OwnedHandle::from_raw_handle(handle) };
        let mut exit_code = 1_u32;
        // SAFETY: the owner keeps the process handle live and exit_code is writable
        // local storage. Both calls borrow the same handle without closing it.
        let (wait_result, queried) = unsafe {
            let waited = WaitForSingleObject(process.as_raw_handle(), INFINITE);
            (
                waited,
                GetExitCodeProcess(process.as_raw_handle(), &mut exit_code),
            )
        };
        if wait_result != WAIT_OBJECT_0 || queried == 0 {
            return Err(format!(
                "wait for elevated uninstaller failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        if exit_code != 0 {
            return Err(format!("elevated uninstaller exited with code {exit_code}"));
        }
        Ok("Elevated cleanup finished; see the uninstall report in the administrator temporary directory.".to_owned())
    }

    fn remove_service(name: &str, report: &mut Vec<String>) -> Result<(), String> {
        if crate::service_lifecycle::remove(name, Duration::from_secs(10))? {
            report.push(format!("removed service: {name}"));
        }
        Ok(())
    }

    fn remove_startup_entries(report: &mut Vec<String>) -> Result<(), String> {
        let values = [product::RUN_VALUE_NAME, product::LEGACY_RUN_VALUE_NAME];
        let run_keys = [
            r"Software\Microsoft\Windows\CurrentVersion\Run",
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run",
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run32",
        ];
        for root in [CURRENT_USER, LOCAL_MACHINE] {
            for path in run_keys {
                for value in values {
                    match root
                        .options()
                        .write()
                        .open(path)
                        .and_then(|key| key.remove_value(value))
                    {
                        Ok(()) => report.push(format!("startup value {value}")),
                        Err(error) if is_not_found(&error) => {}
                        Err(error) => {
                            return Err(format!("remove startup value {value} failed: {error}"));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn remove_application_registry(report: &mut Vec<String>) -> Result<(), String> {
        let subkeys = [
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall\RAMControlCenter",
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall\NeoControl",
            r"Software\Microsoft\Windows\CurrentVersion\App Paths\ram-control-center.exe",
            r"Software\Microsoft\Windows\CurrentVersion\App Paths\neo-control.exe",
        ];
        for root in [CURRENT_USER, LOCAL_MACHINE] {
            for subkey in subkeys {
                match root.remove_tree(subkey) {
                    Ok(()) => report.push(format!("registry key {subkey}")),
                    Err(error) if is_not_found(&error) => {}
                    Err(error) => {
                        return Err(format!("remove registry key {subkey} failed: {error}"));
                    }
                }
            }
        }
        Ok(())
    }

    fn is_not_found(error: &windows_result::Error) -> bool {
        REGISTRY_NOT_FOUND.contains(&(error.code().0 as u32))
    }

    fn schedule_self_removal(path: &std::path::Path) -> Result<(), String> {
        let path = wide(path.as_os_str());
        // SAFETY: path is the running uninstaller's validated absolute path;
        // a null destination schedules this single file for removal at reboot.
        if unsafe { MoveFileExW(path.as_ptr(), std::ptr::null(), MOVEFILE_DELAY_UNTIL_REBOOT) } == 0
        {
            return Err(format!(
                "schedule uninstaller removal: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }

    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }
}

fn main() {
    #[cfg(windows)]
    let result = windows_uninstall::run();
    #[cfg(not(windows))]
    let result: Result<String, String> =
        Err("the Windows uninstaller is only available on Windows".to_owned());

    match result {
        Ok(message) => {
            println!("{message}");
            std::process::exit(0);
        }
        Err(error) => {
            eprintln!("RAM CONTROL CENTER uninstall failed: {error}");
            std::process::exit(1);
        }
    }
}
