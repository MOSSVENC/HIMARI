//! Input ownership and trigger capture, independent of the Windows hook transport.

#![forbid(unsafe_code)]
use himari_protocol::FlexiKeyAction;
use std::collections::HashMap;

pub(crate) type Actions = HashMap<u16, FlexiKeyAction>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Device {
    Keyboard,
    Mouse,
}

impl Device {
    fn index(self) -> usize {
        match self {
            Self::Keyboard => 0,
            Self::Mouse => 1,
        }
    }
    pub(crate) fn as_u8(self) -> u8 {
        self.index() as u8 + 1
    }
}

pub(crate) enum InputDecision {
    Pass,
    Suppress,
    Dispatch(FlexiKeyAction),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Windows,
    Engine,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CapturedInput {
    pub(crate) device: Device,
    pub(crate) key_code: u16,
    pub(crate) name: String,
}

#[derive(Clone, Default)]
enum Capture {
    #[default]
    Idle,
    Armed,
    Down(CapturedInput),
    Ready(CapturedInput),
}

#[derive(Clone, Default)]
pub(crate) struct InputRouter {
    actions: [Actions; 2],
    held: [HashMap<u16, Owner>; 2],
    capture: Capture,
}

impl InputRouter {
    /// Bindings may change while a physical key is held. Its first press owns
    /// the complete sequence through repeats and release, across all changes.
    pub(crate) fn replace_bindings(&mut self, keyboard: Actions, mouse: Actions) -> [Actions; 2] {
        std::mem::replace(&mut self.actions, [keyboard, mouse])
    }

    pub(crate) fn arm_capture(&mut self) {
        self.capture = Capture::Armed;
    }

    /// Discard the capture result while retaining ownership of swallowed presses.
    pub(crate) fn cancel_capture(&mut self) {
        self.capture = Capture::Idle;
    }

    pub(crate) fn take_capture(&mut self) -> Option<CapturedInput> {
        let Capture::Ready(trigger) = &self.capture else {
            return None;
        };
        let trigger = trigger.clone();
        self.capture = Capture::Idle;
        Some(trigger)
    }

    pub(crate) fn hook_requirements(&self) -> (bool, bool) {
        let capturing = !matches!(self.capture, Capture::Idle);
        let needed = |device: usize| {
            capturing
                || !self.actions[device].is_empty()
                || self.held[device]
                    .values()
                    .any(|owner| *owner == Owner::Engine)
        };
        (needed(0), needed(1))
    }

    /// Called on hook activation before its message loop processes input.
    pub(crate) fn seed_pressed(&mut self, device: Device, keys: impl IntoIterator<Item = u16>) {
        let held = &mut self.held[device.index()];
        for key in keys {
            held.entry(key).or_insert(Owner::Windows);
        }
    }

    pub(crate) fn finish_unhook(&mut self, device: Device) {
        self.held[device.index()].retain(|_, owner| *owner == Owner::Engine);
    }

