//! Product identity and migration names used by the Windows application.

pub(crate) const PRODUCT_NAME: &str = "RAM CONTROL CENTER";
pub(crate) const PRODUCT_EXE: &str = "ram-control-center.exe";
pub(crate) const PRODUCT_DATA_DIR: &str = "RAM Control Center";
pub(crate) const LEGACY_DATA_DIR: &str = "NeoControl";

pub(crate) const SERVICE_NAME: &str = "RAMControlCenterBoot";
pub(crate) const LEGACY_SERVICE_NAME: &str = "NeoControlBoot";
pub(crate) const SERVICE_DISPLAY_NAME: &str = "RAM CONTROL CENTER Boot Agent";

pub(crate) const RUN_VALUE_NAME: &str = "RAMControlCenter";
pub(crate) const LEGACY_RUN_VALUE_NAME: &str = "NeoControl";
pub(crate) const RUN_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

pub(crate) const WINDOW_CLASS: &str = "RAMControlCenterTrayWindow";
pub(crate) const MUTEX_NAME: &str = "Local\\RAMControlCenterUiInstance";

pub(crate) fn product_data_dir() -> std::path::PathBuf {
    std::env::var_os("ProgramData")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(PRODUCT_DATA_DIR)
}

pub(crate) fn legacy_data_dir() -> std::path::PathBuf {
    std::env::var_os("ProgramData")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(LEGACY_DATA_DIR)
}
