mod commands;
mod input;
mod io_controls;
mod power_fan;
mod rgb;
mod snapshot;

use std::sync::mpsc::RecvTimeoutError;
use std::thread;
use std::time::{Duration, Instant};

use ram_control_center_protocol::Dchu;
use slint::Weak;

use crate::MainWindow;
use crate::control_sync::{ControlSync, Controls};
use crate::diagnostics::log_action;
use crate::driver_manager::{self, DriverHealth};
use crate::localization::tr;
use crate::telemetry::SystemSampler;
use crate::thermal_cache::ThermalCache;
use crate::ui_update::UiUpdate;
pub(crate) use commands::{CommandReceiver, CommandSender, WorkerCommand, channel};

pub(super) struct CommandOutcome {
    status: Option<(String, i32)>,
}

impl CommandOutcome {
    pub(super) fn success(message: impl Into<String>) -> Self {
        Self {
            status: Some((message.into(), 1)),
        }
    }

    pub(super) fn silent_success() -> Self {
        Self { status: None }
    }

    pub(super) fn failure(message: impl ToString) -> Self {
        Self::with_tone(message.to_string(), 2)
    }

    pub(super) fn with_tone(message: String, tone: i32) -> Self {
        Self {
            status: Some((message, tone)),
        }
    }

    pub(super) fn from_result<E: ToString>(
        result: Result<(), E>,
        success: impl Into<String>,
    ) -> Self {
        match result {
            Ok(()) => Self::success(success),
            Err(error) => Self::failure(error),
        }
    }

    pub(super) fn missing_transport() -> Self {
        Self::failure(tr("Hardware transport is unavailable."))
    }
}

struct WorkerRuntime {
    weak: Weak<MainWindow>,
    sampler: SystemSampler,
    dchu: Option<Dchu>,
    status: Option<(String, i32)>,
    next_retry: Instant,
    control_sync: ControlSync,
    tick: u64,
    thermal: ThermalCache,
    thermal_failures: u8,
    driver_health: DriverHealth,
}

impl WorkerRuntime {
    fn new(weak: Weak<MainWindow>, control_sync: ControlSync) -> Self {
        log_action("worker: start");
        let sampler = SystemSampler::new();
        log_action("worker: sampler ready");
        log_action("worker: opening DCHU");
        let dchu = match Dchu::open() {
            Ok(dchu) => {
                log_action(&format!("worker: DCHU opened at {}", dchu.path()));
                Some(dchu)
            }
            Err(error) => {
                log_action(&format!("worker: DCHU open failed: {error}"));
                snapshot::push_update(
                    &weak,
                    UiUpdate {
                        transport_detail: tr("Signed codec not found").to_owned(),
                        status: Some((error.to_string(), 2)),
                        ..UiUpdate::default()
                    },
                );
                None
            }
        };
        let driver_health = driver_manager::detect(dchu.is_some());
        #[cfg(windows)]
        // The diagnostic checks the vendor forwarding service and ownership
        // flag before registering the direct notification channel.
        if dchu.is_some() && std::env::args().any(|arg| arg == "--trace-fn-events") {
            crate::fn_diagnostics::spawn_trace();
        }
        Self {
            weak,
            sampler,
            dchu,
            status: None,
            next_retry: Instant::now() + Duration::from_secs(5),
            control_sync,
            tick: 0,
            thermal: ThermalCache::default(),
            thermal_failures: 0,
            driver_health,
        }
    }

