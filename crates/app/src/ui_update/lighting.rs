use super::UiUpdate;
use crate::{
    MainWindow,
    control_sync::{Control, Controls},
};

pub(super) fn apply(ui: &MainWindow, update: &UiUpdate, accepted: Controls, sync: Controls) {
    if let Some(keyboard) = update.rgb_keyboard {
        let display_brightness = if keyboard.enabled {
            keyboard.brightness
        } else {
            0
        };
        if accepted.contains(Control::LightingBrightness) {
            ui.set_rgb_enabled(keyboard.enabled);
            ui.set_rgb_brightness(i32::from(display_brightness));
            if sync.contains(Control::LightingBrightness) {
                ui.set_rgb_requested_brightness(i32::from(display_brightness));
            }
        }
        if accepted.contains(Control::Lighting) {
            ui.set_rgb_red(i32::from(keyboard.color.red));
            ui.set_rgb_green(i32::from(keyboard.color.green));
            ui.set_rgb_blue(i32::from(keyboard.color.blue));
            if sync.contains(Control::Lighting) {
                ui.set_rgb_requested_red(i32::from(keyboard.color.red));
                ui.set_rgb_requested_green(i32::from(keyboard.color.green));
                ui.set_rgb_requested_blue(i32::from(keyboard.color.blue));
            }
        }
        if accepted.contains(Control::LightingEffect) {
            ui.set_rgb_effect(i32::from(keyboard.effect));
            if sync.contains(Control::LightingEffect) {
                ui.set_rgb_requested_effect(i32::from(keyboard.effect));
            }
        }
        if accepted.contains(Control::LightingBoot) {
            ui.set_rgb_boot_effect(keyboard.boot_effect);
            if sync.contains(Control::LightingBoot) {
                ui.set_rgb_requested_boot_effect(keyboard.boot_effect);
            }
        }
        if accepted.contains(Control::LightingSleep) {
            ui.set_rgb_sleep_enabled(keyboard.sleep_enabled);
            ui.set_rgb_sleep_hours((keyboard.sleep_seconds / 3600) as i32);
            ui.set_rgb_sleep_minutes(((keyboard.sleep_seconds / 60) % 60) as i32);
            ui.set_rgb_sleep_seconds((keyboard.sleep_seconds % 60) as i32);
            if sync.contains(Control::LightingSleep) {
                ui.set_rgb_requested_sleep_enabled(keyboard.sleep_enabled);
                ui.set_rgb_requested_hours((keyboard.sleep_seconds / 3600) as i32);
                ui.set_rgb_requested_minutes(((keyboard.sleep_seconds / 60) % 60) as i32);
                ui.set_rgb_requested_seconds((keyboard.sleep_seconds % 60) as i32);
            }
        }
    }
}
