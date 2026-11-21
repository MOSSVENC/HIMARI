use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use himari_protocol::{Dchu, FanMode, PowerMode};

use crate::product::{legacy_data_dir, product_data_dir};

#[derive(Clone, Copy, Debug)]
struct BootProfile {
    power: PowerMode,
    fan: FanMode,
}

impl Default for BootProfile {
    fn default() -> Self {
        Self {
            power: PowerMode::Performance,
            fan: FanMode::Auto,
        }
    }
}

/// Reapply the persisted runtime policy after a sleep/resume transition.
pub(crate) fn reapply_saved_policy() -> Result<(), String> {
    let profile = read_profile(&config_path())
        .or_else(|| read_profile(&legacy_data_dir().join("boot-profile.ini")))
        .unwrap_or_default();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match Dchu::open() {
            Ok(dchu) => {
                match dchu.release_proc_hot() {
                    Ok(()) => log_line("processor-hot limit released after resume"),
                    Err(error) => log_line(&format!(
                        "processor-hot release after resume failed: {error}"
                    )),
                }
                dchu.set_power_mode(profile.power)
                    .map_err(|error| format!("power restore failed: {error}"))?;
                if profile.fan != FanMode::Auto {
                    dchu.set_fan_mode(profile.fan)
                        .map_err(|error| format!("fan restore failed: {error}"))?;
                }
                log_line(&format!(
                    "resume policy restored: power={} fan={}",
                    profile.power.as_u8(),
                    profile.fan.as_u8()
                ));
                return Ok(());
            }
            Err(error) => {
                if Instant::now() >= deadline {
                    return Err(format!("DCHU was not ready after resume: {error}"));
                }
            }
        }
        thread::sleep(Duration::from_millis(250));
    }
}

