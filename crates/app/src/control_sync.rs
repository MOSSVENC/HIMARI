//! Orders control writes and snapshots while preserving local editing state.

use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub(crate) enum Control {
    Power,
    Fan,
    FanCurve,
    FanRpm,
    FanOffset,
    AntiDust,
    Lighting,
    LightingBrightness,
    LightingEffect,
    LightingBoot,
    LightingSleep,
    DisplayCare,
    MonitorTimeout,
    ScreenSaver,
    QuickToggles,
    Camera,
    Touchpad,
    Airplane,
    Audio,
    AudioMute,
    Brightness,
    BlueLight,
    Charging,
    Gpu,
    LowRefresh,
}
const ALL_CONTROLS: &[Control] = &[
    Control::Power,
    Control::Fan,
    Control::FanCurve,
    Control::FanRpm,
    Control::FanOffset,
    Control::AntiDust,
    Control::Lighting,
    Control::LightingBrightness,
    Control::LightingEffect,
    Control::LightingBoot,
    Control::LightingSleep,
    Control::DisplayCare,
    Control::MonitorTimeout,
    Control::ScreenSaver,
    Control::QuickToggles,
    Control::Camera,
    Control::Touchpad,
    Control::Airplane,
    Control::Audio,
    Control::AudioMute,
    Control::Brightness,
    Control::BlueLight,
    Control::Charging,
    Control::Gpu,
    Control::LowRefresh,
];
const COUNT: usize = ALL_CONTROLS.len();

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Controls(u32);
impl Controls {
    pub(crate) const ALL: Self = Self((1 << COUNT) - 1);
    pub(crate) const NONE: Self = Self(0);
    pub(crate) const fn one(control: Control) -> Self {
        Self(1 << control as u8)
    }
    pub(crate) const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
    pub(crate) const fn contains(self, control: Control) -> bool {
        self.0 & Self::one(control).0 != 0
    }
    pub(crate) fn any(self, controls: &[Control]) -> bool {
        controls.iter().any(|control| self.contains(*control))
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Revision {
    issued: u64,
    completed: u64,
    pending: u64,
    acknowledged: Option<u64>,
}

#[derive(Clone, Default)]
pub(crate) struct ControlSync(Arc<Mutex<[Revision; COUNT]>>);

pub(crate) struct WriteTicket {
    sync: ControlSync,
    controls: Controls,
    revisions: [u64; COUNT],
}

impl ControlSync {
    pub(crate) fn issue(&self, controls: Controls) -> WriteTicket {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let mut revisions = [0; COUNT];
        for (index, revision) in state.iter_mut().enumerate() {
            if controls.0 & (1 << index) != 0 {
                revision.issued += 1;
                revision.pending += 1;
                revisions[index] = revision.issued;
            }
        }
        WriteTicket {
            sync: self.clone(),
            controls,
            revisions,
        }
    }
    pub(crate) fn snapshot(&self) -> ReadStamp {
        ReadStamp {
            sync: self.clone(),
            revisions: *self.0.lock().unwrap_or_else(|error| error.into_inner()),
        }
    }
}

// Completion also runs if sending to a disconnected worker fails.
impl Drop for WriteTicket {
    fn drop(&mut self) {
        let mut state = self
            .sync
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for (index, revision) in state.iter_mut().enumerate() {
            if self.controls.0 & (1 << index) != 0 {
                revision.completed = revision.completed.max(self.revisions[index]);
                revision.pending -= 1;
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct ReadStamp {
    sync: ControlSync,
    revisions: [Revision; COUNT],
}
impl ReadStamp {
    pub(crate) fn accepted_controls(&self) -> Controls {
        let state = self
            .sync
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        ALL_CONTROLS
            .iter()
            .copied()
            .filter(|control| self.is_current(*control, &state))
            .fold(Controls::NONE, |mask, control| {
                mask.union(Controls::one(control))
            })
    }
    pub(crate) fn selection_controls(&self) -> Controls {
        let state = self
            .sync
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        ALL_CONTROLS
            .iter()
            .copied()
            .filter(|control| {
                self.is_current(*control, &state)
                    && state[*control as usize].acknowledged
                        != Some(self.revisions[*control as usize].issued)
            })
            .fold(Controls::NONE, |mask, control| {
                mask.union(Controls::one(control))
            })
    }
    // Only a successfully delivered value completes selection synchronization.
    // Missing data leaves it pending for the next valid snapshot.
    pub(crate) fn acknowledge(&self, observed: Controls) {
        let mut state = self
            .sync
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for &control in ALL_CONTROLS {
            if observed.contains(control) && self.is_current(control, &state) {
                state[control as usize].acknowledged =
                    Some(self.revisions[control as usize].issued);
            }
        }
    }
    #[cfg(test)]
    pub(crate) fn accepts(&self, control: Control) -> bool {
        let state = self
            .sync
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.is_current(control, &state)
    }
    fn is_current(&self, control: Control, state: &[Revision; COUNT]) -> bool {
        let index = control as usize;
        let captured = self.revisions[index];
        let current = state[index];
        captured.pending == 0
            && current.pending == 0
            && captured.issued == captured.completed
            && captured.issued == current.issued
            && current.issued == current.completed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn synchronization_waits_for_an_observed_value_and_rearms_for_each_write() {
        let sync = ControlSync::default();
        let initial = sync.snapshot();
        initial.acknowledge(Controls::one(Control::Audio));
        assert!(!initial.selection_controls().contains(Control::Audio));
        assert!(initial.selection_controls().contains(Control::Lighting));
        let write = sync.issue(Controls::one(Control::Audio));
        initial.acknowledge(Controls::one(Control::Audio));
        drop(write);
        let latest = sync.snapshot();
        assert!(latest.selection_controls().contains(Control::Audio));
        latest.acknowledge(Controls::one(Control::Audio));
        assert!(!latest.selection_controls().contains(Control::Audio));
    }
    #[test]
    fn an_in_flight_write_rejects_earlier_and_mid_write_samples() {
        let sync = ControlSync::default();
        let before = sync.snapshot();
        let ticket = sync.issue(Controls::one(Control::Lighting));
        let during = sync.snapshot();
        assert!(!before.accepts(Control::Lighting));
        assert!(!during.accepts(Control::Lighting));
        assert!(before.accepts(Control::Audio));
        drop(ticket);
        assert!(!before.accepts(Control::Lighting));
        assert!(!during.accepts(Control::Lighting));
        assert!(sync.snapshot().accepts(Control::Lighting));
    }
    #[test]
    fn an_older_completion_cannot_acknowledge_a_newer_click() {
        let sync = ControlSync::default();
        let first = sync.issue(Controls::one(Control::QuickToggles));
        let second = sync.issue(Controls::one(Control::QuickToggles));
        drop(first);
        assert!(!sync.snapshot().accepts(Control::QuickToggles));
        drop(second);
        assert!(sync.snapshot().accepts(Control::QuickToggles));
    }
    #[test]
    fn completion_order_keeps_every_outstanding_write_pending() {
        let sync = ControlSync::default();
        let first = sync.issue(Controls::one(Control::Lighting));
        let second = sync.issue(Controls::one(Control::Lighting));
        drop(second);
        assert!(!sync.snapshot().accepts(Control::Lighting));
        drop(first);
        assert!(sync.snapshot().accepts(Control::Lighting));
    }
    #[test]
    fn combined_writes_cover_only_their_affected_controls() {
        let sync = ControlSync::default();
        let controls = Controls::one(Control::Power).union(Controls::one(Control::Gpu));
        let ticket = sync.issue(controls);
        let reading = sync.snapshot();
        assert!(!reading.accepts(Control::Power));
        assert!(!reading.accepts(Control::Gpu));
        assert!(reading.accepts(Control::Fan));
        assert!(controls.contains(Control::Power));
        assert!(!controls.contains(Control::Fan));
        drop(ticket);
    }
}
