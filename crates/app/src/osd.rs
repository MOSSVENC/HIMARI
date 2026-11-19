//! Always-on-top Fn/OSD surface owned by the Slint event loop.

use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use slint::{ComponentHandle, Weak};

use crate::OsdWindow;

static WINDOW: OnceLock<Mutex<Option<Weak<OsdWindow>>>> = OnceLock::new();
static GENERATION: AtomicU64 = AtomicU64::new(0);

const DISPLAY_TIME: Duration = Duration::from_millis(2_200);

pub(crate) fn install(window: Weak<OsdWindow>) {
    let slot = WINDOW.get_or_init(|| Mutex::new(None));
    if let Ok(mut slot) = slot.lock() {
        *slot = Some(window);
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
        position_bottom_center(&osd);
        let _ = osd.show();
        let shape_weak = weak.clone();
        slint::Timer::single_shot(Duration::from_millis(40), move || {
            if let Some(osd) = shape_weak.upgrade() {
                apply_rounded_region(&osd);
            }
        });
        slint::Timer::single_shot(DISPLAY_TIME, move || {
            if GENERATION.load(Ordering::Acquire) == generation
                && let Some(osd) = weak.upgrade()
            {
                osd.hide().ok();
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

#[cfg(windows)]
fn apply_rounded_region(window: &OsdWindow) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Graphics::Gdi::{CreateRoundRectRgn, DeleteObject, SetWindowRgn};

    let window_handle = window.window().window_handle();
    let Ok(handle) = window_handle.window_handle() else {
        crate::diagnostics::log_action("OSD native window handle is unavailable");
        return;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return;
    };
    let hwnd = win32.hwnd.get() as windows_sys::Win32::Foundation::HWND;
    // SAFETY: the rounded region is created from scalar dimensions. On a
    // successful SetWindowRgn the system owns the region handle.
    let region = unsafe { CreateRoundRectRgn(0, 0, 361, 97, 36, 36) };
    if region.is_null() {
        return;
    }
    // SAFETY: both handles are live; the region is transferred on success.
    if unsafe { SetWindowRgn(hwnd, region, 1) } == 0 {
        // SAFETY: SetWindowRgn did not take ownership on failure.
        unsafe { DeleteObject(region) };
    }
}

#[cfg(windows)]
fn position_bottom_center(window: &OsdWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

    // SAFETY: both metric indices are scalar values and require no pointers.
    let physical_width = unsafe { GetSystemMetrics(SM_CXSCREEN) } as f32;
    // SAFETY: SM_CYSCREEN is a scalar metric index with no pointer inputs.
    let physical_height = unsafe { GetSystemMetrics(SM_CYSCREEN) } as f32;
    let scale = window.window().scale_factor();
    let logical_width = physical_width / scale;
    let logical_height = physical_height / scale;
    const OSD_WIDTH: f32 = 360.0;
    const OSD_HEIGHT: f32 = 96.0;
    window.window().set_position(slint::LogicalPosition::new(
        (logical_width - OSD_WIDTH) / 2.0,
        logical_height - OSD_HEIGHT - 28.0,
    ));
}

#[cfg(not(windows))]
fn position_bottom_center(_window: &OsdWindow) {}
