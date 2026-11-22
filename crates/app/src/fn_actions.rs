//! Hardware work an Fn event asks for, and the result text it produces.

#![forbid(unsafe_code)]
use himari_protocol::{Adjustment, Dchu, FnEvent, PowerMode};
use slint::ComponentHandle;

use crate::fn_intent::{OsdView, backlight_text, event_title, on_off, power_tiers, state_view};
use crate::localization::tr;
use crate::osd_glyph::Glyph;

pub(crate) fn execute(event: FnEvent) -> Result<Option<OsdView>, String> {
    if let Some(view) = state_view(event, fan_speeds()) {
        return Ok(Some(view));
    }
    let title = event_title(event).to_owned();
    match event {
        FnEvent::BacklightStep(direction) => {
            let (detail, level) = adjust_backlight(direction)?;
            Ok(Some(
                OsdView::new(title, detail, Glyph::Keyboard).with_level(level),
            ))
        }
        FnEvent::BacklightToggle => {
            let (detail, level) = toggle_backlight()?;
            Ok(Some(
                OsdView::new(title, detail, Glyph::Keyboard).with_level(level),
            ))
        }
        FnEvent::BacklightCycle => {
            let (detail, level) = cycle_backlight()?;
            Ok(Some(
                OsdView::new(title, detail, Glyph::Keyboard).with_level(level),
            ))
        }
        FnEvent::FnLockToggle => Ok(Some(OsdView::new(title, toggle_fn_lock()?, Glyph::Lock))),
        FnEvent::NoiselessFanToggle => Ok(Some(OsdView::new(
            title,
            toggle_noiseless_fan()?,
            Glyph::Fan,
        ))),
        FnEvent::PowerModeCycle => {
            let (detail, glyph) = cycle_power_mode()?;
            Ok(Some(OsdView::new(title, detail, glyph)))
        }
        FnEvent::ControlCenterToggle => {
            toggle_control_center()?;
            Ok(Some(OsdView::new(
                title,
                tr("Toggle window").to_owned(),
                Glyph::Window,
            )))
        }
        FnEvent::Camera(enabled) => {
            let device = open_dchu()?;
            device
                .set_camera_enabled(enabled)
                .map_err(|error| error.to_string())?;
            Ok(Some(OsdView::new(title, on_off(enabled), Glyph::Camera)))
        }
        FnEvent::TouchpadToggle { .. } => Ok(Some(OsdView::new(
            title,
            toggle_touchpad()?,
            Glyph::Touchpad,
        ))),
        FnEvent::ComboSwap(_) => Ok(Some(OsdView::new(
            title,
            toggle_combo_swap()?,
            Glyph::Keyboard,
        ))),
        _ => Ok(None),
    }
}

fn adjust_backlight(direction: Adjustment) -> Result<(String, f32), String> {
    let device = open_dchu()?;
    let state = device
        .read_rgb_keyboard_state()
        .map_err(|error| error.to_string())?;
    let target = match direction {
        Adjustment::Down => state.brightness.saturating_sub(1),
        Adjustment::Up => state.brightness.saturating_add(1).min(4),
    };
    device
        .set_rgb_keyboard_brightness(target)
        .map_err(|error| error.to_string())?;
    Ok((backlight_text(target), f32::from(target)))
}

fn toggle_backlight() -> Result<(String, f32), String> {
    let device = open_dchu()?;
    let state = device
        .read_rgb_keyboard_state()
        .map_err(|error| error.to_string())?;
    let target = if state.enabled {
        0
    } else {
        state.brightness.max(1)
    };
    device
        .set_rgb_keyboard_brightness(target)
        .map_err(|error| error.to_string())?;
    Ok((backlight_text(target), f32::from(target)))
}

fn cycle_backlight() -> Result<(String, f32), String> {
    let device = open_dchu()?;
    let state = device
        .read_rgb_keyboard_state()
        .map_err(|error| error.to_string())?;
    let target = if !state.enabled || state.brightness >= 4 {
        0
    } else {
        state.brightness + 1
    };
    device
        .set_rgb_keyboard_brightness(target)
        .map_err(|error| error.to_string())?;
    Ok((backlight_text(target), f32::from(target)))
}

fn toggle_fn_lock() -> Result<String, String> {
    let device = open_dchu()?;
    let state = device.read_quick_toggles();
    let current = state
        .fn_lock
        .ok_or_else(|| tr("Fn Lock state is unavailable").to_owned())?;
    device
        .set_quick_toggle(himari_protocol::QuickToggle::FnLock, !current)
        .map_err(|error| error.to_string())?;
    Ok(if current {
        tr("Off").to_owned()
    } else {
        tr("On").to_owned()
    })
}

