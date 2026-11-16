use ram_control_center_protocol::{Dchu, QuickToggle};

use super::CommandOutcome;
use crate::input::send_virtual_key;
use crate::localization::{localized_toggle_label, tr};
use crate::system_controls;

pub(super) fn apply_camera(dchu: Option<&Dchu>, enabled: bool) -> CommandOutcome {
    let Some(active) = dchu else {
        return CommandOutcome::missing_transport();
    };
    CommandOutcome::from_result(
        active.set_camera_enabled(enabled),
        if enabled {
            tr("Camera enabled.")
        } else {
            tr("Camera disabled.")
        },
    )
}

pub(super) fn apply_touchpad(dchu: Option<&Dchu>, enabled: bool) -> CommandOutcome {
    let Some(active) = dchu else {
        return CommandOutcome::missing_transport();
    };
    CommandOutcome::from_result(
        system_controls::set_touchpad_enabled(active, enabled),
        if enabled {
            tr("Touchpad enabled.")
        } else {
            tr("Touchpad disabled.")
        },
    )
}

pub(super) fn apply_airplane(dchu: Option<&Dchu>, enabled: bool) -> CommandOutcome {
    match system_controls::set_airplane_mode(enabled) {
        Ok(()) => {
            if let Some(active) = dchu {
                let _ = active.notify_airplane_mode(enabled);
            }
            CommandOutcome::success(if enabled {
                tr("Airplane mode enabled.")
            } else {
                tr("Airplane mode disabled.")
            })
        }
        Err(error) => CommandOutcome::failure(error),
    }
}

pub(super) fn apply_quick_toggle(
    dchu: Option<&Dchu>,
    toggle: QuickToggle,
    enabled: bool,
) -> CommandOutcome {
    let Some(active) = dchu else {
        return CommandOutcome::missing_transport();
    };
    CommandOutcome::from_result(
        active.set_quick_toggle(toggle, enabled),
        format!(
            "{} {}{}",
            localized_toggle_label(toggle),
            tr("set to "),
            if enabled {
                tr("enabled")
            } else {
                tr("disabled")
            }
        ),
    )
}

pub(super) fn apply_lock_key(vk: u8) -> CommandOutcome {
    CommandOutcome::from_result(send_virtual_key(vk), tr("Lock key toggled."))
}
