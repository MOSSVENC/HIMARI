use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use ram_control_center_protocol::{
    FlexiKeyAction, FlexiKeyBinding, FlexiKeyDevice, FlexiKeyProfile, FlexiKeyStore,
};
use slint::{ComponentHandle, VecModel, Weak};

use crate::MainWindow;
use crate::diagnostics::log_action;
use crate::flexikey_engine::{self, FlexiKeyEngine};
use crate::flexikey_jobs::FlexiKeyJobs;
pub(crate) use crate::flexikey_store::FlexiKeyViewState;
use crate::flexikey_store::flexikey_profile_path;
use crate::localization::tr;

fn active_profile_position(store: &FlexiKeyStore) -> Option<usize> {
    store.profiles.iter().position(|profile| profile.selected)
}

fn parse_trigger_key_code(value: &str) -> Result<u16, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(tr("Enter or capture a trigger key").to_owned());
    }
    let parsed = if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u16::from_str_radix(hex, 16)
    } else {
        value.parse::<u16>()
    };
    parsed.map_err(|_| format!("{}: {value}", tr("Invalid 16-bit key code")))
}

fn mouse_trigger_name(key_code: u16) -> String {
    let name = match key_code {
        0x01 => tr("Mouse Left"),
        0x04 => tr("Mouse Right"),
        0x10 => tr("Mouse Middle"),
        0x40 => tr("Mouse Back"),
        0x100 => tr("Mouse Forward"),
        _ => tr("Mouse Button"),
    };
    name.to_owned()
}

fn editor_action(
    existing: Option<&FlexiKeyAction>,
    action_type: i32,
    value: &str,
) -> Result<FlexiKeyAction, String> {
    match action_type {
        1 => {
            if value.trim().is_empty() {
                if let Some(action @ FlexiKeyAction::Keypress { .. }) = existing {
                    return Ok(action.clone());
                }
                return Err(tr("Enter a hotkey sequence for the new binding").to_owned());
            }
            Ok(FlexiKeyAction::Keypress {
                steps: flexikey_engine::hotkey_steps(value)?,
            })
        }
        2 => {
            let app_path = value.trim();
            if app_path.is_empty() {
                return Err(tr("Enter a launch target").to_owned());
            }
            let event_id = match existing {
                Some(FlexiKeyAction::Launch { event_id, .. }) => *event_id,
                _ => 0,
            };
            Ok(FlexiKeyAction::Launch {
                app_path: app_path.to_owned(),
                event_id,
            })
        }
        3 => {
            let (start_keys, send_keys) = match existing {
                Some(FlexiKeyAction::Text {
                    start_keys,
                    send_keys,
                    ..
                }) => (start_keys.clone(), send_keys.clone()),
                _ => (Vec::new(), Vec::new()),
            };
            Ok(FlexiKeyAction::Text {
                start_keys,
                text: value.to_owned(),
                send_keys,
            })
        }
        4 => Ok(FlexiKeyAction::Disable),
        _ => Err(tr("Unknown action type").to_owned()),
    }
}

fn editor_binding(
    profile: &FlexiKeyProfile,
    binding_index: i32,
    device: i32,
    key_code: &str,
    key_name: &str,
    action_type: i32,
    value: &str,
) -> Result<FlexiKeyBinding, String> {
    let device = match device {
        1 => FlexiKeyDevice::Keyboard,
        2 => FlexiKeyDevice::Mouse,
        _ => return Err(tr("Choose a trigger device").to_owned()),
    };
    let key_code = parse_trigger_key_code(key_code)?;
    if device == FlexiKeyDevice::Mouse && !matches!(key_code, 0x01 | 0x04 | 0x10 | 0x40 | 0x100) {
        return Err(tr("Mouse triggers must be left, right, middle, back or forward").to_owned());
    }
    let existing = usize::try_from(binding_index)
        .ok()
        .and_then(|index| profile.bindings.get(index));
    let display = if key_name.trim().is_empty() {
        if device == FlexiKeyDevice::Mouse {
            mouse_trigger_name(key_code)
        } else {
            format!("Key 0x{key_code:04X}")
        }
    } else {
        key_name.trim().to_owned()
    };
    Ok(FlexiKeyBinding {
        device,
        key_code: i64::from(key_code),
        key_name: display.clone(),
        key_display: display,
        tip: existing
            .map(|binding| binding.tip.clone())
            .unwrap_or_default(),
        checkbox_name: existing
            .map(|binding| binding.checkbox_name.clone())
            .unwrap_or_default(),
        is_keypress_delay: existing
            .map(|binding| binding.is_keypress_delay)
            .unwrap_or(false),
        action: editor_action(existing.map(|binding| &binding.action), action_type, value)?,
    })
}

