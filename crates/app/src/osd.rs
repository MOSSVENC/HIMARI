//! Overlay presentation router.
//!
//! The card is drawn by the native layered window in `osd_layer`, which is the
//! only path on this machine that can carry per-pixel alpha. The Slint overlay
//! window was retired: its Vulkan surface advertises `COMPOSITE_ALPHA_OPAQUE`
//! only, so it could never be translucent.

use std::sync::Mutex;
use std::sync::OnceLock;

use slint::{ComponentHandle, Weak};

use crate::osd_layer::{self, Presentation};

static MAIN_WINDOW: OnceLock<Mutex<Option<Weak<crate::MainWindow>>>> = OnceLock::new();

pub(crate) fn install(main_window: Weak<crate::MainWindow>) {
    let main = MAIN_WINDOW.get_or_init(|| Mutex::new(None));
    if let Ok(mut main) = main.lock() {
        *main = Some(main_window);
    }
}

/// Present one overlay card.
///
/// The palette and the anchor geometry both live on the UI thread, so the
/// presentation is assembled there and only the finished description is handed
/// to the overlay thread.
pub(crate) fn show(title: &str, detail: &str, accent: slint::Color) {
    let Some(main) = MAIN_WINDOW
        .get()
        .and_then(|slot| slot.lock().ok())
        .and_then(|slot| slot.clone())
    else {
        return;
    };
    let title = title.to_owned();
    let detail = detail.to_owned();
    if slint::invoke_from_event_loop(move || {
        let Some(ui) = main.upgrade() else {
            return;
        };
        let dark = ui.global::<crate::Palette>().get_blend() >= 0.5;
        let panel = if dark {
            [0.11, 0.13, 0.18]
        } else {
            [0.93, 0.95, 0.98]
        };
        let title_colour = if dark {
            [0.96, 0.95, 0.98]
        } else {
            [0.08, 0.10, 0.14]
        };
        let detail_colour = if dark {
            [0.65, 0.67, 0.73]
        } else {
            [0.35, 0.40, 0.47]
        };
        let accent = [
            f32::from(accent.red()) / 255.0,
            f32::from(accent.green()) / 255.0,
            f32::from(accent.blue()) / 255.0,
        ];
        let position = ui.window().position();
        let size = ui.window().size();
        let width = i32::try_from(size.width).unwrap_or(0);
        let height = i32::try_from(size.height).unwrap_or(0);
        // A hidden or unplaced main window yields an off-screen anchor; clamp
        // so the card always lands inside the desktop.
        let origin = (
            (position.x + (width - 320) / 2).max(0),
            (position.y + height - 84 - 28).max(0),
        );
        osd_layer::show(Presentation {
            title,
            detail,
            panel,
            accent,
            title_colour,
            detail_colour,
            origin,
        });
    })
    .is_err()
    {
        crate::diagnostics::log_action(
            "OSD update could not be dispatched to the Slint event loop",
        );
    }
}
