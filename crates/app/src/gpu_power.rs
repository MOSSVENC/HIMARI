//! Runtime NVIDIA dGPU power control recovered from the vendor worker.
//!
//! CC4.0's worker command `0x0013` disables the `ACPI\NVDA0820` platform
//! controller node before writing DCHU block 4/sub-command 34 offset 0. The
//! enable path writes offset 1 and then enables the node. Device-node changes
//! require administrator rights, so this module launches the same executable
//! through the Windows `runas` verb and uses the built-in `pnputil` tool. It
//! never invokes the vendor worker executable.

#[cfg(windows)]
mod platform {
    use std::ffi::OsStr;
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    use std::process::{Command, Output};
    use std::thread;
    use std::time::{Duration, Instant};

    use himari_protocol::Dchu;

    use crate::product::product_data_dir;

    const NVPCF_INSTANCE: &str = r"ACPI\NVDA0820\NPCF";
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const LOG_NAME: &str = "gpu-power.log";

    pub(super) fn request(enabled: bool) -> Result<(), String> {
        let executable = std::env::current_exe()
            .map_err(|error| format!("could not resolve the current executable: {error}"))?;
        let quoted = executable.to_string_lossy().replace('\'', "''");
        let argument = if enabled {
            "--gpu-power-admin on"
        } else {
            "--gpu-power-admin off"
        };
        let script = format!(
            "$p=Start-Process -FilePath '{quoted}' -ArgumentList '{argument}' -Verb RunAs -PassThru -Wait; exit $p.ExitCode"
        );
        let output = run_powershell(&script)?;
        if output.status.success() {
            super::record_discrete_gpu_state(!enabled);
            Ok(())
        } else {
            Err(format!(
                "elevated GPU helper failed with {}; {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }

    pub(super) fn run_cli(enabled: bool) -> i32 {
        match perform(enabled) {
            Ok(()) => 0,
            Err(error) => {
                append_log(&error);
                eprintln!("{error}");
                1
            }
        }
    }

    pub(super) fn status() -> i32 {
        match device_status() {
            Ok(status) => {
                println!("NVPCF_INSTANCE={NVPCF_INSTANCE}");
                println!("NVPCF_STATUS={status}");
                0
            }
            Err(error) => {
                eprintln!("{error}");
                1
            }
        }
    }

    fn perform(enabled: bool) -> Result<(), String> {
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
            pnputil(true)?;
            wait_for_status("OK", Duration::from_secs(8))?;
        } else {
            pnputil(false)?;
            if let Err(error) = dchu.set_nvidia_power(false) {
                let rollback = pnputil(true).map_or_else(
                    |rollback| format!("node restore failed: {rollback}"),
                    |()| "node restored".to_owned(),
                );
                return Err(format!("DCHU 4/[34,0] failed: {error}; {rollback}"));
            }
            wait_for_status("Error", Duration::from_secs(5))?;
        }
        append_log("GPU power: transition verified");
        Ok(())
    }

    fn pnputil(enable: bool) -> Result<(), String> {
        let system_root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let executable = PathBuf::from(system_root)
            .join("System32")
            .join("pnputil.exe");
        let operation = if enable {
            "/enable-device"
        } else {
            "/disable-device"
        };
        let output = hidden_command(&executable, [operation, NVPCF_INSTANCE])
            .map_err(|error| format!("pnputil launch failed: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "pnputil {operation} failed with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(())
    }

    fn device_status() -> Result<String, String> {
        let script = format!(
            "[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); (Get-PnpDevice -InstanceId '{NVPCF_INSTANCE}' -ErrorAction Stop).Status"
        );
        let output = run_powershell(&script)?;
        if !output.status.success() {
            return Err(format!(
                "reading NVPCF status failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    fn wait_for_status(expected: &str, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        loop {
            let last = device_status().unwrap_or_else(|error| format!("unavailable: {error}"));
            if last == expected {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "NVPCF status did not reach {expected}; last status was {last}"
                ));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn hidden_command<I, S>(executable: &std::path::Path, args: I) -> std::io::Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        Command::new(executable)
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    }

    fn run_powershell(script: &str) -> Result<Output, String> {
        let system_root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let executable = PathBuf::from(system_root)
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        hidden_command(
            &executable,
            ["-NoProfile", "-NonInteractive", "-Command", script],
        )
        .map_err(|error| format!("PowerShell launch failed: {error}"))
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

/// Marker file recording the last power state we successfully applied.
///
/// The device node query costs an elevated round trip, so the Fn cycle reads
/// this marker instead: it answers "did we power the discrete GPU down", which
/// is exactly the question the cycle needs.
const STATE_MARKER: &str = "discrete-gpu-off.marker";

#[cfg(windows)]
fn record_discrete_gpu_state(disabled: bool) {
    let root = crate::product::product_data_dir();
    let _ = std::fs::create_dir_all(&root);
    let marker = root.join(STATE_MARKER);
    if disabled {
        let _ = std::fs::write(marker, b"");
    } else {
        let _ = std::fs::remove_file(marker);
    }
}

pub(crate) fn is_disabled() -> bool {
    crate::product::product_data_dir()
        .join(STATE_MARKER)
        .exists()
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