fn format_binding_label(index: usize, binding: &FlexiKeyBinding) -> String {
    let device = match binding.device {
        FlexiKeyDevice::Keyboard => tr("Keyboard"),
        FlexiKeyDevice::Mouse => tr("Mouse"),
        FlexiKeyDevice::None => "---",
    };
    let trigger = if binding.key_display.trim().is_empty() {
        if binding.device == FlexiKeyDevice::Mouse {
            mouse_trigger_name(binding.key_code as u16)
        } else {
            "Key".to_owned()
        }
    } else {
        binding.key_display.trim().to_owned()
    };
    let action = match &binding.action {
        FlexiKeyAction::Keypress { steps } => {
            format!("{} ({} {})", tr("Hotkey"), steps.len(), tr("steps"))
        }
        FlexiKeyAction::Launch { app_path, .. } => {
            format!("{} {}", tr("Launch"), app_path)
        }
        FlexiKeyAction::Text { text, .. } => {
            let preview = text.chars().take(36).collect::<String>();
            if text.chars().count() > 36 {
                format!("{} \"{preview}…\"", tr("Text"))
            } else {
                format!("{} \"{preview}\"", tr("Text"))
            }
        }
        FlexiKeyAction::Disable => tr("Disabled trigger").to_owned(),
    };
    format!("{:02}  {device}  {trigger:<14}  →  {action}", index + 1)
}

fn start_flexikey_capture_poll(
    weak: Weak<MainWindow>,
    engine: FlexiKeyEngine,
    jobs: Rc<FlexiKeyJobs>,
    epoch: u64,
) {
    slint::Timer::single_shot(Duration::from_millis(50), move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if !ui.get_flexikey_capture_armed() || !jobs.capture_is_current(epoch) {
            return;
        }
        if let Some(trigger) = engine.take_captured_trigger() {
            ui.set_flexikey_editor_device(i32::from(trigger.device));
            ui.set_flexikey_editor_key_code(format!("0x{:04X}", trigger.key_code).into());
            let name = if trigger.device == FlexiKeyDevice::Mouse.as_u8() {
                mouse_trigger_name(trigger.key_code)
            } else {
                trigger.name
            };
            ui.set_flexikey_editor_key_name(name.into());
            ui.set_flexikey_capture_armed(false);
            ui.set_flexikey_editor_status(
                tr("Trigger captured. Choose the action and save.").into(),
            );
        } else {
            start_flexikey_capture_poll(ui.as_weak(), engine, jobs, epoch);
        }
    });
}

fn submit_store(
    ui: &MainWindow,
    state: &Rc<RefCell<FlexiKeyViewState>>,
    jobs: &FlexiKeyJobs,
    next: FlexiKeyStore,
    notice: &str,
    activated: Option<usize>,
) -> bool {
    let accepted = match jobs.save(next.clone(), notice.to_owned(), activated) {
        Ok(()) => {
            let mut state = state.borrow_mut();
            state.store = next;
            state.operation_error = None;
            state.operation_notice = None;
            state.activated_index = None;
            state.engine_report = None;
            true
        }
        Err(error) => {
            state.borrow_mut().operation_error = Some(error);
            false
        }
    };
    render_flexikey(ui, &state.borrow());
    accepted
}

pub(crate) fn wire_flexikey(
    ui: &MainWindow,
    engine: FlexiKeyEngine,
) -> Rc<RefCell<FlexiKeyViewState>> {
    wire_flexikey_at(ui, engine, flexikey_profile_path())
}

