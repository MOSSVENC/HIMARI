//! Typed control requests and their ordering metadata.

use crate::control_sync::{Control, ControlSync, Controls, WriteTicket};
use ram_control_center_protocol::{
    AntiDustSchedule, CustomFanCurve, FanMode, GpuSwitchMode, PowerMode, QuickToggle, RgbColor,
};
use std::sync::mpsc::{self, Receiver, SendError, Sender};

pub(crate) enum WorkerCommand {
    ApplyPower(PowerMode),
    ApplyFan(FanMode),
    ApplySlowFan(bool),
    ApplyTurboFan(bool),
    ApplyFanOffset(u8),
    RunAntiDust,
    ApplyAntiDustSchedule(AntiDustSchedule),
    ApplyDisplayToggle(u8, bool),
    ApplyMonitorTimeouts(u32, u32),
    ApplyScreenSaver(bool, u8, u32),
    ApplyRpmEdit {
        cpu: [(u8, u16); 10],
        gpu: [(u8, u16); 10],
    },
    ApplyQuickToggle(QuickToggle, bool),
    ApplyLowRefresh(bool),
    ApplyFanCurve(CustomFanCurve),
    ApplyQuiet(bool),
    ApplyGpuSwitch(GpuSwitchMode),
    ApplyFlexiCharger(u8, u8, u8),
    ApplyRgbBrightness(u8),
    ApplyRgbColor(RgbColor),
    ApplyRgbEffect(u8),
    ApplyRgbSleepTimer(bool, u32),
    ApplyRgbBootEffect(bool),
    ApplyCamera(bool),
    ApplyTouchpad(bool),
    ApplyAirplane(bool),
    ApplyLockKey(u8),
    ApplyBrightness(u8),
    ApplyBlueLight(u8),
    ApplyVolume(u8),
    ApplyMute(bool),
    Refresh,
}

impl WorkerCommand {
    pub(super) fn controls(&self) -> Controls {
        use Control::*;
        use WorkerCommand::*;
        match self {
            ApplyPower(_) => Controls::one(Power).union(Controls::one(Fan)),
            ApplyQuiet(_) => Controls::one(Power).union(Controls::one(Gpu)),
            ApplyFan(_) | ApplySlowFan(_) | ApplyTurboFan(_) => Controls::one(Fan),
            ApplyFanOffset(_) => Controls::one(FanOffset),
            RunAntiDust | ApplyAntiDustSchedule(_) => Controls::one(AntiDust),
            ApplyRpmEdit { .. } => Controls::one(FanRpm).union(Controls::one(Fan)),
            ApplyFanCurve(_) => Controls::one(FanCurve).union(Controls::one(Fan)),
            ApplyRgbBrightness(_) => Controls::one(LightingBrightness),
            ApplyRgbColor(_) => Controls::one(Lighting).union(Controls::one(LightingEffect)),
            ApplyRgbEffect(_) => Controls::one(LightingEffect),
            ApplyRgbSleepTimer(_, _) => Controls::one(LightingSleep),
            ApplyRgbBootEffect(_) => Controls::one(LightingBoot),
            ApplyDisplayToggle(_, _) => Controls::one(DisplayCare),
            ApplyMonitorTimeouts(_, _) => Controls::one(MonitorTimeout),
            ApplyScreenSaver(_, _, _) => Controls::one(ScreenSaver),
            ApplyQuickToggle(_, _) => Controls::one(QuickToggles),
            ApplyCamera(_) => Controls::one(Camera),
            ApplyTouchpad(_) => Controls::one(Touchpad),
            ApplyAirplane(_) => Controls::one(Airplane),
            ApplyVolume(_) => Controls::one(Audio),
            ApplyMute(_) => Controls::one(AudioMute),
            ApplyBrightness(_) => Controls::one(Brightness),
            ApplyBlueLight(_) => Controls::one(BlueLight),
            ApplyFlexiCharger(_, _, _) => Controls::one(Charging),
            ApplyGpuSwitch(_) => Controls::one(Gpu),
            ApplyLowRefresh(_) => Controls::one(LowRefresh),
            ApplyLockKey(_) => Controls::NONE,
            Refresh => Controls::ALL,
        }
    }
}

pub(super) struct QueuedCommand {
    pub(super) command: WorkerCommand,
    pub(super) ticket: WriteTicket,
}

#[derive(Clone)]
pub(crate) struct CommandSender {
    sender: Sender<QueuedCommand>,
    sync: ControlSync,
}

impl CommandSender {
    pub(crate) fn send(&self, command: WorkerCommand) -> Result<(), SendError<WorkerCommand>> {
        let ticket = self.sync.issue(command.controls());
        self.sender
            .send(QueuedCommand { command, ticket })
            .map_err(|error| SendError(error.0.command))
    }
}

pub(crate) struct CommandReceiver {
    pub(super) receiver: Receiver<QueuedCommand>,
    pub(super) sync: ControlSync,
}

pub(crate) fn channel() -> (CommandSender, CommandReceiver) {
    let (sender, receiver) = mpsc::channel();
    let sync = ControlSync::default();
    (
        CommandSender {
            sender,
            sync: sync.clone(),
        },
        CommandReceiver { receiver, sync },
    )
}
