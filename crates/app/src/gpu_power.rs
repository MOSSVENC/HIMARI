//! Runtime NVIDIA dGPU power control recovered from the vendor worker.
//!
//! CC4.0's worker command `0x0013` disables the `ACPI\NVDA0820` platform
//! controller node before writing DCHU block 4/sub-command 34 offset 0. The
//! enable path writes offset 1 and then enables the node. Device-node changes
//! require administrator rights, so this module launches the same executable
//! through the Windows `runas` verb instead of invoking the vendor worker.

#[cfg(windows)]
mod platform {
    use std::ffi::OsStr;
    use std::io::Error;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use std::thread;
    use std::time::Duration;

    use ram_control_center_protocol::Dchu;
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
        CM_Disable_DevNode, CM_Enable_DevNode, CM_Get_DevNode_Status, CR_SUCCESS, DIGCF_ALLCLASSES,
        DIGCF_PRESENT, DN_STARTED, HDEVINFO, SP_DEVINFO_DATA, SPDRP_HARDWAREID,
        SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
        SetupDiGetDeviceRegistryPropertyW,
    };
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_CANCELLED, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, INFINITE, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::Shell::{
        IsUserAnAdmin, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
    };

    use crate::product::product_data_dir;

    const NVPCF_HARDWARE_ID: &str = "ACPI\\NVDA0820";
    const LOG_NAME: &str = "gpu-power.log";

    pub(super) fn request(enabled: bool) -> Result<(), String> {
        // SAFETY: IsUserAnAdmin has no pointer preconditions.
        if unsafe { IsUserAnAdmin() } != 0 {
            return perform(enabled);
        }
        launch_elevated(enabled).and_then(|code| {
            if code == 0 {
                Ok(())
            } else {
                Err(format!("elevated GPU helper exited with code {code}"))
            }
        })
    }

    pub(super) fn run_cli(enabled: bool) -> i32 {
        match request(enabled) {
            Ok(()) => 0,
            Err(error) => {
                append_log(&error);
                eprintln!("{error}");
                1
            }
        }
    }

    pub(super) fn status() -> i32 {
        match nvpc_nodes() {
            Ok(nodes) => {
                println!("NVPCF_NODES={}", nodes.len());
                for node in nodes {
                    match node_started(node.dev_inst) {
                        Ok(started) => println!(
                            "DEVINST={} STARTED={} INSTANCE={}",
                            node.dev_inst, started, node.instance
                        ),
                        Err(error) => println!(
                            "DEVINST={} STATUS_ERROR={} INSTANCE={}",
                            node.dev_inst, error, node.instance
                        ),
                    }
                }
                0
            }
            Err(error) => {
                eprintln!("{error}");
                1
            }
        }
    }

    fn perform(enabled: bool) -> Result<(), String> {
        let nodes = nvpc_nodes()?;
        if nodes.is_empty() {
            return Err(format!("device node {NVPCF_HARDWARE_ID} was not found"));
        }
        let dchu = Dchu::open()
            .map_err(|error| format!("could not open the signed ACPI transport: {error}"))?;
        append_log(if enabled {
            "GPU power: enabling NVPCF and device node"
        } else {
            "GPU power: disabling device node and NVPCF"
        });

        if enabled {
            dchu.set_nvidia_power(true)
                .map_err(|error| format!("DCHU 4/[34,1] failed: {error}"))?;
            set_nodes(&nodes, true)?;
            wait_for_node_state(&nodes, true, Duration::from_secs(8))?;
        } else {
            set_nodes(&nodes, false)?;
            if let Err(error) = dchu.set_nvidia_power(false) {
                let rollback = set_nodes(&nodes, true).map_or_else(
                    |rollback| format!("node restore failed: {rollback}"),
                    |()| "node restored".to_owned(),
                );
                return Err(format!("DCHU 4/[34,0] failed: {error}; {rollback}"));
            }
            wait_for_node_state(&nodes, false, Duration::from_secs(5))?;
        }
        append_log("GPU power: transition verified");
        Ok(())
    }

    struct NvpcNode {
        dev_inst: u32,
        instance: String,
    }

    fn nvpc_nodes() -> Result<Vec<NvpcNode>, String> {
        // SAFETY: null class/enumerator/hwnd select all present devices. The
        // returned device-info set is owned by this function and destroyed once.
        let set: HDEVINFO = unsafe {
            SetupDiGetClassDevsW(
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                DIGCF_PRESENT | DIGCF_ALLCLASSES,
            )
        };
        if set == INVALID_HANDLE_VALUE as isize {
            return Err(format!(
                "SetupDiGetClassDevsW failed: {}",
                Error::last_os_error()
            ));
        }
        let mut nodes = Vec::new();
        let mut index = 0_u32;
        loop {
            let mut info = SP_DEVINFO_DATA {
                cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
                ..SP_DEVINFO_DATA::default()
            };
            // SAFETY: the device-info set and element storage are valid for
            // the duration of this synchronous enumeration call.
            if unsafe { SetupDiEnumDeviceInfo(set, index, &mut info) } == 0 {
                break;
            }
            let mut buffer = [0_u8; 4096];
            let mut required = 0_u32;
            // SAFETY: the output buffer is writable and its full byte length is
            // passed; `required` receives the actual property size.
            let ok = unsafe {
                SetupDiGetDeviceRegistryPropertyW(
                    set,
                    &info,
                    SPDRP_HARDWAREID,
                    ptr::null_mut(),
                    buffer.as_mut_ptr(),
                    buffer.len() as u32,
                    &mut required,
                )
            };
            if ok != 0 {
                let length = usize::try_from(required).unwrap_or(0).min(buffer.len());
                let (pairs, _) = buffer[..length].as_chunks::<2>();
                let units = pairs
                    .iter()
                    .map(|pair| u16::from_le_bytes(*pair))
                    .collect::<Vec<_>>();
                let hardware_ids = units
                    .split(|unit| *unit == 0)
                    .filter(|part| !part.is_empty())
                    .map(String::from_utf16_lossy)
                    .collect::<Vec<_>>();
                if hardware_ids.iter().any(|hardware_id| {
                    hardware_id
                        .to_ascii_uppercase()
                        .contains(&NVPCF_HARDWARE_ID.to_ascii_uppercase())
                }) {
                    nodes.push(NvpcNode {
                        dev_inst: info.DevInst,
                        instance: hardware_ids.join("; "),
                    });
                }
            }
            index = index.saturating_add(1);
        }
        // SAFETY: this function owns the handle returned above.
        unsafe { SetupDiDestroyDeviceInfoList(set) };
        Ok(nodes)
    }

    fn set_nodes(nodes: &[NvpcNode], enabled: bool) -> Result<(), String> {
        for node in nodes {
            // SAFETY: DevInst is a live device instance returned by SetupAPI;
            // flags zero request the immediate transition.
            let result = unsafe {
                if enabled {
                    CM_Enable_DevNode(node.dev_inst, 0)
                } else {
                    CM_Disable_DevNode(node.dev_inst, 0)
                }
            };
            if result != CR_SUCCESS {
                return Err(format!(
                    "{} device node {} failed with CONFIGRET {result}",
                    if enabled { "enable" } else { "disable" },
                    node.instance
                ));
            }
        }
        Ok(())
    }

    fn wait_for_node_state(
        nodes: &[NvpcNode],
        expected_started: bool,
        timeout: Duration,
    ) -> Result<(), String> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let mut all_match = true;
            for node in nodes {
                match node_started(node.dev_inst) {
                    Ok(started) if started == expected_started => {}
                    Ok(_) | Err(_) => all_match = false,
                }
            }
            if all_match {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err(format!(
                    "device node state did not become {} in time",
                    if expected_started {
                        "started"
                    } else {
                        "stopped"
                    }
                ));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn node_started(dev_inst: u32) -> Result<bool, String> {
        let mut status = 0_u32;
        let mut problem = 0_u32;
        // SAFETY: both output pointers refer to valid local storage.
        let result = unsafe { CM_Get_DevNode_Status(&mut status, &mut problem, dev_inst, 0) };
        if result == CR_SUCCESS {
            Ok(status & DN_STARTED != 0)
        } else {
            Err(format!("CM_Get_DevNode_Status returned {result}"))
        }
    }

    fn launch_elevated(enabled: bool) -> Result<u32, String> {
        let executable = std::env::current_exe()
            .map_err(|error| format!("could not resolve the current executable: {error}"))?;
        let operation = wide(OsStr::new("runas"));
        let file = wide(executable.as_os_str());
        let parameters = wide(OsStr::new(if enabled {
            "--gpu-power on"
        } else {
            "--gpu-power off"
        }));
        let mut info = SHELLEXECUTEINFOW {
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            hwnd: ptr::null_mut(),
            lpVerb: operation.as_ptr(),
            lpFile: file.as_ptr(),
            lpParameters: parameters.as_ptr(),
            lpDirectory: ptr::null(),
            nShow: 1,
            ..Default::default()
        };
        // SAFETY: CoInitializeEx has no pointer preconditions for this
        // apartment-scoped helper thread.
        let initialized =
            unsafe { CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED as u32) } >= 0;
        let result = (|| {
            // SAFETY: the structure and all referenced wide strings remain live
            // for this call; the returned process handle is checked and owned.
            if unsafe { ShellExecuteExW(&mut info) } == 0 {
                let error = Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_CANCELLED as i32) {
                    return Err("administrator approval was cancelled".to_owned());
                }
                return Err(format!("ShellExecuteExW(runas) failed: {error}"));
            }
            let handle: HANDLE = info.hProcess;
            if handle.is_null() {
                return Err("elevated GPU helper returned no process handle".to_owned());
            }
            // SAFETY: the handle came from SEE_MASK_NOCLOSEPROCESS and is owned
            // by this function until the single CloseHandle below.
            let wait = unsafe { WaitForSingleObject(handle, INFINITE) };
            let mut code = 0_u32;
            // SAFETY: `handle` is live and `code` points to writable storage.
            let read = unsafe { GetExitCodeProcess(handle, &mut code) };
            // SAFETY: this scope uniquely owns the ShellExecuteEx process handle.
            unsafe { CloseHandle(handle) };
            if wait != WAIT_OBJECT_0 {
                return Err(format!("waiting for the GPU helper returned {wait}"));
            }
            if read == 0 {
                return Err(format!(
                    "reading the GPU helper exit code failed: {}",
                    Error::last_os_error()
                ));
            }
            Ok(code)
        })();
        if initialized {
            // SAFETY: balances the successful CoInitializeEx above.
            unsafe { CoUninitialize() };
        }
        result
    }

    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    fn append_log(message: &str) {
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

#[cfg(windows)]
pub(crate) fn request(enabled: bool) -> Result<(), String> {
    platform::request(enabled)
}

#[cfg(windows)]
pub(crate) fn run_cli(enabled: bool) -> i32 {
    platform::run_cli(enabled)
}

#[cfg(windows)]
pub(crate) fn status() -> i32 {
    platform::status()
}

#[cfg(not(windows))]
pub(crate) fn request(_enabled: bool) -> Result<(), String> {
    Err("NVIDIA power control is only available on Windows".to_owned())
}

#[cfg(not(windows))]
pub(crate) fn run_cli(_enabled: bool) -> i32 {
    eprintln!("NVIDIA power control is only available on Windows");
    1
}

#[cfg(not(windows))]
pub(crate) fn status() -> i32 {
    eprintln!("NVIDIA power control is only available on Windows");
    1
}