pub(crate) fn wire_flexikey_at(
    ui: &MainWindow,
    engine: FlexiKeyEngine,
    path: std::path::PathBuf,
) -> Rc<RefCell<FlexiKeyViewState>> {
    let state = Rc::new(RefCell::new(crate::flexikey_jobs::empty_state(
        path.clone(),
    )));
    let weak = ui.as_weak();
    let jobs = match FlexiKeyJobs::start(engine.clone(), path, move || {
        let weak = weak.clone();
        if let Err(error) = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                ui.invoke_flexikey_worker_ready();
            }
        }) {
            log_action(&format!("FlexiKey UI dispatch failed: {error}"));
        }
    }) {
        Ok(jobs) => Rc::new(jobs),
        Err(error) => {
            state.borrow_mut().engine_error = Some(error);
            render_flexikey(ui, &state.borrow());
            return state;
        }
    };

    let weak = ui.as_weak();
    let completed_state = Rc::clone(&state);
    let completed_jobs = Rc::clone(&jobs);
    ui.on_flexikey_worker_ready(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let replies = completed_jobs.take_replies();
        if let Some((id, confirmed)) = replies.profile
            && completed_jobs.is_current(id)
        {
            *completed_state.borrow_mut() = confirmed;
            render_flexikey(&ui, &completed_state.borrow());
        }
        if let Some((epoch, Err(error))) = replies.capture
            && completed_jobs.capture_is_current(epoch)
        {
            ui.set_flexikey_capture_armed(false);
            ui.set_flexikey_editor_status(error.into());
        }
    });

    let weak = ui.as_weak();
    let select_state = Rc::clone(&state);
    let select_jobs = Rc::clone(&jobs);
    ui.on_select_flexikey_profile(move |index| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        ui.set_flexikey_delete_armed(false);
        if select_state
            .borrow()
            .store
            .active()
            .map(|profile| profile.index)
            == Some(index)
        {
            return;
        }
        let mut next = select_state.borrow().store.clone();
        if next.select_by_profile_index(index) {
            submit_store(&ui, &select_state, &select_jobs, next, "", Some(index));
        }
    });

    let weak = ui.as_weak();
    let new_state = Rc::clone(&state);
    let new_jobs = Rc::clone(&jobs);
    ui.on_new_flexikey_profile(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let mut next = new_state.borrow().store.clone();
        next.add_profile(format!(
            "{} {}",
            tr("Profile"),
            next.profiles.len().saturating_add(1)
        ));
        if submit_store(
            &ui,
            &new_state,
            &new_jobs,
            next,
            tr("A new FlexiKey profile was created and activated."),
            None,
        ) {
            ui.set_flexikey_delete_armed(false);
        }
    });

    let weak = ui.as_weak();
    let rename_state = Rc::clone(&state);
    let rename_jobs = Rc::clone(&jobs);
    ui.on_rename_flexikey_profile(move |name| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let mut next = rename_state.borrow().store.clone();
        let Some(position) = active_profile_position(&next) else {
            return;
        };
        if !next.rename_profile(position, name.as_str()) {
            rename_state.borrow_mut().operation_error =
                Some(tr("Profile name cannot be empty.").to_owned());
            render_flexikey(&ui, &rename_state.borrow());
            return;
        }
        if submit_store(
            &ui,
            &rename_state,
            &rename_jobs,
            next,
            tr("FlexiKey profile name saved."),
            None,
        ) {
            ui.set_flexikey_delete_armed(false);
        }
    });

    let weak = ui.as_weak();
    let delete_state = Rc::clone(&state);
    let delete_jobs = Rc::clone(&jobs);
    ui.on_delete_flexikey_profile(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let mut next = delete_state.borrow().store.clone();
        let Some(position) = active_profile_position(&next) else {
            return;
        };
        if !next.remove_profile(position) {
            delete_state.borrow_mut().operation_error =
                Some(tr("At least one FlexiKey profile must remain.").to_owned());
            render_flexikey(&ui, &delete_state.borrow());
            return;
        }
        if submit_store(
            &ui,
            &delete_state,
            &delete_jobs,
            next,
            tr("FlexiKey profile deleted and the next profile activated."),
            None,
        ) {
            ui.set_flexikey_delete_armed(false);
        }
    });

    let weak = ui.as_weak();
    let devices_state = Rc::clone(&state);
    let devices_jobs = Rc::clone(&jobs);
    ui.on_set_flexikey_device_enabled(move |device, enabled| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let mut next = devices_state.borrow().store.clone();
        let Some(position) = active_profile_position(&next) else {
            return;
        };
        let profile = &mut next.profiles[position];
        match device {
            1 => profile.keyboard_enabled = enabled,
            2 => profile.mouse_enabled = enabled,
            _ => return,
        }
        submit_store(
            &ui,
            &devices_state,
            &devices_jobs,
            next,
            tr("FlexiKey trigger devices saved and reloaded."),
            None,
        );
    });

    let weak = ui.as_weak();
    let open_state = Rc::clone(&state);
    let open_jobs = Rc::clone(&jobs);
    ui.on_open_flexikey_editor(move |binding_index| {
        open_jobs.cancel_capture();
        let state = open_state.borrow();
        let Some(profile) = state.store.active() else {
            return;
        };
        let binding = usize::try_from(binding_index)
            .ok()
            .and_then(|index| profile.bindings.get(index));
        if let Some(ui) = weak.upgrade() {
            if let Some(binding) = binding {
                ui.set_flexikey_editor_binding_index(binding_index);
                ui.set_flexikey_editor_device(i32::from(binding.device.as_u8()));
                ui.set_flexikey_editor_key_code(format!("0x{:04X}", binding.key_code).into());
                ui.set_flexikey_editor_key_name(binding.key_display.clone().into());
                ui.set_flexikey_editor_action(i32::from(binding.action.type_id()));
                ui.set_flexikey_editor_value(
                    match &binding.action {
                        FlexiKeyAction::Launch { app_path, .. } => app_path.clone(),
                        FlexiKeyAction::Text { text, .. } => text.clone(),
                        _ => String::new(),
                    }
                    .into(),
                );
            } else {
                ui.set_flexikey_editor_binding_index(-1);
                ui.set_flexikey_editor_device(1);
                ui.set_flexikey_editor_key_code("".into());
                ui.set_flexikey_editor_key_name("".into());
                ui.set_flexikey_editor_action(1);
                ui.set_flexikey_editor_value("".into());
            }
            ui.set_flexikey_editor_status("".into());
            ui.set_flexikey_capture_armed(false);
            ui.set_flexikey_editor_open(true);
        }
    });

    let weak = ui.as_weak();
    let close_jobs = Rc::clone(&jobs);
    ui.on_close_flexikey_editor(move || {
        close_jobs.cancel_capture();
        if let Some(ui) = weak.upgrade() {
            ui.set_flexikey_capture_armed(false);
            ui.set_flexikey_editor_status("".into());
            ui.set_flexikey_editor_open(false);
        }
    });

    let weak = ui.as_weak();
    let capture_jobs = Rc::clone(&jobs);
    ui.on_capture_flexikey_trigger(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        match capture_jobs.arm_capture() {
            Ok(epoch) => {
                ui.set_flexikey_editor_status(tr("Listening for one key or mouse button…").into());
                ui.set_flexikey_capture_armed(true);
                start_flexikey_capture_poll(
                    ui.as_weak(),
                    engine.clone(),
                    Rc::clone(&capture_jobs),
                    epoch,
                );
            }
            Err(error) => {
                ui.set_flexikey_editor_status(error.into());
                ui.set_flexikey_capture_armed(false);
            }
        }
    });

    let weak = ui.as_weak();
    let cancel_jobs = Rc::clone(&jobs);
    ui.on_cancel_flexikey_capture(move || {
        cancel_jobs.cancel_capture();
        if let Some(ui) = weak.upgrade() {
            ui.set_flexikey_capture_armed(false);
            ui.set_flexikey_editor_status(tr("Capture canceled").into());
        }
    });

    let weak = ui.as_weak();
    let save_state = Rc::clone(&state);
    let save_jobs = Rc::clone(&jobs);
    ui.on_save_flexikey_editor(
        move |binding_index, device, key_code, key_name, action_type, value| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let mut next = save_state.borrow().store.clone();
            let Some(position) = active_profile_position(&next) else {
                return;
            };
            let binding = match editor_binding(
                &next.profiles[position],
                binding_index,
                device,
                key_code.as_str(),
                key_name.as_str(),
                action_type,
                value.as_str(),
            ) {
                Ok(binding) => binding,
                Err(error) => {
                    ui.set_flexikey_editor_status(error.into());
                    return;
                }
            };
            let changed = if let Ok(index) = usize::try_from(binding_index) {
                next.replace_binding(position, index, binding)
            } else {
                next.add_binding(position, binding)
            };
            if !changed {
                ui.set_flexikey_editor_status(tr("The selected binding no longer exists").into());
                return;
            }
            if submit_store(
                &ui,
                &save_state,
                &save_jobs,
                next,
                tr("Binding saved to profile.xml and loaded into the input engine."),
                None,
            ) {
                save_jobs.cancel_capture();
                ui.set_flexikey_capture_armed(false);
                ui.set_flexikey_editor_status("".into());
                ui.set_flexikey_editor_open(false);
            }
        },
    );

    let weak = ui.as_weak();
    let binding_state = Rc::clone(&state);
    let binding_jobs = Rc::clone(&jobs);
    ui.on_delete_flexikey_binding(move |binding_index| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let Ok(index) = usize::try_from(binding_index) else {
            return;
        };
        let mut next = binding_state.borrow().store.clone();
        let Some(position) = active_profile_position(&next) else {
            return;
        };
        if !next.remove_binding(position, index) {
            return;
        }
        if submit_store(
            &ui,
            &binding_state,
            &binding_jobs,
            next,
            tr("Binding removed from profile.xml and the input engine."),
            None,
        ) {
            binding_jobs.cancel_capture();
            ui.set_flexikey_capture_armed(false);
            ui.set_flexikey_editor_open(false);
            ui.set_flexikey_editor_status("".into());
        }
    });

    let weak = ui.as_weak();
    let reload_state = Rc::clone(&state);
    let reload_jobs = Rc::clone(&jobs);
    ui.on_reload_flexikey(move || {
        if let Some(ui) = weak.upgrade() {
            if let Err(error) = reload_jobs.reload() {
                reload_state.borrow_mut().operation_error = Some(error);
            } else {
                reload_state.borrow_mut().engine_report = None;
            }
            ui.set_flexikey_capture_armed(false);
            ui.set_flexikey_editor_open(false);
            render_flexikey(&ui, &reload_state.borrow());
        }
    });
    if let Err(error) = jobs.reload() {
        state.borrow_mut().operation_error = Some(error);
    }
    render_flexikey(ui, &state.borrow());
    state
}

