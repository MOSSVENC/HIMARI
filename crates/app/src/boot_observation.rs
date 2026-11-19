//! Bounded CPU observations after the boot policy write, in the same process.

use crate::telemetry::CpuPerformanceReading;

#[derive(Debug, Default)]
struct Summary {
    samples: u32,
    valid_clock_samples: u32,
    valid_load_samples: u32,
    min_mhz: Option<u32>,
    max_mhz: Option<u32>,
    nominal_mhz: Option<u32>,
}

impl Summary {
    fn record(&mut self, sample: CpuPerformanceReading) {
        self.samples += 1;
        self.nominal_mhz = sample.nominal_mhz.or(self.nominal_mhz);
        self.valid_load_samples += u32::from(sample.load_percent.is_some());
        if let Some(mhz) = sample.clock_mhz {
            self.valid_clock_samples += 1;
            self.min_mhz = Some(self.min_mhz.map_or(mhz, |previous| previous.min(mhz)));
            self.max_mhz = Some(self.max_mhz.map_or(mhz, |previous| previous.max(mhz)));
        }
    }
}

#[cfg(windows)]
pub(crate) fn run(dchu: &ram_control_center_protocol::Dchu, duration: std::time::Duration) {
    use std::time::{Duration, Instant};

    use crate::boot_agent::log_line;
    use crate::runtime_state::system_uptime;
    use crate::telemetry::CpuPerformanceSampler;

    if duration.is_zero() {
        return;
    }
    log_line(&format!(
        "CPU observation start: duration_ms={} uptime_ms={}",
        duration.as_millis(),
        system_uptime().as_millis()
    ));
    let started = Instant::now();
    let deadline = started + duration;
    let mut sampler = CpuPerformanceSampler::new();
    let mut summary = Summary::default();
    let mut next_thermal = started;
    let mut thermal = None;
    let mut thermal_age = None;
    let mut next_sample = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        let wake = next_sample.min(deadline);
        std::thread::sleep(wake.saturating_duration_since(Instant::now()));
        if Instant::now() >= deadline {
            break;
        }
        let sample = sampler.sample();
        let sample_uptime_ms = system_uptime().as_millis();
        summary.record(sample);
        let profile = dchu.read_setting_byte(1, 1);
        let policy = dchu.read_setting_byte(1, 16);
        if Instant::now() >= next_thermal {
            thermal = dchu.read_thermal().ok().and_then(|reading| {
                ram_control_center_protocol::is_plausible_temperature(reading.cpu.temperature_c)
                    .then_some(reading.cpu.temperature_c)
            });
            thermal_age = thermal.map(|_| Instant::now());
            next_thermal = Instant::now() + Duration::from_secs(5);
        }
        let scheme = windows_registry::LOCAL_MACHINE
            .open(r"SYSTEM\CurrentControlSet\Control\Power\User\PowerSchemes")
            .and_then(|key| key.get_string("ActivePowerScheme"))
            .ok();
        log_line(&format!(
            "CPU observation: sample_uptime_ms={sample_uptime_ms} elapsed_ms={} clock_mhz={:?} nominal_mhz={:?} load_percent={:?} profile={profile:?} policy={policy:?} cpu_temp_c={thermal:?} temp_age_ms={:?} scheme={scheme:?}",
            started.elapsed().as_millis(),
            sample.clock_mhz,
            sample.nominal_mhz,
            sample.load_percent,
            thermal_age.map(|sampled| sampled.elapsed().as_millis())
        ));
        next_sample += Duration::from_secs(1);
        // Slow transport reads must not turn missed samples into a busy loop.
        if next_sample <= Instant::now() {
            next_sample = Instant::now() + Duration::from_secs(1);
        }
    }
    log_line(&format!(
        "CPU observation complete: elapsed_ms={} {summary:?}",
        started.elapsed().as_millis()
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_counters_do_not_count_as_zero_frequency_or_idle() {
        let mut summary = Summary::default();
        summary.record(CpuPerformanceReading::default());
        assert_eq!(summary.samples, 1);
        assert_eq!(summary.valid_clock_samples, 0);
        assert_eq!(summary.valid_load_samples, 0);
        assert_eq!((summary.min_mhz, summary.max_mhz), (None, None));
    }

    #[test]
    fn range_records_measured_underclock_and_boost_without_capping() {
        let mut summary = Summary::default();
        for mhz in [400, 3601, 4800] {
            summary.record(CpuPerformanceReading {
                clock_mhz: Some(mhz),
                nominal_mhz: Some(3601),
                load_percent: Some(0.0),
            });
        }
        summary.record(CpuPerformanceReading::default());
        assert_eq!(summary.samples, 4);
        assert_eq!(summary.valid_clock_samples, 3);
        assert_eq!(summary.valid_load_samples, 3);
        assert_eq!(summary.nominal_mhz, Some(3601));
        assert_eq!((summary.min_mhz, summary.max_mhz), (Some(400), Some(4800)));
    }
}
