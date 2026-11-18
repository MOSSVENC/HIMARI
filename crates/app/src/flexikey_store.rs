//! FlexiKey profile snapshots and atomic XML persistence.

use crate::diagnostics::log_action;
use crate::flexikey_engine::EngineReport;
use crate::localization::tr;
use ram_control_center_protocol::{FlexiKeyProfile, FlexiKeyStore};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub(crate) struct FlexiKeyViewState {
    pub(crate) store: FlexiKeyStore,
    pub(crate) path: PathBuf,
    pub(crate) load_error: Option<String>,
    pub(crate) activated_index: Option<usize>,
    pub(crate) operation_error: Option<String>,
    pub(crate) operation_notice: Option<String>,
    pub(crate) engine_report: Option<EngineReport>,
    pub(crate) engine_error: Option<String>,
}

pub(crate) fn flexikey_profile_path() -> PathBuf {
    if let Some(path) = std::env::var_os("NEO_CONTROL_FLEXIKEY_XML") {
        return PathBuf::from(path);
    }
    let system_drive = std::env::var_os("SystemDrive").unwrap_or_else(|| "C:".into());
    PathBuf::from(system_drive)
        .join("ComboKey")
        .join("profile.xml")
}

fn blank_flexikey_store() -> FlexiKeyStore {
    FlexiKeyStore {
        profiles: vec![FlexiKeyProfile {
            index: 0,
            name: tr("Personal").to_owned(),
            icon: String::new(),
            current_device: 1,
            keyboard_enabled: true,
            mouse_enabled: true,
            selected: true,
            bindings: Vec::new(),
        }],
    }
}

pub(crate) fn load_flexikey_view(path: &Path) -> FlexiKeyViewState {
    match std::fs::read_to_string(path) {
        Ok(xml) => match FlexiKeyStore::from_xml(&xml) {
            Ok(store) => FlexiKeyViewState {
                store,
                path: path.to_owned(),
                load_error: None,
                activated_index: None,
                operation_error: None,
                operation_notice: None,
                engine_report: None,
                engine_error: None,
            },
            Err(error) => FlexiKeyViewState {
                store: FlexiKeyStore::default(),
                path: path.to_owned(),
                load_error: Some(error.to_string()),
                activated_index: None,
                operation_error: None,
                operation_notice: None,
                engine_report: None,
                engine_error: None,
            },
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => FlexiKeyViewState {
            store: blank_flexikey_store(),
            path: path.to_owned(),
            load_error: None,
            activated_index: None,
            operation_error: None,
            operation_notice: Some(tr("profile.xml was not found. A blank profile is ready; saving a binding creates the file.")
            .to_owned()),
            engine_report: None,
            engine_error: None,
        },
        Err(error) => FlexiKeyViewState {
            store: FlexiKeyStore::default(),
            path: path.to_owned(),
            load_error: Some(error.to_string()),
            activated_index: None,
            operation_error: None,
            operation_notice: None,
            engine_report: None,
            engine_error: None,
        },
    }
}

pub(crate) fn save_flexikey_store(path: &Path, store: &FlexiKeyStore) -> Result<(), String> {
    let started = std::time::Instant::now();
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    let temporary = path.with_extension("xml.neo.tmp");
    std::fs::write(&temporary, store.to_xml()).map_err(|error| {
        format!(
            "could not write temporary profile file {}: {error}",
            temporary.display()
        )
    })?;

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt as _;
        use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MoveFileExW};
        let source = temporary
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let destination = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        // SAFETY: both paths are live NUL-terminated UTF-16 buffers, and the
        // source file remains available until this synchronous move returns.
        let moved = unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING,
            )
        };
        if moved == 0 {
            let error = std::io::Error::last_os_error();
            let _ = std::fs::remove_file(&temporary);
            return Err(format!("could not replace {}: {error}", path.display()));
        }
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(&temporary, path)
            .map_err(|error| format!("could not replace {}: {error}", path.display()))?;
    }
    log_action(&format!(
        "flexikey: saved {} in {}us",
        path.display(),
        started.elapsed().as_micros()
    ));
    Ok(())
}
