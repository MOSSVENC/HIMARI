//! Read-only process-name checks used by input and Fn ownership arbitration.

pub(crate) fn any_running(names: &[&str]) -> bool {
    #[cfg(windows)]
    {
        use std::mem::size_of;
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        };

        // SAFETY: the snapshot handle is checked before use and closed exactly
        // once; PROCESSENTRY32W is initialized with its required size.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return false;
            }
            let mut entry = PROCESSENTRY32W {
                dwSize: size_of::<PROCESSENTRY32W>() as u32,
                ..PROCESSENTRY32W::default()
            };
            let mut found = false;
            let mut present = Process32FirstW(snapshot, &mut entry) != 0;
            while present {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|character| *character == 0)
                    .unwrap_or(entry.szExeFile.len());
                let current = String::from_utf16_lossy(&entry.szExeFile[..end]);
                if names.iter().any(|name| current.eq_ignore_ascii_case(name)) {
                    found = true;
                    break;
                }
                present = Process32NextW(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
            found
        }
    }
    #[cfg(not(windows))]
    {
        let _ = names;
        false
    }
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