fn flexikey_visible_profile(state: &FlexiKeyViewState) -> Option<&FlexiKeyProfile> {
    state.store.active()
}

fn flexikey_engine_status(state: &FlexiKeyViewState) -> String {
    if let Some(error) = &state.engine_error {
        return format!("{}: {error}", tr("Action engine unavailable"));
    }
    let Some(report) = &state.engine_report else {
        return String::new();
    };
    format!(
        "{} · {} {} · {} {}",
        if report.keyboard_actions == 0 && report.mouse_actions == 0 {
            tr("Input hooks idle")
        } else {
            tr("Input hooks active")
        },
        report.keyboard_actions,
        tr("keyboard action(s)"),
        report.mouse_actions,
        tr("mouse action(s)")
    )
}

pub(crate) fn render_flexikey(ui: &MainWindow, state: &FlexiKeyViewState) {
    let names = state
        .store
        .profiles
        .iter()
        .map(|profile| {
            slint::SharedString::from(format!("{:02}  {}", profile.index + 1, profile.name))
        })
        .collect::<Vec<_>>();
    ui.set_flexikey_profile_names(Rc::new(VecModel::from(names)).into());
    ui.set_flexikey_path(state.path.to_string_lossy().to_string().into());
    let visible_profile = flexikey_visible_profile(state);
    ui.set_flexikey_active_index(
        visible_profile
            .map(|profile| profile.index as i32)
            .unwrap_or(-1),
    );
    ui.set_flexikey_engine_status(flexikey_engine_status(state).into());

    if let Some(profile) = visible_profile {
        let bindings = profile
            .bindings
            .iter()
            .enumerate()
            .map(|(index, binding)| format_binding_label(index, binding).into())
            .collect::<Vec<slint::SharedString>>();
        ui.set_flexikey_binding_labels(Rc::new(VecModel::from(bindings)).into());
        ui.set_flexikey_keyboard_enabled(profile.keyboard_enabled);
        ui.set_flexikey_mouse_enabled(profile.mouse_enabled);
        ui.set_flexikey_profile_name(profile.name.clone().into());
        ui.set_flexikey_active_title(profile.name.clone().into());
        ui.set_flexikey_summary(format_flexikey_profile(profile).into());
    } else {
        ui.set_flexikey_binding_labels(
            Rc::new(VecModel::from(Vec::<slint::SharedString>::new())).into(),
        );
        ui.set_flexikey_keyboard_enabled(false);
        ui.set_flexikey_mouse_enabled(false);
        ui.set_flexikey_profile_name("".into());
        ui.set_flexikey_active_title("FlexiKey".into());
        ui.set_flexikey_summary(tr("No profiles are available.").into());
    }

    let status = if let Some(error) = &state.operation_error {
        error.clone()
    } else if let Some(notice) = &state.operation_notice {
        notice.clone()
    } else if let Some(profile) = state.activated_index.and_then(|index| {
        state
            .store
            .profiles
            .iter()
            .find(|profile| profile.index == index)
    }) {
        format!(
            "{} {} · {}",
            tr("Activated and saved"),
            profile.name,
            tr("action engine reloaded")
        )
    } else if let Some(error) = &state.load_error {
        format!("{} {error}", tr("Profile XML unavailable:"))
    } else {
        let count = state.store.profiles.len();
        format!(
            "{} {count} {}",
            tr("Loaded"),
            tr("profile(s) from CC4.0 XML")
        )
    };
    ui.set_flexikey_status(status.into());
}