    pub(crate) fn route(
        &mut self,
        device: Device,
        key: u16,
        down: bool,
        name: impl FnOnce() -> String,
    ) -> InputDecision {
        let index = device.index();
        if !down {
            return match self.held[index].remove(&key) {
                Some(Owner::Engine) => {
                    if let Capture::Down(trigger) = &self.capture
                        && trigger.device == device
                        && trigger.key_code == key
                    {
                        self.capture = Capture::Ready(trigger.clone());
                    }
                    InputDecision::Suppress
                }
                _ => InputDecision::Pass,
            };
        }
        if let Some(owner) = self.held[index].get(&key) {
            return match owner {
                Owner::Windows => InputDecision::Pass,
                Owner::Engine => InputDecision::Suppress,
            };
        }
        if matches!(self.capture, Capture::Armed) {
            self.held[index].insert(key, Owner::Engine);
            self.capture = Capture::Down(CapturedInput {
                device,
                key_code: key,
                name: name(),
            });
            return InputDecision::Suppress;
        }
        if let Some(action) = self.actions[index].get(&key).cloned() {
            self.held[index].insert(key, Owner::Engine);
            InputDecision::Dispatch(action)
        } else {
            self.held[index].insert(key, Owner::Windows);
            InputDecision::Pass
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Device::{Keyboard as K, Mouse as M};

    fn press(router: &mut InputRouter, device: Device, key: u16) -> InputDecision {
        router.route(device, key, true, || format!("key {key}"))
    }
    fn release(router: &mut InputRouter, device: Device, key: u16) -> InputDecision {
        router.route(device, key, false, || panic!("release requested a name"))
    }
    fn bind(router: &mut InputRouter, key: u16) {
        router.replace_bindings(
            HashMap::from([(key, FlexiKeyAction::Disable)]),
            HashMap::new(),
        );
    }

    #[test]
    fn release_of_a_previously_held_modifier_reaches_windows() {
        let mut r = InputRouter::default();
        bind(&mut r, 29);
        assert!(matches!(release(&mut r, K, 29), InputDecision::Pass));
    }

    #[test]
    fn mapped_press_repeats_and_release_are_balanced() {
        let mut r = InputRouter::default();
        bind(&mut r, 118);
        assert!(matches!(
            press(&mut r, K, 118),
            InputDecision::Dispatch(FlexiKeyAction::Disable)
        ));
        assert!(matches!(press(&mut r, K, 118), InputDecision::Suppress));
        assert!(matches!(release(&mut r, K, 118), InputDecision::Suppress));
        assert!(matches!(release(&mut r, K, 118), InputDecision::Pass));
    }

    #[test]
    fn removing_a_binding_keeps_held_repeats_suppressed_until_release() {
        let mut r = InputRouter::default();
        bind(&mut r, 118);
        assert!(matches!(press(&mut r, K, 118), InputDecision::Dispatch(_)));
        r.replace_bindings(Actions::new(), Actions::new());
        assert_eq!(r.hook_requirements(), (true, false));
        assert!(matches!(press(&mut r, K, 118), InputDecision::Suppress));
        assert!(matches!(release(&mut r, K, 118), InputDecision::Suppress));
        assert_eq!(r.hook_requirements(), (false, false));
        assert!(matches!(press(&mut r, K, 118), InputDecision::Pass));
    }

    #[test]
    fn capture_swallows_every_repeat_and_its_release() {
        let mut r = InputRouter::default();
        r.arm_capture();
        for _ in 0..20 {
            assert!(matches!(press(&mut r, K, 30), InputDecision::Suppress));
        }
        assert!(r.take_capture().is_none());
        assert!(matches!(release(&mut r, K, 30), InputDecision::Suppress));
        assert_eq!(
            r.take_capture(),
            Some(CapturedInput {
                device: K,
                key_code: 30,
                name: "key 30".into()
            })
        );
        assert_eq!(r.hook_requirements(), (false, false));
    }

    #[test]
    fn canceled_capture_drains_held_input_before_unhooking() {
        for device in [K, M] {
            let mut r = InputRouter::default();
            r.arm_capture();
            assert!(matches!(press(&mut r, device, 1), InputDecision::Suppress));
            r.cancel_capture();
            assert_eq!(
                r.hook_requirements(),
                if device == K {
                    (true, false)
                } else {
                    (false, true)
                }
            );
            assert!(matches!(press(&mut r, device, 1), InputDecision::Suppress));
            assert!(matches!(
                release(&mut r, device, 1),
                InputDecision::Suppress
            ));
            assert!(r.take_capture().is_none());
            assert_eq!(r.hook_requirements(), (false, false));
        }
    }

    #[test]
    fn rearming_capture_retains_old_press_ownership_and_records_only_new_trigger() {
        let mut r = InputRouter::default();
        r.arm_capture();
        assert!(matches!(press(&mut r, K, 30), InputDecision::Suppress));
        r.arm_capture();
        assert!(matches!(press(&mut r, K, 30), InputDecision::Suppress));
        assert!(matches!(press(&mut r, M, 1), InputDecision::Suppress));
        assert!(matches!(release(&mut r, K, 30), InputDecision::Suppress));
        assert!(r.take_capture().is_none());
        assert!(matches!(release(&mut r, M, 1), InputDecision::Suppress));
        assert_eq!(r.take_capture().unwrap().device, M);
    }

    #[test]
    fn passed_input_keeps_its_release_when_capture_or_binding_is_armed() {
        let mut r = InputRouter::default();
        assert!(matches!(press(&mut r, K, 30), InputDecision::Pass));
        bind(&mut r, 30);
        r.arm_capture();
        assert!(matches!(press(&mut r, K, 30), InputDecision::Pass));
        assert!(matches!(release(&mut r, K, 30), InputDecision::Pass));
        assert!(r.take_capture().is_none());
        assert!(matches!(press(&mut r, K, 30), InputDecision::Suppress));
        assert!(matches!(release(&mut r, K, 30), InputDecision::Suppress));
        assert!(r.take_capture().is_some());
    }

    #[test]
    fn preexisting_physical_presses_are_seeded_without_reassigning_owned_presses() {
        let mut r = InputRouter::default();
        bind(&mut r, 30);
        r.seed_pressed(K, [30]);
        assert!(matches!(press(&mut r, K, 30), InputDecision::Pass));
        assert!(matches!(release(&mut r, K, 30), InputDecision::Pass));
        assert!(matches!(press(&mut r, K, 30), InputDecision::Dispatch(_)));
        r.seed_pressed(K, [30]);
        assert!(matches!(press(&mut r, K, 30), InputDecision::Suppress));
        assert!(matches!(release(&mut r, K, 30), InputDecision::Suppress));
    }

    #[test]
    fn keyboard_and_mouse_ownership_are_independent() {
        assert_eq!(K.as_u8(), 1);
        assert_eq!(M.as_u8(), 2);
        let mut r = InputRouter::default();
        bind(&mut r, 1);
        assert!(matches!(press(&mut r, K, 1), InputDecision::Dispatch(_)));
        assert!(matches!(press(&mut r, M, 1), InputDecision::Pass));
        assert!(matches!(release(&mut r, M, 1), InputDecision::Pass));
        assert!(matches!(release(&mut r, K, 1), InputDecision::Suppress));
    }

    #[test]
    fn unhook_discards_passed_state_and_keeps_owed_releases() {
        let mut r = InputRouter::default();
        bind(&mut r, 30);
        press(&mut r, K, 30);
        press(&mut r, K, 31);
        r.finish_unhook(K);
        assert!(matches!(release(&mut r, K, 30), InputDecision::Suppress));
        assert!(matches!(release(&mut r, K, 31), InputDecision::Pass));
    }

    #[test]
    fn ownership_is_stable_across_event_and_configuration_interleavings() {
        fn walk(router: InputRouter, held: Option<bool>, depth: usize) {
            if depth == 0 {
                return;
            }
            for operation in 0..7 {
                let mut r = router.clone();
                let mut h = held;
                match operation {
                    0 | 1 => {
                        let suppressed = !matches!(press(&mut r, K, 30), InputDecision::Pass);
                        if let Some(expected) = held {
                            assert_eq!(suppressed, expected);
                        }
                        h = Some(suppressed);
                    }
                    2 => {
                        let suppressed = !matches!(release(&mut r, K, 30), InputDecision::Pass);
                        assert_eq!(suppressed, held.unwrap_or(false));
                        h = None;
                    }
                    3 => r.arm_capture(),
                    4 => r.cancel_capture(),
                    5 => bind(&mut r, 30),
                    6 => {
                        r.replace_bindings(Actions::new(), Actions::new());
                    }
                    _ => unreachable!(),
                }
                walk(r, h, depth - 1);
            }
        }
        walk(InputRouter::default(), None, 6);
    }
}