/// Headless boot path used by the single `himari.exe` binary.
///
/// It never constructs the Slint window. The signed AcpiBridge device is
/// retried until its settings channel is ready, the full persisted power
/// transition is reapplied, and a non-Auto fan policy is applied only after
/// power succeeds. A bounded read-only CPU observation then records actual
/// performance independently of the settings acknowledgement.
pub fn run() -> i32 {
    crate::input_recovery::start_filter_guard();
    let started = Instant::now();
    let config_path = config_path();
    let mut profile = read_profile(&config_path)
        .or_else(|| read_profile(&legacy_data_dir().join("boot-profile.ini")))
        .unwrap_or_default();
    let mut wait_ms = 45_000_u64;
    let mut observe_ms = 60_000_u64;
    let mut observe_only = false;

    let mut args = std::env::args().skip(2);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--observe-only" => observe_only = true,
            "--observe-ms" => {
                let Some(value) = args.next() else {
                    log_line("missing value for --observe-ms");
                    return 1;
                };
                let Ok(value) = value.parse::<u64>() else {
                    log_line(&format!("invalid --observe-ms value {value:?}"));
                    return 1;
                };
                observe_ms = value.min(60_000);
            }
            "--power" => {
                let Some(value) = args.next() else {
                    log_line("missing value for --power");
                    return 1;
                };
                let Ok(raw) = value.parse::<u8>() else {
                    log_line(&format!("invalid --power value {value:?}"));
                    return 1;
                };
                let Some(power) = PowerMode::from_u8(raw) else {
                    log_line(&format!("unsupported --power value {raw}"));
                    return 1;
                };
                profile.power = power;
            }
            "--fan" => {
                let Some(value) = args.next() else {
                    log_line("missing value for --fan");
                    return 1;
                };
                let Ok(raw) = value.parse::<u8>() else {
                    log_line(&format!("invalid --fan value {value:?}"));
                    return 1;
                };
                let Some(fan) = FanMode::from_u8(raw) else {
                    log_line(&format!("unsupported --fan value {raw}"));
                    return 1;
                };
                profile.fan = fan;
            }
            "--wait-ms" => {
                let Some(value) = args.next() else {
                    log_line("missing value for --wait-ms");
                    return 1;
                };
                let Ok(value) = value.parse::<u64>() else {
                    log_line(&format!("invalid --wait-ms value {value:?}"));
                    return 1;
                };
                wait_ms = value.min(180_000);
            }
            "--config" => {
                let Some(path) = args.next() else {
                    log_line("missing value for --config");
                    return 1;
                };
                profile = read_profile(Path::new(&path)).unwrap_or(profile);
            }
            _ => log_line(&format!("ignoring unknown boot-agent argument {arg:?}")),
        }
    }

    log_line(&format!(
        "agent start: target power={} fan={} wait_ms={wait_ms} observe_ms={observe_ms} observe_only={observe_only}",
        profile.power.as_u8(),
        profile.fan.as_u8()
    ));

    if observe_only {
        return match Dchu::open() {
            Ok(dchu) => {
                observe_cpu(&dchu, observe_ms);
                0
            }
            Err(error) => {
                log_line(&format!(
                    "CPU observation could not open the transport: {error}"
                ));
                1
            }
        };
    }

    let deadline = started + Duration::from_millis(wait_ms);
    let mut attempt = 0_u32;
    let mut last_error = None;
    let mut service_checked = false;
    // `Instant` is boot-relative on Windows. Avoid subtracting five seconds
    // when the service starts very early in boot; use the start marker instead.
    let mut last_progress_log = started;

    while Instant::now() <= deadline {
        attempt += 1;

        // Do not read the whole platform state here. The boot path only needs
        // the settings channel and the power command; that keeps the cold-boot
        // critical path short and avoids the expensive thermal/fan reads.
        let opened = match Dchu::open() {
            Ok(dchu) => Ok(dchu),
            Err(first_error) => {
                if !service_checked {
                    service_checked = true;
                    match ensure_acpi_bridge_service() {
                        Ok(()) => log_line("acpi bridge service checked/started"),
                        Err(error) => {
                            log_line(&format!("could not start acpi bridge service: {error}"))
                        }
                    }
                }
                Dchu::open().map_err(|second_error| format!("{first_error}; retry: {second_error}"))
            }
        };

        match opened {
            Ok(dchu) => {
                // The vendor power handler opens by releasing the latched
                // processor-hot limit; without it the package stays capped even
                // in the Performance profile.
                match dchu.release_proc_hot() {
                    Ok(()) => log_line("processor-hot limit released"),
                    Err(error) => log_line(&format!("processor-hot release failed: {error}")),
                }
                let power_ok = match dchu.set_power_mode(profile.power) {
                    Ok(()) => {
                        log_line(&format!(
                            "power reapplied and read back: {}",
                            profile.power.label()
                        ));
                        true
                    }
                    Err(error) => {
                        last_error = Some(format!("power write failed: {error}"));
                        if !service_checked {
                            service_checked = true;
                            match ensure_acpi_bridge_service() {
                                Ok(()) => log_line("acpi bridge service checked/started"),
                                Err(error) => log_line(&format!(
                                    "could not start acpi bridge service: {error}"
                                )),
                            }
                        }
                        false
                    }
                };

                // Auto is already the firmware fallback and does not need a
                // boot-time write. Non-auto policies are applied only after the
                // power command has succeeded.
                let fan_ok = if !power_ok || profile.fan == FanMode::Auto {
                    true
                } else {
                    match dchu.set_fan_mode(profile.fan) {
                        Ok(()) => {
                            log_line(&format!(
                                "fan applied and read back: {}",
                                profile.fan.label()
                            ));
                            true
                        }
                        Err(error) => {
                            last_error = Some(format!("fan write failed: {error}"));
                            false
                        }
                    }
                };

                if power_ok && fan_ok {
                    log_line(&format!(
                        "policy replay complete in {} ms ({} attempts)",
                        started.elapsed().as_millis(),
                        attempt
                    ));
                    observe_cpu(&dchu, observe_ms);
                    return 0;
                }
                if last_progress_log.elapsed() >= Duration::from_secs(5) {
                    log_line(&format!(
                        "still waiting after {} ms (attempt {}): {}",
                        started.elapsed().as_millis(),
                        attempt,
                        last_error.as_deref().unwrap_or("unknown error")
                    ));
                    last_progress_log = Instant::now();
                }
            }
            Err(error) => {
                last_error = Some(error);
                if !service_checked {
                    service_checked = true;
                    match ensure_acpi_bridge_service() {
                        Ok(()) => log_line("acpi bridge service checked/started"),
                        Err(error) => {
                            log_line(&format!("could not start acpi bridge service: {error}"))
                        }
                    }
                }
                if last_progress_log.elapsed() >= Duration::from_secs(5) {
                    log_line(&format!(
                        "still waiting after {} ms (attempt {}): {}",
                        started.elapsed().as_millis(),
                        attempt,
                        last_error.as_deref().unwrap_or("unknown error")
                    ));
                    last_progress_log = Instant::now();
                }
            }
        }
        thread::sleep(Duration::from_millis(10));
    }

    let reason = last_error.unwrap_or_else(|| "DCHU did not become ready".to_owned());
    log_line(&format!(
        "agent failed after {} ms ({} attempts): {reason}",
        started.elapsed().as_millis(),
        attempt
    ));
    1
}

