//! Executes one macro action and releases the inputs it successfully pressed.

use ram_control_center_protocol::{FlexiKeyAction, MacroStep, MacroStepKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReplayKey {
    Scan(u16),
    Unicode(u16),
}

pub(crate) trait ReplaySink {
    fn held(&mut self, scan: u16) -> bool;
    fn send(&mut self, key: ReplayKey, up: bool) -> Result<(), String>;
    fn delay(&mut self, milliseconds: u64);
}

enum Step {
    Delay(u64),
    Key(ReplayKey, bool),
}

fn append_steps(plan: &mut Vec<Step>, steps: &[MacroStep]) -> Result<(), String> {
    for step in steps {
        let key = u16::try_from(step.key_code)
            .ok()
            .filter(|key| *key != 0)
            .ok_or_else(|| format!("invalid macro key {}", step.key_code))?;
        if step.delay_ms > 0 {
            plan.push(Step::Delay(step.delay_ms.min(5000) as u64));
        }
        plan.push(Step::Key(
            ReplayKey::Scan(key),
            step.kind == MacroStepKind::Break,
        ));
    }
    Ok(())
}

pub(crate) fn replay(action: &FlexiKeyAction, sink: &mut impl ReplaySink) -> Result<(), String> {
    // Validate the complete action before producing any input, including the
    // final shortcut of a text action, whose modifiers may span the text.
    let mut plan = Vec::new();
    match action {
        FlexiKeyAction::Keypress { steps } => append_steps(&mut plan, steps)?,
        FlexiKeyAction::Text {
            start_keys,
            text,
            send_keys,
        } => {
            append_steps(&mut plan, start_keys)?;
            for unit in text.encode_utf16() {
                plan.push(Step::Key(ReplayKey::Unicode(unit), false));
                plan.push(Step::Key(ReplayKey::Unicode(unit), true));
                plan.push(Step::Delay(2));
            }
            append_steps(&mut plan, send_keys)?;
        }
        _ => return Err("action does not contain a key sequence".to_owned()),
    }

    let mut owned = Vec::new();
    let mut borrowed = Vec::new();
    let mut outcome = Ok(());
    for step in plan {
        let Step::Key(key, up) = step else {
            if let Step::Delay(milliseconds) = step {
                sink.delay(milliseconds);
            }
            continue;
        };
        if up {
            if let Some(index) = borrowed.iter().position(|held| *held == key) {
                borrowed.remove(index);
                continue;
            }
            if !owned.contains(&key) {
                continue;
            }
        } else if borrowed.contains(&key) {
            continue;
        } else if !owned.contains(&key)
            && let ReplayKey::Scan(scan) = key
            && sink.held(scan)
        {
            borrowed.push(key);
            continue;
        }
        if let Err(error) = sink.send(key, up) {
            outcome = Err(error);
            break;
        }
        if up {
            owned.retain(|held| *held != key);
        } else if !owned.contains(&key) {
            owned.push(key);
        }
    }

    // Continue after individual release failures so every outstanding key gets
    // its own recovery attempt. Never release inputs borrowed from the user.
    let mut failures = Vec::new();
    for key in owned.into_iter().rev() {
        if let Err(error) = sink.send(key, true) {
            failures.push(format!("{key:?}: {error}"));
        }
    }
    if !failures.is_empty() {
        let release = format!("macro key release incomplete: {}", failures.join("; "));
        return Err(match outcome {
            Ok(()) => release,
            Err(error) => format!("{error}; {release}"),
        });
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[derive(Default)]
    struct Sink {
        held: Vec<u16>,
        attempted: Vec<(ReplayKey, bool)>,
        sent: Vec<(ReplayKey, bool)>,
        fail_at: HashSet<usize>,
        delays: Vec<u64>,
    }

    impl ReplaySink for Sink {
        fn held(&mut self, key: u16) -> bool {
            self.held.contains(&key)
        }
        fn send(&mut self, key: ReplayKey, up: bool) -> Result<(), String> {
            let index = self.attempted.len();
            self.attempted.push((key, up));
            if self.fail_at.contains(&index) {
                return Err(format!("send {index} failed"));
            }
            self.sent.push((key, up));
            Ok(())
        }
        fn delay(&mut self, milliseconds: u64) {
            self.delays.push(milliseconds);
        }
    }

    fn step(key: i64, up: bool) -> MacroStep {
        MacroStep {
            key_code: key,
            kind: if up {
                MacroStepKind::Break
            } else {
                MacroStepKind::Make
            },
            delay_ms: 0,
            group: 0,
            display: String::new(),
        }
    }

    fn chord() -> FlexiKeyAction {
        FlexiKeyAction::Keypress {
            steps: vec![
                step(29, false),
                step(31, false),
                step(31, true),
                step(29, true),
            ],
        }
    }

    fn assert_balanced(sink: &Sink) {
        let mut down = Vec::new();
        for &(key, up) in &sink.sent {
            if up {
                down.retain(|held| *held != key);
            } else if !down.contains(&key) {
                down.push(key);
            }
        }
        assert!(down.is_empty(), "unreleased inputs: {down:?}");
    }

    #[test]
    fn every_failed_chord_event_releases_successful_presses() {
        for index in 0..4 {
            let mut sink = Sink {
                fail_at: HashSet::from([index]),
                ..Sink::default()
            };
            assert!(replay(&chord(), &mut sink).is_err());
            assert_balanced(&sink);
        }
    }

    #[test]
    fn held_user_keys_and_unmatched_releases_are_untouched() {
        let action = FlexiKeyAction::Keypress {
            steps: vec![
                step(42, true),
                step(29, false),
                step(29, false),
                step(31, false),
                step(31, true),
                step(29, true),
            ],
        };
        let mut sink = Sink {
            held: vec![29],
            ..Sink::default()
        };
        replay(&action, &mut sink).unwrap();
        assert_eq!(
            sink.sent,
            [(ReplayKey::Scan(31), false), (ReplayKey::Scan(31), true)]
        );
    }

    #[test]
    fn incomplete_sequences_release_in_reverse_press_order() {
        let action = FlexiKeyAction::Keypress {
            steps: vec![step(29, false), step(42, false), step(42, false)],
        };
        let mut sink = Sink::default();
        replay(&action, &mut sink).unwrap();
        assert_eq!(
            &sink.sent[3..],
            [(ReplayKey::Scan(42), true), (ReplayKey::Scan(29), true)]
        );
        assert_balanced(&sink);
    }

    #[test]
    fn text_action_keeps_modifiers_across_text_and_releases_each_failed_event() {
        let action = FlexiKeyAction::Text {
            start_keys: vec![step(29, false)],
            text: "中🦀".into(),
            send_keys: vec![step(29, true)],
        };
        let mut complete = Sink::default();
        replay(&action, &mut complete).unwrap();
        assert_eq!(complete.sent.len(), 8);
        assert_eq!(complete.sent.last(), Some(&(ReplayKey::Scan(29), true)));
        assert_balanced(&complete);
        for index in 0..complete.sent.len() {
            let mut sink = Sink {
                fail_at: HashSet::from([index]),
                ..Sink::default()
            };
            assert!(replay(&action, &mut sink).is_err());
            assert_balanced(&sink);
        }
    }

    #[test]
    fn invalid_final_steps_are_rejected_before_any_input_or_delay() {
        for key in [-1, 0, 65536] {
            let action = FlexiKeyAction::Text {
                start_keys: vec![step(29, false)],
                text: "hello".into(),
                send_keys: vec![step(key, true)],
            };
            let mut sink = Sink::default();
            assert!(replay(&action, &mut sink).is_err());
            assert!(sink.attempted.is_empty());
            assert!(sink.delays.is_empty());
        }
    }

    #[test]
    fn recovery_attempts_all_releases_and_reports_both_errors() {
        let action = FlexiKeyAction::Keypress {
            steps: vec![step(29, false), step(42, false), step(31, false)],
        };
        let mut sink = Sink {
            fail_at: HashSet::from([2, 3]),
            ..Sink::default()
        };
        let error = replay(&action, &mut sink).unwrap_err();
        assert!(error.contains("send 2 failed") && error.contains("send 3 failed"));
        assert_eq!(
            &sink.attempted[3..],
            [(ReplayKey::Scan(42), true), (ReplayKey::Scan(29), true)]
        );
    }

    #[test]
    fn final_release_failure_is_reported_after_successful_steps() {
        let action = FlexiKeyAction::Keypress {
            steps: vec![step(29, false), step(42, false)],
        };
        let mut sink = Sink {
            fail_at: HashSet::from([2]),
            ..Sink::default()
        };
        let error = replay(&action, &mut sink).unwrap_err();
        assert!(error.contains("macro key release incomplete"));
        assert_eq!(
            &sink.attempted[2..],
            [(ReplayKey::Scan(42), true), (ReplayKey::Scan(29), true)]
        );
    }

    #[test]
    fn macro_delays_keep_the_existing_bounds() {
        let mut steps = vec![step(29, false), step(29, true), step(31, true)];
        for (step, delay) in steps.iter_mut().zip([-1, 20, 9000]) {
            step.delay_ms = delay;
        }
        let mut sink = Sink::default();
        replay(&FlexiKeyAction::Keypress { steps }, &mut sink).unwrap();
        assert_eq!(sink.delays, [20, 5000]);
    }
}
