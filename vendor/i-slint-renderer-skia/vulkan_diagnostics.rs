// Copyright © RAM CONTROL CENTER contributors
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

//! Read-only lifecycle observations from the native Vulkan surface.

use i_slint_core::graphics::{Rgba8Pixel, SharedPixelBuffer};
use std::cell::RefCell;
use std::time::Duration;

/// CPU wall time spent in each stage of one successful Vulkan frame.
/// Submission includes the current synchronous GPU-completion wait. These are
/// not GPU timestamps and do not measure compositor or scanout latency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameTimings {
    pub acquire: Duration,
    pub prepare: Duration,
    pub draw: Duration,
    pub submit: Duration,
    pub present: Duration,
    pub total: Duration,
}

/// The physical device that owns the surface's logical device and swapchain.
#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub name: String,
    pub device_type: String,
    pub driver_version: u32,
    pub vendor_id: u32,
    pub device_id: u32,
}

/// Events emitted on the UI thread. Submitted means queue submission succeeded;
/// it does not mean that the image has reached the display.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameEvent {
    Setup,
    BeforeRendering,
    Submitted,
    Timings(FrameTimings),
}

type Observer = Box<dyn FnMut(FrameEvent, &DeviceInfo)>;
/// RGBA pixels copied from an acquired Vulkan render target, or a readback error.
pub type SnapshotResult = Result<SharedPixelBuffer<Rgba8Pixel>, String>;
pub(crate) type SnapshotCallback = Box<dyn FnOnce(SnapshotResult)>;
thread_local! {
    static OBSERVER: RefCell<Option<Observer>> = const { RefCell::new(None) };
    static SNAPSHOT: RefCell<Option<SnapshotCallback>> = const { RefCell::new(None) };
}

/// Read back the next acquired Vulkan image after drawing and deliver its pixels
/// after submission. Callers must request a redraw and keep the event loop running.
/// Returns false if a capture is already pending. GPU readback stalls the frame;
/// callers must keep capture frames outside performance measurement windows.
pub fn request_snapshot(callback: impl FnOnce(SnapshotResult) + 'static) -> bool {
    SNAPSHOT.with_borrow_mut(|slot| {
        if slot.is_some() {
            return false;
        }
        *slot = Some(Box::new(callback));
        true
    })
}

pub(crate) fn take_snapshot_request() -> Option<SnapshotCallback> {
    SNAPSHOT.with_borrow_mut(Option::take)
}

/// Replaces the observer for Vulkan surfaces on the current UI thread.
/// Observers must not reenter rendering or change the observer synchronously.
pub fn set_observer(observer: impl FnMut(FrameEvent, &DeviceInfo) + 'static) {
    OBSERVER.with_borrow_mut(|slot| *slot = Some(Box::new(observer)));
}

pub(crate) fn notify(event: FrameEvent, info: &DeviceInfo) {
    OBSERVER.with_borrow_mut(|slot| {
        if let Some(observer) = slot.as_mut() {
            observer(event, info);
        }
    });
}
