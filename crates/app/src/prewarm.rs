//! Off-screen glyph prewarm for the startup path.
//!
//! The first visit of a page pays Skia's glyph rasterisation cost. Walking every
//! page once while the window sits far off-screen fills those caches before the
//! user navigates, which brings every page's first frame inside one 60 Hz frame.
//!
//! The window is shrunk to a couple of pixels before its first map. Slint applies
//! `set_position` only after the window is mapped, so without the shrink the
//! default top-left position is briefly visible; a 2x2 window at that position is
//! not. The real size is restored as soon as the window is off-screen.

#![forbid(unsafe_code)]

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::{ComponentHandle, LogicalPosition, LogicalSize};

use crate::{MainWindow, diagnostics};

/// How long each page stays visible while prewarming. One frame at 60 Hz is
/// 16.7 ms; 24 ms leaves room for the first frame's raster work.
const PAGE_MS: u64 = 20;
/// Delay before the walk so the window and renderer exist.
const START_MS: u64 = 50;

/// Size the window is created at so the un-positioned first map is invisible.
const PARKED_EDGE: f32 = 2.0;

/// How the window is hidden for the walk.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Concealment {
    /// Shrink to a couple of pixels before the first map, so the top-left corner
    /// Slint maps before `set_position` lands is effectively invisible. Used on
    /// the real startup path where the user can see the screen.
    Parked,
    /// Keep the real size and rely on the off-screen position alone. Used by the
    /// measurement path, where the resize would add swapchain work that belongs
    /// to startup rather than to page drawing.
    Solid,
}

/// Walk all eleven pages with the window far off-screen, then hide it and put it
/// back at a sane on-screen position for the next user-visible show.
pub(crate) fn run(
    ui: &MainWindow,
    concealment: Concealment,
) -> Result<(), Box<dyn std::error::Error>> {
    let started = Instant::now();
    let window = ui.window();
    // Queried only when it is needed: reading the size can force the platform
    // window to exist before `show`, which changes when the first map happens.
    let real_size = match concealment {
        Concealment::Parked => {
            let size = window.size();
            window.set_size(LogicalSize::new(PARKED_EDGE, PARKED_EDGE));
            Some(size)
        }
        Concealment::Solid => None,
    };
    window.set_position(LogicalPosition::new(-4000.0, -4000.0));
    ui.show()?;
    // Off-screen now, so the full size can come back before any page is walked.
    if let Some(size) = real_size {
        window.set_size(size);
    }

    let weak = ui.as_weak();
    let step = Rc::new(Cell::new(0_i32));
    let restore = Rc::new(Cell::new(real_size.map(|size| (size.width, size.height))));
    fn advance(
        ui: slint::Weak<MainWindow>,
        step: Rc<Cell<i32>>,
        restore: Rc<Cell<Option<(u32, u32)>>>,
    ) {
        let Some(window) = ui.upgrade() else {
            return;
        };
        let page = step.get();
        if page > 10 {
            // Resize while the window is still mapped and off-screen. Resizing a
            // hidden window leaves the Vulkan swapchain invalid, and the next show
            // then fails to present.
            if let Some((width, height)) = restore.get() {
                window
                    .window()
                    .set_size(LogicalSize::new(width as f32, height as f32));
            }
            let _ = window.hide();
            let _ = slint::quit_event_loop();
            return;
        }
        window.set_selected_page(page);
        window.set_navigation_page(page);
        step.set(page + 1);
        let weak = window.as_weak();
        let restore_next = Rc::clone(&restore);
        slint::Timer::single_shot(Duration::from_millis(PAGE_MS), move || {
            advance(weak, step, restore_next);
        });
    }

    let restore_start = Rc::clone(&restore);
    slint::Timer::single_shot(Duration::from_millis(START_MS), move || {
        advance(weak, Rc::clone(&step), restore_start);
    });
    slint::run_event_loop()?;

    // The window stays hidden until the tray or the user opens it. Leaving it at
    // (-4000, -4000) would show an invisible window, so centre it now. The size is
    // restored again here: hiding a window that was created at the parked size
    // drops the earlier restore on some compositors, which then costs a full
    // relayout on the next show.
    let scale = window.scale_factor();
    let size = window.size();
    if let Some((left, top, right, bottom)) = crate::osd_layer::work_area() {
        let x = left + (right - left - i32::try_from(size.width).unwrap_or(0)) / 2;
        let y = top + (bottom - top - i32::try_from(size.height).unwrap_or(0)) / 2;
        window.set_position(LogicalPosition::new(x as f32 / scale, y as f32 / scale));
    }

    diagnostics::log_action(&format!(
        "prewarm pages=11 elapsed_ms={}",
        started.elapsed().as_millis()
    ));
    Ok(())
}
