#[cfg(windows)]
pub(crate) const INJECTION_MAGIC: usize = 0x4e45_4f46;

#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct KeyEvent {
    key: u16,
    up: bool,
}

#[cfg(any(windows, test))]
fn chord_events(keys: &[(u16, bool)]) -> Vec<KeyEvent> {
    keys.iter()
        .filter(|(_, held)| !held)
        .map(|&(key, _)| KeyEvent { key, up: false })
        .chain(
            keys.iter()
                .rev()
                .filter(|(_, held)| !held)
                .map(|&(key, _)| KeyEvent { key, up: true }),
        )
        .collect()
}

#[cfg(any(windows, test))]
fn release_events(sent: &[KeyEvent]) -> Vec<KeyEvent> {
    let mut held = Vec::new();
    for event in sent {
        if event.up {
            held.retain(|&key| key != event.key);
        } else if !held.contains(&event.key) {
            held.push(event.key);
        }
    }
    held.into_iter()
        .rev()
        .map(|key| KeyEvent { key, up: true })
        .collect()
}

#[cfg(windows)]
fn keyboard_input(event: &KeyEvent) -> windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    };
    let extended = matches!(event.key, 0x5b | 0x5c | 0x90);
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: event.key,
                wScan: 0,
                dwFlags: if event.up { KEYEVENTF_KEYUP } else { 0 }
                    | if extended { KEYEVENTF_EXTENDEDKEY } else { 0 },
                time: 0,
                dwExtraInfo: INJECTION_MAGIC,
            },
        },
    }
}

#[cfg(windows)]
pub(crate) fn held_keys(keys: &[u16]) -> Vec<(u16, bool)> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    keys.iter()
        .map(|&key| {
            // SAFETY: GetAsyncKeyState takes a virtual-key code and no pointers.
            (key, unsafe { GetAsyncKeyState(i32::from(key)) } < 0)
        })
        .collect()
}

#[cfg(windows)]
pub(crate) fn insert_inputs(
    inputs: &[windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT],
) -> usize {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{INPUT, SendInput};
    // SAFETY: The slice contains initialized INPUT values, remains live for
    // the synchronous call, and its exact length and INPUT stride are passed.
    unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        ) as usize
    }
}

#[cfg(windows)]
fn send_chord(keys: &[u16]) -> Result<(), String> {
    let held = held_keys(keys);
    if held.last().is_some_and(|(_, held)| *held) {
        return Err("input trigger is already held".to_owned());
    }
    let events = chord_events(&held);
    let inputs: Vec<_> = events.iter().map(keyboard_input).collect();
    let sent = insert_inputs(&inputs);
    if sent == inputs.len() {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    // Release only keys that this batch pressed. Existing user-held keys
    // were excluded when building the batch and never enter this recovery.
    let releases: Vec<_> = release_events(&events[..sent.min(events.len())])
        .iter()
        .map(keyboard_input)
        .collect();
    if !releases.is_empty() && insert_inputs(&releases) != releases.len() {
        return Err(format!(
            "Windows inserted {sent}/{} input events: {error}; key release incomplete",
            inputs.len()
        ));
    }
    Err(format!(
        "Windows inserted {sent}/{} input events: {error}",
        inputs.len()
    ))
}

#[cfg(windows)]
pub(crate) fn send_virtual_key(vk: u8) -> Result<(), String> {
    send_chord(&[u16::from(vk)])
}

#[cfg(not(windows))]
pub(crate) fn send_virtual_key(_vk: u8) -> Result<(), String> {
    Err("virtual key input is only available on Windows".to_owned())
}

#[cfg(windows)]
pub(crate) fn send_touchpad_chord() -> Result<(), String> {
    send_chord(&[0x11, 0x5b, 0x87])
}

#[cfg(not(windows))]
pub(crate) fn send_touchpad_chord() -> Result<(), String> {
    Err("touchpad input is only available on Windows".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chord_leaves_previously_held_modifiers_untouched() {
        for mask in 0..4 {
            let keys = [(0x11, mask & 1 != 0), (0x5b, mask & 2 != 0), (0x87, false)];
            let events = chord_events(&keys);
            for (key, held) in keys {
                let matching: Vec<_> = events.iter().filter(|event| event.key == key).collect();
                assert_eq!(matching.len(), if held { 0 } else { 2 });
            }
            assert!(release_events(&events).is_empty());
        }
    }
    #[test]
    fn every_partial_batch_releases_only_its_own_pressed_keys() {
        let events = chord_events(&[(0x11, true), (0x5b, false), (0x87, false)]);
        for length in 0..=events.len() {
            let prefix = &events[..length];
            let releases = release_events(prefix);
            assert!(releases.iter().all(|event| event.up && event.key != 0x11));
            let mut recovered = prefix.to_vec();
            recovered.extend(releases);
            assert!(release_events(&recovered).is_empty());
        }
    }
}