fn format_flexikey_profile(profile: &FlexiKeyProfile) -> String {
    format!(
        "{} {} · {} {} · {} {}\n{} {}",
        profile.bindings.len(),
        tr("binding(s)"),
        tr("keyboard"),
        if profile.keyboard_enabled {
            tr("enabled")
        } else {
            tr("disabled")
        },
        tr("mouse"),
        if profile.mouse_enabled {
            tr("enabled")
        } else {
            tr("disabled")
        },
        tr("Device mode:"),
        profile.current_device
    )
}

#[cfg(test)]
mod flexikey_tests {
    use super::*;
    use crate::flexikey_store::{load_flexikey_view, save_flexikey_store};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn temp_profile_path(label: &str) -> PathBuf {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "neo-flexikey-{label}-{}-{nanos}-{sequence}.xml",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn selected_profile_is_saved_and_reloaded() {
        let path = temp_profile_path("save-test");
        let xml = r#"<root>
            <Profile_0 Name="Gaming" IsSelected="1"/>
            <Profile_1 Name="Work" IsSelected="0"/>
        </root>"#;
        let mut store = FlexiKeyStore::from_xml(xml).expect("initial store");
        assert!(store.select(1));
        save_flexikey_store(&path, &store).expect("profile save");
        assert!(!path.with_extension("xml.neo.tmp").exists());
        let saved = std::fs::read_to_string(&path).expect("saved XML");
        let loaded = FlexiKeyStore::from_xml(&saved).expect("saved XML must parse");
        assert_eq!(
            loaded.active().map(|profile| profile.name.as_str()),
            Some("Work")
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn editor_trigger_codes_accept_decimal_and_hex() {
        assert_eq!(parse_trigger_key_code("0xE053"), Ok(0xE053));
        assert_eq!(parse_trigger_key_code("64"), Ok(64));
        assert!(parse_trigger_key_code("").is_err());
        assert!(parse_trigger_key_code("not-a-key").is_err());
    }

    #[test]
    fn editing_launch_preserves_vendor_event_id() {
        let existing = FlexiKeyAction::Launch {
            app_path: r"C:\old.exe".to_owned(),
            event_id: 42,
        };
        let updated = editor_action(Some(&existing), 2, r"C:\new.exe").expect("launch update");
        assert_eq!(
            updated,
            FlexiKeyAction::Launch {
                app_path: r"C:\new.exe".to_owned(),
                event_id: 42,
            }
        );
    }

    #[test]
    fn editing_text_preserves_start_and_send_keys() {
        let step = |key_code| ram_control_center_protocol::MacroStep {
            key_code,
            kind: ram_control_center_protocol::MacroStepKind::Make,
            delay_ms: 5,
            group: 0,
            display: String::new(),
        };
        let existing = FlexiKeyAction::Text {
            start_keys: vec![step(1)],
            text: "old".to_owned(),
            send_keys: vec![step(2)],
        };
        let updated = editor_action(Some(&existing), 3, "new text").expect("text update");
        assert_eq!(
            updated,
            FlexiKeyAction::Text {
                start_keys: vec![step(1)],
                text: "new text".to_owned(),
                send_keys: vec![step(2)],
            }
        );
    }

    #[test]
    fn mouse_binding_rejects_vendor_unknown_button() {
        let profile = FlexiKeyProfile {
            index: 0,
            name: "Mouse".to_owned(),
            icon: String::new(),
            current_device: 2,
            keyboard_enabled: true,
            mouse_enabled: true,
            selected: true,
            bindings: Vec::new(),
        };
        let error = editor_binding(&profile, -1, 2, "0x20", "Side", 4, "")
            .expect_err("unknown mouse button must fail");
        assert!(error.contains("鼠标触发") || error.contains("Mouse triggers"));
    }

    #[test]
    fn missing_profile_offers_a_blank_active_profile() {
        let path = temp_profile_path("missing");
        let state = load_flexikey_view(&path);
        let profile = state.store.active().expect("blank active profile");
        assert!(profile.bindings.is_empty());
        assert!(profile.keyboard_enabled);
        assert!(profile.mouse_enabled);
        assert!(state.load_error.is_none());
        assert!(state.operation_notice.is_some());
    }
}
