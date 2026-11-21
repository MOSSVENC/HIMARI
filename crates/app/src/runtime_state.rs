#[cfg(windows)]
use std::time::{Duration, SystemTime};

use himari_protocol::PowerMode;

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

/// Parse the persisted `dark` flag out of `theme.ini` content.
///
/// Missing files, unknown keys and malformed values all fall back to `false`,
/// so a corrupt or partial file can never leave the UI in an undefined state.
pub(crate) fn parse_theme_dark(text: &str) -> Option<bool> {
    text.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        if key.trim() != "dark" {
            return None;
        }
        match value.trim() {
            "1" | "true" => Some(true),
            "0" | "false" => Some(false),
            _ => None,
        }
    })
}

pub(crate) fn theme_text(dark: bool) -> String {
    format!("dark={}\n", u8::from(dark))
}

pub(crate) fn theme_dark() -> bool {
    std::fs::read_to_string(product_data_dir().join("theme.ini"))
        .ok()
        .and_then(|text| parse_theme_dark(&text))
        .unwrap_or(false)
}

pub(crate) fn persist_theme_dark(dark: bool) {
    let root = product_data_dir();
    if std::fs::create_dir_all(&root).is_err() {
        return;
    }
    let _ = std::fs::write(root.join("theme.ini"), theme_text(dark));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_flag_round_trips_through_the_marker_text() {
        for dark in [true, false] {
            assert_eq!(parse_theme_dark(&theme_text(dark)), Some(dark));
        }
    }

    #[test]
    fn theme_parser_ignores_unknown_and_malformed_lines() {
        assert_eq!(parse_theme_dark(""), None);
        assert_eq!(parse_theme_dark("accent=cyan\n"), None);
        assert_eq!(parse_theme_dark("dark=maybe\n"), None);
        assert_eq!(parse_theme_dark("no-separator\n"), None);
        assert_eq!(parse_theme_dark("accent=cyan\ndark = 1\n"), Some(true));
        assert_eq!(parse_theme_dark("dark=false\n"), Some(false));
    }
}