#[cfg(windows)]
fn ensure_acpi_bridge_service() -> Result<(), String> {
    use std::ffi::OsStr;
    use windows_service::service::ServiceAccess;
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
    use windows_sys::Win32::Foundation::ERROR_SERVICE_ALREADY_RUNNING;

    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|error| describe_service_error("OpenSCManagerW", error))?;
    let service = manager
        .open_service("AcpiBridge", ServiceAccess::START)
        .map_err(|error| describe_service_error("OpenServiceW(AcpiBridge)", error))?;
    match service.start::<&OsStr>(&[]) {
        Ok(()) => Ok(()),
        Err(windows_service::Error::Winapi(error))
            if error.raw_os_error() == Some(ERROR_SERVICE_ALREADY_RUNNING as i32) =>
        {
            Ok(())
        }
        Err(error) => Err(describe_service_error("StartServiceW(AcpiBridge)", error)),
    }
}

#[cfg(windows)]
fn describe_service_error(operation: &str, error: windows_service::Error) -> String {
    match error {
        windows_service::Error::Winapi(error) => {
            let code = error.raw_os_error().unwrap_or_default();
            format!("{operation} failed: {error} (code {code})")
        }
        error => format!("{operation} failed: {error}"),
    }
}

#[cfg(not(windows))]
fn ensure_acpi_bridge_service() -> Result<(), String> {
    Ok(())
}

fn config_path() -> PathBuf {
    product_data_dir().join("boot-profile.ini")
}

fn read_profile(path: &Path) -> Option<BootProfile> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut result = BootProfile::default();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let raw = value.trim().parse::<u8>().ok();
        match key.trim() {
            "power" => result.power = raw.and_then(PowerMode::from_u8).unwrap_or(result.power),
            "fan" => result.fan = raw.and_then(FanMode::from_u8).unwrap_or(result.fan),
            _ => {}
        }
    }
    Some(result)
}

fn observe_cpu(_dchu: &Dchu, _observe_ms: u64) {
    #[cfg(windows)]
    crate::boot_observation::run(_dchu, Duration::from_millis(_observe_ms));
}

pub(crate) fn log_line(message: &str) {
    let root = product_data_dir();
    if std::fs::create_dir_all(&root).is_err() {
        return;
    }
    let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("boot-agent.log"))
    else {
        return;
    };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or_default();
    let _ = writeln!(file, "{stamp} {message}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_defaults_are_conservative() {
        let profile = BootProfile::default();
        assert_eq!(profile.power, PowerMode::Performance);
        assert_eq!(profile.fan, FanMode::Auto);
    }
}
