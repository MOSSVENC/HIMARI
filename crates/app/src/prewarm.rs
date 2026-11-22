//! Optional off-screen glyph prewarm for the tray-resident start path.
//!
//! The first visit of a page pays Skia's glyph rasterisation cost. Walking every
//! page once while the window sits far off-screen fills those caches before the
//! user navigates. This is opt-in (`HIMARI_PREWARM=1`) because it changes the
//! startup path and needs a real tray session to judge.

#![forbid(unsafe_code)]

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::{ComponentHandle, LogicalPosition};

use crate::{MainWindow, diagnostics};

/// How long each page stays visible while prewarming. One frame at 60 Hz is
/// 16.7 ms; 24 ms leaves room for the first frame's raster work.
const PAGE_MS: u64 = 24;
/// Delay before the walk so the window and renderer exist.
const START_MS: u64 = 80;

/// Walk all eleven pages with the window far off-screen, then hide it and put it
/// back at a sane on-screen position for the next user-visible show.
pub(crate) fn run(ui: &MainWindow) -> Result<(), Box<dyn std::error::Error>> {
    let started = Instant::now();
    let window = ui.window();
    window.set_position(LogicalPosition::new(-4000.0, -4000.0));
    ui.show()?;

    let weak = ui.as_weak();
    let step = Rc::new(Cell::new(0_i32));
    fn advance(ui: slint::Weak<MainWindow>, step: Rc<Cell<i32>>) {
        let Some(window) = ui.upgrade() else {
            return;
        };
        let page = step.get();
        if page > 10 {
            let _ = window.hide();
            let _ = slint::quit_event_loop();
            return;
        }
        window.set_selected_page(page);
        window.set_navigation_page(page);
        step.set(page + 1);
        let weak = window.as_weak();
        slint::Timer::single_shot(Duration::from_millis(PAGE_MS), move || {
            advance(weak, step);
        });
    }

    slint::Timer::single_shot(Duration::from_millis(START_MS), move || {
        advance(weak, Rc::clone(&step));
    });
    slint::run_event_loop()?;

    // The window stays hidden until the tray or the user opens it. Leaving it at
    // (-4000, -4000) would show an invisible window, so centre it now.
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
