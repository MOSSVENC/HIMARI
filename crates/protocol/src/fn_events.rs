//! AcpiBridge notification framing and the verified FnKey dispatch subset.
//!
//! The transport returns an i64. FnKey's Arg001 receiver converts that value
//! to i32, dispatches `value & 255`, and passes `value >> 8` as an argument.
//! Event IDs describe requests as well as completed state notifications.
//! See docs/fn-event-protocol.md for the dispatch evidence and remaining gates.

use crate::{DchuError, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FnNotification {
    raw: i64,
}

impl FnNotification {
    pub fn from_payload(payload: &[u8]) -> Result<Self> {
        let bytes: [u8; 8] = payload.try_into().map_err(|_| DchuError::ControlFailed {
            operation: "read AcpiBridge notification value",
            detail: format!("expected 8 bytes, received {}", payload.len()),
        })?;
        Ok(Self {
            raw: i64::from_le_bytes(bytes),
        })
    }

    /// Parse the payload returned by the AcpiBridge notification IOCTL.
    /// Firmware on the reference machine returns either an i32 or an i64;
    /// the i32 form is sign-extended so negative arguments retain their value.
    pub fn from_driver_payload(payload: &[u8]) -> Result<Self> {
        match payload.len() {
            4 => Ok(Self {
                raw: i64::from(i32::from_le_bytes(
                    payload.try_into().expect("length checked"),
                )),
            }),
            8 => Self::from_payload(payload),
            length => Err(DchuError::ControlFailed {
                operation: "read AcpiBridge notification value",
                detail: format!("expected 4 or 8 bytes, received {length}"),
            }),
        }
    }

    pub const fn raw(self) -> i64 {
        self.raw
    }

    /// Preserve values outside FnKey's signed 32-bit message format as raw data.
    pub fn message(self) -> Option<(u8, i32)> {
        let value = i32::try_from(self.raw).ok()?;
        Some(((value & 255) as u8, value >> 8))
    }

    pub fn event(self) -> Option<FnEvent> {
        let (code, _) = self.message()?;
        Some(match code {
            0x20 | 0x81 => FnEvent::BacklightStep(Adjustment::Down),
            0x21 | 0x82 => FnEvent::BacklightStep(Adjustment::Up),
            0x25 => FnEvent::FnLockToggle,
            0x3a => FnEvent::FanNotice(FanNotice::Turbo),
            0x3f | 0x9e | 0x9f => FnEvent::BacklightToggle,
            0x40 => FnEvent::FanNotice(FanNotice::AntiDustRunning),
            0x59 => FnEvent::PowerModeCycle,
            0x6a => FnEvent::FanNotice(FanNotice::MaxQ),
            0x6b => FnEvent::FanNotice(FanNotice::Custom),
            0x70 => FnEvent::NoiselessFanToggle,
            0x8a => FnEvent::BacklightCycle,
            0x8f => FnEvent::FanNotice(FanNotice::Maximum),
            0x95 => FnEvent::ControlCenterToggle,
            0xa6 => FnEvent::Docking(true),
            0xa7 => FnEvent::Docking(false),
            0xc7 => FnEvent::NumLock(false),
            0xc8 => FnEvent::NumLock(true),
            0xc9 => FnEvent::CapsLock(false),
            0xca => FnEvent::CapsLock(true),
            0xcb => FnEvent::ScrollLock(false),
            0xcc => FnEvent::ScrollLock(true),
            0xde => FnEvent::SilentMode(true),
            0xdf => FnEvent::SilentMode(false),
            0xea => FnEvent::MobileRadio(false),
            0xeb => FnEvent::MobileRadio(true),
            0xf6 => FnEvent::Camera(false),
            0xf7 => FnEvent::Camera(true),
            0xf8 => FnEvent::CameraMicrophone(false),
            0xf9 => FnEvent::CameraMicrophone(true),
            // Both branches toggle the current Windows state on a Precision
            // Touchpad. Only the legacy path treats these IDs as fixed targets.
            0xfc => FnEvent::TouchpadToggle {
                legacy_enabled: false,
            },
            0xfd => FnEvent::TouchpadToggle {
                legacy_enabled: true,
            },
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Adjustment {
    Down,
    Up,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FanNotice {
    AntiDustRunning,
    Turbo,
    MaxQ,
    Custom,
    Maximum,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FnEvent {
    BacklightStep(Adjustment),
    BacklightToggle,
    BacklightCycle,
    FnLockToggle,
    FanNotice(FanNotice),
    NoiselessFanToggle,
    PowerModeCycle,
    ControlCenterToggle,
    Docking(bool),
    NumLock(bool),
    CapsLock(bool),
    ScrollLock(bool),
    SilentMode(bool),
    MobileRadio(bool),
    Camera(bool),
    CameraMicrophone(bool),
    TouchpadToggle { legacy_enabled: bool },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification(value: i64) -> FnNotification {
        FnNotification::from_payload(&value.to_le_bytes()).unwrap()
    }

    #[test]
    fn message_separates_command_and_signed_argument() {
        let event = notification(0x0012_34f7);
        assert_eq!(event.message(), Some((0xf7, 0x1234)));
        assert_eq!(event.event(), Some(FnEvent::Camera(true)));
        assert_eq!(notification(-9).message(), Some((0xf7, -1)));
    }

    #[test]
    fn out_of_range_payload_cannot_alias_a_known_event() {
        for raw in [0x1_0000_00f7, 0xffff_fff7, i64::MAX, i64::MIN] {
            let event = notification(raw);
            assert_eq!(event.raw(), raw);
            assert_eq!(event.message(), None);
            assert_eq!(event.event(), None);
        }
    }

    #[test]
    fn every_incomplete_or_oversized_frame_is_rejected() {
        for size in 0..=16 {
            let result = FnNotification::from_payload(&vec![0xf7; size]);
            assert_eq!(result.is_ok(), size == 8, "frame size {size}");
        }
    }

    #[test]
    fn driver_payload_accepts_i32_and_i64_forms() {
        let narrow = FnNotification::from_driver_payload(&(-9_i32).to_le_bytes()).unwrap();
        let wide = FnNotification::from_driver_payload(&(-9_i64).to_le_bytes()).unwrap();
        assert_eq!(narrow.raw(), -9);
        assert_eq!(wide.raw(), -9);
        assert_eq!(narrow.event(), Some(FnEvent::Camera(true)));
        assert_eq!(wide.event(), Some(FnEvent::Camera(true)));
        assert!(FnNotification::from_driver_payload(&[0_u8; 2]).is_err());
    }

    #[test]
    fn touchpad_requests_retain_the_legacy_target_and_toggle_semantics() {
        assert_eq!(
            notification(0xfc).event(),
            Some(FnEvent::TouchpadToggle {
                legacy_enabled: false
            })
        );
        assert_eq!(
            notification(0xfd).event(),
            Some(FnEvent::TouchpadToggle {
                legacy_enabled: true
            })
        );
    }

    #[test]
    fn firmware_notices_are_distinct_from_control_requests() {
        assert_eq!(
            notification(0x40).event(),
            Some(FnEvent::FanNotice(FanNotice::AntiDustRunning))
        );
        assert_eq!(
            notification(0x8f).event(),
            Some(FnEvent::FanNotice(FanNotice::Maximum))
        );
        assert_eq!(notification(0x25).event(), Some(FnEvent::FnLockToggle));
        assert_eq!(notification(0x59).event(), Some(FnEvent::PowerModeCycle));
        assert_eq!(notification(0xc9).event(), Some(FnEvent::CapsLock(false)));
        assert_eq!(notification(0xca).event(), Some(FnEvent::CapsLock(true)));
    }

    #[test]
    fn unknown_commands_and_osd_resource_indices_remain_unmapped() {
        // OSD resource 24 means touchpad-off, but event 24 has no such mapping.
        for code in [0, 24, 25, 0x41, 0x42, 0x48, 0xff] {
            assert_eq!(notification(code).event(), None, "event {code:#x}");
        }
    }
}