    fn run(mut self, receiver: CommandReceiver) {
        let mut next_sample = Instant::now() + Duration::from_secs(1);
        loop {
            match receiver
                .receiver
                .recv_timeout(next_sample.saturating_duration_since(Instant::now()))
            {
                Ok(request) => {
                    let commands::QueuedCommand { command, ticket } = request;
                    let refresh = matches!(&command, WorkerCommand::Refresh);
                    let scope = command.controls();
                    if refresh {
                        drop(ticket);
                        self.refresh();
                    } else {
                        let outcome = self.execute(command);
                        drop(ticket);
                        self.finish(outcome, scope);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            // Input commands must not postpone periodic telemetry indefinitely.
            if Instant::now() >= next_sample {
                self.on_timeout();
                next_sample = Instant::now() + Duration::from_secs(1);
            }
        }
    }

    fn execute(&mut self, command: WorkerCommand) -> CommandOutcome {
        match command {
            WorkerCommand::ApplyPower(mode) => power_fan::apply_power(self.dchu.as_ref(), mode),
            WorkerCommand::ApplyFan(mode) => power_fan::apply_fan(self.dchu.as_ref(), mode),
            WorkerCommand::ApplySlowFan(enabled) => {
                power_fan::apply_slow_fan(self.dchu.as_ref(), enabled)
            }
            WorkerCommand::ApplyTurboFan(enabled) => {
                power_fan::apply_turbo_fan(self.dchu.as_ref(), enabled)
            }
            WorkerCommand::ApplyFanOffset(percent) => {
                power_fan::apply_fan_offset(self.dchu.as_ref(), percent)
            }
            WorkerCommand::RunAntiDust => power_fan::run_anti_dust(self.dchu.as_ref()),
            WorkerCommand::ApplyAntiDustSchedule(schedule) => {
                power_fan::apply_anti_dust_schedule(self.dchu.as_ref(), schedule)
            }
            WorkerCommand::ApplyRpmEdit { cpu, gpu } => {
                power_fan::apply_rpm_edit(self.dchu.as_ref(), cpu, gpu)
            }
            WorkerCommand::ApplyFanCurve(curve) => {
                power_fan::apply_fan_curve(self.dchu.as_ref(), curve)
            }
            WorkerCommand::ApplyQuiet(disable_dgpu) => {
                power_fan::apply_quiet(self.dchu.as_ref(), disable_dgpu)
            }
            WorkerCommand::ApplyGpuSwitch(mode) => {
                power_fan::apply_gpu_switch(self.dchu.as_ref(), mode)
            }
            WorkerCommand::ApplyFlexiCharger(mode, start, stop) => {
                power_fan::apply_flexi_charger(self.dchu.as_ref(), mode, start, stop)
            }
            WorkerCommand::ApplyLowRefresh(enabled) => {
                power_fan::apply_low_refresh(self.dchu.as_ref(), enabled)
            }
            WorkerCommand::ApplyDisplayToggle(toggle, enabled) => {
                io_controls::apply_display_toggle(self.dchu.as_ref(), toggle, enabled)
            }
            WorkerCommand::ApplyMonitorTimeouts(ac, dc) => {
                io_controls::apply_monitor_timeouts(ac, dc)
            }
            WorkerCommand::ApplyScreenSaver(enabled, kind, minutes) => {
                io_controls::apply_screen_saver(self.dchu.as_ref(), enabled, kind, minutes)
            }
            WorkerCommand::ApplyBrightness(percent) => io_controls::apply_brightness(percent),
            WorkerCommand::ApplyBlueLight(level) => io_controls::apply_blue_light(level),
            WorkerCommand::ApplyVolume(percent) => io_controls::apply_volume(percent),
            WorkerCommand::ApplyMute(muted) => io_controls::apply_mute(muted),
            WorkerCommand::ApplyRgbBrightness(level) => {
                rgb::apply_brightness(self.dchu.as_ref(), level)
            }
            WorkerCommand::ApplyRgbColor(color) => rgb::apply_color(self.dchu.as_ref(), color),
            WorkerCommand::ApplyRgbEffect(effect) => rgb::apply_effect(self.dchu.as_ref(), effect),
            WorkerCommand::ApplyRgbSleepTimer(enabled, seconds) => {
                rgb::apply_sleep_timer(self.dchu.as_ref(), enabled, seconds)
            }
            WorkerCommand::ApplyRgbBootEffect(enabled) => {
                rgb::apply_boot_effect(self.dchu.as_ref(), enabled)
            }
            WorkerCommand::ApplyCamera(enabled) => input::apply_camera(self.dchu.as_ref(), enabled),
            WorkerCommand::ApplyTouchpad(enabled) => {
                input::apply_touchpad(self.dchu.as_ref(), enabled)
            }
            WorkerCommand::ApplyAirplane(enabled) => {
                input::apply_airplane(self.dchu.as_ref(), enabled)
            }
            WorkerCommand::ApplyQuickToggle(toggle, enabled) => {
                input::apply_quick_toggle(self.dchu.as_ref(), toggle, enabled)
            }
            WorkerCommand::ApplyLockKey(vk) => input::apply_lock_key(vk),
            WorkerCommand::Refresh => unreachable!("refresh is handled by the receive loop"),
        }
    }

    fn finish(&mut self, outcome: CommandOutcome, read_scope: Controls) {
        self.status = outcome.status;
        self.push(read_scope);
    }

    fn push(&mut self, read_scope: Controls) {
        snapshot::push_snapshot(
            &self.weak,
            &mut self.sampler,
            self.dchu.as_ref(),
            snapshot::SnapshotInput {
                status: self.status.clone(),
                thermal: self.thermal.current(Instant::now()),
                driver_health: &self.driver_health,
                control_sync: &self.control_sync,
            },
            read_scope,
        );
    }

    fn refresh(&mut self) {
        if self.dchu.is_none() {
            match Dchu::open() {
                Ok(opened) => {
                    self.status = Some((tr("Signed DCHU transport connected.").to_owned(), 1));
                    self.dchu = Some(opened);
                }
                Err(error) => self.status = Some((error.to_string(), 2)),
            }
            self.next_retry = Instant::now() + Duration::from_secs(5);
        }
        self.refresh_thermal();
        self.driver_health = driver_manager::detect(self.dchu.is_some());
        drop(self.control_sync.issue(Controls::ALL));
        self.push(Controls::ALL);
    }

    fn on_timeout(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        if self.dchu.is_none() && Instant::now() >= self.next_retry {
            if let Ok(opened) = Dchu::open() {
                self.dchu = Some(opened);
                self.status = Some((tr("Signed DCHU transport connected.").to_owned(), 1));
            }
            self.next_retry = Instant::now() + Duration::from_secs(5);
        }
        let full_read = self.tick == 1 || self.tick.is_multiple_of(5);
        if full_read {
            self.refresh_thermal();
            self.driver_health = driver_manager::detect(self.dchu.is_some());
        }
        self.push(if full_read {
            Controls::ALL
        } else {
            Controls::NONE
        });
    }

    fn refresh_thermal(&mut self) {
        let Some(dchu) = self.dchu.as_ref() else {
            self.thermal_failures = self.thermal_failures.saturating_add(1);
            return;
        };
        let started = Instant::now();
        match dchu.read_thermal() {
            Ok(sample) if sample.has_plausible_temperature() => {
                log_action(&format!(
                    "worker: thermal cpu={}C gpu={}C fan3={}C fan4={}C sampled in {} us",
                    sample.cpu.temperature_c,
                    sample.gpu.temperature_c,
                    sample.fan3.temperature_c,
                    sample.fan4.temperature_c,
                    started.elapsed().as_micros()
                ));
                self.thermal.observe(sample, Instant::now());
                self.thermal_failures = 0;
            }
            Ok(sample) => {
                self.thermal_failures = self.thermal_failures.saturating_add(1);
                log_action(&format!(
                    "worker: thermal sample ignored (cpu={}C gpu={}C fan3={}C fan4={}C, failures={})",
                    sample.cpu.temperature_c,
                    sample.gpu.temperature_c,
                    sample.fan3.temperature_c,
                    sample.fan4.temperature_c,
                    self.thermal_failures
                ));
            }
            Err(error) => {
                self.thermal_failures = self.thermal_failures.saturating_add(1);
                log_action(&format!(
                    "worker: thermal read failed ({error}, failures={})",
                    self.thermal_failures
                ));
            }
        }
    }
}

pub(crate) fn spawn_worker(receiver: CommandReceiver, weak: Weak<MainWindow>) {
    thread::spawn(move || WorkerRuntime::new(weak, receiver.sync.clone()).run(receiver));
}
