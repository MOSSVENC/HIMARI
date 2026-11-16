use super::UiUpdate;
use crate::{
    MainWindow,
    control_sync::{Control, Controls},
};

pub(super) fn apply(ui: &MainWindow, update: &UiUpdate, accepted: Controls, sync: Controls) {
    if let Some(care) = update.display_care {
        ui.set_display_care_available(true);
        if accepted.contains(Control::DisplayCare) {
            ui.set_taskbar_auto_hide(care.taskbar_auto_hide);
            ui.set_taskbar_transparency(care.taskbar_transparency);
            ui.set_display_dark_mode(care.dark_mode);
        }
        if accepted.contains(Control::MonitorTimeout) {
            ui.set_monitor_ac_minutes(care.monitor_ac_minutes as i32);
            ui.set_monitor_dc_minutes(care.monitor_dc_minutes as i32);
            if sync.contains(Control::MonitorTimeout) {
                ui.set_monitor_ac_requested(care.monitor_ac_minutes as i32);
                ui.set_monitor_dc_requested(care.monitor_dc_minutes as i32);
            }
        }
        if accepted.contains(Control::ScreenSaver) {
            ui.set_screen_saver_enabled(care.screen_saver_enabled);
            ui.set_screen_saver_type(i32::from(care.screen_saver_type));
            ui.set_screen_saver_minutes(care.screen_saver_minutes as i32);
            if sync.contains(Control::ScreenSaver) {
                ui.set_screen_saver_requested_enabled(care.screen_saver_enabled);
                ui.set_screen_saver_requested_type(i32::from(care.screen_saver_type));
                ui.set_screen_saver_requested_minutes(care.screen_saver_minutes as i32);
            }
        }
    }
    if let Some(enabled) = update.camera_enabled
        && accepted.contains(Control::Camera)
    {
        ui.set_camera_enabled(enabled);
        ui.set_camera_known(true);
    }
    if let Some(enabled) = update.touchpad_enabled
        && accepted.contains(Control::Touchpad)
    {
        ui.set_touchpad_enabled(enabled);
        ui.set_touchpad_known(true);
    }
    if let Some(enabled) = update.airplane_enabled
        && accepted.contains(Control::Airplane)
    {
        ui.set_airplane_enabled(enabled);
        ui.set_airplane_known(true);
    }
    if let Some(brightness) = update.brightness
        && accepted.contains(Control::Brightness)
    {
        ui.set_display_brightness(i32::from(brightness));
        if sync.contains(Control::Brightness) {
            ui.set_display_brightness_requested(i32::from(brightness));
        }
    }
    if let Some(blue_light) = update.blue_light
        && accepted.contains(Control::BlueLight)
    {
        ui.set_blue_light_level(i32::from(blue_light));
        if sync.contains(Control::BlueLight) {
            ui.set_blue_light_requested(i32::from(blue_light));
        }
    }
    if let Some(audio) = update.audio {
        ui.set_audio_known(true);
        if accepted.contains(Control::Audio) {
            ui.set_audio_volume(i32::from(audio.volume_percent));
            if sync.contains(Control::Audio) {
                ui.set_audio_volume_requested(i32::from(audio.volume_percent));
            }
        }
        if accepted.contains(Control::AudioMute) {
            ui.set_audio_muted(audio.muted);
        }
    }
}
