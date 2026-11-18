// Copyright © RAM CONTROL CENTER contributors
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

//! Read-only lifecycle observations from the native Vulkan surface.

use std::cell::RefCell;

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
}

type Observer = Box<dyn FnMut(FrameEvent, &DeviceInfo)>;
thread_local! {
    static OBSERVER: RefCell<Option<Observer>> = const { RefCell::new(None) };
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
