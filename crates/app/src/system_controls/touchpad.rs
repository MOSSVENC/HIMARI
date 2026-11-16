use ram_control_center_protocol::Dchu;

#[cfg(windows)]
pub fn touchpad_enabled() -> Result<bool, String> {
    super::registry::read_registry_dword(
        r"Software\Microsoft\Windows\CurrentVersion\PrecisionTouchPad\Status",
        "Enabled",
    )?
    .map(|value| value != 0)
    .ok_or_else(|| "Windows Precision Touchpad state is unavailable".to_owned())
}

#[cfg(not(windows))]
pub fn touchpad_enabled() -> Result<bool, String> {
    Err("touchpad state is only available on Windows".to_owned())
}

/// Apply an explicit state through the OEM's Windows touchpad chord, then
/// persist its settings-page mirror after Windows confirms the transition.
pub fn set_touchpad_enabled(device: &Dchu, enabled: bool) -> Result<(), String> {
    apply(&mut PlatformTouchpad(device), enabled)
}

trait Touchpad {
    fn read(&mut self) -> Result<bool, String>;
    fn toggle(&mut self) -> Result<(), String>;
    fn persist(&mut self, enabled: bool) -> Result<(), String>;
    fn wait(&mut self);
}

struct PlatformTouchpad<'a>(&'a Dchu);

impl Touchpad for PlatformTouchpad<'_> {
    fn read(&mut self) -> Result<bool, String> {
        touchpad_enabled()
    }
    fn toggle(&mut self) -> Result<(), String> {
        crate::input::send_touchpad_chord()
    }
    fn persist(&mut self, enabled: bool) -> Result<(), String> {
        self.0
            .set_touchpad_enabled(enabled)
            .map_err(|error| error.to_string())
    }
    fn wait(&mut self) {
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

fn confirm(device: &mut impl Touchpad, enabled: bool) -> Result<(), String> {
    for attempt in 0..=20 {
        if device.read()? == enabled {
            return Ok(());
        }
        if attempt != 20 {
            device.wait();
        }
    }
    Err(format!(
        "Windows touchpad did not reach requested state {enabled} within 500 ms"
    ))
}

fn apply(device: &mut impl Touchpad, enabled: bool) -> Result<(), String> {
    let previous = device.read()?;
    if previous == enabled {
        return Ok(());
    }
    device.toggle()?;
    // An accepted input sequence is not proof that the Windows setting changed.
    // Leave the mirror intact if no confirmed transition has been observed.
    confirm(device, enabled)?;
    if let Err(error) = device.persist(enabled) {
        let restore = (|| {
            if device.read()? != previous {
                device.toggle()?;
                confirm(device, previous)?;
            }
            device.persist(previous)
        })();
        return Err(match restore {
            Ok(()) => format!("{error}; previous touchpad state restored"),
            Err(restore) => format!("{error}; touchpad restoration failed: {restore}"),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct FakeTouchpad {
        enabled: bool,
        toggles: usize,
        writes: Vec<bool>,
        waits: usize,
        delay: usize,
        pending: Option<bool>,
        missing: bool,
        ignore_toggle: bool,
        fail_toggle: bool,
        fail_persist: bool,
    }
    impl Touchpad for FakeTouchpad {
        fn read(&mut self) -> Result<bool, String> {
            if self.missing {
                Err("missing state".to_owned())
            } else {
                Ok(self.enabled)
            }
        }
        fn toggle(&mut self) -> Result<(), String> {
            self.toggles += 1;
            if self.fail_toggle {
                return Err("input rejected".to_owned());
            }
            if !self.ignore_toggle {
                if self.delay == 0 {
                    self.enabled = !self.enabled;
                } else {
                    self.pending = Some(!self.enabled);
                }
            }
            Ok(())
        }
        fn persist(&mut self, enabled: bool) -> Result<(), String> {
            self.writes.push(enabled);
            if self.fail_persist {
                self.fail_persist = false;
                Err("mirror write failed".to_owned())
            } else {
                Ok(())
            }
        }
        fn wait(&mut self) {
            self.waits += 1;
            if self.waits >= self.delay
                && let Some(enabled) = self.pending.take()
            {
                self.enabled = enabled;
            }
        }
    }
    #[test]
    fn repeated_targets_do_not_toggle_again() {
        let mut device = FakeTouchpad::default();
        for target in [false, true, true, false, false] {
            apply(&mut device, target).unwrap();
        }
        assert_eq!(device.toggles, 2);
        assert_eq!(device.writes, [true, false]);
        assert!(!device.enabled);
    }
    #[test]
    fn waits_for_windows_before_persisting_the_mirror() {
        let mut device = FakeTouchpad {
            delay: 3,
            ..Default::default()
        };
        apply(&mut device, true).unwrap();
        assert_eq!(device.waits, 3);
        assert_eq!(device.writes, [true]);
    }
    #[test]
    fn missing_state_and_rejected_input_do_not_write_the_mirror() {
        for mut device in [
            FakeTouchpad {
                missing: true,
                ..Default::default()
            },
            FakeTouchpad {
                fail_toggle: true,
                ..Default::default()
            },
        ] {
            assert!(apply(&mut device, true).is_err());
            assert!(device.writes.is_empty());
            assert!(!device.enabled);
        }
    }
    #[test]
    fn an_ignored_chord_times_out_without_claiming_a_change() {
        let mut device = FakeTouchpad {
            ignore_toggle: true,
            ..Default::default()
        };
        assert!(apply(&mut device, true).unwrap_err().contains("500 ms"));
        assert_eq!(device.toggles, 1);
        assert_eq!(device.waits, 20);
        assert!(device.writes.is_empty());
    }
    #[test]
    fn mirror_failure_restores_the_confirmed_windows_state() {
        let mut device = FakeTouchpad {
            fail_persist: true,
            ..Default::default()
        };
        assert!(
            apply(&mut device, true)
                .unwrap_err()
                .contains("previous touchpad state restored")
        );
        assert_eq!(device.toggles, 2);
        assert_eq!(device.writes, [true, false]);
        assert!(!device.enabled);
    }
}