fn toggle_noiseless_fan() -> Result<String, String> {
    let device = open_dchu()?;
    let state = device.read_platform_state();
    let current = state
        .slow_fan
        .ok_or_else(|| tr("Noiseless fan state is unavailable").to_owned())?;
    device
        .set_slow_fan(!current)
        .map_err(|error| error.to_string())?;
    Ok(if current {
        tr("Off").to_owned()
    } else {
        tr("On").to_owned()
    })
}

fn cycle_power_mode() -> Result<(String, Glyph), String> {
    let device = open_dchu()?;
    let capabilities = device
        .read_capabilities()
        .map_err(|error| error.to_string())?;
    let tiers = power_tiers(&capabilities);
    if tiers.is_empty() {
        return Err(tr("No power profiles are supported").to_owned());
    }
    let current_mode = device
        .read_setting_byte(1, 1)
        .and_then(PowerMode::from_u8)
        .ok_or_else(|| tr("Power mode is unavailable").to_owned())?;
    let discrete_gpu_off = crate::gpu_power::is_disabled();
    let current = tiers
        .iter()
        .position(|tier| {
            tier.powers_down_discrete_gpu() == discrete_gpu_off
                && (!tier.powers_down_discrete_gpu() || tier.profile() == current_mode)
                && (tier.powers_down_discrete_gpu() || tier.profile() == current_mode)
        })
        .unwrap_or_else(|| {
            tiers
                .iter()
                .position(|tier| !tier.powers_down_discrete_gpu() && tier.profile() == current_mode)
                .unwrap_or(0)
        });
    let next = tiers[(current + 1) % tiers.len()];

    // Leave the previous tier cleanly: bring the discrete GPU back before the
    // next profile is applied when the cycle moves away from the quiet tier.
    if !next.powers_down_discrete_gpu() && discrete_gpu_off {
        crate::gpu_power::request(true)?;
    }
    device
        .set_power_mode(next.profile())
        .map_err(|error| error.to_string())?;
    crate::runtime_state::persist_power_mode(next.profile());

    let mut detail = next.label().to_owned();
    if next.powers_down_discrete_gpu() {
        crate::gpu_power::request(false)?;
        detail = format!("{detail} · {}", tr("discrete GPU powered off"));
    } else if discrete_gpu_off {
        detail = format!("{detail} · {}", tr("discrete GPU restored"));
    }
    let glyph = next.glyph();
    Ok((detail, glyph))
}

/// Current CPU and GPU fan speeds, from the thermal mailbox.
///
/// The Fn fan notice reports both fans because the platform runs two and a
/// single figure would hide which one moved.
fn fan_speeds() -> Option<(u32, u32)> {
    let sample = open_dchu().ok()?.read_thermal().ok()?;
    if !sample.has_plausible_temperature() {
        return None;
    }
    Some((sample.cpu.rpm, sample.gpu.rpm))
}

/// Flip the Win/Fn key swap and report the state it landed in.
///
/// The vendor handler for the same notification reads the current swap state
/// and shows the opposite, so the action and the feedback agree.
fn toggle_combo_swap() -> Result<String, String> {
    let device = open_dchu()?;
    let current = device
        .read_quick_toggles()
        .swap_win_fn
        .ok_or_else(|| tr("Keys are locked").to_owned())?;
    device
        .set_quick_toggle(himari_protocol::QuickToggle::SwapWinFn, !current)
        .map_err(|error| error.to_string())?;
    Ok(if current {
        tr("Off").to_owned()
    } else {
        tr("On").to_owned()
    })
}

fn toggle_touchpad() -> Result<String, String> {
    let device = open_dchu()?;
    let current = crate::system_controls::touchpad_enabled()?;
    crate::system_controls::set_touchpad_enabled(&device, !current)?;
    Ok(if current {
        tr("Off").to_owned()
    } else {
        tr("On").to_owned()
    })
}

fn toggle_control_center() -> Result<(), String> {
    let weak = crate::fn_owner::main_window()
        .ok_or_else(|| tr("Main window is unavailable").to_owned())?;
    slint::invoke_from_event_loop(move || {
        if let Some(window) = weak.upgrade() {
            if window.window().is_visible() {
                window.hide().ok();
            } else {
                window.show().ok();
            }
        }
    })
    .map_err(|error| error.to_string())
}

fn open_dchu() -> Result<Dchu, String> {
    Dchu::open().map_err(|error| error.to_string())
}

pub(crate) fn log_line(message: &str) {
    use std::io::Write as _;

    crate::diagnostics::log_action(message);
    let root = crate::product::product_data_dir();
    if std::fs::create_dir_all(&root).is_err() {
        return;
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("fn-owner.log"))
    {
        let _ = writeln!(file, "{message}");
    }
}
