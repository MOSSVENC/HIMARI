//! Retains independent sensor samples through brief mailbox read failures.

#![forbid(unsafe_code)]
use himari_protocol::{FanChannel, ThermalSample, is_plausible_temperature};
use std::time::{Duration, Instant};

const MAX_AGE: Duration = Duration::from_secs(15);

#[derive(Default)]
pub(crate) struct ThermalCache {
    channels: [Option<(FanChannel, Instant)>; 4],
}

impl ThermalCache {
    pub(crate) fn observe(&mut self, sample: ThermalSample, now: Instant) {
        for (slot, channel) in
            self.channels
                .iter_mut()
                .zip([sample.cpu, sample.gpu, sample.fan3, sample.fan4])
        {
            if is_plausible_temperature(channel.temperature_c) {
                *slot = Some((channel, now));
            }
        }
    }
    pub(crate) fn current(&self, now: Instant) -> Option<ThermalSample> {
        let current = self.channels.map(|slot| {
            slot.filter(|(_, sampled)| now.saturating_duration_since(*sampled) < MAX_AGE)
                .map(|(value, _)| value)
        });
        if current[0].is_none() && current[1].is_none() {
            return None;
        }
        Some(ThermalSample {
            cpu: current[0].unwrap_or_default(),
            gpu: current[1].unwrap_or_default(),
            fan3: current[2].unwrap_or_default(),
            fan4: current[3].unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(cpu: u8, gpu: u8) -> ThermalSample {
        ThermalSample {
            cpu: FanChannel {
                temperature_c: cpu,
                rpm: 2100,
                ..FanChannel::default()
            },
            gpu: FanChannel {
                temperature_c: gpu,
                rpm: 2200,
                ..FanChannel::default()
            },
            ..ThermalSample::default()
        }
    }
    #[test]
    fn partial_zero_response_preserves_only_the_previous_valid_sensor() {
        let mut cache = ThermalCache::default();
        let now = Instant::now();
        cache.observe(sample(55, 47), now);
        cache.observe(sample(0, 48), now + Duration::from_secs(5));
        let latest = cache.current(now + Duration::from_secs(5)).unwrap();
        assert_eq!(latest.cpu.temperature_c, 55);
        assert_eq!(latest.gpu.temperature_c, 48);
        let expired_cpu = cache.current(now + Duration::from_secs(15)).unwrap();
        assert_eq!(expired_cpu.cpu, FanChannel::default());
        assert_eq!(expired_cpu.gpu.temperature_c, 48);
    }
    #[test]
    fn invalid_sample_never_extends_freshness() {
        let mut cache = ThermalCache::default();
        let now = Instant::now();
        cache.observe(sample(55, 47), now);
        cache.observe(sample(0, 255), now + Duration::from_secs(14));
        assert!(cache.current(now + Duration::from_secs(14)).is_some());
        assert!(cache.current(now + Duration::from_secs(15)).is_none());
    }
    #[test]
    fn no_sample_and_absent_extra_fans_remain_unavailable() {
        let mut cache = ThermalCache::default();
        let now = Instant::now();
        cache.observe(sample(0, 0), now);
        assert!(cache.current(now).is_none());
        cache.observe(sample(55, 47), now);
        assert_eq!(cache.current(now).unwrap().fan3, FanChannel::default());
        assert_eq!(cache.current(now).unwrap().fan4, FanChannel::default());
    }
}
