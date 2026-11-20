//! Always-on-top Fn/OSD surface owned by the Slint event loop.

use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use slint::{ComponentHandle, Weak};

use crate::OsdWindow;

static WINDOW: OnceLock<Mutex<Option<Weak<OsdWindow>>>> = OnceLock::new();
static MAIN_WINDOW: OnceLock<Mutex<Option<Weak<crate::MainWindow>>>> = OnceLock::new();
static GENERATION: AtomicU64 = AtomicU64::new(0);

const DISPLAY_TIME: Duration = Duration::from_millis(2_200);

pub(crate) fn set_theme(dark: bool, blend: f32) {
    let Some(window) = WINDOW
        .get()
        .and_then(|slot| slot.lock().ok())
        .and_then(|slot| slot.clone())
    else {
        return;
    };
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(osd) = window.upgrade() {
            osd.global::<crate::Palette>().set_dark(dark);
            osd.global::<crate::Palette>().set_blend(blend);
        }
    });
}

pub(crate) fn install(window: Weak<OsdWindow>, main_window: Weak<crate::MainWindow>) {
    let slot = WINDOW.get_or_init(|| Mutex::new(None));
    if let Ok(mut slot) = slot.lock() {
        *slot = Some(window);
    }
    let main = MAIN_WINDOW.get_or_init(|| Mutex::new(None));
    if let Ok(mut main) = main.lock() {
        *main = Some(main_window);
    }
}

pub(crate) fn show(title: &str, detail: &str, accent: slint::Color) {
    let Some(window) = WINDOW
        .get()
        .and_then(|slot| slot.lock().ok())
        .and_then(|slot| slot.clone())
    else {
        return;
    };
    let generation = GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    let title = title.to_owned();
    let detail = detail.to_owned();
    if slint::invoke_from_event_loop(move || {
        let weak = window.clone();
        let Some(osd) = window.upgrade() else {
            return;
        };
        osd.set_osd_title(title.into());
        osd.set_osd_detail(detail.into());
        osd.set_osd_accent(accent);
        osd.set_osd_reveal(0.0);
        position_over_main_window(&osd);
        let _ = osd.show();
        slint::Timer::single_shot(Duration::from_millis(1), {
            let weak = weak.clone();
            move || {
                if GENERATION.load(Ordering::Acquire) == generation
                    && let Some(osd) = weak.upgrade()
                {
                    osd.set_osd_reveal(1.0);
                }
            }
        });
        slint::Timer::single_shot(DISPLAY_TIME, move || {
            if GENERATION.load(Ordering::Acquire) == generation && weak.upgrade().is_some() {
                // A colour-keyed window cannot fade: a blended pixel stops
                // matching the key and survives as a tinted smear. Withdraw
                // the overlay after a short settle instead.
                slint::Timer::single_shot(Duration::from_millis(180), move || {
                    if GENERATION.load(Ordering::Acquire) == generation
                        && let Some(osd) = weak.upgrade()
                    {
                        osd.hide().ok();
                    }
                });
            }
        });
    })
    .is_err()
    {
        crate::diagnostics::log_action(
            "OSD update could not be dispatched to the Slint event loop",
        );
    }
}

fn position_over_main_window(window: &OsdWindow) {
    let Some(main) = MAIN_WINDOW
        .get()
        .and_then(|slot| slot.lock().ok())
        .and_then(|slot| slot.clone())
        .and_then(|main| main.upgrade())
    else {
        return;
    };
    let position = main.window().position();
    let size = main.window().size();
    let scale = window.window().scale_factor();
    let width = (320.0 * scale).round() as i32;
    let height = (84.0 * scale).round() as i32;
    window.window().set_position(slint::PhysicalPosition::new(
        position.x + (i32::try_from(size.width).unwrap_or(0) - width) / 2,
        position.y + i32::try_from(size.height).unwrap_or(0) - height - 28,
    ));
}
