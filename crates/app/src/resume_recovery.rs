//! Sleep/resume policy restoration using wall-clock gap detection.
//!
//! The interactive process is suspended while Windows sleeps. On wake, a
//! monotonic five-second poll observes a wall-clock gap much larger than its
//! expected cadence and reapplies the persisted policy.

use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

const POLL_INTERVAL: Duration = Duration::from_secs(5);
const RESUME_THRESHOLD: Duration = Duration::from_secs(12);

pub(crate) fn start() {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    if let Err(error) = std::thread::Builder::new()
        .name("power-resume-recovery".to_owned())
        .spawn(watch_loop)
    {
        log_line(&format!("resume recovery thread could not start: {error}"));
    }
}

fn watch_loop() {
    let mut previous = SystemTime::now();
    loop {
        std::thread::sleep(POLL_INTERVAL);
        let now = SystemTime::now();
        let elapsed = now.duration_since(previous).unwrap_or_default();
        if resume_gap(elapsed) {
            log_line(&format!("resume gap detected: {}s", elapsed.as_secs()));
            match crate::boot_agent::reapply_saved_policy() {
                Ok(()) => log_line("resume policy restore completed"),
                Err(error) => log_line(&format!("resume policy restore failed: {error}")),
            }
        }
        previous = now;
    }
}

fn resume_gap(elapsed: Duration) -> bool {
    elapsed >= RESUME_THRESHOLD
}

fn log_line(message: &str) {
    use std::io::Write as _;

    crate::diagnostics::log_action(message);
    let root = crate::product::product_data_dir();
    if std::fs::create_dir_all(&root).is_err() {
        return;
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("resume-recovery.log"))
    {
        let _ = writeln!(file, "{message}");
    }
}

#[cfg(test)]
mod tests {
    use super::{RESUME_THRESHOLD, resume_gap};

    #[test]
    fn normal_poll_cadence_is_not_a_resume() {
        assert!(!resume_gap(
            RESUME_THRESHOLD - std::time::Duration::from_secs(1)
        ));
    }

    #[test]
    fn suspended_gap_triggers_resume() {
        assert!(resume_gap(RESUME_THRESHOLD));
        assert!(resume_gap(std::time::Duration::from_secs(120)));
    }
}
