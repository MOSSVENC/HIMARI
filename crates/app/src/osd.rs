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
use crate::platform_profile;

static MAIN_WINDOW: OnceLock<Mutex<Option<Weak<crate::MainWindow>>>> = OnceLock::new();

/// Colours of one overlay card for the persisted theme.
#[derive(Clone, Copy)]
pub(crate) struct CardPalette {
    pub(crate) panel: [f32; 3],
    pub(crate) title: [f32; 3],
    pub(crate) detail: [f32; 3],
}

pub(crate) fn card_palette(dark: bool) -> CardPalette {
    if dark {
        CardPalette {
            panel: [0.11, 0.13, 0.18],
            title: [0.96, 0.95, 0.98],
            detail: [0.65, 0.67, 0.73],
        }
    } else {
        CardPalette {
            panel: [0.93, 0.95, 0.98],
            title: [0.08, 0.10, 0.14],
            detail: [0.35, 0.40, 0.47],
        }
    }
}

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
    spin: bool,
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
        let palette = card_palette(dark);
        let panel = palette.panel;
        let title_colour = palette.title;
        let detail_colour = palette.detail;
        let accent = [
            f32::from(accent.red()) / 255.0,
            f32::from(accent.green()) / 255.0,
            f32::from(accent.blue()) / 255.0,
        ];
        // Size the card from the display scale so it keeps the same apparent
        // size at 125 % / 150 %, like the vendor overlay does.
        let scale = ui.window().scale_factor();
        let width = (platform_profile::CARD_WIDTH * scale).round() as i32;
        let height = (platform_profile::CARD_HEIGHT * scale).round() as i32;
        let position = ui.window().position();
        let size = ui.window().size();
        let anchor_width = i32::try_from(size.width).unwrap_or(0);
        let anchor_height = i32::try_from(size.height).unwrap_or(0);
        // The vendor overlay centres on the work area above the taskbar. Use
        // the monitor under the window first, so a multi-monitor desktop gets
        // the card on the screen the user is looking at, then fall back to the
        // primary work area, then to the window itself.
        let window = platform_profile::Rect {
            left: position.x,
            top: position.y,
            right: position.x + anchor_width,
            bottom: position.y + anchor_height,
        };
        let work = platform_profile::work_area_near(window)
            .filter(|rect| rect.is_usable())
            .or_else(|| {
                osd_layer::work_area().map(|(left, top, right, bottom)| platform_profile::Rect {
                    left,
                    top,
                    right,
                    bottom,
                })
            });
        let origin = match work {
            Some(work) => platform_profile::card_origin(
                work,
                scale,
                (platform_profile::CARD_WIDTH, platform_profile::CARD_HEIGHT),
            ),
            None => (
                (position.x + (anchor_width - width) / 2).max(0),
                (position.y + anchor_height
                    - height
                    - (platform_profile::CARD_MARGIN * scale).round() as i32)
                    .max(0),
            ),
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
            spin,
        });
    })
    .is_err()
    {
        crate::diagnostics::log_action(
            "OSD update could not be dispatched to the Slint event loop",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::card_palette;

    #[test]
    fn card_palette_follows_the_theme_instead_of_being_fixed() {
        let dark = card_palette(true);
        let light = card_palette(false);
        let sum = |c: [f32; 3]| c.iter().sum::<f32>();
        assert!(sum(dark.panel) < sum(light.panel));
        assert!(sum(dark.title) > sum(light.title));
        assert!(sum(dark.detail) > sum(light.detail));
    }
}
