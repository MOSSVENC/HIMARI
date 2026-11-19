#[cfg(windows)]
use std::time::{Duration, SystemTime};

use ram_control_center_protocol::PowerMode;

use crate::product::{legacy_data_dir, product_data_dir};

pub(crate) fn persist_power_mode(mode: PowerMode) {
    let root = product_data_dir();
    if std::fs::create_dir_all(&root).is_err() {
        return;
    }
    let path = root.join("boot-profile.ini");
    let initial = std::fs::read_to_string(&path)
        .or_else(|_| std::fs::read_to_string(legacy_data_dir().join("boot-profile.ini")))
        .unwrap_or_default();
    let mut lines = initial.lines().map(str::to_owned).collect::<Vec<_>>();
    let mut found_power = false;
    let mut found_fan = false;
    for line in &mut lines {
        let key = line.split('=').next().unwrap_or("").trim();
        if key == "power" {
            *line = format!("power={}", mode.as_u8());
            found_power = true;
        } else if key == "fan" {
            found_fan = true;
        }
    }
    if !found_power {
        lines.push(format!("power={}", mode.as_u8()));
    }
    if !found_fan {
        lines.push("fan=0".to_owned());
    }
    let text = format!("{}\n", lines.join("\n"));
    let _ = std::fs::write(path, text);
}

#[cfg(windows)]
pub(crate) fn system_uptime() -> Duration {
    use windows_sys::Win32::System::SystemInformation::GetTickCount64;
    // SAFETY: GetTickCount64 has no pointer preconditions or memory effects.
    Duration::from_millis(unsafe { GetTickCount64() })
}

#[cfg(windows)]
pub(crate) fn flexi_recovery_ready() -> bool {
    let root = product_data_dir();
    let marker = root.join("flexi-recovery.marker");
    let now = SystemTime::now();
    let marker_time = match std::fs::metadata(&marker).and_then(|meta| meta.modified()) {
        Ok(time) => time,
        Err(_) => {
            let _ = std::fs::create_dir_all(&root);
            let _ = std::fs::write(&marker, b"");
            return false;
        }
    };
    let boot_time = now.checked_sub(system_uptime()).unwrap_or(now);
    boot_time
        .duration_since(marker_time)
        .map(|elapsed| elapsed > Duration::from_secs(5))
        .unwrap_or(false)
}

#[cfg(not(windows))]
pub(crate) fn flexi_recovery_ready() -> bool {
    false
}
