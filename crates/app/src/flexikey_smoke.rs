//! Exercise production FlexiKey callbacks and persistence in an isolated window.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use himari_protocol::FlexiKeyStore;
use slint::ComponentHandle;

use crate::MainWindow;
use crate::flexikey_engine::FlexiKeyEngine;
use crate::flexikey_store::FlexiKeyViewState;

struct Fixture(PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            eprintln!("remove FlexiKey test fixture {}: {error}", self.0.display());
        }
    }
}

#[derive(Default)]
struct Measurements {
    step: usize,
    longest_callback: Duration,
    failure: Option<String>,
}

pub(crate) fn run() -> Result<(), Box<dyn std::error::Error>> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let directory =
        std::env::temp_dir().join(format!("ram-flexikey-ui-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&directory)?;
    let fixture = Fixture(directory);
    let path = fixture.0.join("profile.xml");
    std::fs::write(
        &path,
        r#"<root>
        <Profile_0 Name="Personal" IsSelected="1" IsKbEnable="0" IsMouseEnable="0"/>
        <Profile_1 Name="Work" IsSelected="0" IsKbEnable="0" IsMouseEnable="0"/>
    </root>"#,
    )?;
    let ui = MainWindow::new()?;
    ui.set_selected_page(9);
    ui.set_navigation_page(9);
    let state = crate::flexikey_ui::wire_flexikey_at(&ui, FlexiKeyEngine::start(), path.clone());
    let measurements = Rc::new(RefCell::new(Measurements::default()));
    ui.show()?;
    schedule(
        ui.as_weak(),
        state,
        Rc::clone(&measurements),
        Instant::now() + Duration::from_secs(15),
    );
    slint::run_event_loop()?;
    let data = measurements.borrow();
    if let Some(error) = &data.failure {
        return Err(error.clone().into());
    }
    if data.step != 11 {
        return Err("FlexiKey callback sequence did not complete".into());
    }
    let saved = FlexiKeyStore::from_xml(&std::fs::read_to_string(path)?)?;
    let active = saved.active().ok_or("saved profile is unavailable")?;
    if active.index != 1
        || active.name != "Work renamed"
        || active.keyboard_enabled
        || !active.mouse_enabled
        || !active.bindings.is_empty()
        || saved.profiles.len() != 2
    {
        return Err(format!("unexpected saved profile: {active:?}").into());
    }
    println!(
        "flexikey_ui callbacks=11 immediate_draft=passed persisted_readback=passed max_callback_ms={:.3}",
        data.longest_callback.as_secs_f64() * 1000.0
    );
    if data.longest_callback > Duration::from_micros(16_670) {
        return Err("FlexiKey callback exceeded the 16.67 ms UI budget".into());
    }
    Ok(())
}

fn schedule(
    weak: slint::Weak<MainWindow>,
    state: Rc<RefCell<FlexiKeyViewState>>,
    measurements: Rc<RefCell<Measurements>>,
    deadline: Instant,
) {
    slint::Timer::single_shot(Duration::from_millis(10), move || {
        let Some(ui) = weak.upgrade() else {
            let _ = slint::quit_event_loop();
            return;
        };
        let error = {
            let state = state.borrow();
            state
                .operation_error
                .as_ref()
                .or(state.engine_error.as_ref())
                .or(state.load_error.as_ref())
                .cloned()
        };
        if let Some(error) = error
            .or_else(|| (Instant::now() >= deadline).then(|| "FlexiKey UI test timed out".into()))
        {
            measurements.borrow_mut().failure = Some(error);
            let _ = slint::quit_event_loop();
            return;
        }
        if state.borrow().engine_report.is_none() {
            schedule(weak, state, measurements, deadline);
            return;
        }
        let step = measurements.borrow().step;
        if step == 11 {
            let _ = slint::quit_event_loop();
            return;
        }
        let started = Instant::now();
        let valid = match step {
            0 => {
                ui.invoke_select_flexikey_profile(1);
                ui.get_flexikey_active_index() == 1
            }
            1 => {
                ui.invoke_set_flexikey_device_enabled(1, true);
                ui.get_flexikey_keyboard_enabled()
            }
            2 => {
                ui.invoke_set_flexikey_device_enabled(2, true);
                ui.get_flexikey_mouse_enabled()
            }
            3 => {
                ui.invoke_rename_flexikey_profile("Work renamed".into());
                ui.get_flexikey_profile_name() == "Work renamed"
            }
            4 => {
                ui.invoke_set_flexikey_device_enabled(1, false);
                !ui.get_flexikey_keyboard_enabled()
            }
            5 => {
                ui.invoke_open_flexikey_editor(-1);
                ui.invoke_save_flexikey_editor(-1, 1, "0x76".into(), "F24".into(), 4, "".into());
                !ui.get_flexikey_editor_open()
                    && state.borrow().store.active().unwrap().bindings.len() == 1
            }
            6 => {
                ui.invoke_delete_flexikey_binding(0);
                state.borrow().store.active().unwrap().bindings.is_empty()
            }
            7 => {
                ui.invoke_new_flexikey_profile();
                state.borrow().store.profiles.len() == 3
            }
            8 => {
                ui.invoke_delete_flexikey_profile();
                state.borrow().store.profiles.len() == 2
            }
            9 => {
                ui.invoke_select_flexikey_profile(1);
                ui.get_flexikey_active_index() == 1
            }
            10 => {
                ui.invoke_reload_flexikey();
                true
            }
            _ => unreachable!(),
        };
        let mut data = measurements.borrow_mut();
        data.longest_callback = data.longest_callback.max(started.elapsed());
        if !valid {
            data.failure = Some(format!(
                "FlexiKey callback {step} did not update its visible draft immediately"
            ));
            drop(data);
            let _ = slint::quit_event_loop();
            return;
        }
        data.step += 1;
        drop(data);
        schedule(weak, state, measurements, deadline);
    });
}
