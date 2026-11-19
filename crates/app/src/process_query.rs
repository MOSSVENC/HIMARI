//! Read-only process-name checks used by input and Fn ownership arbitration.

use sysinfo::{ProcessesToUpdate, System};

pub(crate) fn any_running(names: &[&str]) -> bool {
    if names.is_empty() {
        return false;
    }
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);
    system.processes().values().any(|process| {
        let current = process.name().to_string_lossy();
        matches_name(&current, names)
    })
}

fn matches_name(current: &str, names: &[&str]) -> bool {
    names.iter().any(|name| current.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::{any_running, matches_name};

    #[test]
    fn empty_process_set_never_matches() {
        assert!(!any_running(&[]));
    }

    #[test]
    fn process_names_are_case_insensitive() {
        assert!(matches_name("CC40.EXE", &["cc40.exe"]));
        assert!(matches_name("FnKey.exe", &["FnKey.exe"]));
        assert!(!matches_name("ram-control-center.exe", &["FnKey.exe"]));
    }
}
