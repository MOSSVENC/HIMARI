//! Platform geometry and capability decisions that must not assume one
//! monitor, 100 % scaling or one discrete-GPU vendor.
//!
//! The overlay used to be placed with arithmetic against the primary work
//! area only. This module keeps the pixel decisions pure (and testable on the
//! host) and leaves the two Windows monitor calls behind one narrow boundary.

/// Logical overlay card, before display scaling.
pub(crate) const CARD_WIDTH: f32 = 320.0;
pub(crate) const CARD_HEIGHT: f32 = 84.0;
/// Gap between the card and the bottom of the work area, in logical pixels.
pub(crate) const CARD_MARGIN: f32 = 24.0;

/// A work area or monitor rectangle in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub(crate) const fn width(self) -> i32 {
        self.right - self.left
    }

    pub(crate) const fn height(self) -> i32 {
        self.bottom - self.top
    }

    /// Centre point, used to pick the monitor under a window.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) const fn center(self) -> (i32, i32) {
        (self.left + self.width() / 2, self.top + self.height() / 2)
    }

    /// True when the rectangle has a usable area.
    pub(crate) const fn is_usable(self) -> bool {
        self.width() > 0 && self.height() > 0
    }
}

/// Bottom-centre origin of the overlay card inside `work`, in physical pixels.
///
/// The card keeps the same apparent size at 125 % / 150 %, so the card and the
/// margin are scaled by the monitor DPI before placement. A card wider than the
/// work area is pinned to its left edge instead of being pushed off-screen.
pub(crate) fn card_origin(work: Rect, scale: f32, border: (f32, f32)) -> (i32, i32) {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let width = (border.0 * scale).round() as i32;
    let height = (border.1 * scale).round() as i32;
    let margin = (CARD_MARGIN * scale).round() as i32;
    let x = work.left + (work.width() - width) / 2;
    let y = work.bottom - height - margin;
    let max_x = (work.right - width).max(work.left);
    let max_y = (work.bottom - height).max(work.top);
    (x.clamp(work.left, max_x), y.clamp(work.top, max_y))
}

/// Which backend, if any, can switch the discrete GPU's power.
///
/// The signed ACPI transport exposes the capability as a vendor flag, not as a
/// backend name. Naming the backend here keeps the decision separate from the
/// command encoding: a model without the flag is `Unsupported` and must not
/// receive an `NVPCF` write, while the reference machine is the NVIDIA platform
/// controller whose node lives in `gpu_power`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiscreteGpuBackend {
    NvidiaPlatformController,
    Unsupported,
}

impl DiscreteGpuBackend {
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) const fn is_supported(self) -> bool {
        matches!(self, Self::NvidiaPlatformController)
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::NvidiaPlatformController => "nvidia-platform-controller",
            Self::Unsupported => "unsupported",
        }
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn discrete_gpu_backend(
    capabilities: &himari_protocol::PlatformCapabilities,
) -> DiscreteGpuBackend {
    if capabilities.nvidia_power_off {
        DiscreteGpuBackend::NvidiaPlatformController
    } else {
        DiscreteGpuBackend::Unsupported
    }
}

/// Work area of the monitor nearest to `anchor`, in physical pixels.
///
/// `anchor` is the main window rectangle; a window that straddles two monitors
/// resolves to the one holding its centre, which is what dragging a window
/// between screens should do.
#[cfg(windows)]
pub(crate) fn work_area_near(anchor: Rect) -> Option<Rect> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
    };

    let (x, y) = anchor.center();
    let point = POINT { x, y };
    // SAFETY: MonitorFromPoint takes the point by value and returns a monitor
    // handle or NULL; it reads nothing from this frame.
    let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_null() {
        return None;
    }
    let mut info: MONITORINFO = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        rcMonitor: windows_sys::Win32::Foundation::RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        rcWork: windows_sys::Win32::Foundation::RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        dwFlags: 0,
    };
    // SAFETY: the handle comes from MonitorFromPoint, `info.cbSize` names the
    // struct size and the pointer is valid for the duration of the call.
    let ok = unsafe { GetMonitorInfoW(monitor, &mut info) };
    if ok == 0 {
        return None;
    }
    Some(Rect {
        left: info.rcWork.left,
        top: info.rcWork.top,
        right: info.rcWork.right,
        bottom: info.rcWork.bottom,
    })
}

#[cfg(not(windows))]
pub(crate) fn work_area_near(_anchor: Rect) -> Option<Rect> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn card_sits_at_the_bottom_centre_of_a_1366x768_work_area() {
        let work = work(0, 0, 1366, 728);
        let (x, y) = card_origin(work, 1.0, (CARD_WIDTH, CARD_HEIGHT));
        assert_eq!(x, (1366 - 320) / 2);
        assert_eq!(y, 728 - 84 - 24);
    }

    #[test]
    fn card_scales_with_the_monitor_and_stays_centred_at_3840x2160() {
        let work = work(0, 0, 3840, 2120);
        let (x, y) = card_origin(work, 1.5, (CARD_WIDTH, CARD_HEIGHT));
        assert_eq!(x, (3840 - 480) / 2);
        assert_eq!(y, 2120 - 126 - 36);
    }

    #[test]
    fn card_is_clamped_inside_a_secondary_monitor_with_a_negative_origin() {
        // A monitor placed to the left of the primary has a negative left edge.
        let work = work(-1920, 0, 0, 1040);
        let (x, y) = card_origin(work, 1.0, (CARD_WIDTH, CARD_HEIGHT));
        assert!(x >= -1920, "x={x} left the work area");
        assert_eq!(x, -1920 + (1920 - 320) / 2);
        assert_eq!(y, 1040 - 84 - 24);
    }

    #[test]
    fn card_wider_than_the_work_area_pins_to_the_left_edge() {
        let narrow = work(0, 0, 280, 600);
        let (x, _y) = card_origin(narrow, 1.0, (CARD_WIDTH, CARD_HEIGHT));
        assert_eq!(x, 0);
    }

    #[test]
    fn a_window_partly_off_screen_still_gets_a_visible_card() {
        let work = work(0, 0, 1920, 1040);
        // Window dragged so only its right part remains on-screen.
        let anchor = Rect {
            left: -800,
            top: 200,
            right: 1000,
            bottom: 900,
        };
        let (x, y) = card_origin(work, 1.0, (CARD_WIDTH, CARD_HEIGHT));
        assert!(x >= 0);
        assert!(y + 84 <= 1040);
        assert_eq!(anchor.center(), (100, 550));
    }

    #[test]
    fn a_bogus_scale_falls_back_to_one() {
        let work = work(0, 0, 1280, 720);
        let (x, y) = card_origin(work, f32::NAN, (CARD_WIDTH, CARD_HEIGHT));
        assert_eq!(x, (1280 - 320) / 2);
        assert_eq!(y, 720 - 84 - 24);
    }

    #[test]
    fn discrete_gpu_power_is_gated_by_the_capability_flag() {
        let off = himari_protocol::PlatformCapabilities::default();
        assert_eq!(discrete_gpu_backend(&off), DiscreteGpuBackend::Unsupported);
        assert!(!discrete_gpu_backend(&off).is_supported());
        let on = himari_protocol::PlatformCapabilities {
            nvidia_power_off: true,
            ..Default::default()
        };
        assert_eq!(
            discrete_gpu_backend(&on),
            DiscreteGpuBackend::NvidiaPlatformController
        );
        assert!(discrete_gpu_backend(&on).is_supported());
    }
}
