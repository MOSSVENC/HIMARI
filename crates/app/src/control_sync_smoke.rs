//! Exercises control readbacks against the actual window property model.

use crate::MainWindow;
use crate::control_sync::{Control, ControlSync, Controls};
use crate::system_controls::{AudioState, DisplayCareState};
use crate::ui_update::{UiUpdate, apply_update};
use ram_control_center_protocol::{
    FanChannel, FlexiChargerState, GpuSwitchState, PlatformState, PowerMode, QuickToggleState,
    RgbColor, RgbKeyboardState, ThermalSample,
};

pub(crate) fn run() -> i32 {
    match verify() {
        Ok(()) => {
            println!(
                "control sync: stale readbacks, rapid edits, failed writes and independent draft preservation passed"
            );
            0
        }
        Err(error) => {
            eprintln!("control sync: {error}");
            1
        }
    }
}

fn verify() -> Result<(), String> {
    let ui = MainWindow::new().map_err(|error| error.to_string())?;
    let sync = ControlSync::default();
    let lighting = RgbKeyboardState {
        enabled: true,
        brightness: 3,
        effect: 8,
        color: RgbColor {
            red: 50,
            green: 60,
            blue: 70,
        },
        sleep_enabled: true,
        sleep_seconds: 30,
        ..RgbKeyboardState::default()
    };
    let sample = || UiUpdate {
        read_stamp: Some(sync.snapshot()),
        rgb_keyboard: Some(lighting),
        display_care: Some(DisplayCareState {
            monitor_ac_minutes: 10,
            monitor_dc_minutes: 5,
            ..DisplayCareState::default()
        }),
        audio: Some(AudioState {
            volume_percent: 40,
            muted: false,
        }),
        ..UiUpdate::default()
    };
    apply_update(&ui, sample());
    ui.set_rgb_requested_red(217);
    ui.set_rgb_requested_seconds(45);
    ui.set_monitor_ac_requested(23);
    ui.set_audio_volume_requested(72);

    let mut unrelated = sample();
    unrelated.quick_toggles.fn_lock = Some(true);
    apply_update(&ui, unrelated);
    ensure(
        ui.get_rgb_requested_red() == 217,
        "Fn readback overwrote unsaved RGB color",
    )?;
    ensure(
        ui.get_rgb_requested_seconds() == 45,
        "Fn readback overwrote unsaved sleep timer",
    )?;
    ensure(
        ui.get_monitor_ac_requested() == 23,
        "Fn readback overwrote monitor timeout draft",
    )?;
    ensure(
        ui.get_audio_volume_requested() == 72,
        "Fn readback overwrote volume draft",
    )?;

    drop(sync.issue(Controls::one(Control::LightingBrightness)));
    apply_update(&ui, sample());
    ensure(
        ui.get_rgb_requested_red() == 217,
        "brightness readback overwrote color draft",
    )?;
    ensure(
        ui.get_rgb_requested_brightness() == 3,
        "brightness confirmation was not applied",
    )?;

    let stale = sample();
    let ticket = sync.issue(Controls::one(Control::DisplayCare));
    ui.set_taskbar_auto_hide(true);
    apply_update(&ui, stale.clone());
    ensure(
        ui.get_taskbar_auto_hide(),
        "pending taskbar change was overwritten",
    )?;
    drop(ticket);
    apply_update(&ui, stale);
    ensure(
        ui.get_taskbar_auto_hide(),
        "completed taskbar change accepted an earlier snapshot",
    )?;
    // A fresh failed-write readback must restore actual state.
    apply_update(&ui, sample());
    ensure(
        !ui.get_taskbar_auto_hide(),
        "failed write did not restore fresh hardware state",
    )?;

    let first = sync.issue(Controls::one(Control::LightingSleep));
    ui.set_rgb_requested_sleep_enabled(false);
    let second = sync.issue(Controls::one(Control::LightingSleep));
    ui.set_rgb_requested_sleep_enabled(true);
    drop(first);
    let mut intermediate = sample();
    intermediate.rgb_keyboard.as_mut().unwrap().sleep_enabled = false;
    drop(second);
    apply_update(&ui, intermediate);
    ensure(
        ui.get_rgb_requested_sleep_enabled(),
        "older completion replaced latest sleep intent",
    )?;

    // An absent first sample must not consume initialization for that domain.
    let recovering = ControlSync::default();
    apply_update(
        &ui,
        UiUpdate {
            read_stamp: Some(recovering.snapshot()),
            ..UiUpdate::default()
        },
    );
    let mut recovered = sample();
    recovered.read_stamp = Some(recovering.snapshot());
    recovered.rgb_keyboard.as_mut().unwrap().color.red = 91;
    apply_update(&ui, recovered);
    ensure(
        ui.get_rgb_requested_red() == 91,
        "initial read failure discarded later valid RGB initialization",
    )?;

    ui.set_power_mode(i32::from(PowerMode::Entertainment.as_u8()));
    ui.set_toggle_fn_value(true);
    ui.set_toggle_fn_known(true);
    apply_update(
        &ui,
        UiUpdate {
            platform: PlatformState::default(),
            quick_toggles: QuickToggleState::default(),
            ..UiUpdate::default()
        },
    );
    ensure(
        ui.get_power_mode() == i32::from(PowerMode::Entertainment.as_u8()),
        "missing power read fabricated Performance",
    )?;
    ensure(
        ui.get_toggle_fn_value() && ui.get_toggle_fn_known(),
        "failed quick-toggle read cleared a confirmed value",
    )?;
    apply_update(
        &ui,
        UiUpdate {
            flexi_ready: true,
            flexi_charger: Some(FlexiChargerState {
                mode: 1,
                start_threshold: 70,
                stop_threshold: 80,
                threshold_choices: vec![70, 80],
            }),
            gpu_switch: Some(GpuSwitchState {
                current_mode: 3,
                supported_mask: 6,
            }),
            ..UiUpdate::default()
        },
    );
    let gpu_text = ui.get_gpu_switch_text();
    apply_update(
        &ui,
        UiUpdate {
            flexi_ready: true,
            ..UiUpdate::default()
        },
    );
    ensure(
        ui.get_flexi_ready() && ui.get_flexi_mode() == 1,
        "temporary charge read failure cleared the confirmed channel",
    )?;
    ensure(
        ui.get_gpu_switch_text() == gpu_text,
        "temporary GPU read failure cleared the confirmed mode",
    )?;
    apply_update(
        &ui,
        UiUpdate {
            thermal: Some(ThermalSample {
                gpu: FanChannel {
                    temperature_c: 49,
                    ..FanChannel::default()
                },
                ..ThermalSample::default()
            }),
            ..UiUpdate::default()
        },
    );
    ensure(
        ui.get_cpu_temp() == "--°C" && ui.get_cpu_rpm() == "--",
        "expired CPU channel displayed a zero sample",
    )?;
    ensure(
        ui.get_gpu_temp() == "49°C",
        "valid GPU channel was lost with an expired CPU channel",
    )?;
    Ok(())
}

fn ensure(condition: bool, message: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.to_owned())
    }
}
