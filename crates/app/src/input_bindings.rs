//! Decisions for mapped input, independent of the Windows hook transport.

use std::collections::{HashMap, HashSet};

use ram_control_center_protocol::FlexiKeyAction;

pub(crate) enum InputDecision {
    Pass,
    Suppress,
    Dispatch(FlexiKeyAction),
}

pub(crate) fn dispatch(
    actions: &HashMap<u16, FlexiKeyAction>,
    suppressed_down: &mut HashSet<u16>,
    key: u16,
    down: bool,
) -> InputDecision {
    if !down {
        // A release may belong to a press made before the hook/profile became
        // active. Windows must receive that release to clear its modifier state.
        return if suppressed_down.remove(&key) {
            InputDecision::Suppress
        } else {
            InputDecision::Pass
        };
    }
    match actions.get(&key) {
        Some(action) if suppressed_down.insert(key) => InputDecision::Dispatch(action.clone()),
        Some(_) => InputDecision::Suppress,
        None => InputDecision::Pass,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_of_a_previously_held_modifier_reaches_windows() {
        let actions = HashMap::from([(0x1d, FlexiKeyAction::Disable)]);
        assert!(matches!(
            dispatch(&actions, &mut HashSet::new(), 0x1d, false),
            InputDecision::Pass
        ));
    }

    #[test]
    fn mapped_press_repeats_and_release_are_balanced() {
        let actions = HashMap::from([(0x76, FlexiKeyAction::Disable)]);
        let mut pressed = HashSet::new();
        assert!(matches!(
            dispatch(&actions, &mut pressed, 0x76, true),
            InputDecision::Dispatch(FlexiKeyAction::Disable)
        ));
        assert!(matches!(
            dispatch(&actions, &mut pressed, 0x76, true),
            InputDecision::Suppress
        ));
        assert!(matches!(
            dispatch(&actions, &mut pressed, 0x76, false),
            InputDecision::Suppress
        ));
        assert!(matches!(
            dispatch(&actions, &mut pressed, 0x76, false),
            InputDecision::Pass
        ));
        assert!(pressed.is_empty());
    }

    #[test]
    fn unrelated_input_always_passes() {
        let actions = HashMap::from([(0x76, FlexiKeyAction::Disable)]);
        let mut pressed = HashSet::new();
        for down in [true, true, false] {
            assert!(matches!(
                dispatch(&actions, &mut pressed, 0x1d, down),
                InputDecision::Pass
            ));
        }
        assert!(pressed.is_empty());
    }
}
