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

/// Logical overlay size before display scaling.
const BASE_WIDTH: f32 = 320.0;
const BASE_HEIGHT: f32 = 84.0;
/// Gap between the card and the bottom of the work area.
const BASE_MARGIN: f32 = 24.0;

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
/// Present one overlay card with a glyph and an optional level bar.
pub(crate) fn show_with(
    title: &str,
    detail: &str,
    accent: slint::Color,
    glyph: crate::osd_glyph::Glyph,
    level: f32,
) {
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
        // Size the card from the display scale so it keeps the same apparent
        // size at 125 % / 150 %, like the vendor overlay does.
        let scale = ui.window().scale_factor();
        let width = (BASE_WIDTH * scale).round() as i32;
        let height = (BASE_HEIGHT * scale).round() as i32;
        let margin = (BASE_MARGIN * scale).round() as i32;
        // Anchor to the primary work area, which is what the vendor overlay
        // does: bottom centre of the desktop, above the taskbar.
        let origin = match osd_layer::work_area() {
            Some((left, _top, right, bottom)) => {
                (left + (right - left - width) / 2, bottom - height - margin)
            }
            None => {
                let position = ui.window().position();
                let size = ui.window().size();
                let anchor_width = i32::try_from(size.width).unwrap_or(0);
                let anchor_height = i32::try_from(size.height).unwrap_or(0);
                (
                    (position.x + (anchor_width - width) / 2).max(0),
                    (position.y + anchor_height - height - margin).max(0),
                )
            }
        };
        osd_layer::show(Presentation {
            title,
            detail,
            panel,
            accent,
            title_colour,
            detail_colour,
            origin,
            width,
            height,
            scale,
            glyph: glyph.id(),
            level,
        });
    })
    .is_err()
    {
        crate::diagnostics::log_action(
            "OSD update could not be dispatched to the Slint event loop",
        );
    }
}
